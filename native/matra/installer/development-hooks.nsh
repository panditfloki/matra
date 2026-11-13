; Development installer never migrates or removes production hooks/startup.
!define MATRA_HOOK_DIR "${__FILEDIR__}"
Var MatraPrepare
!macro MATRA_PREPARE_DEVELOPMENT
  nsExec::ExecToStack '"$MatraPrepare" --prepare-preview-install "$INSTDIR"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "Development installation stopped. $1" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  SetOutPath "$INSTDIR"
!macroend
!ifmacrondef CheckIfAppIsRunning
  !error "Development packaging requires the reviewed Tauri process-check macro."
!endif
!macroundef CheckIfAppIsRunning
!macro CheckIfAppIsRunning executableName productName
  !insertmacro MATRA_PREPARE_DEVELOPMENT
!macroend
!macro NSIS_HOOK_PREINSTALL
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=matra-prepare.exe "${MATRA_HOOK_DIR}\..\..\target\hook\release\matra-hook.exe"
  StrCpy $MatraPrepare "$PLUGINSDIR\matra-prepare.exe"
  !insertmacro MATRA_PREPARE_DEVELOPMENT
!macroend
!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $MatraPrepare "$INSTDIR\matra-hook.exe"
  !insertmacro MATRA_PREPARE_DEVELOPMENT
  ${If} $UpdateMode != 1
    nsExec::ExecToStack '"$INSTDIR\matra.exe" setup-uninstall'
    Pop $0
    Pop $1
    ${If} $0 != 0
      MessageBox MB_OK|MB_ICONSTOP "Development integrations could not be removed. $1" /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
!macroend
