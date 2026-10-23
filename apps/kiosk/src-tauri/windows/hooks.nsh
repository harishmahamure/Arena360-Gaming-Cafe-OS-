; Arena 360 Kiosk installer hooks (Tauri NSIS `installerHooks`).
;
; - The kiosk supports 64-bit Windows 10 version 1803 (build 17134) or later with at least 4 GB
;   of RAM. Anything else is refused before anything is installed, with a message naming the
;   requirement.
; - arena-kiosk-svc (bundled as an externalBin next to the app) is the LocalSystem service:
;   it is registered after install, and stopped and removed before an upgrade or uninstall.
; - The `player` account is locked down after install (`lockdown apply`: the kiosk as its shell,
;   the policy keys, no drives or autorun; Story 1.17) and put back before uninstall
;   (`lockdown restore`: its previous values from the backup; the account is kept).
; - NSIS cannot read the JS catalogue: each refusal text duplicates the
;   packages/ui/src/messages/en.json key named next to it (scripts/kiosk-installer.test.mjs
;   checks they match).

!include x64.nsh
!include FileFunc.nsh

; Stops and removes an installed arena-kiosk-svc; "not installed" (or no exe yet) is fine.
!macro ARENA_REMOVE_KIOSK_SERVICE
  nsExec::Exec '"$INSTDIR\arena-kiosk-svc.exe" service uninstall'
  Pop $0 ; exit code, ignored
!macroend

!macro NSIS_HOOK_PREINSTALL
  Push $0
  Push $1

  ; Windows 10 1803 (build 17134) or later. Unreadable compares as 0: refused.
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  IntCmp $0 17134 arena_build_ok arena_build_too_old arena_build_ok
  arena_build_too_old:
    Pop $1
    Pop $0
    ; key "kiosk.installer.windows_too_old"
    MessageBox MB_OK|MB_ICONSTOP "Arena 360 Kiosk needs Windows 10 version 1803 or later." /SD IDOK
    Abort "Arena 360 Kiosk needs Windows 10 version 1803 or later."
  arena_build_ok:

  ; 64-bit Windows only.
  ${IfNot} ${RunningX64}
    Pop $1
    Pop $0
    ; key "kiosk.installer.not_64bit"
    MessageBox MB_OK|MB_ICONSTOP "Arena 360 Kiosk needs 64-bit Windows." /SD IDOK
    Abort "Arena 360 Kiosk needs 64-bit Windows."
  ${EndIf}

  ; At least 4 GB of installed RAM (GetPhysicallyInstalledSystemMemory, in KB). A failed call
  ; leaves 0: refused.
  StrCpy $0 0
  System::Call "kernel32::GetPhysicallyInstalledSystemMemory(*l .r0) i .r1"
  IntCmp $0 4194304 arena_ram_ok arena_ram_too_low arena_ram_ok
  arena_ram_too_low:
    Pop $1
    Pop $0
    ; key "kiosk.installer.ram_too_low"
    MessageBox MB_OK|MB_ICONSTOP "Arena 360 Kiosk needs at least 4 GB of memory." /SD IDOK
    Abort "Arena 360 Kiosk needs at least 4 GB of memory."
  arena_ram_ok:

  ; An upgrade: the old service holds its executable open.
  !insertmacro ARENA_REMOVE_KIOSK_SERVICE
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  nsExec::ExecToLog '"$INSTDIR\arena-kiosk-svc.exe" service install'
  Pop $0
  ; The kiosk is not safe to run without the service: a failure aborts the install (so does a
  ; failed lockdown below). /SD IDOK makes the message auto-dismiss in silent mode; the Abort
  ; still returns non-zero.
  StrCmp $0 "0" arena_service_ok
    ; key "kiosk.installer.service_failed"
    MessageBox MB_OK|MB_ICONSTOP "The Arena 360 Kiosk service could not be installed." /SD IDOK
    Abort "The Arena 360 Kiosk service could not be installed."
  arena_service_ok:
  ; No hard-coded account: lockdown reads player_user from the provisioned config.
  nsExec::ExecToLog '"$INSTDIR\arena-kiosk-svc.exe" lockdown apply'
  Pop $0
  StrCmp $0 "0" arena_lockdown_ok
    ; key "kiosk.installer.lockdown_failed"
    MessageBox MB_OK|MB_ICONSTOP "The player account could not be locked down. The kiosk is not secure until this is fixed." /SD IDOK
    Abort "The player account could not be locked down. The kiosk is not secure until this is fixed."
  arena_lockdown_ok:
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Push $0
  Push $1
  ; An in-place update runs this uninstaller before reinstalling; keep the lockdown so the
  ; player is never handed a desktop between the two. Tauri passes /UPDATE on that run.
  ${GetOptions} $CMDLINE "/UPDATE" $1
  IfErrors arena_do_restore
    Goto arena_skip_restore
  arena_do_restore:
  ; Before the files go: the player's shell must not point at a removed app.
  nsExec::ExecToLog '"$INSTDIR\arena-kiosk-svc.exe" lockdown restore'
  Pop $0
  StrCmp $0 "0" arena_skip_restore
    ; key "kiosk.installer.lockdown_restore_failed"
    MessageBox MB_OK|MB_ICONEXCLAMATION "The player account's previous settings could not be restored." /SD IDOK
    ; Fallback so the PC is still usable: point the player's Shell back at Explorer.
    nsExec::ExecToLog '"$INSTDIR\arena-kiosk-svc.exe" lockdown reset-shell'
    Pop $0
  arena_skip_restore:
  !insertmacro ARENA_REMOVE_KIOSK_SERVICE
  Pop $1
  Pop $0
!macroend
