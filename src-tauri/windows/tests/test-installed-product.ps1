# Real installed-product acceptance for the Windows release candidate.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallDirectory,
    [Parameter(Mandatory = $true)]
    [string]$EvidenceDirectory,
    [Parameter(Mandatory = $true)]
    [string]$RuntimeRoot
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$InstallDirectory = [System.IO.Path]::GetFullPath($InstallDirectory)
$EvidenceDirectory = [System.IO.Path]::GetFullPath($EvidenceDirectory)
$RuntimeRoot = [System.IO.Path]::GetFullPath($RuntimeRoot)
$AppPath = Join-Path $InstallDirectory 'citrate-core.exe'
$MemPath = Join-Path $InstallDirectory 'mem-mcp.exe'
$NodePath = Join-Path $InstallDirectory 'citrate.exe'
$CommsPath = Join-Path $InstallDirectory 'comms-member-daemon.exe'
$AppIdentifier = 'ai.citrate.core'
$RoamingKnownFolder = [Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)
$LocalKnownFolder = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
$AppDataDirectory = $null
$LocalAppDataDirectory = $null
$MemoryDirectory = $null
$ImportProgressPath = $null
$ImportMarkerPath = $null
$AppDataDirectoryOwned = $false
$LocalAppDataDirectoryOwned = $false
$Acceptance = [ordered]@{
    schemaVersion = 1
    startedUtc = (Get-Date).ToUniversalTime().ToString('o')
    passed = $false
    error = $null
    renderedApplication = $null
    interruptedImport = $null
    resumedImport = $null
    node = $null
    comms = $null
    knownFolders = [ordered]@{
        roaming = $RoamingKnownFolder
        local = $LocalKnownFolder
        roamingAppDirectory = $null
        localAppDirectory = $null
        memoryDirectory = $null
    }
    cleanup = [ordered]@{
        processTrees = [ordered]@{
            attempted = $false
            succeeded = $false
            error = $null
        }
        roamingAppDirectory = [ordered]@{
            path = $null
            owned = $false
            attempted = $false
            removed = $false
            outcome = 'not-started'
            error = $null
        }
        localAppDirectory = [ordered]@{
            path = $null
            owned = $false
            attempted = $false
            removed = $false
            outcome = 'not-started'
            error = $null
        }
    }
}
$App = $null
$Comms = $null

