---
title: "Story 6.26: Launcher cards open the player's own Steam, Epic or Riot account"
type: "feature"
created: "2026-10-29"
status: "done"
baseline_commit: "cc86d71"
context:
  - "{project-root}/_bmad-output/planning-artifacts/epics.md"
  - "{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md"
  - "{project-root}/_bmad-output/implementation-artifacts/spec-6-23-the-games-tab-shows-the-library-and-launches-a-game-in-one-c.md"
warnings: []
deferred:
  - "The card's mark is the launcher's initial on a solid square: the Steam, Epic and Riot logo artwork is not bundled (a licensing and asset decision). `LauncherCard` takes the id, so artwork is a later swap inside the card."
  - "A launcher is listed when it is installed and opened only if the process allow-list lets its executable start. The Hub adds a launcher's install rules to the allow-list only while the library holds a game for that launcher, so on a branch with no Steam game a Steam card is shown but opening it is refused and logged (`kiosk.log{error, launcher_open_failed}`, HUD toast). Having the Hub allow the launchers that cards show is a later change."
  - "The usual install places are the 64-bit default folders (`Program Files (x86)\\Steam`, `Epic Games\\Launcher`, `C:\\Riot Games`); a launcher elsewhere is found only through the scan's recorded folder, which is written at the next scan (first connection, then daily 05:00)."
  - "An open launcher outside this service is found by image name in any session; it is brought forward only if it has a visible window in the player's session."
  - "Real Windows is not exercised here: `CreateProcessAsUserW` for a launcher, the Toolhelp process search, and `SetForegroundWindow` on a real launcher window. `tests/scan.rs` is Unix-only and was compiled but not run."
---

<intent-contract>

## Intent

**Problem:** A player who owns games on Steam, Epic or Riot has no way to reach their own account from the kiosk: the Games tab lists only the café's library, and the shell has no desktop to find a launcher from.

**Approach:** The kiosk service answers two more app requests. `launchers` lists the launchers installed on this PC (Steam, Epic, Riot, in that order), found at the folder the Story 6.4 scan recorded (`scan.launchers` in the queue kv, written whenever a scan is queued) or at the launcher's usual place. `launcher_open{id}` starts that launcher's executable in the player's session through the existing `GameHost`, after the same allow-list check games get. A launcher is signed out at every session end (Story 6.12), so a fresh start is on its login screen and the service never sees credentials. A launcher already open (one this service started, or any of its programs found by image name) is not started again: the answer is `already_running{pid}` and the shell brings its window forward. A start queues a new device intent `launcher.opened`; the Hub writes a `launcher_opened` event with the launcher, the live session and the player. The app shows a `launcher-card` row, "Play on your own account", above the filter rail and the grid.

## Boundaries & Constraints

**Always:** the service decides (installed, allowed, already open); the app only asks, naming `steam`, `epic` or `riot` and never a path; a launcher never starts twice at once; a failure is logged to the Hub as `kiosk.log{error, launcher_open_failed}` and the player sees the HUD's one "Couldn't start <name>" toast; nothing starts while the PC is locked or has no session; a launcher that is not installed has no card.

