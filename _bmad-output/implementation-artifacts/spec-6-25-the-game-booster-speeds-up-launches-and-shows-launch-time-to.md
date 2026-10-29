---
title: 'Story 6.25: The game booster speeds up launches and shows launch time to staff'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '07d303f'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-23-the-games-tab-shows-the-library-and-launches-a-game-in-one-c.md'
warnings: []
deferred:
  - 'A game that starts through a stub or launcher process (the tracked process hands over to a child) is measured by the tracked process only: a stub that never shows a window gives a launch with no time. Walking the process tree is a later change.'
  - 'A PC without a High performance power plan (some Modern Standby machines hide it) keeps its own plan: `powercfg /setactive` fails, the failure is logged, and the game still starts.'
  - 'The launch record is queued once the first window is seen, the process exits, or 3 minutes pass; a service restart or crash in that window loses the record (the game itself keeps running).'
  - 'Real Windows is not exercised here: `SetPriorityClass`, `powercfg`, closing programs and the `window-watch` helper in a real player session.'
---

<intent-contract>

## Intent

**Problem:** A game starts beside whatever else the PC is running, at normal priority on a balanced power plan, and staff cannot tell which PCs launch games slowly.

**Approach:** Three branch settings (`kiosk.booster_close`, `kiosk.booster_priority`, `kiosk.booster_power_plan`, edited in Settings → Games → Booster through `settings.set`) reach every PC like any `kiosk.*` setting. A launch (Story 6.23) first closes the listed programs through the process enforcer's kill path, raises the power plan when enabled (remembering the original in the queue kv), starts the game, then sets its priority. The session-end cleanup gets a `power_plan` step that restores the remembered plan. From the click, the service watches for the started process's first window (a `window-watch` helper in the player's session, since a session-0 service cannot see its windows); `game.launched` is queued once it is known and carries `launch_ms` (`null` on exit or after 3 minutes). The Hub reads `game_launched` events for the tile sheet's "Last launch" line and each game's 7-day median per PC.

## Boundaries & Constraints

**Always:** a program that will not close, a plan or priority that will not change, or a missing helper is logged (a `kiosk.log` warning for the Hub) and the game still starts; no error is shown to the player; the original power plan is restored by the cleanup and a failed restore keeps the memory for the next cleanup; only a GUID reaches `powercfg`; the app, the service and protected images are never closed.

**Never:** a new intent kind or permission; a path or argument from the app; the booster for an `app` entry.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Defaults | nothing set | list empty, priority Above normal, power plan off; close nothing, set Above normal |
| Close list | `discord.exe, OneDrive.exe` | closed before the game starts; nothing started yet when asked |
| Stubborn program | closer reports failures | `kiosk.log` warn `booster_failed`; game starts |
| Priority Normal | `normal` | nothing set |
| Power plan on | Balanced active | original kept in kv, High performance set; a second launch keeps the first original |
| Already High | High active | nothing changed, nothing remembered |
| Session ends | kv holds a plan | cleanup `power_plan` sets it back, clears kv |
| First window at 14.2 s | helper exits 0 | `game.launched{launch_ms: 14200}`, `issued_at` = the click |
| No window / exits first | 3 min or exit | `game.launched{launch_ms: null}`, no error |
| Tile sheet | PC has a measured launch | "Last launch  Game · 14.2 s" |
| Settings → Games | launches in 7 days | per PC, per game: median seconds and launch count, or "no time measured" |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{booster,kiosk,lib,scope}.rs` -- keys, defaults, `parse_booster_programs`, `BoosterPriority`, `median_ms`, `KioskGameLaunched.launch_ms`, the launch-times view types.
- `crates/arena-core/src/{settings_defaults,launch_times,snapshot}.rs`, `apply/{settings_set,kiosk_log}.rs` -- the three settings (defaults, rules, keyed fields), `launch_ms` in the event detail, the median/last-launch build, `SnapshotReader::game_launch_times`.
- `crates/arena-store/src/{kiosk_scans,lib}.rs` -- `list_game_launches` / `Store::game_launches`.
- `crates/arena-api/src/{ui_library,ui_auth}.rs` -- `GET /ui/games/launch-times`.
- `crates/arena-kiosk-svc/src/{operating,games,games_windows,cleanup,cli,main}.rs` -- the booster read from the setting cache, the launch flow, window watch, `WinGames` priority / power / watcher, the `power_plan` cleanup step, `window-watch <pid>`.
- `apps/hub/src/lib/{booster.ts,BoosterPanel.svelte,TileSheet.svelte}`, `apps/hub/src/routes/settings/games/+page.svelte`, `packages/ui/src/messages/en.json` -- the Booster panel, launch times, the sheet line.

## Tasks & Acceptance

- [x] Settings `kiosk.booster_close` / `_priority` / `_power_plan` (defaults, rules, `settings.set` validation, pushed to paired PCs)
- [x] Launch: close the listed programs, start, set priority, switch the power plan; failures logged, game still starts
- [x] Session end restores the original power plan (`power_plan` cleanup step, survives a restart)
- [x] Launch time: window watcher, `game.launched.launch_ms`, 3 minute timeout with no time and no error
- [x] Hub: `GET /ui/games/launch-times`, Settings → Games → Booster panel with 7-day medians, tile sheet "Last launch"
- [x] Tests: Rust (proto, store, core, api, kiosk-svc), Playwright (`booster.spec.ts`, `booster.unit.spec.ts`, `tile-sheet.spec.ts`)

## Spec Change Log

- The launch time rides on `game.launched` itself instead of a second intent: the record is queued later (first window, exit or timeout) with the click as `issued_at`, so no new intent kind, scope entry or proptest op was needed.
- The power plan is raised before the game starts (with the closes), not after, so the game's first frames already run on it; the priority is set after the process exists.
- The median covers the Hub's whole stored window: an intent older than its 7-day TTL is refused at apply, so nothing older reaches `device_events` to be excluded by the query's `since`.

## Verification

- `cargo test -p arena-proto` 239 pass (booster rules, `launch_ms`); `-p arena-store` 87 pass (`list_game_launches`); `-p arena-core` lib 1148 pass (settings rules, `launch_ms` detail, seed defaults, `launch_times`), `proptest_apply` passes; `-p arena-api` lib 290 and `--test lan` 71 pass (`GET /ui/games/launch-times`, pushed `kiosk.booster_*` keys).
- `cargo test -p arena-kiosk-svc --lib` 174 pass; the same 14 lockdown / pipe / DPAPI / config / `procs::the_app_path_is_re_read_from_the_config` tests fail as before (Windows privileges on this host). New: `games::tests` (booster, priority, power plan, launch time, timeout), `operating`, `cli` (`window-watch`), `games_windows` (plan parsing), `cleanup` step list.
- `cargo fmt --check` clean; `cargo clippy -D warnings` clean on proto, store, core, api, kiosk-svc lib and bins (the `tests/discovery.rs` unused-import errors predate this story).
- `pnpm --filter @arena/hub check` 0 errors; Playwright hub: `booster.spec.ts` + `booster.unit.spec.ts` + `library.spec.ts` + `tile-sheet.spec.ts` pass on desktop and phone; `check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` pass; `check:generated` lists the new generated types until they are committed; `check:tokens-used` fails on `--ad-reserved` in `floor/+page.svelte` as before.

Not run: a real Hub-to-kiosk round trip, `SetPriorityClass`, `powercfg`, closing programs, or the `window-watch` helper against a real game on Windows.
