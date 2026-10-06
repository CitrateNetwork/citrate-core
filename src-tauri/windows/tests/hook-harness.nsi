; SCL-S0.7 / S1.6a: a minimal installer that compiles src-tauri/windows/installer-hooks.nsh and
; runs NSIS_HOOK_PREINSTALL against the folder given with /D=, exactly as Tauri's Install section
; does (SetOutPath $INSTDIR, the hook, then the main-app check). Used by
; test-stop-own-sidecars.ps1 on a hosted Windows runner; never shipped.
;
; makensis /DHOOKS=<path to installer-hooks.nsh> /DOUTFILE=<harness.exe> hook-harness.nsi
; <harness.exe> /S /D=<install dir>        (/D= must be last and unquoted)

Unicode true
!ifndef HOOKS
  !error "pass /DHOOKS=<path to installer-hooks.nsh>"
!endif
!ifndef OUTFILE
  !define OUTFILE "hook-harness.exe"
!endif

!define PRODUCTNAME "Citrate Core hook harness"
!define MAINBINARYNAME "citrate-core-hook-harness-main"

!include LogicLib.nsh

; Compile-shape stand-in for Tauri's CheckIfAppIsRunning (utils.nsh): same signature and the same
; ${__LINE__}-based labels, so the harness proves the hook compiles where Tauri expands it twice in
; one section. It looks for no process: the main-app check is Tauri's, not under test here.
!macro CheckIfAppIsRunning executableName productName
  !define UniqueID ${__LINE__}
  DetailPrint "main-app check (stand-in) for ${executableName}"
  Goto app_check_done_${UniqueID}
  app_check_done_${UniqueID}:
  !undef UniqueID
!macroend

!include "${HOOKS}"

Name "${PRODUCTNAME}"
OutFile "${OUTFILE}"
RequestExecutionLevel user
SilentInstall silent
InstallDir "$TEMP\citrate-hook-harness"

Section Install
  SetOutPath $INSTDIR
  !ifmacrodef NSIS_HOOK_PREINSTALL
    !insertmacro NSIS_HOOK_PREINSTALL
  !else
    !error "installer-hooks.nsh defines no NSIS_HOOK_PREINSTALL"
  !endif
  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
SectionEnd
