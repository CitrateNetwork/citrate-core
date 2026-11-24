# created: 2026-11-24 | branch: feat/059-windows
# author: Larry Klosowski + Claude Opus 5.5 (Windows lane, 0.5.9)
# status: active
#
# Smoke test of the Windows NSIS installer ON A THROWAWAY GITHUB-HOSTED WINDOWS RUNNER
# (.github/workflows/release-windows.yml, job `smoke`). Never run it on a member's or the owner's
# PC: it installs, launches and uninstalls Citrate Core for the current user and leaves the app's
# data behind. Modelled on scripts/ci/linux-native-acceptance.sh (launch, sidecars, quit).
#
#   1. silent per-user install (/S), no admin prompt; record the installed files and their sha256
#   2. launch citrate-core.exe; the supervisor starts the sidecars the app starts by itself
#   3. the bundled node serves local RPC (127.0.0.1:8545, node.toml [rpc]) and reports chain 40204,
#      the r1007 genesis hash and the consensus fingerprint of release/network-cut.json
#   4. the node connects to the r1007 boot peers (TCP 30303 to boot1-3 / rpc.citrate.ai) and its
#      applied height moves
#   5. quit by closing the main window (WM_CLOSE, what the title-bar close button sends); the app
#      and every process it started exit
#   6. silent uninstall; record what is left in the install dir, %APPDATA% and %LOCALAPPDATA%
#
# Evidence (no key material: only logs, node.toml, listings and JSON) goes to -Evidence. Exit 0
# when every required check passes, 1 otherwise.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$Evidence,
    [Parameter(Mandatory = $true)][string]$NetworkCut,
    # The node's base rev (release/components.json citrate-node source.rev). When the binary was
    # built from another rev (a Windows target rev), its fingerprint is checked by recomputing it
    # with git_sha = this base (scripts/ci/consensus-fingerprint.py).
    [Parameter(Mandatory = $true)][string]$BaseRev,
    [Parameter(Mandatory = $true)][string]$FingerprintScript,
    [int]$LaunchTimeout = 240,
    [int]$PeerTimeout = 420,
    [int]$QuitTimeout = 90,
    # The memory daemon starts after the first-run knowledge import finishes.
    [int]$MemoryTimeout = 1800
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

New-Item -ItemType Directory -Force $Evidence | Out-Null
$Log = Join-Path $Evidence 'smoke.log'
$cut = Get-Content $NetworkCut -Raw | ConvertFrom-Json
$WantGenesis = $cut.genesisHash.ToLower()
$WantFingerprint = $cut.consensusFingerprint.ToLower()
$BootHosts = @($cut.bootnodes | ForEach-Object { ($_ -split '@')[1] -split ':' | Select-Object -First 1 })
$AppId = 'ai.citrate.core'
$Results = [ordered]@{}
$Required = New-Object System.Collections.Generic.List[string]
$Facts = [ordered]@{ image = "$env:ImageOS $env:ImageVersion"; os = (Get-CimInstance Win32_OperatingSystem | ForEach-Object { "$($_.Caption) $($_.Version) build $($_.BuildNumber)" }) }