**Never:** a path, URI or argument from the app; a launcher other than the three; a credential read, stored or typed by the service (the vault is Epic 13); a new permission; a second mutation path (the Hub record is an intent through `apply()`); an open launcher counted as "a game running" (the media behind the tab keeps playing).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Row | Steam and Riot installed | two cards, Steam then Riot, each with mark, name, "Use your own account" |
| None installed | no launcher found | no row at all |
| Open | card activated | `launcher_open{id}`; service starts the executable; `launcher.opened` queued |
| Already open | card again, or opened by a game's link | `already_running{pid}`; shell brings the window forward; no second copy, no second record |
| Open now | launcher up | card shows "Open" and keeps its words as its name |
| Not installed / not allowed / will not start | refused | `failed`; `kiosk.log` error; HUD toast; no `launcher.opened` |
| Locked / no session | request | `FORBIDDEN`, nothing starts; the list is empty |
| Session ends | player signed in | Story 6.12 signs the launcher out and removes its saved login |
| Hub unreachable | any | still opens; `launcher.opened` waits in the queue |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{kiosk,intent,scope,lib}.rs` -- `KioskLauncherOpened`, `OPENABLE_LAUNCHERS`, `IntentKind::KioskLauncherOpened` (`launcher.opened`, device actor, paired scope).
- `crates/arena-core/src/apply/{kiosk_log,mod,mutate,validate}.rs` -- one `device_events` row `launcher_opened` (JSON detail with launcher, session, player).
- `crates/arena-api/src/lan/session.rs` -- the device event rate limit covers it.
- `crates/arena-kiosk-svc/src/{launchers,games,games_windows,app,intents,lib}.rs`, `scan/schedule.rs` -- the `launchers` / `launcher_open` requests, launcher tracking apart from games, `GameHost::find_process` (Toolhelp on Windows), the scan's launcher record.
- `apps/kiosk/src-tauri/src/{main,bridge}.rs`, `ipc-names.json` -- `launchers`, `launcher_open` commands (window to front, failure toast).
- `apps/kiosk/src/lib/{games.ts,LauncherCard.svelte,Games.svelte,Shell.svelte,bridge.ts}`, `routes/+page.svelte`, `packages/ui/src/messages/en.json`, `docs/kiosk-accessibility.md` -- the row, polling with the games, strings.

## Tasks & Acceptance

- [x] `launcher.opened` intent (proto, apply, scope, rate limit, proptest op, generated types)
- [x] Kiosk service: `launchers` and `launcher_open`, install lookup (scan record, usual places), allow-list check, already-open lookup, queued record, fake and Windows hosts
- [x] The scan records the launchers it found for the cards
- [x] Tauri shell: commands, window to front, failure toast
- [x] Kiosk app: `launcher-card` row, polling, `Open` state
- [x] Tests: Rust (proto, apply, proptest, kiosk-svc launchers), Playwright (`launchers.spec.ts`)

## Spec Change Log

- The Hub's record is a `device_events` row (`launcher_opened`) from a new intent kind, as `game.launched` is for games, rather than a `kiosk.log`: `kiosk.log` carries no player.
- Launchers are tracked apart from games in `GameState`, so an open launcher does not stop the tile media (`any_running` is games only).
- The card has no `aria-label`: its text is its accessible name (axe `label-content-name-mismatch` flagged a label that differed from the visible text).

## Verification

- `cargo test -p arena-proto` 240 pass (`launcher.opened` validation, kind tables); `-p arena-core` lib 1149 pass (`launcher_opened_writes_one_info_event...`), `proptest_apply` passes (new `LauncherOpen` op); `-p arena-api` lib 290 and `--test lan` 71 pass.
- `cargo test -p arena-kiosk-svc --lib` 182 pass, including 7 new `launchers::tests`; the same 14 lockdown / pipe / DPAPI / config / `procs::the_app_path_is_re_read_from_the_config` tests fail as before (Windows privileges on this host). `-p arena-kiosk-ui --bins` 11 pass.
- `cargo fmt --check` clean; `cargo clippy -D warnings` clean on proto, core, api, kiosk-svc lib and kiosk-ui (the `tests/discovery.rs` unused-import errors predate this story).
- `pnpm --filter @arena/kiosk check` 0 errors; Playwright kiosk suite 262 pass (7 new in `launchers.spec.ts`); the 4 `theme.spec.ts` tests fail as before (doubled drive-letter path). `check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` pass; `check:generated` lists the new generated types until they are committed.

Not run: a real Hub-to-kiosk round trip, the Tauri shell, `CreateProcessAsUserW` for a launcher or `SetForegroundWindow` on a real launcher window on Windows; `tests/scan.rs` (Unix only).
