# SCL-S0.7 / S1.6a: script-level test of the Windows installer's pre-install step.
#
# Runs on Windows (Windows PowerShell 5.1, the shell the installer uses). It starts real
# long-running processes, then checks that only this installation's own sidecars are stopped:
#
#   stopped:  <install>\hermes.exe, <install>\citrate.exe          (exact listed paths)
#   left:     <decoy>\hermes.exe                                    (same name, other folder)
#             <install>\sub\hermes.exe                              (inside, but not exact)
#             <install>-other\hermes.exe                            (prefix of the install path)
#             <install>\unlisted.exe                                (in the folder, not a sidecar)
#
# Twice: calling stop-own-sidecars.ps1 directly, and through the NSIS hook compiled into
# hook-harness.nsi (when -MakeNsis is given). Then (#305) the hook built with a stop script that
# never returns: the hook must end it, and what it started, within its outer bound, and the
# install must continue. Every process it starts is cleaned up.
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File test-stop-own-sidecars.ps1 [-MakeNsis <makensis.exe>]
[CmdletBinding()]
param([string]$MakeNsis = '')

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$script = Join-Path (Split-Path -Parent $here) 'stop-own-sidecars.ps1'
$hooks = Join-Path (Split-Path -Parent $here) 'installer-hooks.nsh'
$harnessSrc = Join-Path $here 'hook-harness.nsi'
$names = 'citrate,node-agent,mem-mcp,llama-server,ipfs,comms-member-daemon,cluster-daemon,hermes'

$base = Join-Path ([System.IO.Path]::GetTempPath()) ("citrate-s07-" + [guid]::NewGuid().ToString('N'))
$install = Join-Path $base 'Citrate Core'
$decoy = Join-Path $base 'Somewhere Else'
$nested = Join-Path $install 'sub'
$sibling = Join-Path $base 'Citrate Core-other'
foreach ($d in @($install, $decoy, $nested, $sibling)) { New-Item -ItemType Directory -Path $d | Out-Null }

$failures = New-Object 'System.Collections.Generic.List[string]'
$started = New-Object 'System.Collections.Generic.List[int]'

function Check([bool]$Ok, [string]$What) {
    if ($Ok) { Write-Host "ok   - $What" } else { Write-Host "FAIL - $What"; $failures.Add($What) }
}

# A real long-running console program (no window), compiled once.
$sleeper = Join-Path $base 'sleeper.exe'
Add-Type -OutputType ConsoleApplication -OutputAssembly $sleeper -TypeDefinition @'
public static class Sleeper {
    public static void Main() { System.Threading.Thread.Sleep(600000); }
}
'@

$layout = [ordered]@{
    own_hermes   = Join-Path $install 'hermes.exe'
    own_citrate  = Join-Path $install 'citrate.exe'
    decoy_hermes = Join-Path $decoy 'hermes.exe'
    nested       = Join-Path $nested 'hermes.exe'
    sibling      = Join-Path $sibling 'hermes.exe'
    unlisted     = Join-Path $install 'unlisted.exe'
}
foreach ($p in $layout.Values) { Copy-Item $sleeper $p }

function Start-All {
    $pids = @{}
    foreach ($k in $layout.Keys) {
        $proc = Start-Process -FilePath $layout[$k] -PassThru -WindowStyle Hidden
        $started.Add($proc.Id)
        $pids[$k] = $proc.Id
    }
    Start-Sleep -Milliseconds 500
    return $pids
}

function Is-Alive([int]$ProcessId) {
    return [bool](Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)
}

function Stop-All {
    foreach ($id in $started) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    $started.Clear()
    Start-Sleep -Milliseconds 300
}

function Assert-Outcome($Pids, [string]$Via) {
    Check (-not (Is-Alive $Pids.own_hermes)) "$Via stops <install>\hermes.exe"
    Check (-not (Is-Alive $Pids.own_citrate)) "$Via stops <install>\citrate.exe"
    Check (Is-Alive $Pids.decoy_hermes) "$Via leaves hermes.exe in another folder running"
    Check (Is-Alive $Pids.nested) "$Via leaves <install>\sub\hermes.exe running"
    Check (Is-Alive $Pids.sibling) "$Via leaves <install>-other\hermes.exe running"
    Check (Is-Alive $Pids.unlisted) "$Via leaves a non-sidecar exe in the install folder running"
}

function Invoke-Script([string[]]$ScriptArgs) {
    $psExe = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $all = @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $script) + $ScriptArgs
    $ErrorActionPreference = 'Continue'
    & $psExe @all | ForEach-Object { Write-Host "     | $_" }
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    return $code
}

