# Hosted clean-Windows smoke test for the exact unpublished v0.5.0 production installer.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [Parameter(Mandatory = $true)]
    [string]$EvidenceDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ExpectedInstallerSize = 523802727
$ExpectedInstallerSha256 = '41d3a271dd8d66cbd8339607f13510d9a26bf5349f85ea8396d4ef63974e0c55'
$ExpectedFiles = [ordered]@{
    'citrate-core.exe'                 = '994a5090905d791892621e9e266e297b54aa8a736520da835f798d4b15ff5659'
    'citrate.exe'                      = '4dd5c793ea19400c5777e5281f73784b95a979970877e1a06fc2cf20421b69c3'
    'node-agent.exe'                   = '79f9c652f887fc8ea97f095427570a33465ae2f43d929bb9de4689903d09244e'
    'mem-mcp.exe'                      = 'd86f532c188a7ca9479a7d58a56fb6a56b8862274a1475fad675d8804d322c81'
    'comms-member-daemon.exe'          = 'eb4aef4d8d6ef0f215cf53d729d050a09c02ab3db2d446f3857f2636bba4fc98'
    'cluster-daemon.exe'               = 'a630dda936d1335cb8421b9b087576a7d80976d76b6528d650f8f75e0383227c'
    'hermes.exe'                       = 'e35a89187c46c574c8c3c5b5ec121df5b7e3e91c0dc640a93afa6c49ea973150'
    'ipfs.exe'                         = '9e63e925e3ee0697446448b119f47d731b7ee55ce74e4c53d7d2b647bb3a9f56'
    'llama-server.exe'                 = '8cac1d9ff401933db6588cc04b2a5868a6716e057929ee714325aa1278c9969b'
    'llama\llama-server-impl.dll'     = 'fb5e5fa88d885ddec59ff9de535c8ca26c5133f466c3a2b4360e7ebaf0633e40'
}

$Checks = New-Object 'System.Collections.Generic.List[object]'
$Failures = New-Object 'System.Collections.Generic.List[string]'
$Started = New-Object 'System.Collections.Generic.List[System.Diagnostics.Process]'
$RuntimeRoot = $env:RUNNER_TEMP
if (-not $RuntimeRoot) { $RuntimeRoot = [System.IO.Path]::GetTempPath() }
$InstallDirectory = Join-Path $RuntimeRoot 'Citrate Core Hosted Acceptance'
$FixtureDirectory = Join-Path $RuntimeRoot 'citrate-installer-fixtures'
$StartedUtc = (Get-Date).ToUniversalTime().ToString('o')
$CaughtError = $null

New-Item -ItemType Directory -Path $EvidenceDirectory -Force | Out-Null

function Add-Check([string]$Name, [bool]$Passed, [object]$Actual, [object]$Expected) {
    $Checks.Add([pscustomobject]@{
        name = $Name
        passed = $Passed
        actual = $Actual
        expected = $Expected
    })
    if (-not $Passed) { $Failures.Add("$Name (actual: $Actual; expected: $Expected)") }
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-ProcessRecord([int]$ProcessId) {
    $Process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $Process) { return [pscustomobject]@{ id = $ProcessId; running = $false; path = $null } }
    $Path = $null
    try { $Path = $Process.Path } catch { $Path = $null }
    return [pscustomobject]@{ id = $ProcessId; running = $true; path = $Path }
}

function Stop-StartedProcesses {
    foreach ($Process in $Started) {
        try {
            if (-not $Process.HasExited) { Stop-Process -InputObject $Process -Force -ErrorAction SilentlyContinue }
        } catch {
            Write-Host "cleanup notice: process $($Process.Id) was already unavailable"
        }
    }
    $Started.Clear()
}

