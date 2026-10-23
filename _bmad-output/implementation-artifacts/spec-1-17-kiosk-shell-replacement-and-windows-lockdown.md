---
title: 'Story 1.17: Kiosk shell replacement and Windows lockdown'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'b9ed3d22956addde691e438ffa1fda70b034c072'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-16-kiosk-privilege-split-locked-service-and-unprivileged-ui.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A player on a café PC can still reach Explorer, Task Manager, Run, settings and USB drives, and can close or tab away from the kiosk.

**Approach:** Lock down only the `player` account:
- **Shell replacement:** the account's per-user `Winlogon\Shell` launches the kiosk app, so Explorer never starts.
- **Policy keys:** set per-user keys that disable Task Manager, Run, close/logoff, lock and change password, and that hide drives and turn off autorun.
- **Keyboard hook:** a low-level hook in the kiosk app swallows Win combinations and Alt+F4.
- **Window:** a topmost window covers every monitor.

Every change is computed as a pure, testable plan of registry operations. A privileged `lockdown apply|restore` command runs that plan from the installer and uninstaller, and restore puts back the exact previous values.

## Boundaries & Constraints

**Always:**
- **Lockdown plan (`arena-kiosk-core::lockdown`, pure, cross-platform):**
  - `plan_apply(shell_path) -> Vec<RegOp>` and `plan_restore(backup) -> Vec<RegOp>`, where `RegOp = Set{key, name, value: Dword|Sz} | Delete{key, name}`.
  - Keys are relative to the player hive root.
  - Values set:
    - `Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell` = `"<install dir>\arena-kiosk-ui.exe"`
    - `...\Policies\System`: `DisableTaskMgr=1`, `DisableLockWorkstation=1`, `DisableChangePassword=1`
    - `...\Policies\Explorer`: `NoRun=1`, `NoClose=1`, `NoLogoff=1`, `NoDriveTypeAutoRun=0xFF`, `NoAutorun=1`, `NoDrives=0x03FFFFFB` (all letters except C:), `NoViewOnDrive=0x03FFFFFB`
  - Before applying, the old value of every touched name, or "absent", is captured into a JSON `LockdownBackup` stored at `%PROGRAMDATA%\Arena360\Kiosk\lockdown-backup.json`. Restore writes back exactly those values and deletes the names that were absent.
  - Applying is idempotent: applying again keeps the original backup.
- **Executor (`arena-kiosk-svc lockdown apply|restore --player-user <name>`, Windows, run elevated by the installer):**
  - Ensure a local standard (non-admin) account `<name>` exists. If it doesn't, create it with `NetUserAdd` using a random 32-character password that is not stored; auto-logon is out of scope.
  - Ensure its profile exists (`CreateProfile`).
  - Load its `NTUSER.DAT` under `HKEY_USERS\ArenaPlayerTemp` (`RegLoadKeyW`, which needs `SeRestorePrivilege` and `SeBackupPrivilege`). Run the plan there, then unload the hive. If the user is currently signed in, write to `HKEY_USERS\<SID>` instead.
  - Never touch HKLM policy or other accounts.
  - Behind a `RegistryBackend` trait, with a fake in-memory backend for tests.
- **Installer hooks (Kiosk NSIS, from 1.16):**
  - After install: `lockdown apply --player-user player`.
  - Before uninstall: `lockdown restore`. The `player` account is kept, but its shell and policy values return to their prior values.
- **Keyboard hook (in `arena-kiosk-ui`, the player session):**
  - Install `WH_KEYBOARD_LL` on a dedicated thread with a message loop.
  - A pure `fn decide(event: KeyEvent, modifiers) -> Swallow|Pass` in `arena-kiosk-core` swallows: LWin, RWin and any chord with them; Alt+F4; Alt+Tab; Alt+Esc; Ctrl+Esc; and Alt+Space.
  - It passes Ctrl+Alt+Shift+A, which is reserved for admin exit in 1.20.
  - Ctrl+Alt+Del can't be hooked; the policy keys cover it.