function Say([string]$m) { $line = "[{0:HH:mm:ss}] {1}" -f (Get-Date).ToUniversalTime(), $m; Write-Host $line; Add-Content -Path $Log -Value $line }
function Check([string]$name, [bool]$ok, [string]$detail, [switch]$Req) {
    $Results[$name] = [ordered]@{ ok = $ok; detail = $detail; required = [bool]$Req }
    if ($Req) { $Required.Add($name) }
    Say ("{0} {1}: {2}" -f ($(if ($ok) { 'PASS' } else { 'FAIL' })), $name, $detail)
}
# A property of a JSON-RPC result, or $null when the node did not send it (StrictMode-safe).
function P($o, [string]$name) {
    if ($null -eq $o) { return $null }
    if ($o.PSObject.Properties.Name -contains $name) { return $o.$name }
    return $null
}
# PBA-L7b-004: the per-install Windows pipe name both ends derive from the socket path P and the nonce
# in <P>.pipe-nonce (citrate-core src-tauri/src/ipc_name.rs windows_pipe_name).
function Pipe-Name([string]$p, [byte[]]$nonce) {
    $slug = -join ([IO.Path]::GetFileName($p).ToCharArray() | ForEach-Object { if ($_ -match '[A-Za-z0-9._-]') { $_ } else { '-' } })
    $bytes = [Text.Encoding]::UTF8.GetBytes('citrate/ipc-pipe/v1') + [Text.Encoding]::UTF8.GetBytes($p) + [byte[]]@(0) + $nonce
    $h = [Security.Cryptography.SHA256]::Create().ComputeHash([byte[]]$bytes)
    "citrate-" + (-join ($h[0..15] | ForEach-Object { $_.ToString('x2') })) + "-$slug"
}
function Pipes { @([IO.Directory]::GetFiles('\\.\pipe\') | ForEach-Object { $_.Substring(9) }) }

# Whether the daemon behind socket path $sock listens on the per-install name the app connects to.
function Probe-Ipc([string]$label, [string]$sock) {
    $noncePath = "$sock.pipe-nonce"
    if (-not (Test-Path $noncePath)) { return [ordered]@{ ok = $false; detail = "no $noncePath (the app never connected to $label)" } }
    $nonce = [IO.File]::ReadAllBytes($noncePath)
    $want = Pipe-Name $sock $nonce
    $all = Pipes
    $slug = [IO.Path]::GetFileName($sock)
    $bare = $all -contains $slug
    $ok = $all -contains $want
    [ordered]@{ ok = $ok; detail = "per-install pipe $want listening: $ok; bare '$slug' pipe (the pre-fix name): $bare; pipes ending in ${slug}: $((@($all | Where-Object { $_ -like "*$slug" })) -join ', ')" }
}

function Rpc([string]$method, [object[]]$params = @()) {
    $body = @{ jsonrpc = '2.0'; id = 1; method = $method; params = $params } | ConvertTo-Json -Compress -Depth 5
    $r = Invoke-RestMethod -Uri 'http://127.0.0.1:8545' -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 5
    if ($r.PSObject.Properties.Name -contains 'error' -and $r.error) { throw "$method error: $($r.error | ConvertTo-Json -Compress)" }
    return $r.result
}
function Under([string]$root) {
    $rootFull = [IO.Path]::GetFullPath($root).TrimEnd('\') + '\'
    @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($rootFull, [StringComparison]::OrdinalIgnoreCase) })
}
function Listing([string]$dir, [string]$file) {
    if (Test-Path $dir) {
        Get-ChildItem $dir -Recurse -Force -ErrorAction SilentlyContinue |
            ForEach-Object { "{0,14}  {1}" -f $(if ($_.PSIsContainer) { '<dir>' } else { $_.Length }), $_.FullName.Substring($dir.Length) } |
            Out-File (Join-Path $Evidence $file) -Encoding utf8
    } else { "absent: $dir" | Out-File (Join-Path $Evidence $file) -Encoding utf8 }
}
function Save-Logs([string]$tag) {
    $dst = Join-Path $Evidence "logs-$tag"
    New-Item -ItemType Directory -Force $dst | Out-Null
    foreach ($root in @((Join-Path $env:APPDATA $AppId), (Join-Path $env:LOCALAPPDATA $AppId))) {
        if (-not (Test-Path $root)) { continue }
        # Logs, crash records and the rendered node.toml only: never key files, the chain DB or the vault.
        # Only the app's own logs\ tree, the rendered node.toml and crash records: never the chain
        # database (RocksDB *.log WAL files), WebView storage, key files or the vault.
        $logsDir = Join-Path $root 'logs'
        Get-ChildItem $root -Recurse -File -Force -ErrorAction SilentlyContinue |
            Where-Object {
                (($_.FullName.StartsWith($logsDir + '\', [StringComparison]::OrdinalIgnoreCase) -and $_.Extension -eq '.log') -or
                 $_.Name -eq 'node.toml' -or $_.Name -eq 'crash-records.jsonl') -and
                $_.Length -lt 20MB -and $_.Name -notmatch 'key|secret|vault|seed'
            } |
            ForEach-Object {
                $rel = $_.FullName.Substring($root.Length).TrimStart('\') -replace '[\\/:]', '__'
                Copy-Item $_.FullName (Join-Path $dst ("{0}__{1}" -f (Split-Path $root -Leaf), $rel)) -ErrorAction SilentlyContinue
            }
    }
}

$app = $null
$InstallDir = $null

# The uninstall entries of Citrate Core under one hive (an entry without a DisplayName is skipped;
# StrictMode would otherwise throw on it).
function Find-Uninstall([string]$hive) {
    @(Get-ChildItem "${hive}:\Software\Microsoft\Windows\CurrentVersion\Uninstall" -ErrorAction SilentlyContinue | Where-Object {
        $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
        $p -and ($p.PSObject.Properties.Name -contains 'DisplayName') -and ($p.DisplayName -like 'Citrate Core*')
    })
}

try {
    # ── 1. install ───────────────────────────────────────────────────────────────────────────────
    $sig = Get-AuthenticodeSignature $Installer
    $Facts.installerSha256 = (Get-FileHash $Installer -Algorithm SHA256).Hash.ToLower()
    $Facts.installerSignature = "$($sig.Status)"
    Say "installer $Installer sha256 $($Facts.installerSha256) signature $($sig.Status)"
    $t0 = Get-Date
    $p = Start-Process -FilePath $Installer -ArgumentList '/S' -PassThru -Wait
    $Facts.installSeconds = [int]((Get-Date) - $t0).TotalSeconds
    $key = @(Find-Uninstall 'HKCU') | Select-Object -First 1
    $hklm = @(Find-Uninstall 'HKLM')
    $props = if ($key) { Get-ItemProperty $key.PSPath } else { $null }
    if ($props) {
        $InstallDir = if ($props.PSObject.Properties.Name -contains 'InstallLocation' -and $props.InstallLocation) { $props.InstallLocation.Trim('"') } else { Split-Path ($props.UninstallString.Trim('"')) -Parent }
        $Facts.displayVersion = if ($props.PSObject.Properties.Name -contains 'DisplayVersion') { $props.DisplayVersion } else { $null }
        $Facts.uninstallString = $props.UninstallString
    }
    Check 'install' (($p.ExitCode -eq 0) -and $InstallDir -and (Test-Path (Join-Path $InstallDir 'citrate-core.exe'))) "exit $($p.ExitCode) in $($Facts.installSeconds)s; per-user key $(if ($key) { $key.PSChildName } else { 'missing' }); dir $InstallDir" -Req
    Check 'per-user (no machine-wide entry)' ($hklm.Count -eq 0) "HKLM uninstall entries: $($hklm.Count)" -Req
    if (-not $InstallDir) { throw 'no install dir' }
    $Facts.installDir = $InstallDir
    Get-ChildItem $InstallDir -Recurse -File | Where-Object { $_.Extension -in '.exe', '.dll' } |
        ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.FullName.Substring($InstallDir.Length).TrimStart('\') } |
        Out-File (Join-Path $Evidence 'installed-binaries.sha256') -Encoding ascii
    Listing $InstallDir 'install-dir.txt'
    $want = 'citrate-core', 'citrate', 'node-agent', 'mem-mcp', 'llama-server', 'ipfs', 'comms-member-daemon', 'cluster-daemon', 'hermes'
    $missing = @($want | Where-Object { -not (Test-Path (Join-Path $InstallDir "$_.exe")) })
    Check 'installed sidecars' ($missing.Count -eq 0) "missing: $(if ($missing) { $missing -join ', ' } else { 'none' }); llama/llama-server.exe present: $(Test-Path (Join-Path $InstallDir 'llama\llama-server.exe'))" -Req

    # Boot peers reachable from this runner at all (so a node failure is not a runner network gap)
    $reach = foreach ($h in $BootHosts) { $c = New-Object Net.Sockets.TcpClient; try { $ok = $c.ConnectAsync($h, 30303).Wait(5000) -and $c.Connected } catch { $ok = $false } finally { $c.Dispose() }; "${h}:30303=$ok" }
    Say "runner reachability: $($reach -join ' ')"
    $BootIps = @{}
    foreach ($h in $BootHosts) { try { foreach ($a in [Net.Dns]::GetHostAddresses($h)) { $BootIps[$a.ToString()] = $h } } catch { } }

    # ── 2. launch ────────────────────────────────────────────────────────────────────────────────
    $exe = Join-Path $InstallDir 'citrate-core.exe'
    $app = Start-Process -FilePath $exe -WorkingDirectory $InstallDir -PassThru
    Say "launched citrate-core.exe pid $($app.Id)"
    $node = $null
    $deadline = (Get-Date).AddSeconds($LaunchTimeout)
    while ((Get-Date) -lt $deadline -and -not $app.HasExited) {
        $node = Under $InstallDir | Where-Object { $_.Name -eq 'citrate.exe' } | Select-Object -First 1
        if ($node) { break }
        Start-Sleep -Seconds 2
    }
    Check 'app runs' (-not $app.HasExited) $(if ($app.HasExited) { "exited with $($app.ExitCode)" } else { "pid $($app.Id) running" }) -Req
    Check 'node sidecar started' ([bool]$node) $(if ($node) { "citrate.exe pid $($node.ProcessId) after $([int]($LaunchTimeout - ($deadline - (Get-Date)).TotalSeconds))s" } else { "no citrate.exe under the install dir after ${LaunchTimeout}s" }) -Req

    # ── 3. local RPC: chain, genesis, fingerprint ─────────────────────────────────────────────────
    $ident = $null
    $deadline = (Get-Date).AddSeconds(180)
    while ((Get-Date) -lt $deadline) {
        try { $ident = Rpc 'citrate_nodeIdentity'; break } catch { Start-Sleep -Seconds 3 }
    }
    if ($ident) {
        $ident | ConvertTo-Json -Depth 8 | Out-File (Join-Path $Evidence 'nodeIdentity-first.json') -Encoding utf8
        $cid0 = "$(P $ident 'chainId')"; $gen = "$(P $ident 'genesisHash')".ToLower(); $fp = "$(P $ident 'consensusFingerprint')".ToLower()
        Check 'chain id 40204 (0x9d0c)' ($cid0 -eq '0x9d0c') "chainId $cid0" -Req
        Check 'r1007 genesis hash' ($gen -eq $WantGenesis) "genesisHash $gen (want $WantGenesis)" -Req
        $git = "$(P $ident 'gitSha')"
        $Facts.nodeGitSha = $git
        $Facts.reportedFingerprint = $fp
        if ($git -and $BaseRev.StartsWith($git)) {
            Check 'consensus fingerprint' ($fp -eq $WantFingerprint) "consensusFingerprint $fp (want $WantFingerprint; node at the base rev $git)" -Req
        } else {
            Check 'consensus fingerprint (reported)' ($fp -match '^0x[0-9a-f]{32}$') "consensusFingerprint $fp from a node at $git (base $($BaseRev.Substring(0,12))); recomputed against the base after quit" -Req
        }
        try { $cid = Rpc 'eth_chainId'; Check 'eth_chainId' ("$cid" -eq '0x9d0c') "eth_chainId $cid" } catch { Check 'eth_chainId' $false "$_" }
    } else {
        Check 'local RPC answers' $false 'citrate_nodeIdentity did not answer on 127.0.0.1:8545 within 180s' -Req
    }

    # ── 4. boot peers and sync progress ─────────────────────────────────────────────────────────
    $samples = New-Object System.Collections.Generic.List[object]
    $bootSeen = @{}
    $firstH = $null; $lastH = $null; $peers = 0
    $deadline = (Get-Date).AddSeconds($PeerTimeout)
    while ((Get-Date) -lt $deadline) {
        $nodeNow = Under $InstallDir | Where-Object { $_.Name -eq 'citrate.exe' } | Select-Object -First 1
        if ($nodeNow) {
            foreach ($c in @(Get-NetTCPConnection -OwningProcess $nodeNow.ProcessId -State Established -ErrorAction SilentlyContinue)) {
                if ($BootIps.ContainsKey($c.RemoteAddress) -and $c.RemotePort -eq 30303) { $bootSeen[$BootIps[$c.RemoteAddress]] = $c.RemoteAddress }
            }
        }
        try {
            $pc = [Convert]::ToInt64((Rpc 'net_peerCount'), 16); $peers = [Math]::Max($peers, $pc)
            $i = Rpc 'citrate_nodeIdentity'
            $ss = P $i 'syncState'
            $h = [int64](P $ss 'appliedHeight')
            if ($null -eq $firstH) { $firstH = $h }; $lastH = $h
            $samples.Add([ordered]@{ t = (Get-Date).ToUniversalTime().ToString('o'); peers = $pc; state = (P $ss 'state'); appliedHeight = $h; boot = ($bootSeen.Keys -join ',') })
        } catch { $samples.Add([ordered]@{ t = (Get-Date).ToUniversalTime().ToString('o'); error = "$_" }) }
        if ($bootSeen.Count -gt 0 -and $peers -gt 0 -and $null -ne $firstH -and $lastH -gt $firstH + 50) { break }
        Start-Sleep -Seconds 10
    }
    $samples | ConvertTo-Json -Depth 5 | Out-File (Join-Path $Evidence 'sync-samples.json') -Encoding utf8
    Check 'connected to r1007 boot peers' ($bootSeen.Count -gt 0) "established TCP 30303 to: $(if ($bootSeen.Count) { ($bootSeen.GetEnumerator() | ForEach-Object { "$($_.Key) ($($_.Value))" }) -join ', ' } else { 'none' }); max net_peerCount $peers" -Req
    Check 'sync progresses' ($null -ne $firstH -and $lastH -gt $firstH) "appliedHeight $firstH -> $lastH" -Req
    try { (Rpc 'citrate_nodeIdentity') | ConvertTo-Json -Depth 8 | Out-File (Join-Path $Evidence 'nodeIdentity-last.json') -Encoding utf8 } catch { }

    # Memory IPC: after the first-run knowledge import, the app starts mem-mcp's daemon and probes it
    # over the per-install pipe. Required: the daemon listens on exactly the name the app connects to.
    $memSock = Join-Path $env:APPDATA "$AppId\memory\memdag.sock"
    $deadline = (Get-Date).AddSeconds($MemoryTimeout)
    $mem = $null
    do {
        $mem = Probe-Ipc 'memory' $memSock
        if ($mem.ok) { break }
        Start-Sleep -Seconds 15
    } while ((Get-Date) -lt $deadline)
    $prog = Join-Path $env:APPDATA "$AppId\memory\knowledge-corpus.progress.json"
    if (Test-Path $prog) { Copy-Item $prog (Join-Path $Evidence 'knowledge-corpus.progress.json') }
    Check 'memory IPC (mem-mcp on the per-install pipe)' $mem.ok $mem.detail -Req
    # Groups and chat: their daemons start on demand (Groups, sign-in), not at launch; probed when present.
    foreach ($d in @(@{ n = 'cluster IPC (cluster-daemon)'; s = "cluster\cluster.sock" }, @{ n = 'comms IPC (comms-member-daemon)'; s = "comms\member.sock" })) {
        $sock = Join-Path $env:APPDATA "$AppId\$($d.s)"
        if (Test-Path "$sock.pipe-nonce") { $r = Probe-Ipc $d.n $sock; Check $d.n $r.ok $r.detail }
        else { Check $d.n $true "not started at launch (no $sock.pipe-nonce); started on demand from the UI, not exercised here" }
    }
    Pipes | Where-Object { $_ -like 'citrate-*' -or $_ -like '*.sock' } | Out-File (Join-Path $Evidence 'pipes.txt') -Encoding utf8

    # Which sidecars the app started by itself (reported; the node is the required one)
    $running = Under $InstallDir
    $Facts.processes = @($running | ForEach-Object { "$($_.Name) pid $($_.ProcessId) parent $($_.ParentProcessId)" })
    Say "running under the install dir: $($Facts.processes -join '; ')"
    Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.OwningProcess -in @($running | ForEach-Object ProcessId) } |
        ForEach-Object { "{0}:{1} pid {2}" -f $_.LocalAddress, $_.LocalPort, $_.OwningProcess } | Out-File (Join-Path $Evidence 'listeners.txt') -Encoding utf8
    Save-Logs 'running'

    # ── 5. quit ──────────────────────────────────────────────────────────────────────────────────
    $before = @($running | ForEach-Object ProcessId)
    $app.Refresh()
    $asked = $app.CloseMainWindow()
    Say "CloseMainWindow (WM_CLOSE) sent: $asked"
    $gone = $app.WaitForExit($QuitTimeout * 1000)
    $left = @()
    if ($gone) {
        $deadline = (Get-Date).AddSeconds(30)
        do { $left = @(Under $InstallDir); if (-not $left) { break }; Start-Sleep -Seconds 2 } while ((Get-Date) -lt $deadline)
    }
    Check 'quit ends the app' ($asked -and $gone) "main window accepted close: $asked; app exited within ${QuitTimeout}s: $gone" -Req
    Check 'quit ends every sidecar' ($gone -and $left.Count -eq 0) "left after quit: $(if ($left) { ($left | ForEach-Object { "$($_.Name) $($_.ProcessId)" }) -join ', ' } else { 'none' }) (of $($before.Count) running before)" -Req
    if (-not $gone -or $left) { Under $InstallDir | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }; Start-Sleep -Seconds 3 }
    $app = $null
    Save-Logs 'after-quit'

    # The node's own consensus view of the synced data dir (the cut's evidence command)
    $toml = Get-ChildItem (Join-Path $env:APPDATA $AppId) -Recurse -Filter node.toml -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($toml) {
        $out = & (Join-Path $InstallDir 'citrate.exe') --config $toml.FullName consensus --json 2>&1 | Out-String
        $out | Out-File (Join-Path $Evidence 'consensus.json') -Encoding utf8
        $cj = Join-Path $Evidence 'consensus.json'
        $fpOut = & python $FingerprintScript $cj --base-sha $BaseRev --want $WantFingerprint 2>&1 | Out-String
        $fpOk = ($LASTEXITCODE -eq 0)
        $fpOut | Out-File (Join-Path $Evidence 'consensus-fingerprint.json') -Encoding utf8
        Check 'same consensus as the base rev (recomputed fingerprint)' $fpOk "scripts/ci/consensus-fingerprint.py --base-sha $($BaseRev.Substring(0,12)) --want ${WantFingerprint}: $(($fpOut -replace '\s+', ' ').Trim())" -Req
    } else {
        Check 'same consensus as the base rev (recomputed fingerprint)' $false "no rendered node.toml under %APPDATA%\$AppId, so consensus --json could not run" -Req
    }

    # ── 6. uninstall ─────────────────────────────────────────────────────────────────────────────
    $un = Join-Path $InstallDir 'uninstall.exe'
    if (Test-Path $un) {
        $u = Start-Process -FilePath $un -ArgumentList '/S' -PassThru -Wait
        $deadline = (Get-Date).AddSeconds(120)
        while ((Get-Date) -lt $deadline -and (Test-Path (Join-Path $InstallDir 'citrate-core.exe'))) { Start-Sleep -Seconds 2 }
        $keyLeft = @(Find-Uninstall 'HKCU').Count -gt 0
        $binsLeft = @(Get-ChildItem $InstallDir -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in '.exe', '.dll' })
        Check 'uninstall' ($u.ExitCode -eq 0 -and $binsLeft.Count -eq 0 -and -not $keyLeft) "exit $($u.ExitCode); binaries left $($binsLeft.Count); uninstall key left: $([bool]$keyLeft)" -Req
    } else {
        Check 'uninstall' $false "no uninstall.exe in $InstallDir" -Req
    }
    Listing $InstallDir 'after-uninstall-install-dir.txt'
    Listing (Join-Path $env:APPDATA $AppId) 'after-uninstall-appdata.txt'
    Listing (Join-Path $env:LOCALAPPDATA $AppId) 'after-uninstall-localappdata.txt'
    $Facts.leftInAppData = (Test-Path (Join-Path $env:APPDATA $AppId))
    $Facts.leftInLocalAppData = (Test-Path (Join-Path $env:LOCALAPPDATA $AppId))
}
catch {
    Check 'harness' $false "stopped early: $($_.Exception.Message) at $($_.InvocationInfo.PositionMessage)" -Req
    if ($app -and -not $app.HasExited) { Save-Logs 'error' }
}
finally {
    if ($app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
    if ($InstallDir) { Under $InstallDir | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } }
}

$failed = @($Required | Where-Object { -not $Results[$_].ok })
[ordered]@{ passed = ($failed.Count -eq 0); failedRequired = $failed; facts = $Facts; checks = $Results } |
    ConvertTo-Json -Depth 8 | Out-File (Join-Path $Evidence 'result.json') -Encoding utf8
if ($env:GITHUB_STEP_SUMMARY) {
    $md = @("### Windows smoke test ($($Facts.image); $($Facts.os))", '', '| check | result | detail |', '|---|---|---|')
    foreach ($k in $Results.Keys) { $r = $Results[$k]; $md += "| $k$(if ($r.required) { '' } else { ' (reported)' }) | $(if ($r.ok) { 'pass' } else { '**FAIL**' }) | $(($r.detail -replace '\|', '/') -replace '\r?\n', ' ') |" }
    $md | Out-File $env:GITHUB_STEP_SUMMARY -Append -Encoding utf8
}
if ($failed.Count) { Say "FAILED: $($failed -join ', ')"; exit 1 }
Say 'all required checks passed'
exit 0
