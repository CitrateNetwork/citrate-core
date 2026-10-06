# SCL-S0.7 (RT-04): stop THIS installation's own sidecar processes before the installer copies
# files over them. Run by the NSIS pre-install hook (installer-hooks.nsh) with Windows PowerShell.
#
# Why: a sidecar .exe that is still running holds its image file open, so overwriting it during a
# manual 0.4.x to 0.5.0 install can fail mid-install or leave mixed versions. Tauri's own check
# only covers the main binary.
#
# Matching rule (the only one): a process is stopped only when its executable path is EXACTLY
# <InstallDir>\<name>.exe for one of the listed sidecar names (full-path equality, compared
# case-insensitively as Windows paths are). Never by bare name, never by prefix: the same name in
# another folder, a sub-folder, or a sibling folder whose name starts with the install folder's
# name is left alone, and so is any process whose path cannot be read.
#
# Steps: a graceful step (a polite close request, then a bounded wait; sidecars whose parent app
# has gone also exit on their own when their stdin closes), then terminate whatever is left, then
# a bounded wait for it to be gone.
#
# Exit codes: 0 = none of this installation's sidecars is running; 1 = some are still running;
# 2 = bad arguments or the process list could not be read (nothing was signalled).
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallDir,
    # Comma-separated sidecar names without ".exe" (a single string: -File passes no arrays).
    [Parameter(Mandatory = $true)][string]$Names,
    [int]$GraceSeconds = 5,
    [int]$KillWaitSeconds = 5
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Write-Log([string]$Message) {
    Write-Output "citrate-installer: $Message"
}

try {
    $root = [System.IO.Path]::GetFullPath($InstallDir).TrimEnd('\', '/')
} catch {
    Write-Log "the install directory is not a usable path: '$InstallDir'"
    exit 2
}
if ($root -eq '' -or -not [System.IO.Path]::IsPathRooted($root)) {
    Write-Log "the install directory is not an absolute path: '$InstallDir'"
    exit 2
}

$targets = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase)
$imageNames = New-Object 'System.Collections.Generic.List[string]'
foreach ($raw in ($Names -split ',')) {
    $name = $raw.Trim()
    if ($name -eq '') { continue }
    # A plain file name only: no separators, no "..", no wildcard.
    if ($name -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$' -or $name.Contains('..')) {
        Write-Log "refusing sidecar name '$name'"
        exit 2
    }
    [void]$targets.Add([System.IO.Path]::Combine($root, "$name.exe"))
    $imageNames.Add("$name.exe")
}
if ($targets.Count -eq 0) {
    Write-Log 'no sidecar names given'
    exit 2
}

# The WMI filter only narrows the scan by image name; the exact path decides.
$filter = ($imageNames | ForEach-Object { "Name = '$_'" }) -join ' OR '

# This installation's own running sidecars: one record per process (pid, exact path, creation
# time, so a reused pid is never mistaken for the process seen earlier).
function Get-OwnSidecars {
    $found = @()
    foreach ($p in @(Get-CimInstance -ClassName Win32_Process -Filter $filter)) {
        $path = $p.ExecutablePath
        if (-not $path) { continue }
        try { $full = [System.IO.Path]::GetFullPath($path) } catch { continue }
        if ($targets.Contains($full)) {
            $found += [pscustomobject]@{
                Id      = [int]$p.ProcessId
                Path    = $full
                Created = $p.CreationDate
            }
        }
    }
    return , $found
}

function Get-StillRunning($Seen) {
    $now = Get-OwnSidecars
    $left = @()
    foreach ($s in $Seen) {
        foreach ($n in $now) {
            if ($n.Id -eq $s.Id -and $n.Created -eq $s.Created) { $left += $n }
        }
    }
    return , $left
}

function Wait-Gone($Seen, [int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        $left = Get-StillRunning $Seen
        if ($left.Count -eq 0) { return , $left }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $deadline)
    return , (Get-StillRunning $Seen)
}

try {
    $own = Get-OwnSidecars
} catch {
    Write-Log "could not read the process list; nothing was stopped: $($_.Exception.Message)"
    exit 2
}
if ($own.Count -eq 0) {
    Write-Log "no sidecar of this installation is running ($root)"
    exit 0
}

try {
    foreach ($s in $own) {
        Write-Log "stopping $($s.Path) (pid $($s.Id))"
        # Graceful step: a close request to that pid only (no /F, no /T, never /IM). A windowless
        # sidecar refuses it; that is expected, and the terminate step below covers it.
        $ErrorActionPreference = 'Continue'
        & "$env:SystemRoot\System32\taskkill.exe" /PID $s.Id *> $null
        $ErrorActionPreference = 'Stop'
    }
    $left = Wait-Gone $own $GraceSeconds

    foreach ($s in $left) {
        Write-Log "terminating $($s.Path) (pid $($s.Id))"
        Stop-Process -Id $s.Id -Force -ErrorAction SilentlyContinue
    }
    if ($left.Count -gt 0) {
        $left = Wait-Gone $left $KillWaitSeconds
    }
} catch {
    Write-Log "stopping failed: $($_.Exception.Message)"
    exit 1
}

if ($left.Count -gt 0) {
    foreach ($s in $left) { Write-Log "still running: $($s.Path) (pid $($s.Id))" }
    exit 1
}
Write-Log "stopped $($own.Count) sidecar process(es) of this installation"
exit 0