function Assert-DirectKnownFolderAppPath(
    [string]$KnownFolder,
    [string]$AppDirectory,
    [string]$Label
) {
    if ([string]::IsNullOrWhiteSpace($KnownFolder)) {
        throw "$Label Known Folder path is empty"
    }
    $CanonicalKnownFolder = [System.IO.Path]::GetFullPath($KnownFolder).TrimEnd('\', '/')
    if (-not (Test-Path -LiteralPath $CanonicalKnownFolder -PathType Container)) {
        throw "$Label Known Folder parent does not exist: $CanonicalKnownFolder"
    }
    if ([string]::IsNullOrWhiteSpace($AppDirectory)) {
        throw "$Label app directory path is empty"
    }
    $CanonicalAppDirectory = [System.IO.Path]::GetFullPath($AppDirectory).TrimEnd('\', '/')
    $ActualParent = [System.IO.Path]::GetDirectoryName($CanonicalAppDirectory)
    $ActualLeaf = [System.IO.Path]::GetFileName($CanonicalAppDirectory)
    if (-not [string]::Equals(
        $ActualParent,
        $CanonicalKnownFolder,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
        throw "$Label app directory is not a direct child of its Known Folder: $CanonicalAppDirectory"
    }
    if ($ActualLeaf -cne $AppIdentifier) {
        throw "$Label app directory has an unexpected leaf name: $CanonicalAppDirectory"
    }
    return $CanonicalAppDirectory
}

function New-OwnedAppDirectory([string]$Path, [string]$Label) {
    New-Item -ItemType Directory -Path $Path -ErrorAction Stop | Out-Null
    $Item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (-not $Item.PSIsContainer) {
        throw "$Label app path is not a directory after creation: $Path"
    }
    if ($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        throw "$Label app directory is a reparse point after creation: $Path"
    }
}

function Remove-OwnedKnownFolderAppDirectory(
    [string]$Path,
    [string]$KnownFolder,
    [string]$Label
) {
    if (-not (Test-Path -LiteralPath $Path)) { return $false }
    $CanonicalPath = Assert-DirectKnownFolderAppPath `
        -KnownFolder $KnownFolder -AppDirectory $Path -Label $Label
    if (-not (Test-Path -LiteralPath $CanonicalPath -PathType Container)) {
        throw "$Label cleanup target is not a directory: $CanonicalPath"
    }
    $Item = Get-Item -LiteralPath $CanonicalPath -Force -ErrorAction Stop
    if (-not $Item.PSIsContainer) {
        throw "$Label cleanup target is not a container: $CanonicalPath"
    }
    if ($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        throw "$Label cleanup target is a reparse point: $CanonicalPath"
    }
    Remove-Item -LiteralPath $CanonicalPath -Recurse -Force -ErrorAction Stop
    if (Test-Path -LiteralPath $CanonicalPath) {
        throw "$Label app directory cleanup failed: $CanonicalPath"
    }
    return $true
}

function Get-FreeLoopbackPort {
    $Listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $Listener.Start()
    try { return ([System.Net.IPEndPoint]$Listener.LocalEndpoint).Port } finally { $Listener.Stop() }
}

function Get-ExactProcesses([string]$ExecutablePath) {
    $Canonical = [System.IO.Path]::GetFullPath($ExecutablePath)
    return @(
        Get-CimInstance Win32_Process | Where-Object {
            $_.ExecutablePath -and
            [string]::Equals(
                [System.IO.Path]::GetFullPath([string]$_.ExecutablePath),
                $Canonical,
                [System.StringComparison]::OrdinalIgnoreCase
            )
        }
    )
}

function Test-ImportProcess([object]$ProcessRow) {
    return $ProcessRow.CommandLine -and
        [regex]::IsMatch(
            [string]$ProcessRow.CommandLine,
            '(?i)(?:^|\s)import-corpus(?:\s|$)'
        )
}

function Get-ImportProcesses {
    return @(Get-ExactProcesses $MemPath | Where-Object { Test-ImportProcess $_ })
}

function Get-MemoryDaemonProcesses {
    return @(Get-ExactProcesses $MemPath | Where-Object { -not (Test-ImportProcess $_) })
}

function Wait-NoImportProcess([int]$TimeoutSeconds = 30) {
    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Rows = @(Get-ImportProcesses)
        if ($Rows.Count -eq 0) { return }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $Deadline)
    throw "knowledge importer remained after shutdown: PIDs $($Rows.ProcessId -join ',')"
}

function Wait-MemoryDaemonProcess([int]$TimeoutSeconds = 60) {
    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Importers = @(Get-ImportProcesses)
        if ($Importers.Count -ne 0) {
            throw "knowledge importer still runs after completion: PIDs $($Importers.ProcessId -join ',')"
        }
        $Daemons = @(Get-MemoryDaemonProcesses)
        if ($Daemons.Count -eq 1) { return $Daemons[0] }
        if ($Daemons.Count -gt 1) {
            throw "production memory autostart launched multiple daemons: PIDs $($Daemons.ProcessId -join ',')"
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $Deadline)
    throw 'production memory autostart did not launch the long-running daemon after import completion'
}

function Wait-NoExactProcess([string]$ExecutablePath, [int]$TimeoutSeconds = 30) {
    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Rows = @(Get-ExactProcesses $ExecutablePath)
        if ($Rows.Count -eq 0) { return }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $Deadline)
    throw "process remained after shutdown: $ExecutablePath (PIDs $($Rows.ProcessId -join ','))"
}

function Invoke-CdpCommand(
    [string]$WebSocketUrl,
    [string]$Method,
    [hashtable]$Parameters,
    [int]$TimeoutSeconds = 60
) {
    $Socket = [System.Net.WebSockets.ClientWebSocket]::new()
    $Cancellation = [System.Threading.CancellationTokenSource]::new()
    $Cancellation.CancelAfter([TimeSpan]::FromSeconds($TimeoutSeconds))
    try {
        $WebSocketUri = [Uri]$WebSocketUrl
        $Socket.Options.SetRequestHeader('Origin', "http://127.0.0.1:$($WebSocketUri.Port)")
        $Socket.ConnectAsync($WebSocketUri, $Cancellation.Token).GetAwaiter().GetResult()
        $Request = @{
            id = 1
            method = $Method
            params = $Parameters
        } | ConvertTo-Json -Depth 20 -Compress
        $Bytes = [System.Text.Encoding]::UTF8.GetBytes($Request)
        $Socket.SendAsync(
            [ArraySegment[byte]]::new($Bytes),
            [System.Net.WebSockets.WebSocketMessageType]::Text,
            $true,
            $Cancellation.Token
        ).GetAwaiter().GetResult()

        $Buffer = New-Object byte[] 65536
        while ($true) {
            $Message = [System.IO.MemoryStream]::new()
            do {
                $Received = $Socket.ReceiveAsync(
                    [ArraySegment[byte]]::new($Buffer),
                    $Cancellation.Token
                ).GetAwaiter().GetResult()
                if ($Received.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
                    throw "CDP socket closed while waiting for $Method"
                }
                $Message.Write($Buffer, 0, $Received.Count)
            } while (-not $Received.EndOfMessage)
            $Json = [System.Text.Encoding]::UTF8.GetString($Message.ToArray()) | ConvertFrom-Json
            if (($Json.PSObject.Properties.Name -contains 'id') -and $Json.id -eq 1) {
                if ($Json.PSObject.Properties.Name -contains 'error') {
                    throw "CDP $Method failed: $($Json.error | ConvertTo-Json -Compress)"
                }
                return $Json.result
            }
        }
    } finally {
        $Cancellation.Dispose()
        $Socket.Dispose()
    }
}

function Invoke-JavaScript(
    [string]$WebSocketUrl,
    [string]$Expression,
    [bool]$AwaitPromise = $true,
    [int]$TimeoutSeconds = 60
) {
    $Result = Invoke-CdpCommand -WebSocketUrl $WebSocketUrl -Method 'Runtime.evaluate' -Parameters @{
        expression = $Expression
        awaitPromise = $AwaitPromise
        returnByValue = $true
        userGesture = $true
    } -TimeoutSeconds $TimeoutSeconds
    if ($Result.PSObject.Properties.Name -contains 'exceptionDetails') {
        throw "JavaScript evaluation failed: $($Result.exceptionDetails | ConvertTo-Json -Depth 8 -Compress)"
    }
    if (($Result.result.PSObject.Properties.Name -contains 'subtype') -and
        $Result.result.subtype -eq 'error') {
        throw "JavaScript returned an error: $($Result.result.description)"
    }
    if ($Result.result.PSObject.Properties.Name -contains 'value') {
        return $Result.result.value
    }
    return $null
}

function Invoke-TauriCommand(
    [string]$WebSocketUrl,
    [string]$Command,
    [hashtable]$Arguments = @{},
    [int]$TimeoutSeconds = 60
) {
    $CommandJson = ConvertTo-Json -InputObject $Command -Compress
    $ArgumentsJson = ConvertTo-Json -InputObject $Arguments -Depth 12 -Compress
    $Expression = "(async()=>await window.__TAURI_INTERNALS__.invoke($CommandJson,$ArgumentsJson))()"
    return Invoke-JavaScript -WebSocketUrl $WebSocketUrl -Expression $Expression `
        -AwaitPromise $true -TimeoutSeconds $TimeoutSeconds
}

function Start-InstalledApp {
    $Port = Get-FreeLoopbackPort
    $Info = [System.Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $AppPath
    $Info.WorkingDirectory = $InstallDirectory
    $Info.UseShellExecute = $false
    $Info.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] =
        "--remote-debugging-port=$Port --remote-allow-origins=http://127.0.0.1:$Port"
    $Process = [System.Diagnostics.Process]::Start($Info)
    if (-not $Process) { throw 'failed to start installed Citrate Core' }
    $null = $Process.Handle
    $Deadline = (Get-Date).AddSeconds(60)
    $LastError = $null
    do {
        if ($Process.HasExited) { throw "installed app exited before rendering: $($Process.ExitCode)" }
        try {
            $Targets = @(Invoke-RestMethod -Uri "http://127.0.0.1:$Port/json/list" -TimeoutSec 2)
            $Page = $Targets | Where-Object {
                $_.type -eq 'page' -and $_.webSocketDebuggerUrl
            } | Select-Object -First 1
            if ($Page) {
                return [pscustomobject]@{
                    process = $Process
                    webSocketUrl = [string]$Page.webSocketDebuggerUrl
                    debugPort = $Port
                }
            }
        } catch {
            $LastError = $_.Exception.Message
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $Deadline)
    throw "installed WebView did not expose its loopback CDP target: $LastError"
}

function Stop-InstalledApp([System.Diagnostics.Process]$Process) {
    if ($Process.HasExited) { return }
    if (-not $Process.CloseMainWindow()) { throw 'installed app has no closable main window' }
    if (-not $Process.WaitForExit(30000)) { throw 'installed app did not exit after WM_CLOSE' }
    Wait-NoExactProcess $MemPath
    Wait-NoExactProcess $NodePath
}

function Stop-InstalledProcessTrees {
    foreach ($Path in @($AppPath, $MemPath, $NodePath, $CommsPath)) {
        foreach ($Row in @(Get-ExactProcesses $Path)) {
            $Output = @(& taskkill.exe /PID $Row.ProcessId /T /F 2>&1)
            if ($LASTEXITCODE -ne 0) {
                throw "failed to stop acceptance process tree $($Row.ProcessId) ($Path): $($Output -join '; ')"
            }
        }
        Wait-NoExactProcess $Path
    }
}

function Save-RenderedScreenshot([string]$WebSocketUrl) {
    $Facts = Invoke-JavaScript -WebSocketUrl $WebSocketUrl -Expression @'
(() => {
  const visible = [...document.querySelectorAll('body *')].filter((el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  });
  return {
    readyState: document.readyState,
    title: document.title,
    bodyWidth: document.body.getBoundingClientRect().width,
    bodyHeight: document.body.getBoundingClientRect().height,
    visibleElements: visible.length,
    hasTauriInvoke: typeof window.__TAURI_INTERNALS__?.invoke === 'function'
  };
})()
'@
    if ($Facts.readyState -cne 'complete' -or
        $Facts.title -notlike '*Citrate*' -or
        [double]$Facts.bodyWidth -lt 800 -or
        [double]$Facts.bodyHeight -lt 500 -or
        [int]$Facts.visibleElements -lt 10 -or
        -not $Facts.hasTauriInvoke) {
        throw "installed application did not render the expected Tauri surface: $($Facts | ConvertTo-Json -Compress)"
    }
    $Capture = Invoke-CdpCommand -WebSocketUrl $WebSocketUrl -Method 'Page.captureScreenshot' `
        -Parameters @{ format = 'png'; fromSurface = $true } -TimeoutSeconds 60
    $ScreenshotPath = Join-Path $EvidenceDirectory 'installed-application.png'
    [System.IO.File]::WriteAllBytes($ScreenshotPath, [Convert]::FromBase64String([string]$Capture.data))
    Add-Type -AssemblyName System.Drawing
    $Bitmap = [System.Drawing.Bitmap]::new($ScreenshotPath)
    try {
        $Colours = [System.Collections.Generic.HashSet[int]]::new()
        for ($Y = 0; $Y -lt $Bitmap.Height; $Y += 20) {
            for ($X = 0; $X -lt $Bitmap.Width; $X += 20) {
                $null = $Colours.Add($Bitmap.GetPixel($X, $Y).ToArgb())
            }
        }
        if ($Bitmap.Width -lt 800 -or $Bitmap.Height -lt 500 -or $Colours.Count -lt 8) {
            throw "rendered screenshot is blank or undersized: $($Bitmap.Width)x$($Bitmap.Height), $($Colours.Count) sampled colours"
        }
        return [pscustomobject]@{
            facts = $Facts
            screenshot = [System.IO.Path]::GetFileName($ScreenshotPath)
            screenshotSha256 = (Get-FileHash -LiteralPath $ScreenshotPath -Algorithm SHA256).Hash.ToLowerInvariant()
            width = $Bitmap.Width
            height = $Bitmap.Height
            sampledColours = $Colours.Count
        }
    } finally {
        $Bitmap.Dispose()
    }
}

function Wait-ImportInterruptPoint([int]$TimeoutSeconds = 3600) {
    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Importers = @(Get-ImportProcesses)
        if ($Importers.Count -gt 1) {
            throw "production autostart launched multiple knowledge importers: PIDs $($Importers.ProcessId -join ',')"
        }
        if (Test-Path -LiteralPath $ImportProgressPath -PathType Leaf) {
            try {
                $Progress = Get-Content -LiteralPath $ImportProgressPath -Raw | ConvertFrom-Json
                if ($Progress.state -eq 'failed') {
                    throw "production knowledge import failed before interruption: $($Progress | ConvertTo-Json -Compress)"
                }
                if ($Progress.state -eq 'running' -and
                    @($Progress.tenants_done).Count -gt 0 -and
                    [long]$Progress.done -gt 0 -and
                    $Importers.Count -eq 1) {
                    return [pscustomobject]@{
                        progress = $Progress
                        importerProcessId = [int]$Importers[0].ProcessId
                        importerCommandLine = [string]$Importers[0].CommandLine
                    }
                }
            } catch {
                if ($_.Exception.Message -like 'production knowledge import failed*') { throw }
                # The producer atomically renames, but retry a transient read during antivirus scans.
            }
        }
        if ($Importers.Count -eq 0 -and
            (Test-Path -LiteralPath $ImportMarkerPath -PathType Leaf)) {
            throw 'knowledge import completed before an interruption point was observed'
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $Deadline)
    throw 'knowledge import did not reach an interruptible tenant boundary within one hour'
}

function Wait-ImportResumeCompletion(
    [string[]]$ExpectedFinishedTenants,
    [int]$TimeoutSeconds = 3600
) {
    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $FirstRunningProgress = $null
    $LastRunningProgress = $null
    $ProgressObservationCount = 0
    $ImporterProcessIds = [System.Collections.Generic.HashSet[int]]::new()
    do {
        $Importers = @(Get-ImportProcesses)
        if ($Importers.Count -gt 1) {
            throw "resumed production autostart launched multiple importers: PIDs $($Importers.ProcessId -join ',')"
        }
        foreach ($Importer in $Importers) {
            $null = $ImporterProcessIds.Add([int]$Importer.ProcessId)
        }

        $Progress = $null
        if (Test-Path -LiteralPath $ImportProgressPath -PathType Leaf) {
            try {
                $Progress = Get-Content -LiteralPath $ImportProgressPath -Raw | ConvertFrom-Json
            } catch {
                # Retry a transient read while the producer atomically replaces the record.
            }
        }
        if ($Progress) {
            foreach ($Tenant in $ExpectedFinishedTenants) {
                if ($Tenant -notin @($Progress.tenants_done)) {
                    throw "resumed production import lost previously completed tenant: $Tenant"
                }
            }
            if ($Progress.state -eq 'failed') {
                throw "resumed production knowledge import failed: $($Progress | ConvertTo-Json -Compress)"
            }
            if ($Progress.state -eq 'running') {
                if ($Importers.Count -eq 1) {
                    if ($null -eq $FirstRunningProgress) { $FirstRunningProgress = $Progress }
                    $LastRunningProgress = $Progress
                    $ProgressObservationCount++
                }
            }
        }

        $MarkerExists = Test-Path -LiteralPath $ImportMarkerPath -PathType Leaf
        $ProgressExists = Test-Path -LiteralPath $ImportProgressPath
        if ($MarkerExists -and -not $ProgressExists -and $Importers.Count -eq 0) {
            if ($null -eq $FirstRunningProgress -or
                $ProgressObservationCount -lt 1 -or
                $ImporterProcessIds.Count -lt 1) {
                throw 'import completion was observed without convincing production resume evidence'
            }
            return [pscustomobject]@{
                firstRunningProgress = $FirstRunningProgress
                lastRunningProgress = $LastRunningProgress
                progressObservationCount = $ProgressObservationCount
                importerProcessIds = @($ImporterProcessIds)
                marker = (Get-Content -LiteralPath $ImportMarkerPath -Raw).Trim()
            }
        }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $Deadline)
    throw 'production knowledge-import resume did not complete within one hour'
}

function Invoke-JsonRpc([string]$Uri, [string]$Method, [object[]]$Parameters) {
    $Body = @{ jsonrpc = '2.0'; id = 1; method = $Method; params = $Parameters } |
        ConvertTo-Json -Depth 8 -Compress
    $Response = Invoke-RestMethod -Method Post -Uri $Uri -ContentType 'application/json' `
        -Body $Body -TimeoutSec 15
    if ($Response.PSObject.Properties.Name -contains 'error') {
        throw "RPC $Method failed at $Uri`: $($Response.error | ConvertTo-Json -Compress)"
    }
    return $Response.result
}

function Get-NodeRpcListenerIdentity {
    $Listeners = @(Get-NetTCPConnection -State Listen -LocalPort 8545 -ErrorAction Stop)
    if ($Listeners.Count -ne 1) {
        throw "Expected exactly one listener on port 8545, found $($Listeners.Count)"
    }

    $Listener = $Listeners[0]
    $AllowedLocalAddresses = @('127.0.0.1', '::1', '0.0.0.0', '::')
    if ([string]$Listener.LocalAddress -notin $AllowedLocalAddresses) {
        throw "Port 8545 listener address is not loopback-capable: $($Listener.LocalAddress)"
    }

    $OwnerProcessId = [long]$Listener.OwningProcess
    if ($OwnerProcessId -le 0) {
        throw 'Port 8545 listener did not expose a valid owning process'
    }
    $Owners = @(Get-CimInstance -ClassName Win32_Process `
        -Filter "ProcessId = $OwnerProcessId" -ErrorAction Stop)
    if ($Owners.Count -ne 1) {
        throw "Expected exactly one Win32_Process for port 8545 owner $OwnerProcessId, found $($Owners.Count)"
    }

    $Owner = $Owners[0]
    if ([string]::IsNullOrWhiteSpace([string]$Owner.ExecutablePath)) {
        throw "Port 8545 owner $OwnerProcessId did not expose an executable path"
    }
    if ($null -eq $Owner.CreationDate) {
        throw "Port 8545 owner $OwnerProcessId did not expose a creation identity"
    }

    $ExpectedPath = [System.IO.Path]::GetFullPath($NodePath)
    $OwnerPath = [System.IO.Path]::GetFullPath([string]$Owner.ExecutablePath)
    if (-not [string]::Equals($OwnerPath, $ExpectedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Port 8545 is owned by '$OwnerPath', expected installed node '$ExpectedPath'"
    }

    $CreationTimeUtc = ([datetime]$Owner.CreationDate).ToUniversalTime()
    return [pscustomobject]@{
        pid = $OwnerProcessId
        executablePath = $OwnerPath
        creationTimeUtc = $CreationTimeUtc.ToString('o')
        creationTimeUtcTicks = $CreationTimeUtc.Ticks
        localAddress = [string]$Listener.LocalAddress
        localPort = [int]$Listener.LocalPort
    }
}

function Test-NodeAcceptance([string]$WebSocketUrl) {
    $Deadline = (Get-Date).AddMinutes(30)
    $AllLogs = [System.Collections.Generic.HashSet[string]]::new()
    $Status = $null
    do {
        $Status = Invoke-TauriCommand -WebSocketUrl $WebSocketUrl -Command 'node_status' -TimeoutSeconds 30
        $Logs = @(Invoke-TauriCommand -WebSocketUrl $WebSocketUrl -Command 'node_logs' -TimeoutSeconds 30)
        foreach ($Log in $Logs) {
            $Line = [string]$Log.line
            if ($Line) { $null = $AllLogs.Add($Line) }
        }
        if (@($AllLogs | Where-Object { $_ -match 'state root mismatch' }).Count -gt 0) {
            throw 'installed node reported a state-root mismatch before block 2001'
        }
        if ($Status.state -eq 'running' -and [long]$Status.height -ge 2001) { break }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $Deadline)
    if (-not $Status -or $Status.state -ne 'running' -or [long]$Status.height -lt 2001) {
        throw "installed node did not reach block 2001 under supervisor health: $($Status | ConvertTo-Json -Compress)"
    }

    $ListenerBefore = Get-NodeRpcListenerIdentity
    $LocalGenesis = Invoke-JsonRpc -Uri 'http://127.0.0.1:8545' `
        -Method 'eth_getBlockByNumber' -Parameters @('0x0', $false)
    $RemoteGenesis = Invoke-JsonRpc -Uri 'https://rpc.citrate.ai' `
        -Method 'eth_getBlockByNumber' -Parameters @('0x0', $false)
    $Local2001 = Invoke-JsonRpc -Uri 'http://127.0.0.1:8545' `
        -Method 'eth_getBlockByNumber' -Parameters @('0x7d1', $false)
    $Remote2001 = Invoke-JsonRpc -Uri 'https://rpc.citrate.ai' `
        -Method 'eth_getBlockByNumber' -Parameters @('0x7d1', $false)
    $ListenerAfter = Get-NodeRpcListenerIdentity
    foreach ($Property in @('pid', 'creationTimeUtcTicks', 'localAddress', 'localPort')) {
        if ($ListenerBefore.$Property -ne $ListenerAfter.$Property) {
            throw "Port 8545 listener identity changed during consensus RPC queries: $Property"
        }
    }
    if (-not [string]::Equals(
        [string]$ListenerBefore.executablePath,
        [string]$ListenerAfter.executablePath,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
        throw 'Port 8545 listener executable changed during consensus RPC queries'
    }
    $ExpectedGenesisHash = '0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1'
    $ExpectedGenesisStateRoot = '0x01f184a07c7937bb703493f7b1f147f5327320a35473b7baf21c3d41ef6ba8cc'
    if ($LocalGenesis.hash -cne $ExpectedGenesisHash -or
        $LocalGenesis.stateRoot -cne $ExpectedGenesisStateRoot -or
        $RemoteGenesis.hash -cne $ExpectedGenesisHash -or
        $RemoteGenesis.stateRoot -cne $ExpectedGenesisStateRoot) {
        throw 'installed node or canonical RPC does not match the release-pinned genesis identity'
    }
    if ($Local2001.hash -cne $Remote2001.hash -or
        $Local2001.stateRoot -cne $Remote2001.stateRoot) {
        throw 'installed node block 2001 does not match the independent canonical RPC witness'
    }
    $VerifiedLines = @($AllLogs | Where-Object { $_ -match 'root verified' })
    if ($VerifiedLines.Count -eq 0) {
        throw 'installed node reached block 2001 without retaining any executor root-verification evidence'
    }

    $Record = [pscustomobject]@{
        supervisorStatus = $Status
        rpcListener = [pscustomobject]@{
            before = $ListenerBefore
            after = $ListenerAfter
        }
        genesis = [pscustomobject]@{
            expectedHash = $ExpectedGenesisHash
            expectedStateRoot = $ExpectedGenesisStateRoot
            local = $LocalGenesis
            canonical = $RemoteGenesis
        }
        block2001 = [pscustomobject]@{
            local = $Local2001
            canonical = $Remote2001
        }
        rootVerifiedLineCount = $VerifiedLines.Count
        stateRootMismatchLineCount = 0
        capturedLogLineCount = $AllLogs.Count
    }
    $Record | ConvertTo-Json -Depth 12 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'installed-node-consensus.json') -Encoding UTF8
    return $Record
}

function New-RandomHex([int]$ByteCount) {
    $Bytes = New-Object byte[] $ByteCount
    $Random = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $Random.GetBytes($Bytes) } finally { $Random.Dispose() }
    return [Convert]::ToHexString($Bytes).ToLowerInvariant()
}

function Test-CommsAcceptance {
    $Root = Join-Path $RuntimeRoot "citrate-comms-acceptance-$PID"
    if (Test-Path -LiteralPath $Root) { throw "Comms acceptance collision: $Root" }
    New-Item -ItemType Directory -Path $Root | Out-Null
    $SocketPath = Join-Path $Root 'member.sock'
    $BearerPath = Join-Path $Root 'member.bearer'
    $SeedPath = Join-Path $Root 'member.seed'
    $Bearer = New-RandomHex 32
    $Seed = New-RandomHex 32
    [System.IO.File]::WriteAllText($BearerPath, $Bearer)
    [System.IO.File]::WriteAllText($SeedPath, $Seed)

    $Info = [System.Diagnostics.ProcessStartInfo]::new()
    $Info.FileName = $CommsPath
    $Info.WorkingDirectory = $InstallDirectory
    $Info.UseShellExecute = $false
    $Info.Environment['CITRATE_MEMBER_SOCKET'] = $SocketPath
    $Info.Environment['CITRATE_MEMBER_BEARER_FILE'] = $BearerPath
    $Info.Environment['CITRATE_MEMBER_SEED_FILE'] = $SeedPath
    $Info.Environment['CITRATE_MEMBER_DOMAIN'] = 'relay.citrate.internal'
    $Process = [System.Diagnostics.Process]::Start($Info)
    if (-not $Process) { throw 'failed to start installed Comms daemon' }
    $script:Comms = $Process
    $null = $Process.Handle

    $NoncePath = "$SocketPath.pipe-nonce"
    $Deadline = (Get-Date).AddSeconds(30)
    do {
        if ($Process.HasExited) { throw "installed Comms daemon exited during startup: $($Process.ExitCode)" }
        if (Test-Path -LiteralPath $NoncePath -PathType Leaf) { break }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $Deadline)
    if (-not (Test-Path -LiteralPath $NoncePath -PathType Leaf)) {
        throw 'installed Comms daemon did not create its per-install pipe nonce'
    }
    $Nonce = [System.IO.File]::ReadAllBytes($NoncePath)
    if ($Nonce.Length -ne 32) { throw "Comms pipe nonce has invalid length: $($Nonce.Length)" }
    $Prefix = [System.Text.Encoding]::UTF8.GetBytes('citrate/ipc-pipe/v1')
    $PathBytes = [System.Text.Encoding]::UTF8.GetBytes($SocketPath)
    $HashInput = New-Object byte[] ($Prefix.Length + $PathBytes.Length + 1 + $Nonce.Length)
    [Array]::Copy($Prefix, 0, $HashInput, 0, $Prefix.Length)
    [Array]::Copy($PathBytes, 0, $HashInput, $Prefix.Length, $PathBytes.Length)
    [Array]::Copy($Nonce, 0, $HashInput, $Prefix.Length + $PathBytes.Length + 1, $Nonce.Length)
    $Sha = [System.Security.Cryptography.SHA256]::Create()
    try { $Digest = $Sha.ComputeHash($HashInput) } finally { $Sha.Dispose() }
    $Tag = ([Convert]::ToHexString($Digest[0..15])).ToLowerInvariant()
    $PipeName = "citrate-$Tag-member.sock"

    $Pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
        '.',
        $PipeName,
        [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::Asynchronous
    )
    try {
        $Pipe.Connect(30000)
        $Writer = [System.IO.StreamWriter]::new($Pipe, [System.Text.UTF8Encoding]::new($false), 1024, $true)
        $Reader = [System.IO.StreamReader]::new($Pipe, [System.Text.UTF8Encoding]::new($false), $false, 1024, $true)
        try {
            $Writer.AutoFlush = $true
            $Writer.WriteLine((@{ type = 'auth'; token = $Bearer } | ConvertTo-Json -Compress))
            $ReadyTask = $Reader.ReadLineAsync()
            if (-not $ReadyTask.Wait(10000)) { throw 'Comms bearer handshake timed out' }
            $ReadyLine = $ReadyTask.Result
            $Ready = $ReadyLine | ConvertFrom-Json
            if ($Ready.type -cne 'ready') { throw "Comms bearer handshake failed: $ReadyLine" }
            $Writer.WriteLine((@{ op = 'listGroups' } | ConvertTo-Json -Compress))
            $ResponseTask = $Reader.ReadLineAsync()
            if (-not $ResponseTask.Wait(10000)) { throw 'Comms listGroups request timed out' }
            $ResponseLine = $ResponseTask.Result
            $Response = $ResponseLine | ConvertFrom-Json
            if ($Response.type -cne 'groups' -or $null -eq $Response.groups) {
                throw "Comms listGroups response was invalid: $ResponseLine"
            }
        } finally {
            $Reader.Dispose()
            $Writer.Dispose()
        }
    } finally {
        $Pipe.Dispose()
    }
    $Record = [pscustomobject]@{
        processId = $Process.Id
        executablePath = $CommsPath
        pipeNameSha256Tag = $Tag
        bearerHandshake = 'ready'
        request = 'listGroups'
        responseType = [string]$Response.type
        groupCount = @($Response.groups).Count
    }
    $Record | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'installed-comms-ipc.json') -Encoding UTF8
    Stop-Process -InputObject $Process -Force -ErrorAction Stop
    if (-not $Process.WaitForExit(10000)) { throw 'installed Comms daemon did not stop' }
    $script:Comms = $null
    Wait-NoExactProcess $CommsPath
    Remove-Item -LiteralPath $Root -Recurse -Force
    return $Record
}

$AcceptanceError = $null
try {
    foreach ($Path in @($AppPath, $MemPath, $NodePath, $CommsPath)) {
        if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
            throw "installed acceptance binary is missing: $Path"
        }
    }
    if ([string]::IsNullOrWhiteSpace($RoamingKnownFolder)) {
        throw 'Roaming ApplicationData Known Folder path is empty'
    }
    if ([string]::IsNullOrWhiteSpace($LocalKnownFolder)) {
        throw 'LocalApplicationData Known Folder path is empty'
    }
    $AppDataDirectory = Join-Path $RoamingKnownFolder $AppIdentifier
    $LocalAppDataDirectory = Join-Path $LocalKnownFolder $AppIdentifier
    $AppDataDirectory = Assert-DirectKnownFolderAppPath `
        -KnownFolder $RoamingKnownFolder -AppDirectory $AppDataDirectory -Label 'Roaming'
    $LocalAppDataDirectory = Assert-DirectKnownFolderAppPath `
        -KnownFolder $LocalKnownFolder -AppDirectory $LocalAppDataDirectory -Label 'Local'
    $MemoryDirectory = Join-Path $AppDataDirectory 'memory'
    $ImportProgressPath = Join-Path $MemoryDirectory 'knowledge-corpus.progress.json'
    $ImportMarkerPath = Join-Path $MemoryDirectory 'knowledge-corpus.imported'
    $Acceptance.knownFolders.roamingAppDirectory = $AppDataDirectory
    $Acceptance.knownFolders.localAppDirectory = $LocalAppDataDirectory
    $Acceptance.knownFolders.memoryDirectory = $MemoryDirectory
    $Acceptance.cleanup.roamingAppDirectory.path = $AppDataDirectory
    $Acceptance.cleanup.localAppDirectory.path = $LocalAppDataDirectory

    foreach ($Collision in @(
        [pscustomobject]@{ path = $AppDataDirectory; label = 'Roaming' },
        [pscustomobject]@{ path = $LocalAppDataDirectory; label = 'Local' }
    )) {
        if (Test-Path -LiteralPath $Collision.path) {
            throw "$($Collision.label) app data collision; refusing to touch pre-existing user data: $($Collision.path)"
        }
    }
    New-OwnedAppDirectory -Path $AppDataDirectory -Label 'Roaming'
    $AppDataDirectoryOwned = $true
    $Acceptance.cleanup.roamingAppDirectory.owned = $true
    New-OwnedAppDirectory -Path $LocalAppDataDirectory -Label 'Local'
    $LocalAppDataDirectoryOwned = $true
    $Acceptance.cleanup.localAppDirectory.owned = $true

    $Session1 = Start-InstalledApp
    $App = $Session1.process
    $Acceptance.renderedApplication = Save-RenderedScreenshot $Session1.webSocketUrl

    $InterruptPoint = Wait-ImportInterruptPoint
    $BeforeInterruption = $InterruptPoint.progress
    Stop-InstalledApp $App
    $App = $null
    Wait-NoImportProcess
    $AfterInterruption = Get-Content -LiteralPath $ImportProgressPath -Raw | ConvertFrom-Json
    if ($AfterInterruption.state -cne 'interrupted' -or @($AfterInterruption.tenants_done).Count -eq 0) {
        throw "graceful app exit did not persist interrupted import state: $($AfterInterruption | ConvertTo-Json -Compress)"
    }
    foreach ($Tenant in @($BeforeInterruption.tenants_done)) {
        if ($Tenant -notin @($AfterInterruption.tenants_done)) {
            throw "graceful interruption lost completed tenant: $Tenant"
        }
    }
    $FinishedTenants = @($AfterInterruption.tenants_done)
    $InterruptedImporterOrphans = @(Get-ImportProcesses).Count
    if ($InterruptedImporterOrphans -ne 0) {
        throw "graceful app exit left $InterruptedImporterOrphans knowledge importer process(es)"
    }
    $Acceptance.interruptedImport = [pscustomobject]@{
        observedBoundary = $InterruptPoint
        afterExit = $AfterInterruption
        importerOrphans = $InterruptedImporterOrphans
    }

    $Session2 = Start-InstalledApp
    $App = $Session2.process
    $ResumeObservation = Wait-ImportResumeCompletion -ExpectedFinishedTenants $FinishedTenants
    if (Test-Path -LiteralPath $ImportProgressPath) {
        throw 'completed resumed import left its progress file behind'
    }
    if (-not (Test-Path -LiteralPath $ImportMarkerPath -PathType Leaf)) {
        throw 'completed resumed import did not write its digest marker'
    }
    Wait-NoImportProcess
    $MemoryDaemon = Wait-MemoryDaemonProcess
    $ResumedImporterOrphans = @(Get-ImportProcesses).Count
    if ($ResumedImporterOrphans -ne 0) {
        throw "completed resume left $ResumedImporterOrphans knowledge importer process(es)"
    }
    $Acceptance.resumedImport = [pscustomobject]@{
        observation = $ResumeObservation
        previouslyFinishedTenants = $FinishedTenants
        marker = (Get-Content -LiteralPath $ImportMarkerPath -Raw).Trim()
        memoryDaemonProcessId = [int]$MemoryDaemon.ProcessId
        memoryDaemonCommandLine = [string]$MemoryDaemon.CommandLine
        importerOrphans = $ResumedImporterOrphans
    }

    $Acceptance.node = Test-NodeAcceptance $Session2.webSocketUrl
    $Acceptance.comms = Test-CommsAcceptance
    Stop-InstalledApp $App
    $App = $null
    $NodeProcessesAfterShutdown = @(Get-ExactProcesses $NodePath).Count
    if ($NodeProcessesAfterShutdown -ne 0) {
        throw "graceful app shutdown left $NodeProcessesAfterShutdown installed node process(es)"
    }
    $Acceptance.node | Add-Member -NotePropertyName gracefulShutdownProcessCount `
        -NotePropertyValue $NodeProcessesAfterShutdown
    $Acceptance.node | ConvertTo-Json -Depth 12 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'installed-node-consensus.json') -Encoding UTF8
    $MemoryProcessesAfterShutdown = @(Get-ExactProcesses $MemPath).Count
    if ($MemoryProcessesAfterShutdown -ne 0) {
        throw "graceful app shutdown left $MemoryProcessesAfterShutdown installed memory process(es)"
    }
    $Acceptance.resumedImport | Add-Member -NotePropertyName gracefulShutdownMemoryProcessCount `
        -NotePropertyValue $MemoryProcessesAfterShutdown
    $Acceptance.passed = $true
} catch {
    $AcceptanceError = $_.Exception.Message
    $Acceptance.error = $AcceptanceError
    throw
} finally {
    $CleanupErrors = [System.Collections.Generic.List[string]]::new()
    $Acceptance.cleanup.processTrees.attempted = $true
    try {
        Stop-InstalledProcessTrees
        $Acceptance.cleanup.processTrees.succeeded = $true
    } catch {
        $Acceptance.cleanup.processTrees.error = $_.Exception.Message
        $CleanupErrors.Add("process cleanup failed: $($_.Exception.Message)")
    }
    foreach ($Cleanup in @(
        [pscustomobject]@{
            path = $AppDataDirectory
            knownFolder = $RoamingKnownFolder
            label = 'Roaming'
            owned = $AppDataDirectoryOwned
            evidence = $Acceptance.cleanup.roamingAppDirectory
        },
        [pscustomobject]@{
            path = $LocalAppDataDirectory
            knownFolder = $LocalKnownFolder
            label = 'Local'
            owned = $LocalAppDataDirectoryOwned
            evidence = $Acceptance.cleanup.localAppDirectory
        }
    )) {
        if (-not $Cleanup.owned) {
            $Cleanup.evidence.outcome = 'not-owned'
            continue
        }
        if (-not $Acceptance.cleanup.processTrees.succeeded) {
            $Cleanup.evidence.outcome = 'skipped-process-cleanup-failed'
            $Cleanup.evidence.error = 'Directory cleanup was skipped because exact process-tree cleanup failed.'
            $CleanupErrors.Add("$($Cleanup.label) directory cleanup skipped after process cleanup failure")
            continue
        }
        $Cleanup.evidence.attempted = $true
        try {
            $Removed = Remove-OwnedKnownFolderAppDirectory `
                -Path $Cleanup.path -KnownFolder $Cleanup.knownFolder -Label $Cleanup.label
            $Cleanup.evidence.removed = $Removed
            $Cleanup.evidence.outcome = if ($Removed) { 'removed' } else { 'already-absent' }
        } catch {
            $Cleanup.evidence.outcome = 'unsafe-or-failed'
            $Cleanup.evidence.error = $_.Exception.Message
            $CleanupErrors.Add("$($Cleanup.label) directory cleanup failed closed: $($_.Exception.Message)")
        }
    }
    if ($CleanupErrors.Count -gt 0) {
        $Acceptance.passed = $false
        $CleanupMessage = $CleanupErrors -join '; '
        $Acceptance.error = if ($AcceptanceError) {
            "$AcceptanceError; $CleanupMessage"
        } else {
            $CleanupMessage
        }
    }
    $Acceptance.finishedUtc = (Get-Date).ToUniversalTime().ToString('o')
    $Acceptance | ConvertTo-Json -Depth 20 |
        Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'installed-product-acceptance.json') -Encoding UTF8
    if ($CleanupErrors.Count -gt 0) {
        throw "installed-product cleanup failed: $($CleanupErrors -join '; '); acceptance error: $AcceptanceError"
    }
}

Write-Host 'Installed-product acceptance passed.'