function Invoke-Installer([string]$Scenario) {
    $Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $Process = Start-Process -FilePath $InstallerPath -ArgumentList @('/S', "/D=$InstallDirectory") -PassThru
    $null = $Process.Handle
    $Exited = $Process.WaitForExit(300000)
    if (-not $Exited) {
        & taskkill.exe /PID $Process.Id /T /F 2>&1 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory "$Scenario-taskkill.log")
    }
    $Stopwatch.Stop()
    $ExitCode = if ($Exited) { $Process.ExitCode } else { $null }
    $Record = [pscustomobject]@{
        scenario = $Scenario
        startedUtc = (Get-Date).ToUniversalTime().Subtract($Stopwatch.Elapsed).ToString('o')
        elapsedSeconds = [math]::Round($Stopwatch.Elapsed.TotalSeconds, 3)
        exitedWithin300Seconds = $Exited
        exitCode = $ExitCode
    }
    $Record | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory "$Scenario-installer.json") -Encoding UTF8
    Add-Check "$Scenario installer exits within 300 seconds" $Exited $Record.elapsedSeconds '< 300'
    Add-Check "$Scenario installer exits successfully" ($Exited -and $ExitCode -eq 0) $ExitCode 0
    return $Record
}

function Test-InstalledPayload([string]$Scenario) {
    $Rows = @(
        foreach ($RelativePath in $ExpectedFiles.Keys) {
            $Path = Join-Path $InstallDirectory $RelativePath
            $Present = Test-Path -LiteralPath $Path -PathType Leaf
            $ActualHash = if ($Present) { Get-Sha256 $Path } else { $null }
            $ExpectedHash = $ExpectedFiles[$RelativePath]
            Add-Check "$Scenario payload $RelativePath" ($Present -and $ActualHash -eq $ExpectedHash) $ActualHash $ExpectedHash
            [pscustomobject]@{
                file = $RelativePath
                present = $Present
                sha256 = $ActualHash
                expectedSha256 = $ExpectedHash
                matches = ($Present -and $ActualHash -eq $ExpectedHash)
            }
        }
    )
    $Rows | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory "$Scenario-payload.json") -Encoding UTF8
}

try {
    $InstallerPath = [System.IO.Path]::GetFullPath($InstallerPath)
    $Installer = Get-Item -LiteralPath $InstallerPath
    $InstallerHash = Get-Sha256 $InstallerPath
    $Signature = Get-AuthenticodeSignature -LiteralPath $InstallerPath
    $InstallerRecord = [pscustomobject]@{
        path = $InstallerPath
        size = $Installer.Length
        sha256 = $InstallerHash
        authenticode = [string]$Signature.Status
        productVersion = $Installer.VersionInfo.ProductVersion
    }
    $InstallerRecord | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'installer-identity.json') -Encoding UTF8
    Add-Check 'installer size' ($Installer.Length -eq $ExpectedInstallerSize) $Installer.Length $ExpectedInstallerSize
    Add-Check 'installer SHA-256' ($InstallerHash -eq $ExpectedInstallerSha256) $InstallerHash $ExpectedInstallerSha256
    Add-Check 'installer Authenticode status' ([string]$Signature.Status -eq 'NotSigned') ([string]$Signature.Status) 'NotSigned'
    Add-Check 'installer product version' ($Installer.VersionInfo.ProductVersion -eq '0.5.0') $Installer.VersionInfo.ProductVersion '0.5.0'
    if ($Failures.Count -gt 0) { throw 'installer identity did not match the authorized draft asset' }

    Remove-Item -LiteralPath $InstallDirectory -Recurse -Force -ErrorAction SilentlyContinue
    Add-Check 'fresh baseline is absent' (-not (Test-Path -LiteralPath $InstallDirectory)) (Test-Path -LiteralPath $InstallDirectory) $false

    $null = Invoke-Installer 'fresh-install'
    Add-Check 'fresh install directory exists' (Test-Path -LiteralPath $InstallDirectory -PathType Container) (Test-Path -LiteralPath $InstallDirectory -PathType Container) $true
    Test-InstalledPayload 'fresh-install'
    if ($Failures.Count -gt 0) { throw 'fresh-install validation failed' }

    New-Item -ItemType Directory -Path $FixtureDirectory -Force | Out-Null
    $Sleeper = Join-Path $FixtureDirectory 'sleeper.exe'
    Add-Type -OutputType ConsoleApplication -OutputAssembly $Sleeper -TypeDefinition @'
