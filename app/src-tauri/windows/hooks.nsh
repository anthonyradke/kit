; kit's NSIS installer hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).

; Uninstalling: take kit's audio effects (noise suppression, the EQ) off the
; headset and remove them, since they live outside kit's folder, in Program
; Files and the audio engine's settings. kit.exe asks for administrator
; rights only when something is installed. Updates don't run the uninstaller,
; but skip it in update mode all the same.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\kit.exe" --remove-audio-effects' $0
  ${EndIf}
!macroend
