; SCL-S0.7 (RT-04): Tauri NSIS installer hooks for Citrate Core on Windows.
; Referenced from src-tauri/tauri.bundle-windows.conf.json (bundle > windows > nsis > installerHooks).
;
; NSIS_HOOK_PREINSTALL runs in the installer's Install section before any file is copied. During a
; manual install over a running older version (0.4.2 to 0.5.0), the older version's sidecar .exe
; files are still running and hold their image files open, so copying over them can fail or leave
; mixed versions. Tauri's own "app is running" check covers only the main binary.
;
; Order:
;   1. Tauri's own main-app check runs first (same prompt and behaviour as Tauri's template, which
;      runs it again right after this hook as a no-op), so the old app's supervisors cannot start
;      a sidecar again while we stop them.
;   2. stop-own-sidecars.ps1 stops only processes whose executable path is exactly
;      $INSTDIR\<sidecar>.exe for the sidecars below: never by bare name, never by prefix, never
;      a process from another location. Graceful step first, then terminate.
; A non-zero result is logged and the install continues; a file that is still locked then gets
; NSIS's own retry prompt.

!ifndef CITRATE_INSTALLER_HOOKS_NSH
!define CITRATE_INSTALLER_HOOKS_NSH

!include LogicLib.nsh

; The directory of this file, captured at include time (the hook macro expands elsewhere).
!define CITRATE_HOOKS_DIR "${__FILEDIR__}"

; The sidecars this installation ships: the basenames of bundle.externalBin in
; tauri.bundle-windows.conf.json. A unit test (src-tauri/src/windows_installer_hook_tests.rs)
; keeps the two lists equal.
!define CITRATE_SIDECARS "citrate,node-agent,mem-mcp,llama-server,ipfs,comms-member-daemon,cluster-daemon,hermes"

!macro CITRATE_STOP_OWN_SIDECARS
  Push $0
  InitPluginsDir
  File "/oname=$PLUGINSDIR\citrate-stop-own-sidecars.ps1" "${CITRATE_HOOKS_DIR}\stop-own-sidecars.ps1"
  DetailPrint "Stopping this installation's own background services in $INSTDIR"
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\citrate-stop-own-sidecars.ps1" -InstallDir "$INSTDIR" -Names "${CITRATE_SIDECARS}"'
  Pop $0
  ${If} $0 != 0
    DetailPrint "Some background services of this installation could not be stopped (result $0)"
  ${EndIf}
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !ifmacrodef CheckIfAppIsRunning
    !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  !endif
  !insertmacro CITRATE_STOP_OWN_SIDECARS
!macroend

!endif
