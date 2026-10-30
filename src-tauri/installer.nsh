; Keep the source directory at include time: __FILEDIR__ changes when macros expand.
!define PROPAD_HOOK_DIR "${__FILEDIR__}"
!include "LogicLib.nsh"

!macro PROPAD_SELECT_POWERSHELL
  ; NSIS normally runs as x86 even for an x64 app. Use native x64 PowerShell so
  ; exact executable-path inspection also works for the running x64 dashboard.
  StrCpy $R9 "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  ${If} ${FileExists} "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
    StrCpy $R9 "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  InitPluginsDir
  File "/oname=$PLUGINSDIR\propad-migrate.ps1" "${PROPAD_HOOK_DIR}\migrate-legacy.ps1"
  !insertmacro PROPAD_SELECT_POWERSHELL
  nsExec::ExecToStack /TIMEOUT=20000 '"$R9" -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File "$PLUGINSDIR\propad-migrate.ps1" -Mode PreInstall -InstallDirectory "$INSTDIR" -StateFile "$PLUGINSDIR\propad-install-state.json"'
  Pop $0
  Pop $1
  DetailPrint "$1"
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "无法安全更新手柄电量看板。请先从系统托盘退出旧版，再重新运行安装程序。原程序和设置尚未替换。$\r$\n$\r$\n$1" /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro PROPAD_SELECT_POWERSHELL
  nsExec::ExecToStack /TIMEOUT=10000 '"$R9" -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File "$PLUGINSDIR\propad-migrate.ps1" -Mode PostInstall -InstallDirectory "$INSTDIR" -StateFile "$PLUGINSDIR\propad-install-state.json"'
  Pop $0
  Pop $1
  DetailPrint "$1"
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "程序文件已安装，但登录启动设置未能完成迁移。请在看板设置中核对“登录时启动”。$\r$\n$\r$\n$1" /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  InitPluginsDir
  File "/oname=$PLUGINSDIR\propad-migrate.ps1" "${PROPAD_HOOK_DIR}\migrate-legacy.ps1"
  !insertmacro PROPAD_SELECT_POWERSHELL
  nsExec::ExecToStack /TIMEOUT=15000 '"$R9" -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File "$PLUGINSDIR\propad-migrate.ps1" -Mode PreUninstall -InstallDirectory "$INSTDIR" -StateFile "$PLUGINSDIR\propad-uninstall-state.json" -UpdateMode "$UpdateMode"'
  Pop $0
  Pop $1
  DetailPrint "$1"
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "看板仍在运行，卸载已停止。请先从系统托盘退出，再重新卸载。设置和备份会保留。$\r$\n$\r$\n$1" /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Tauri's template deletes the product-named Run value. Restore an unrelated
  ; value, if one occupied that name, without modifying any other startup entry.
  !insertmacro PROPAD_SELECT_POWERSHELL
  nsExec::ExecToStack /TIMEOUT=10000 '"$R9" -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File "$PLUGINSDIR\propad-migrate.ps1" -Mode PostUninstall -InstallDirectory "$INSTDIR" -StateFile "$PLUGINSDIR\propad-uninstall-state.json" -UpdateMode "$UpdateMode"'
  Pop $0
  Pop $1
  DetailPrint "$1"
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION|MB_OK "程序已卸载，但启动项保护检查未完成。用户设置与旧版备份仍保留。$\r$\n$\r$\n$1" /SD IDOK
    SetErrorLevel 2
  ${EndIf}
!macroend
