# Hosted clean-Windows smoke test for a parameterized Citrate Core installer.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [Parameter(Mandatory = $true)]
    [string]$EvidenceDirectory,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedVersion,
    [Parameter(Mandatory = $true)]
    [string]$SourceSha,
    [Parameter(Mandatory = $true)]
    [string]$PayloadManifestPath,
    [string]$ExpectedInstallerSha256,
    [Nullable[long]]$ExpectedInstallerSize
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Checks = New-Object 'System.Collections.Generic.List[object]'
$Failures = New-Object 'System.Collections.Generic.List[string]'
$Started = New-Object 'System.Collections.Generic.List[System.Diagnostics.Process]'
$RuntimeRoot = $env:RUNNER_TEMP
if (-not $RuntimeRoot) { $RuntimeRoot = [System.IO.Path]::GetTempPath() }
$RuntimeRoot = [System.IO.Path]::GetFullPath($RuntimeRoot)
$RunIdentity = if ($env:GITHUB_RUN_ID) {
    "$($env:GITHUB_RUN_ID)-$($env:GITHUB_RUN_ATTEMPT)"
} else {
    "local-$PID"
}
$InstallDirectory = Join-Path $RuntimeRoot "Citrate Core Hosted Acceptance-$RunIdentity"
$FixtureDirectory = Join-Path $RuntimeRoot "citrate-installer-fixtures-$RunIdentity"
$InputDirectory = Join-Path $RuntimeRoot "citrate-installer-input-$RunIdentity"
$InstallDirectoryOwned = $false
$FixtureDirectoryOwned = $false
$InputDirectoryOwned = $false
$InstallerReadLock = $null
$StartedUtc = (Get-Date).ToUniversalTime().ToString('o')
$CaughtError = $null
$Manifest = $null
$PayloadRows = @()
$Sleeper = $null
$SidecarNames = @(
    'citrate.exe',
    'node-agent.exe',
    'mem-mcp.exe',
    'llama-server.exe',
    'ipfs.exe',
    'comms-member-daemon.exe',
    'cluster-daemon.exe',
    'hermes.exe'
)

if ([string]::IsNullOrWhiteSpace($EvidenceDirectory)) { throw 'EvidenceDirectory is required' }
$EvidenceDirectory = [System.IO.Path]::GetFullPath($EvidenceDirectory)
if (Test-Path -LiteralPath $EvidenceDirectory) {
    $EvidenceItem = Get-Item -LiteralPath $EvidenceDirectory -Force
    if ($EvidenceItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        throw "EvidenceDirectory must not be a reparse point: $EvidenceDirectory"
    }
}
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

function Get-ProcessByIdOrNull([int]$ProcessId) {
    try {
        return [System.Diagnostics.Process]::GetProcessById($ProcessId)
    } catch [System.ArgumentException] {
        return $null
    }
}

function Test-ReparsePoint([string]$Path) {
    $Item = Get-Item -LiteralPath $Path -Force
    return [bool]($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)
}

