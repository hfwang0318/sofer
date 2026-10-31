!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Stopping llama-server before replacing runtime files..."
  ExecWait '"$SYSDIR\taskkill.exe" /F /IM llama-server.exe /T'
  Sleep 800
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Stopping llama-server before uninstalling..."
  ExecWait '"$SYSDIR\taskkill.exe" /F /IM llama-server.exe /T'
  Sleep 500
!macroend
