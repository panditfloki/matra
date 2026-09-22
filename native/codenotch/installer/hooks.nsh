!define MATRA_HOOK_DIR "${__FILEDIR__}"
Var MatraPowerShell

!macro NSIS_HOOK_PREINSTALL
  InitPluginsDir
  StrCpy $MatraPowerShell "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  IfFileExists "$WINDIR\SysNative\WindowsPowerShell\v1.0\powershell.exe" 0 +2
    StrCpy $MatraPowerShell "$WINDIR\SysNative\WindowsPowerShell\v1.0\powershell.exe"
  SetOutPath "$PLUGINSDIR"
  File /oname=matra-migrate.ps1 "${MATRA_HOOK_DIR}\migrate.ps1"
  nsExec::ExecToStack '"$MatraPowerShell" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\matra-migrate.ps1" -Phase Pre -InstallDir "$INSTDIR"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "Matra could not prepare the upgrade. $1" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToStack '"$MatraPowerShell" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\matra-migrate.ps1" -Phase Post -InstallDir "$INSTDIR"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "Matra files were installed, but migration failed. $1" /SD IDOK
    SetErrorLevel 1
    Quit
  ${EndIf}
!macroend