public static class Sleeper {
    public static void Main() { System.Threading.Thread.Sleep(600000); }
}
'@
    $DecoyDirectory = Join-Path $FixtureDirectory 'decoy'
    New-Item -ItemType Directory -Path $DecoyDirectory | Out-Null
    $OwnNames = @('citrate-core.exe', 'hermes.exe', 'citrate.exe')
    $OwnProcesses = @{}
    foreach ($Name in $OwnNames) {
        $Path = Join-Path $InstallDirectory $Name
        Copy-Item -LiteralPath $Sleeper -Destination $Path -Force
        $Process = Start-Process -FilePath $Path -PassThru -WindowStyle Hidden
        $Started.Add($Process)
        $OwnProcesses[$Name] = $Process.Id
    }
    $DecoyPath = Join-Path $DecoyDirectory 'hermes.exe'
    Copy-Item -LiteralPath $Sleeper -Destination $DecoyPath
    $Decoy = Start-Process -FilePath $DecoyPath -PassThru -WindowStyle Hidden
    $Started.Add($Decoy)
    Start-Sleep -Seconds 1

    $Before = [pscustomobject]@{
        own = @($OwnProcesses.Keys | ForEach-Object { Get-ProcessRecord $OwnProcesses[$_] })
        decoy = Get-ProcessRecord $Decoy.Id
    }
    $Before | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'running-reinstall-before-processes.json') -Encoding UTF8
    foreach ($Name in $OwnNames) {
        Add-Check "running fixture $Name started" ([bool](Get-Process -Id $OwnProcesses[$Name] -ErrorAction SilentlyContinue)) $OwnProcesses[$Name] 'running process'
    }

    $null = Invoke-Installer 'running-reinstall'
    Start-Sleep -Seconds 1
    $After = [pscustomobject]@{
        own = @($OwnProcesses.Keys | ForEach-Object { Get-ProcessRecord $OwnProcesses[$_] })
        decoy = Get-ProcessRecord $Decoy.Id
    }
    $After | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'running-reinstall-after-processes.json') -Encoding UTF8
    foreach ($Name in $OwnNames) {
        Add-Check "running reinstall stops owned $Name" (-not (Get-Process -Id $OwnProcesses[$Name] -ErrorAction SilentlyContinue)) (Get-ProcessRecord $OwnProcesses[$Name]) 'not running'
    }
    Add-Check 'running reinstall leaves same-name decoy alive' ([bool](Get-Process -Id $Decoy.Id -ErrorAction SilentlyContinue)) (Get-ProcessRecord $Decoy.Id) 'running'
    Test-InstalledPayload 'running-reinstall'

    $HookProcesses = @(
        Get-CimInstance Win32_Process | Where-Object {
            $_.CommandLine -and $_.CommandLine -like '*citrate-stop-own-sidecars.ps1*'
        } | Select-Object ProcessId, Name, ExecutablePath, CommandLine
    )
    $HookProcesses | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'remaining-hook-processes.json') -Encoding UTF8
    Add-Check 'no installer hook process remains' ($HookProcesses.Count -eq 0) $HookProcesses.Count 0
} catch {
    $CaughtError = $_.Exception.Message
    $Failures.Add("unhandled error: $CaughtError")
} finally {
    Stop-StartedProcesses
    Remove-Item -LiteralPath $FixtureDirectory -Recurse -Force -ErrorAction SilentlyContinue
    $Result = [pscustomobject]@{
        startedUtc = $StartedUtc
        finishedUtc = (Get-Date).ToUniversalTime().ToString('o')
        runner = [pscustomobject]@{
            os = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
            osArchitecture = [string][System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
            powershellVersion = [string]$PSVersionTable.PSVersion
            user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
        }
        installDirectory = $InstallDirectory
        checks = $Checks
        failureCount = $Failures.Count
        failures = $Failures
        error = $CaughtError
        scope = 'Hosted production-installer smoke only; no human GUI, passkey, wallet, entitlement, or visual C4 claim.'
    }
    $Result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'result.json') -Encoding UTF8
}

if ($Failures.Count -gt 0) {
    $Failures | ForEach-Object { Write-Host "FAIL - $_" }
    exit 1
}
Write-Host 'All hosted production-installer smoke checks passed.'
exit 0
