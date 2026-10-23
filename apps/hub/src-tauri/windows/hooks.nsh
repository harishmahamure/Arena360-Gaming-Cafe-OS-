; Arena 360 Hub installer hooks (Tauri NSIS `installerHooks`).
;
; - The Hub supports Windows 10 version 1803 (build 17134) or later. Older builds are refused
;   before anything is installed.
; - The arena-hub service runs detached from the UI, so its running executable would block an
;   upgrade or uninstall: both stop it first.

; Stops a running arena-hub.exe; "not running" (a non-zero exit) is fine.
!macro ARENA_STOP_HUB_SERVICE
  nsExec::Exec 'taskkill /IM arena-hub.exe /F'
  Pop $0 ; taskkill's exit code, ignored
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  Push $0
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ; $0 < 17134 (or unreadable, which compares as 0): refuse. Equal or greater: continue.
  IntCmp $0 17134 arena_windows_ok arena_windows_too_old arena_windows_ok
  arena_windows_too_old:
    Pop $0
    ; NSIS cannot read the JS catalogue: this text duplicates
    ; packages/ui/src/messages/en.json key "hub.installer.windows_too_old"
    ; (scripts/installer-hooks.test.mjs checks they match).
    MessageBox MB_OK|MB_ICONSTOP "Arena 360 Hub needs Windows 10 version 1803 or later." /SD IDOK
    Abort "Arena 360 Hub needs Windows 10 version 1803 or later."
  arena_windows_ok:
  !insertmacro ARENA_STOP_HUB_SERVICE
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Push $0
  !insertmacro ARENA_STOP_HUB_SERVICE
  Pop $0
!macroend
