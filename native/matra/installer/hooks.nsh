!define MATRA_HOOK_DIR "${__FILEDIR__}"
Var MatraPrepare

!macro MATRA_PREPARE
  nsExec::ExecToStack '"$MatraPrepare" --prepare-install "$INSTDIR"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "Matra could not prepare the upgrade. $1" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_PREINSTALL
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=matra-prepare.exe "${MATRA_HOOK_DIR}\..\..\target\hook\release\matra-hook.exe"
  StrCpy $MatraPrepare "$PLUGINSDIR\matra-prepare.exe"
  !insertmacro MATRA_PREPARE
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToStack '"$INSTDIR\matra.exe" setup-migrate'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "Matra files were installed, but migration failed. $1" /SD IDOK
    SetErrorLevel 1
    Quit
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $MatraPrepare "$INSTDIR\matra-hook.exe"
  !insertmacro MATRA_PREPARE
  ; Tauri's update mode reuses the installation. Do not remove saved opt-ins.
  ${If} $UpdateMode != 1
    nsExec::ExecToStack '"$INSTDIR\matra.exe" setup-uninstall'
    Pop $0
    Pop $1
    ${If} $0 != 0
      MessageBox MB_OK|MB_ICONSTOP "Matra could not clean up its hooks and startup entry. $1" /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
!macroend
