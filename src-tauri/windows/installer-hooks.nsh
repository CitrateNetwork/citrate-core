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
;
; Outer bound (#305): the script's own waits start only once PowerShell is up and the process
; list has been read, and on some machines PowerShell start-up or the CIM query does not return.
; So the step runs PowerShell itself (CreateProcess, no window) inside a job object of its own
; and waits at most CITRATE_STOP_TIMEOUT_MS for it, start-up included. On timeout it ends that
; job (the PowerShell it started and anything that PowerShell started, nothing else), says so in
; the details, and the install continues (fail-open, as above). The script's log is written to a
; file and replayed into the details, since the process has no console to capture.

!ifndef CITRATE_INSTALLER_HOOKS_NSH
!define CITRATE_INSTALLER_HOOKS_NSH

!include LogicLib.nsh

; The directory of this file, captured at include time (the hook macro expands elsewhere).
!define CITRATE_HOOKS_DIR "${__FILEDIR__}"

; The sidecars this installation ships: the basenames of bundle.externalBin in
; tauri.bundle-windows.conf.json. A unit test (src-tauri/src/windows_installer_hook_tests.rs)
; keeps the two lists equal.
!define CITRATE_SIDECARS "citrate,node-agent,mem-mcp,llama-server,ipfs,comms-member-daemon,cluster-daemon,hermes"

; Outer bound for the whole stop step, PowerShell start-up included: 30 s. The script's own
; worst case is about 10 s (5 s graceful wait + 5 s after terminate) plus the process-list reads.
; After a timeout, up to CITRATE_STOP_REAP_MS more for the ended job's processes to be gone.
!ifndef CITRATE_STOP_TIMEOUT_MS
  !define CITRATE_STOP_TIMEOUT_MS 30000
!endif
!define CITRATE_STOP_REAP_MS 5000
; The script compiled into the installer (the hook test swaps in one that never returns).
!ifndef CITRATE_STOP_SCRIPT_SRC
  !define CITRATE_STOP_SCRIPT_SRC "${CITRATE_HOOKS_DIR}\stop-own-sidecars.ps1"
!endif
; CREATE_SUSPENDED | CREATE_NO_WINDOW: the process joins the job before it runs any code.
!define CITRATE_CREATE_FLAGS 0x08000004
; sizeof(STARTUPINFOW) and sizeof(PROCESS_INFORMATION) for this installer's pointer size.
!if ${NSIS_PTR_SIZE} > 4
  !define CITRATE_SI_SIZE 104
  !define CITRATE_PI_SIZE 24
!else
  !define CITRATE_SI_SIZE 68
  !define CITRATE_PI_SIZE 16
!endif

!macro CITRATE_STOP_OWN_SIDECARS
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  Push $8
  Push $9
  InitPluginsDir
  File "/oname=$PLUGINSDIR\citrate-stop-own-sidecars.ps1" "${CITRATE_STOP_SCRIPT_SRC}"
  Delete "$PLUGINSDIR\citrate-stop-own-sidecars.log"
  DetailPrint "Stopping this installation's own background services in $INSTDIR"
  StrCpy $1 '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\citrate-stop-own-sidecars.ps1" -InstallDir "$INSTDIR" -Names "${CITRATE_SIDECARS}" -LogPath "$PLUGINSDIR\citrate-stop-own-sidecars.log"'
  ; $8 = the result: the script's exit code, "timeout", or "error" (PowerShell did not start).
  StrCpy $8 "error"
  ; $9 = a job object for the PowerShell this step starts ("0" when one could not be made).
  System::Call 'kernel32::CreateJobObjectW(p 0, p 0) p .r9'
  System::Alloc ${CITRATE_SI_SIZE}
  Pop $2
  System::Call '*$2(i ${CITRATE_SI_SIZE})'
  System::Alloc ${CITRATE_PI_SIZE}
  Pop $3
  System::Call 'kernel32::CreateProcessW(p 0, w r1, p 0, p 0, i 0, i ${CITRATE_CREATE_FLAGS}, p 0, p 0, p r2, p r3) i .r0'
  ${If} $0 != 0
    System::Call '*$3(p .r4, p .r5, i, i)'
    ${If} $9 != 0
      System::Call 'kernel32::AssignProcessToJobObject(p r9, p r4) i .r6'
      ${If} $6 = 0
        System::Call 'kernel32::CloseHandle(p r9)'
        StrCpy $9 0
      ${EndIf}
    ${EndIf}
    System::Call 'kernel32::ResumeThread(p r5)'
    System::Call 'kernel32::WaitForSingleObject(p r4, i ${CITRATE_STOP_TIMEOUT_MS}) i .r6'
    ${If} $6 = 0
      System::Call 'kernel32::GetExitCodeProcess(p r4, *i .r7)'
      StrCpy $8 $7
    ${Else}
      ; Not finished in time (or the wait failed): end only what this step started.
      StrCpy $8 "timeout"
      ${If} $9 != 0
        System::Call 'kernel32::TerminateJobObject(p r9, i 1)'
      ${Else}
        System::Call 'kernel32::TerminateProcess(p r4, i 1)'
      ${EndIf}
      System::Call 'kernel32::WaitForSingleObject(p r4, i ${CITRATE_STOP_REAP_MS})'
    ${EndIf}
    System::Call 'kernel32::CloseHandle(p r5)'
    System::Call 'kernel32::CloseHandle(p r4)'
  ${EndIf}
  ${If} $9 != 0
    System::Call 'kernel32::CloseHandle(p r9)'
  ${EndIf}
  System::Free $2
  System::Free $3
  ; The script's own log lines (UTF-16LE, no BOM), into the details.
  ClearErrors
  FileOpen $6 "$PLUGINSDIR\citrate-stop-own-sidecars.log" r
  ${IfNot} ${Errors}
    ${Do}
      ClearErrors
      FileReadUTF16LE $6 $7
      ${If} ${Errors}
        ${ExitDo}
      ${EndIf}
      ${Do}
        StrCpy $5 $7 1 -1
        ${If} $5 == "$\r"
        ${OrIf} $5 == "$\n"
          StrCpy $7 $7 -1
        ${Else}
          ${ExitDo}
        ${EndIf}
      ${Loop}
      DetailPrint $7
    ${Loop}
    FileClose $6
  ${EndIf}
  ${If} $8 == "timeout"
    DetailPrint "The check for this installation's background services did not finish within ${CITRATE_STOP_TIMEOUT_MS} ms and was stopped (result timeout); installing continues, and Windows reports any file that is still in use"
  ${ElseIf} $8 == "error"
    DetailPrint "The check for this installation's background services could not be started (result error); installing continues"
  ${ElseIf} $8 != 0
    DetailPrint "Some background services of this installation could not be stopped (result $8)"
  ${EndIf}
  !ifdef CITRATE_HOOK_TEST_RESULT_FILE
    ; Hook test builds only (hook-harness.nsi): record the result for the test to read.
    FileOpen $6 "${CITRATE_HOOK_TEST_RESULT_FILE}" w
    FileWrite $6 $8
    FileClose $6
  !endif
  Pop $9
  Pop $8
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !ifmacrodef CheckIfAppIsRunning
    !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  !endif
  !insertmacro CITRATE_STOP_OWN_SIDECARS
!macroend

!endif