function Remove-OwnedTestDirectory([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $FullPath = [System.IO.Path]::GetFullPath($Path)
    $Parent = [System.IO.Path]::GetDirectoryName($FullPath)
    if (-not [string]::Equals($Parent, $RuntimeRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label is outside the runtime root: $FullPath"
    }
    if (Test-ReparsePoint $FullPath) { throw "$Label is a reparse point: $FullPath" }
    if (-not (Test-Path -LiteralPath $FullPath -PathType Container)) {
        throw "$Label is not a directory: $FullPath"
    }
    Remove-Item -LiteralPath $FullPath -Recurse -Force -ErrorAction Stop
}

function Resolve-PayloadPath([string]$RelativePath) {
    if ([string]::IsNullOrWhiteSpace($RelativePath) -or
        [System.IO.Path]::IsPathRooted($RelativePath) -or
        $RelativePath -match '(^|[\\/])\.{1,2}([\\/]|$)' -or
        $RelativePath.Contains(':') -or
        $RelativePath -match '[\x00-\x1f]' -or
        $RelativePath.EndsWith('/') -or
        $RelativePath.EndsWith('\')) {
        throw "payload manifest contains an unsafe path: $RelativePath"
    }

    $Separator = [string][System.IO.Path]::DirectorySeparatorChar
    $Normalized = $RelativePath.Replace('/', $Separator).Replace('\', $Separator)
    $Root = $InstallDirectory.TrimEnd('\', '/') + $Separator
    $FullPath = [System.IO.Path]::GetFullPath((Join-Path $InstallDirectory $Normalized))
    if (-not $FullPath.StartsWith($Root, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "payload manifest path escapes the install directory: $RelativePath"
    }

    return [pscustomobject]@{
        relative = $Normalized
        full = $FullPath
    }
}

function Get-ProcessRecord([int]$ProcessId) {
    $Process = Get-ProcessByIdOrNull $ProcessId
    if (-not $Process) { return [pscustomobject]@{ id = $ProcessId; running = $false; path = $null } }
    $Path = $null
    try { $Path = $Process.Path } catch { $Path = $null }
    return [pscustomobject]@{ id = $ProcessId; running = $true; path = $Path }
}

function New-ProcessIdentity([System.Diagnostics.Process]$Process, [string]$ExpectedPath) {
    $Process.Refresh()
    $ActualPath = [System.IO.Path]::GetFullPath($Process.Path)
    $CanonicalExpectedPath = [System.IO.Path]::GetFullPath($ExpectedPath)
    if (-not [string]::Equals($ActualPath, $CanonicalExpectedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "started process path mismatch: $ActualPath; expected: $CanonicalExpectedPath"
    }
    return [pscustomobject]@{
        id = $Process.Id
        path = $CanonicalExpectedPath
        startTimeUtcTicks = $Process.StartTime.ToUniversalTime().Ticks
    }
}

function Get-ProcessIdentityState([object]$Identity) {
    $Process = Get-ProcessByIdOrNull ([int]$Identity.id)
    if (-not $Process) {
        return [pscustomobject]@{
            id = $Identity.id
            originalRunning = $false
            identityMatches = $false
            currentPath = $null
            currentStartTimeUtcTicks = $null
        }
    }
    $CurrentPath = [System.IO.Path]::GetFullPath($Process.Path)
    $CurrentStartTimeUtcTicks = $Process.StartTime.ToUniversalTime().Ticks
    $Matches = [string]::Equals($CurrentPath, [string]$Identity.path, [System.StringComparison]::OrdinalIgnoreCase) -and
        $CurrentStartTimeUtcTicks -eq [long]$Identity.startTimeUtcTicks
    return [pscustomobject]@{
        id = $Identity.id
        originalRunning = $Matches
        identityMatches = $Matches
        currentPath = $CurrentPath
        currentStartTimeUtcTicks = $CurrentStartTimeUtcTicks
    }
}

function Stop-StartedProcesses {
    foreach ($Process in @($Started)) {
        try {
            if (-not $Process.HasExited) {
                Stop-Process -InputObject $Process -Force -ErrorAction Stop
                if (-not $Process.WaitForExit(10000)) {
                    throw "process $($Process.Id) did not exit after Stop-Process"
                }
            }
        } catch {
            $Failures.Add("cleanup failed for process $($Process.Id): $($_.Exception.Message)")
        }
    }
    $Started.Clear()
}

function Invoke-BoundedProcess(
    [string]$FilePath,
    [string[]]$ArgumentList,
    [string]$Scenario,
    [int]$TimeoutMilliseconds = 300000
) {
    $Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $Process = Start-Process -FilePath $FilePath -ArgumentList $ArgumentList -PassThru
    $Started.Add($Process)
    $null = $Process.Handle
    $Exited = $Process.WaitForExit($TimeoutMilliseconds)
    $TerminationSucceeded = $true
    $TaskkillExitCode = $null
    if (-not $Exited) {
        $TaskkillOutput = @(& taskkill.exe /PID $Process.Id /T /F 2>&1)
        $TaskkillExitCode = $LASTEXITCODE
        $TaskkillOutput |
            Set-Content -LiteralPath (Join-Path $EvidenceDirectory "$Scenario-taskkill.log") -Encoding UTF8
        $TerminationSucceeded = $TaskkillExitCode -eq 0 -and $Process.WaitForExit(10000)
        Add-Check "$Scenario timed-out process tree terminates" $TerminationSucceeded `
            "taskkill exit $TaskkillExitCode" 'taskkill exit 0 and process exited'
    }
    $Stopwatch.Stop()
    $ExitCode = if ($Exited) { $Process.ExitCode } else { $null }
    if ($Process.HasExited) { $null = $Started.Remove($Process) }
    $Record = [pscustomobject]@{
        scenario = $Scenario
        elapsedSeconds = [math]::Round($Stopwatch.Elapsed.TotalSeconds, 3)
        exitedWithinTimeout = $Exited
        timeoutSeconds = $TimeoutMilliseconds / 1000
        exitCode = $ExitCode
        taskkillExitCode = $TaskkillExitCode
        terminationSucceeded = $TerminationSucceeded
    }
    $Record | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory "$Scenario-process.json") -Encoding UTF8
    return $Record
}

function New-SleeperFixture {
    if ([string]::IsNullOrWhiteSpace($env:SystemRoot)) {
        throw 'SystemRoot is required to resolve Windows PowerShell 5.1'
    }
    $WindowsPowerShellPath = Join-Path `
        $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $WindowsPowerShellPath = [System.IO.Path]::GetFullPath($WindowsPowerShellPath)
    if (-not (Test-Path -LiteralPath $WindowsPowerShellPath -PathType Leaf)) {
        throw "Windows PowerShell 5.1 executable is missing: $WindowsPowerShellPath"
    }
    if (Test-ReparsePoint $WindowsPowerShellPath) {
        throw "Windows PowerShell 5.1 executable must not be a reparse point: $WindowsPowerShellPath"
    }

    $CompileScriptPath = Join-Path $FixtureDirectory 'compile-sleeper.ps1'
    $SleeperPath = Join-Path $FixtureDirectory 'sleeper.exe'
    $CompilerMetadataPath = Join-Path $EvidenceDirectory 'sleeper-compiler.json'
    $CompileScript = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutputAssembly,
    [Parameter(Mandatory = $true)]
    [string]$MetadataPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type -OutputType ConsoleApplication -OutputAssembly $OutputAssembly -TypeDefinition @"
public static class Sleeper {
    public static void Main() { System.Threading.Thread.Sleep(600000); }
}
"@

[ordered]@{
    PSEdition = [string]$PSVersionTable.PSEdition
    PSVersion = [string]$PSVersionTable.PSVersion
} | ConvertTo-Json | Set-Content -LiteralPath $MetadataPath -Encoding UTF8
'@
    [System.IO.File]::WriteAllText(
        $CompileScriptPath,
        $CompileScript,
        [System.Text.UTF8Encoding]::new($false)
    )
    $CompileScriptItem = Get-Item -LiteralPath $CompileScriptPath -Force
    if (-not $CompileScriptItem.PSIsContainer -and
        -not ($CompileScriptItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        $CompileArguments = @(
            '-NoLogo',
            '-NoProfile',
            '-NonInteractive',
            '-ExecutionPolicy', 'Bypass',
            '-File', "`"$CompileScriptPath`"",
            '-OutputAssembly', "`"$SleeperPath`"",
            '-MetadataPath', "`"$CompilerMetadataPath`""
        )
    } else {
        throw "sleeper compile script is not a regular file: $CompileScriptPath"
    }

    $CompileRecord = Invoke-BoundedProcess -FilePath $WindowsPowerShellPath `
        -ArgumentList $CompileArguments -Scenario 'compile-sleeper-fixture' `
        -TimeoutMilliseconds 60000
    Add-Check 'sleeper fixture compilation exits within 60 seconds' `
        $CompileRecord.exitedWithinTimeout $CompileRecord.elapsedSeconds '< 60'
    Add-Check 'sleeper fixture compilation exits successfully' `
        ($CompileRecord.exitedWithinTimeout -and $CompileRecord.exitCode -eq 0) `
        $CompileRecord.exitCode 0
    if (-not $CompileRecord.exitedWithinTimeout -or $CompileRecord.exitCode -ne 0) {
        throw 'Windows PowerShell 5.1 sleeper compilation failed'
    }

    if (-not (Test-Path -LiteralPath $CompilerMetadataPath -PathType Leaf)) {
        throw "sleeper compiler metadata is missing: $CompilerMetadataPath"
    }
    $CompilerMetadata = Get-Content -LiteralPath $CompilerMetadataPath -Raw | ConvertFrom-Json
    Add-Check 'sleeper compiler PSEdition' `
        ([string]$CompilerMetadata.PSEdition -ceq 'Desktop') `
        ([string]$CompilerMetadata.PSEdition) 'Desktop'
    $CompilerVersion = [version]([string]$CompilerMetadata.PSVersion)
    Add-Check 'sleeper compiler PowerShell version' `
        ($CompilerVersion.Major -eq 5 -and $CompilerVersion.Minor -eq 1) `
        ([string]$CompilerMetadata.PSVersion) '5.1'

    $SleeperExists = Test-Path -LiteralPath $SleeperPath -PathType Leaf
    $SleeperReparsePoint = if ($SleeperExists) { Test-ReparsePoint $SleeperPath } else { $false }
    Add-Check 'sleeper fixture is a regular non-reparse file' `
        ($SleeperExists -and -not $SleeperReparsePoint) `
        "exists=$SleeperExists; reparsePoint=$SleeperReparsePoint" `
        'exists=True; reparsePoint=False'
    if ([string]$CompilerMetadata.PSEdition -cne 'Desktop' -or
        $CompilerVersion.Major -ne 5 -or $CompilerVersion.Minor -ne 1 -or
        -not $SleeperExists -or $SleeperReparsePoint) {
        throw 'Windows PowerShell 5.1 sleeper compilation validation failed'
    }

    $Probe = $null
    try {
        $Probe = Start-Process -FilePath $SleeperPath -PassThru -WindowStyle Hidden
        $Started.Add($Probe)
        $ProbeIdentity = New-ProcessIdentity -Process $Probe -ExpectedPath $SleeperPath
        Start-Sleep -Milliseconds 250
        $ProbeState = Get-ProcessIdentityState $ProbeIdentity
        Add-Check 'sleeper fixture starts as a long-running console process' `
            $ProbeState.identityMatches $ProbeState 'same PID, creation time, and executable path'
        if (-not $ProbeState.identityMatches) {
            throw 'compiled sleeper fixture did not remain running with its original identity'
        }
    } finally {
        if ($Probe) {
            if (-not $Probe.HasExited) {
                Stop-Process -InputObject $Probe -Force -ErrorAction Stop
                if (-not $Probe.WaitForExit(10000)) {
                    throw 'sleeper fixture launch probe did not stop'
                }
            }
            $null = $Started.Remove($Probe)
        }
    }
    return $SleeperPath
}

function Invoke-Installer([string]$Scenario) {
    $Record = Invoke-BoundedProcess -FilePath $InstallerPath `
        -ArgumentList @('/S', "/D=$InstallDirectory") -Scenario $Scenario
    Add-Check "$Scenario installer exits within 300 seconds" $Record.exitedWithinTimeout $Record.elapsedSeconds '< 300'
    Add-Check "$Scenario installer exits successfully" `
        ($Record.exitedWithinTimeout -and $Record.exitCode -eq 0) $Record.exitCode 0
    return $Record
}

function Test-InstalledPayload([string]$Scenario) {
    $Rows = @(
        foreach ($Entry in $Manifest.files) {
            $RelativePath = [string]$Entry.path
            $Resolved = Resolve-PayloadPath $RelativePath
            $Path = $Resolved.full
            $Present = Test-Path -LiteralPath $Path -PathType Leaf
            $ActualHash = if ($Present) { Get-Sha256 $Path } else { $null }
            $ActualSize = if ($Present) { (Get-Item -LiteralPath $Path).Length } else { $null }
            $IsReparsePoint = if ($Present) {
                [bool]((Get-Item -LiteralPath $Path).Attributes -band [System.IO.FileAttributes]::ReparsePoint)
            } else {
                $false
            }
            $ExpectedHash = ([string]$Entry.sha256).ToLowerInvariant()
            $ExpectedSize = [long]$Entry.size
            $Matches = $Present -and -not $IsReparsePoint -and `
                $ActualHash -eq $ExpectedHash -and $ActualSize -eq $ExpectedSize
            Add-Check "$Scenario payload $RelativePath" $Matches `
                "$ActualHash/$ActualSize" "$ExpectedHash/$ExpectedSize"
            [pscustomobject]@{
                file = $RelativePath
                present = $Present
                sha256 = $ActualHash
                size = $ActualSize
                expectedSha256 = $ExpectedHash
                expectedSize = $ExpectedSize
                reparsePoint = $IsReparsePoint
                matches = $Matches
            }
        }
    )
    $Rows | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory "$Scenario-payload.json") -Encoding UTF8
    $script:PayloadRows = $Rows
}

function Read-PayloadManifest {
    if (-not (Test-Path -LiteralPath $PayloadManifestPath -PathType Leaf)) {
        throw "payload manifest does not exist: $PayloadManifestPath"
    }
    try {
        $Parsed = Get-Content -LiteralPath $PayloadManifestPath -Raw | ConvertFrom-Json
    } catch {
        throw "payload manifest is not valid JSON: $($_.Exception.Message)"
    }
    if ($Parsed.schemaVersion -ne 1) { throw 'payload manifest schemaVersion must be 1' }
    if ([string]$Parsed.version -ne $ExpectedVersion) { throw 'payload manifest version does not match ExpectedVersion' }
    if ([string]$Parsed.sourceSha -cne $SourceSha) { throw 'payload manifest sourceSha does not match SourceSha' }
    if (-not $Parsed.files -or @($Parsed.files).Count -lt 1) { throw 'payload manifest files must be non-empty' }

    $Seen = @{}
    foreach ($Entry in @($Parsed.files)) {
        $RelativePath = [string]$Entry.path
        $Resolved = Resolve-PayloadPath $RelativePath
        $Hash = [string]$Entry.sha256
        $Size = 0L
        if ($Seen.ContainsKey($Resolved.relative)) { throw "payload manifest contains a duplicate path: $RelativePath" }
        $Seen[$Resolved.relative] = $true
        if ($Hash -cnotmatch '^[0-9a-f]{64}$') { throw "payload manifest contains an invalid SHA-256 for $RelativePath" }
        if (-not [long]::TryParse([string]$Entry.size, [ref]$Size) -or $Size -le 0) {
            throw "payload manifest contains an invalid size for $RelativePath"
        }
    }
    foreach ($Required in @('citrate-core.exe') + $SidecarNames) {
        if (-not $Seen.ContainsKey($Required)) { throw "payload manifest is missing required file: $Required" }
    }
    return $Parsed
}

try {
    foreach ($RequiredText in @($InstallerPath, $ExpectedVersion, $SourceSha, $PayloadManifestPath)) {
        if ([string]::IsNullOrWhiteSpace($RequiredText)) { throw 'required string parameter is empty' }
    }
    if ($ExpectedVersion -cne '0.5.1') { throw 'ExpectedVersion must be exactly 0.5.1' }
    if ($SourceSha -cne 'b521d98e80217346bcb68294301ded129d0f1846') {
        throw 'SourceSha must be the approved v0.5.1 Windows source commit'
    }
    if ($ExpectedInstallerSha256 -and $ExpectedInstallerSha256 -cnotmatch '^[0-9a-f]{64}$') {
        throw 'ExpectedInstallerSha256 must be a lowercase 64-character SHA-256'
    }
    if ($null -ne $ExpectedInstallerSize -and $ExpectedInstallerSize -le 0) {
        throw 'ExpectedInstallerSize must be greater than zero'
    }

    $InstallerPath = [System.IO.Path]::GetFullPath($InstallerPath)
    $PayloadManifestPath = [System.IO.Path]::GetFullPath($PayloadManifestPath)
    $InstallRoot = $InstallDirectory.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    foreach ($InputPath in @($InstallerPath, $PayloadManifestPath, $EvidenceDirectory)) {
        if ($InputPath.StartsWith($InstallRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "release test input must be outside the disposable install directory: $InputPath"
        }
    }
    if (-not (Test-Path -LiteralPath $InstallerPath -PathType Leaf)) { throw "installer does not exist: $InstallerPath" }
    if (-not (Test-Path -LiteralPath $PayloadManifestPath -PathType Leaf)) {
        throw "payload manifest does not exist: $PayloadManifestPath"
    }
    foreach ($InputPath in @($InstallerPath, $PayloadManifestPath)) {
        if (Test-ReparsePoint $InputPath) { throw "release test input must not be a reparse point: $InputPath" }
    }
    if (Test-Path -LiteralPath $InputDirectory) { throw "private input staging already exists: $InputDirectory" }
    New-Item -ItemType Directory -Path $InputDirectory | Out-Null
    $InputDirectoryOwned = $true
    $StagedInstallerPath = Join-Path $InputDirectory 'installer.exe'
    $StagedManifestPath = Join-Path $InputDirectory 'payload-manifest.json'
    Copy-Item -LiteralPath $InstallerPath -Destination $StagedInstallerPath
    Copy-Item -LiteralPath $PayloadManifestPath -Destination $StagedManifestPath
    foreach ($StagedPath in @($StagedInstallerPath, $StagedManifestPath)) {
        if (Test-ReparsePoint $StagedPath) { throw "private staged input is a reparse point: $StagedPath" }
    }
    $InstallerPath = [System.IO.Path]::GetFullPath($StagedInstallerPath)
    $PayloadManifestPath = [System.IO.Path]::GetFullPath($StagedManifestPath)
    $InstallerReadLock = [System.IO.File]::Open(
        $InstallerPath,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::Read
    )
    $Manifest = Read-PayloadManifest
    $EvidenceManifestPath = Join-Path $EvidenceDirectory 'payload-manifest.json'
    if (Test-Path -LiteralPath $EvidenceManifestPath) { throw "evidence collision: $EvidenceManifestPath" }
    Copy-Item -LiteralPath $PayloadManifestPath -Destination $EvidenceManifestPath

    $Installer = Get-Item -LiteralPath $InstallerPath
    $InstallerHash = Get-Sha256 $InstallerPath
    $Signature = Get-AuthenticodeSignature -LiteralPath $InstallerPath
    $InstallerRecord = [pscustomobject]@{
        path = $InstallerPath
        size = $Installer.Length
        sha256 = $InstallerHash
        authenticode = [string]$Signature.Status
        productVersion = $Installer.VersionInfo.ProductVersion
        expectedVersion = $ExpectedVersion
        sourceSha = $SourceSha
    }
    $InstallerRecord | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'installer-identity.json') -Encoding UTF8
    if ($ExpectedInstallerSha256) {
        Add-Check 'installer SHA-256' ($InstallerHash -ceq $ExpectedInstallerSha256) $InstallerHash $ExpectedInstallerSha256
    }
    if ($null -ne $ExpectedInstallerSize) {
        Add-Check 'installer size' ($Installer.Length -eq $ExpectedInstallerSize) $Installer.Length $ExpectedInstallerSize
    }
    Add-Check 'installer Authenticode status' ([string]$Signature.Status -eq 'NotSigned') ([string]$Signature.Status) 'NotSigned'
    Add-Check 'installer product version' ($Installer.VersionInfo.ProductVersion -eq $ExpectedVersion) $Installer.VersionInfo.ProductVersion $ExpectedVersion
    if ($Failures.Count -gt 0) { throw 'installer identity did not match the expected release inputs' }

    if (Test-Path -LiteralPath $FixtureDirectory) { throw "fixture directory already exists: $FixtureDirectory" }
    New-Item -ItemType Directory -Path $FixtureDirectory | Out-Null
    $FixtureDirectoryOwned = $true
    $Sleeper = New-SleeperFixture

    if (Test-Path -LiteralPath $InstallDirectory) { throw "fresh install directory already exists: $InstallDirectory" }
    Add-Check 'fresh baseline is absent' (-not (Test-Path -LiteralPath $InstallDirectory)) `
        (Test-Path -LiteralPath $InstallDirectory) $false

    $InstallDirectoryOwned = $true
    $null = Invoke-Installer 'fresh-install'
    Add-Check 'fresh install directory exists' `
        (Test-Path -LiteralPath $InstallDirectory -PathType Container) `
        (Test-Path -LiteralPath $InstallDirectory -PathType Container) $true
    if (Test-Path -LiteralPath $InstallDirectory -PathType Container) {
        Add-Check 'fresh install directory is not a reparse point' `
            (-not (Test-ReparsePoint $InstallDirectory)) `
            (Test-ReparsePoint $InstallDirectory) $false
    }
    Test-InstalledPayload 'fresh-install'
    if ($Failures.Count -gt 0) { throw 'fresh-install validation failed' }

    $ProductAcceptance = Join-Path $PSScriptRoot 'test-installed-product.ps1'
    if (-not (Test-Path -LiteralPath $ProductAcceptance -PathType Leaf)) {
        throw "installed-product acceptance script is missing: $ProductAcceptance"
    }
    & $ProductAcceptance -InstallDirectory $InstallDirectory `
        -EvidenceDirectory $EvidenceDirectory -RuntimeRoot $RuntimeRoot
    Add-Check 'installed product acceptance' $true `
        'render/import-resume/node/comms/supervisor passed' `
        'render/import-resume/node/comms/supervisor passed'

    $DecoyDirectory = Join-Path $FixtureDirectory 'decoy'
    New-Item -ItemType Directory -Path $DecoyDirectory | Out-Null
    $OwnProcesses = @{}
    foreach ($Name in $SidecarNames) {
        $Path = Join-Path $InstallDirectory $Name
        Copy-Item -LiteralPath $Sleeper -Destination $Path -Force
        $Process = Start-Process -FilePath $Path -PassThru -WindowStyle Hidden
        $Started.Add($Process)
        $OwnProcesses[$Name] = New-ProcessIdentity -Process $Process -ExpectedPath $Path
    }
    $DecoyPath = Join-Path $DecoyDirectory 'hermes.exe'
    Copy-Item -LiteralPath $Sleeper -Destination $DecoyPath
    $Decoy = Start-Process -FilePath $DecoyPath -PassThru -WindowStyle Hidden
    $Started.Add($Decoy)
    $DecoyIdentity = New-ProcessIdentity -Process $Decoy -ExpectedPath $DecoyPath
    Start-Sleep -Seconds 1

    $Before = [pscustomobject]@{
        own = @($OwnProcesses.Keys | ForEach-Object { Get-ProcessIdentityState $OwnProcesses[$_] })
        decoy = Get-ProcessIdentityState $DecoyIdentity
    }
    $Before | ConvertTo-Json -Depth 5 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'running-reinstall-before-processes.json') -Encoding UTF8
    foreach ($Name in $SidecarNames) {
        $State = Get-ProcessIdentityState $OwnProcesses[$Name]
        Add-Check "running fixture $Name started" `
            $State.identityMatches $State 'same PID, creation time, and executable path'
    }

    $null = Invoke-Installer 'running-reinstall'
    Start-Sleep -Seconds 1
    $After = [pscustomobject]@{
        own = @($OwnProcesses.Keys | ForEach-Object { Get-ProcessIdentityState $OwnProcesses[$_] })
        decoy = Get-ProcessIdentityState $DecoyIdentity
    }
    $After | ConvertTo-Json -Depth 5 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'running-reinstall-after-processes.json') -Encoding UTF8
    foreach ($Name in $SidecarNames) {
        $State = Get-ProcessIdentityState $OwnProcesses[$Name]
        Add-Check "running reinstall stops owned $Name" `
            (-not $State.originalRunning) $State 'original process identity not running'
    }
    $DecoyState = Get-ProcessIdentityState $DecoyIdentity
    Add-Check 'running reinstall leaves same-name decoy alive' `
        $DecoyState.identityMatches $DecoyState 'same PID, creation time, and executable path'
    if (Test-Path -LiteralPath $InstallDirectory -PathType Container) {
        Add-Check 'reinstalled directory is not a reparse point' `
            (-not (Test-ReparsePoint $InstallDirectory)) `
            (Test-ReparsePoint $InstallDirectory) $false
    }
    if ($Failures.Count -gt 0) { throw 'running-reinstall process validation failed' }
    Test-InstalledPayload 'running-reinstall'

    $HookProcesses = @(
        Get-CimInstance Win32_Process | Where-Object {
            $_.CommandLine -and $_.CommandLine -like '*citrate-stop-own-sidecars.ps1*'
        } | Select-Object ProcessId, Name, ExecutablePath, CommandLine
    )
    ConvertTo-Json -InputObject @($HookProcesses) -Depth 4 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'remaining-hook-processes.json') -Encoding UTF8
    Add-Check 'no installer hook process remains' ($HookProcesses.Count -eq 0) $HookProcesses.Count 0

    $Uninstaller = Join-Path $InstallDirectory 'uninstall.exe'
    Add-Check 'uninstaller exists' (Test-Path -LiteralPath $Uninstaller -PathType Leaf) `
        (Test-Path -LiteralPath $Uninstaller -PathType Leaf) $true
    if (Test-Path -LiteralPath $Uninstaller -PathType Leaf) {
        Add-Check 'uninstaller is not a reparse point' (-not (Test-ReparsePoint $Uninstaller)) `
            (Test-ReparsePoint $Uninstaller) $false
        if ($Failures.Count -gt 0) { throw 'uninstaller identity validation failed' }
        $UninstallRecord = Invoke-BoundedProcess -FilePath $Uninstaller -ArgumentList @('/S') -Scenario 'uninstall'
        Add-Check 'uninstaller exits within 300 seconds' $UninstallRecord.exitedWithinTimeout $UninstallRecord.elapsedSeconds '< 300'
        Add-Check 'uninstaller exits successfully' `
            ($UninstallRecord.exitedWithinTimeout -and $UninstallRecord.exitCode -eq 0) $UninstallRecord.exitCode 0
        Start-Sleep -Seconds 2
        Add-Check 'uninstall removes installation directory' (-not (Test-Path -LiteralPath $InstallDirectory)) `
            (Test-Path -LiteralPath $InstallDirectory) $false
    }
} catch {
    $CaughtError = $_.Exception.Message
    $Failures.Add("unhandled error: $CaughtError")
} finally {
    Stop-StartedProcesses
    if ($InstallerReadLock) {
        try {
            $InstallerReadLock.Dispose()
        } catch {
            $Failures.Add("installer read-lock cleanup failed: $($_.Exception.Message)")
        }
    }
    foreach ($Cleanup in @(
        [pscustomobject]@{ path = $FixtureDirectory; label = 'fixture directory'; owned = $FixtureDirectoryOwned },
        [pscustomobject]@{ path = $InputDirectory; label = 'private input directory'; owned = $InputDirectoryOwned },
        [pscustomobject]@{ path = $InstallDirectory; label = 'install directory'; owned = $InstallDirectoryOwned }
    )) {
        if (-not $Cleanup.owned) { continue }
        try {
            Remove-OwnedTestDirectory -Path $Cleanup.path -Label $Cleanup.label
        } catch {
            $Failures.Add("$($Cleanup.label) cleanup failed: $($_.Exception.Message)")
        }
    }
    $Result = [pscustomobject]@{
        schemaVersion = 1
        startedUtc = $StartedUtc
        finishedUtc = (Get-Date).ToUniversalTime().ToString('o')
        expectedVersion = $ExpectedVersion
        sourceSha = $SourceSha
        runner = [pscustomobject]@{
            os = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
            osArchitecture = [string][System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
            powershellVersion = [string]$PSVersionTable.PSVersion
            user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
        }
        installDirectory = $InstallDirectory
        checks = $Checks
        payload = $PayloadRows
        failureCount = $Failures.Count
        failures = $Failures
        error = $CaughtError
        authenticodeExpected = 'NotSigned'
        certification = 'not certified'
        guiChecks = 'rendered installed WebView captured and pixel-checked through loopback CDP'
    }
    $Result | ConvertTo-Json -Depth 8 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'result.json') -Encoding UTF8
}

if ($Failures.Count -gt 0) {
    $Failures | ForEach-Object { Write-Host "FAIL - $_" }
    exit 1
}
Write-Host 'All hosted release-installer smoke checks passed.'
exit 0