- **Window (`arena-kiosk-ui`):**
  - Undecorated and non-resizable, with `alwaysOnTop: true` and `skipTaskbar: true`.
  - On start and on display change, its rect is set to the union of all monitor rects (`EnumDisplayMonitors`). The union is computed in a pure function in kiosk-core.
  - The Tauri close request is prevented (`on_window_event` `CloseRequested` → `api.prevent_close()`).
  - With no session, the UI shows the catalogue "locked" placeholder ("This PC is ready. Log in at the counter or with your account."), with a small catalogue-keyed hint.
- **Non-Windows:** all Windows calls are `cfg(windows)`. On other OSes the executor command prints "lockdown is only available on Windows" and exits 2. The pure modules compile and are tested everywhere.
- **README:** a "Kiosk lockdown" section listing every key and value, explaining the backup and restore, and saying the escape suite in Story 1.19 verifies all of this on real Windows.

**Never:**
- Changing the shell or policies of administrators or other accounts.
- HKLM-wide lockdown.
- Auto-logon (later).
- The process allow-list (1.18).
- Admin exit (1.20).
- Storing the player account password.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Plan apply | an empty fake hive | every Set op listed above; the backup records all names as absent | N/A |
| Plan apply twice | after the first apply | no change; the backup is unchanged (keeps the originals) | N/A |
| Restore | a backup with Shell absent and NoRun=0 before apply | Shell deleted; NoRun set back to 0; the other added names deleted | N/A |
| Win key | LWin down | Swallow | N/A |
| Win+D chord | LWin held, then D | Swallow | N/A |
| Alt+F4 | Alt held, F4 | Swallow | N/A |
| Admin chord | Ctrl+Alt+Shift+A | Pass | N/A |
| Typing | plain letter keys | Pass | N/A |
| Monitors | (0,0,1920,1080) + (1920,-200,2560,1440) | union (0,-200,4480,1440) | N/A |
| Other OS | `lockdown apply` on Linux/macOS | prints the message, exits 2 | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-kiosk-core` -- 1.16 backoff, ipc and watchdog (proto-only layer). Add `lockdown.rs`, `keys.rs` and `monitors.rs` as pure code.
- `crates/arena-kiosk-svc` -- 1.16 traits (SecretStore and others), the `windows` crate pin, and the subcommand parsing. Add the `lockdown` subcommand and the `RegistryBackend`.
- `apps/kiosk/src-tauri/src/main.rs` and `tauri.conf.json` -- the 1.16 IPC client. Add the hook thread, window placement and prevented close.
- `apps/kiosk/src/routes/**` -- the 1.16 status line. Add the locked placeholder.
- The Kiosk NSIS hooks and installer conf from 1.16 -- add the lockdown apply/restore steps.

## Tasks & Acceptance

**Execution:**
- [x] `arena-kiosk-core`: lockdown plan and backup, key `decide`, monitor union, each with unit tests for every matrix row.
- [x] `arena-kiosk-svc`: the `lockdown` subcommand, the Windows executor (account, profile, hive load), the fake backend and the executor tests on the fake backend.
- [x] `apps/kiosk`: the Windows hook thread, topmost all-monitor window, prevented close, locked placeholder screen and catalogue keys; a Playwright check of the placeholder text.
- [x] Installer hooks, README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` on Linux and macOS, then every non-Windows matrix row passes.
- Given the CI Windows job, then the Windows code compiles.
- Given `pnpm check` and the kiosk UI tests, then both pass.

## Implementation Notes

- **Plan (`arena_kiosk_core::lockdown`).** `RegValue` is `Dword | Sz | Raw{kind, bytes}`: the lockdown only sets Dword/Sz, but a previous value of another type (e.g. a REG_BINARY `NoDriveTypeAutoRun`) is kept byte for byte so restore is exact. The Shell value is the quoted path (the install dir has spaces). `capture_backup(existing, …)` keeps an existing backup as is and only reads names it does not list yet. The backup carries `player_user` and `player_sid`.
- **Executor (`arena-kiosk-svc::lockdown`).** `PlayerAccounts` (`ensure_player`, `find_player`, `with_hive`) + `RegistryBackend`; `FakeRegistry` is public, the fake accounts live in the tests. The backup is saved before the first registry write (a failed apply can still be restored). Apply refuses an administrator account and a backup for another account/SID; restore deletes the backup when done (no backup: nothing to do; account deleted: backup dropped). `--player-user` is optional (apply defaults to `player`, restore to the backup's account). The shell path is `configured_app_path` (config `app_path`, else next to the service). Windows: `NetUserGetInfo`/`NetUserAdd` (USER_PRIV_USER, random 32-char password from `BCryptGenRandom` covering all four classes, never stored), `CreateProfile`, ProfileList `ProfileImagePath` (read-only HKLM), `HKEY_USERS\<SID>` if loaded else `RegLoadKeyW` under `ArenaPlayerTemp` (privileges enabled, stale load unloaded first, unload retried on drop). `protect_data_dir` runs before either action.
- **App.** `shell.rs`: `cover_all_monitors` (Windows: `EnumDisplayMonitors`; elsewhere Tauri's monitors) → union → physical position/size, then `show()` (the window starts `visible: false`; `fullscreen` was dropped because it covers one monitor). A `kiosk-input` thread installs `WH_KEYBOARD_LL` and a hidden top-level window that receives `WM_DISPLAYCHANGE` and re-places the window on the main thread. `KeyFilter` tracks modifiers itself plus the hook's `LLKHF_ALTDOWN`. Beyond the spec: debug builds honour `ARENA_KIOSK_DEV_UNLOCKED=1` (no hook, not topmost, closable) so a dev's Windows desktop stays usable; release builds ignore it.
- **UI.** `player.locked.ready` / `player.locked.hint`; Playwright `tests/locked.spec.ts`. Installer texts `kiosk.installer.lockdown_failed` / `lockdown_restore_failed`; restore runs before the service is removed, and the pre-install (upgrade) hook never restores.
- **Windows compile check locally:** as in 1.16, plus a stub `llvm-rc` on PATH for tauri-winres (in a dir that does **not** shadow the host `cc`/`ar`), and `RUSTC` pointed at the rustup 1.95 toolchain (Homebrew's cargo ignores `rust-toolchain.toml`).

### Review-fix round (coordinator feedback)

- **Plan additions.** `DisableRegistryTools=1`; the three accessibility-hotkey `Flags` (StickyKeys 506, Keyboard Response 122, ToggleKeys 58, REG_SZ). New `RegOp::DeleteKeyIfEmpty`; backup now records per-key existence and, for the three protected keys, the previous DACL SDDL. `plan_restore` restores values, then deletes keys the lockdown created that are left empty (deepest first). After apply the protected keys' DACLs are tightened (SYSTEM/Admins full, player read, inheritance broken); restore puts them back. `capture_backup` gained `key_exists`/`key_sddl` closures. `KeyFilter::resync`/`on_event_synced` take the OS's live modifier state (`GetAsyncKeyState`), seeded at install.
- **Executor.** `load_backup` enforces `BACKUP_VERSION`. `apply` validates the player name (≤20, no trailing dot) and refuses a shell that is missing or not named `arena-kiosk-ui.exe`; `ensure_player` also refuses membership in Administrators/Power Users/Backup Operators/Remote Desktop Users/Hyper-V Admins (by group SID RID). `with_hive` holds a process mutex and a machine-wide `Global\ArenaKioskLockdown`, and loads the temp hive under a per-pid mount name. `user_sid` qualifies with the computer name; `get` loops on `ERROR_MORE_DATA`. New `lockdown reset-shell` (fallback → Explorer). `main.rs` apply reads `player_user` from config when no flag.
- **App.** `claim_single_instance` (`Global\ArenaKioskUi`) — a second instance exits. Hook install retries with backoff and re-installs every 60 s; while inactive it reports over IPC and the service queues `kiosk.log{error, lockdown_degraded}` (new `Bridge::set_lockdown_degraded`, re-sent on reconnect). `WM_DPICHANGED` re-covers; `WM_ACTIVATEAPP` re-asserts topmost/focus; `union` None falls back to the primary rect, then fullscreen; a missing window still starts the hook. `harden_webview` (`with_webview`, webview2-com) disables context menus, browser accelerator keys, devtools and status bar; `guards.ts` repeats this in the webview (context menu, the key combos, `window.print`) and positions the locked placeholder on the primary monitor.
- **Tests.** Core: created-key cleanup, protected-SDDL capture, modifier resync. Svc: shell/name refusals, group refusal (fake), failing-restore-keeps-backup, `player_sid: None` restore, reset-shell, version check; `tests/exit_codes.rs` (off-Windows exit 2). App: `shell::tests::dev_unlock_is_release_gated` (release-gated), Playwright `guards.spec.ts`. The hive load/unload and real ACL work remain Windows-only, covered by the 1.19 escape suite (type-checked here via the msvc target).

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Two app instances: Winlogon shell plus the 1.16 watchdog launcher; no single-instance guard | high | Two topmost windows and hooks; one copy is outside the job. | patch (single-instance mutex; the service adopts the shell instance into its job) |
| 2 | Lockdown fails open when the hook can't install, or Windows silently removes it (blind, edge) | high | Only a log line. | patch (retry, heartbeat check, report degraded via IPC/kiosk.log) |
| 3 | WebView2 escapes: context menu, browser accelerators (print, devtools, find) and file pickers | high | Common dialogs browse the file system. | patch |
| 4 | Accessibility hotkeys (Sticky/Filter/Toggle keys) open system dialogs | high | Classic kiosk escape. | patch (Flags values in the plan) |
| 5 | Player-writable HKCU Winlogon/Policies keys let the player rewrite their own Shell | high | Default key security. | patch (SYSTEM/Admins ACL on touched keys plus DisableRegistryTools) |
| 6 | KeyFilter modifiers stick after the secure desktop; Ctrl/Win already down at install time (edge, verify-gap) | medium | Swallows plain keys until restart. | patch (resync via GetAsyncKeyState) |
| 7 | Restore leaves created empty keys; "exact" claim is false (blind, edge) | low | Backup doesn't record key existence. | patch |
| 8 | `lockdown apply` ignores config `player_user`; installer hard-codes it | medium | Locks down the wrong account. | patch |
| 9 | Shell path never checked to exist | medium | Black screen on sign-in. | patch |
| 10 | Changes only take effect at the next sign-in, which isn't surfaced | low | The installer reports success while the session is unlocked. | patch (warn plus a documented sign-out) |
| 11 | Mixed DPI resize, dead zones, content on a seam; topmost/focus not re-asserted (blind, edge) | medium | Window can shrink or lose top. | patch |
| 12 | Upgrade may run the old uninstaller's restore | medium | Unlocked mid-upgrade. | patch (skip restore on update; detect via the NSIS update flag) |
| 13 | A failed lockdown still reports install success; a failed restore deletes files (blind, edge, verify-gap) | medium | Silent fleet risk. | patch (fail the install with a non-zero exit; abort uninstall or reset Shell first) |
| 14 | Admin check narrow (other privileged groups); unqualified account name | low | Direct fix. | patch |
| 15 | Created account unusable until auto-logon; not documented | low | Doc fix. | patch |
| 16 | Windows executor untested (hive load, profile, privileges) (blind, verify-gap) | medium | Needs real Windows. | patch (Windows-only tests behind `#[ignore]` run in CI's Windows job where possible) + defer the end-to-end run to 1.19 |
| 17 | Service install failure still runs lockdown | medium | PC unusable. | patch |
| 18 | Backup version not checked; ERROR_MORE_DATA; concurrent runs share the hive name; player name >20 chars | low | Direct fixes. | patch |
| 19 | Failed restore keeps the backup — untested (verify-gap) | low | Pre-verified gap. | patch |
| 20 | Off-Windows exit 2 untested (verify-gap) | low | Pre-verified gap. | patch |
| 21 | Shell wiring (close refusal, hook mapping, `dev_unlocked` gate) untested (verify-gap) | low | Needs a real session; the gate unit test is cheap. | patch (gate test) + defer the rest to 1.19 |
| 22 | Backup with `player_sid: None` path untested | low | Direct test. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked` -- expected: pass
- `cargo run -p arena-kiosk-svc -- lockdown apply --player-user player` on macOS -- expected: the "only on Windows" message, exit 2
- `pnpm check` -- expected: pass