try {
    # 1. Direct call.
    $pids = Start-All
    $code = Invoke-Script @('-InstallDir', $install, '-Names', $names, '-GraceSeconds', '1')
    Check ($code -eq 0) "script exits 0 when every own sidecar is gone (got $code)"
    Assert-Outcome $pids 'script'
    Stop-All

    # 2. A trailing separator and a different letter case name the same folder.
    $pids = Start-All
    # Two backslashes: the argument is quoted (it has a space), and a single trailing backslash
    # would escape the closing quote in Windows argv parsing. The script then sees exactly one.
    $code = Invoke-Script @('-InstallDir', ($install.ToUpperInvariant() + '\\'), '-Names', $names, '-GraceSeconds', '1')
    Check ($code -eq 0) "script accepts a trailing separator and other case (got $code)"
    Assert-Outcome $pids 'script (case, trailing \)'
    Stop-All

    # 3. Nothing of ours running: a no-op, exit 0, nothing else touched.
    $decoyOnly = Start-Process -FilePath $layout.decoy_hermes -PassThru -WindowStyle Hidden
    $started.Add($decoyOnly.Id)
    $code = Invoke-Script @('-InstallDir', $install, '-Names', $names)
    Check ($code -eq 0) "script exits 0 when none of its sidecars runs (got $code)"
    Check (Is-Alive $decoyOnly.Id) 'script leaves the decoy running when nothing of ours runs'
    Stop-All

    # 4. A name that is not a plain file name is refused before anything is signalled.
    $pids = Start-All
    $code = Invoke-Script @('-InstallDir', $install, '-Names', 'hermes,..\Somewhere Else\hermes')
    Check ($code -eq 2) "script refuses a name with a path in it (got $code)"
    Check (Is-Alive $pids.own_hermes) 'a refused call signals nothing'
    Stop-All

    # 5. Through the NSIS hook, compiled into a minimal installer.
    if ($MakeNsis -ne '') {
        $harness = Join-Path $base 'hook-harness.exe'
        $ErrorActionPreference = 'Continue'
        $result = Join-Path $base 'hook-result.txt'
        & $MakeNsis -V2 "-DHOOKS=$hooks" "-DOUTFILE=$harness" "-DCITRATE_HOOK_TEST_RESULT_FILE=$result" $harnessSrc | ForEach-Object { Write-Host "     | $_" }
        $mk = $LASTEXITCODE
        $ErrorActionPreference = 'Stop'
        Check ($mk -eq 0) "makensis builds the hook harness (got $mk)"
        if ($mk -eq 0) {
            $pids = Start-All
            # /D= must be the last argument and is taken verbatim (no quotes).
            $run = Start-Process -FilePath $harness -ArgumentList @('/S', "/D=$install") -PassThru
            $null = $run.Handle  # keep the handle so ExitCode is readable after the wait
            $run.WaitForExit()
            Check ($run.ExitCode -eq 0) "the hook harness exits 0 (got $($run.ExitCode))"
            $got = if (Test-Path -LiteralPath $result) { (Get-Content -LiteralPath $result -Raw).Trim() } else { '<none>' }
            if (Test-Path -LiteralPath "$result.log") {
                Get-Content -LiteralPath "$result.log" -Encoding Unicode | ForEach-Object { Write-Host "     | $_" }
            }
            Check ($got -eq '0') "the hook reports the script's exit code 0 (got $got)"
            Assert-Outcome $pids 'NSIS hook'
            Stop-All
        }

        # 6. #305: a stop step that never returns (PowerShell or the CIM query hanging). Built with
        # an 8 s bound and a script that starts a child process, then sleeps for ten minutes.
        $hang = Join-Path $base 'hang.ps1'
        $hangPid = Join-Path $base 'hang.pid'
        $hangChild = Join-Path $base 'hang-child.pid'
        $hangResult = Join-Path $base 'hang-result.txt'
        $hangHarness = Join-Path $base 'hang-harness.exe'
        Set-Content -LiteralPath $hang -Encoding ASCII -Value @"
`$PID | Set-Content -LiteralPath '$hangPid'
`$c = Start-Process -FilePath '$sleeper' -PassThru -WindowStyle Hidden
`$c.Id | Set-Content -LiteralPath '$hangChild'
Start-Sleep -Seconds 600
"@
        $ErrorActionPreference = 'Continue'
        & $MakeNsis -V2 "-DHOOKS=$hooks" "-DOUTFILE=$hangHarness" '-DCITRATE_STOP_TIMEOUT_MS=8000' "-DCITRATE_STOP_SCRIPT_SRC=$hang" "-DCITRATE_HOOK_TEST_RESULT_FILE=$hangResult" $harnessSrc | ForEach-Object { Write-Host "     | $_" }
        $mk = $LASTEXITCODE
        $ErrorActionPreference = 'Stop'
        Check ($mk -eq 0) "makensis builds the hung-hook harness (got $mk)"
        if ($mk -eq 0) {
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            $run = Start-Process -FilePath $hangHarness -ArgumentList @('/S', "/D=$install") -PassThru
            $null = $run.Handle
            $exited = $run.WaitForExit(90000)
            $sw.Stop()
            if (-not $exited) { Stop-Process -Id $run.Id -Force -ErrorAction SilentlyContinue }
            Check $exited 'the hung-hook harness finishes instead of waiting on the stop step'
            $secs = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            # 8 s bound + up to 5 s reap, plus start-up slack on a hosted runner.
            Check ($exited -and $sw.Elapsed.TotalSeconds -lt 30) "it finishes within the outer bound (took $secs s)"
            if ($exited) { Check ($run.ExitCode -eq 0) "the install continues after a timeout (exit $($run.ExitCode))" }
            $got = if (Test-Path -LiteralPath $hangResult) { (Get-Content -LiteralPath $hangResult -Raw).Trim() } else { '<none>' }
            Check ($got -eq 'timeout') "the hook reports the timeout (got $got)"
            Check (Test-Path -LiteralPath $hangChild) 'the hung script ran (so the bound covered running code, not only start-up)'
            foreach ($f in @($hangPid, $hangChild)) {
                if (Test-Path -LiteralPath $f) {
                    $id = [int](Get-Content -LiteralPath $f -Raw).Trim()
                    $started.Add($id)
                    Check (-not (Is-Alive $id)) "the process the hook started is gone afterwards ($([System.IO.Path]::GetFileName($f)), pid $id)"
                }
            }
            Stop-All
        }
    } else {
        Write-Host 'skip - NSIS hook run (no -MakeNsis given)'
    }
} finally {
    Stop-All
    Remove-Item -Recurse -Force $base -ErrorAction SilentlyContinue
}

if ($failures.Count -gt 0) {
    Write-Host "$($failures.Count) check(s) failed"
    exit 1
}
Write-Host 'all checks passed'
exit 0
