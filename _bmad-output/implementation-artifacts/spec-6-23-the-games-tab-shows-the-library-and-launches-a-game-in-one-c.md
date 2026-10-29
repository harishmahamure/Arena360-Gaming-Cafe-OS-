---
title: 'Story 6.23: The Games tab shows the library and launches a game in one click'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '222c9ba'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-13-the-player-shell-with-top-bar-pill-nav-and-time-chip.md'
warnings: []
deferred:
  - 'Cover art on the tiles: the derived `kiosk.library` setting carries no cover hash, so tiles are the `surface` fallback with the centred name. `GameTile` already takes a `cover` (a Hub `/media/<sha256>` URL) and draws the name bottom-left on the tile scrim when it has one; feeding it is a later change to the derived setting or a service lookup into the Hub media cache.'
  - 'A game started from a launcher link (`steam://...`) hands over to the launcher, so the running signal follows the process the service started (the launcher), not the game it goes on to start. Exe-started games are tracked exactly.'
  - 'A `game.launched` replayed after the session ended names the session and player live on the device when the Hub applies it (none, if the session has ended), not the ones at `issued_at`.'
  - 'The registered link handler is read from `HKCR` as the service (SYSTEM): a launcher that registers its scheme only under the player account (`HKCU`) is not found, and the launch fails and is logged.'
---

<intent-contract>

## Intent

**Problem:** The shell's Games tab is a placeholder: a player sees "No games here yet" whatever the café installed, and cannot start anything from the kiosk.

**Approach:** The kiosk service answers two app requests. `games` lists the enabled library games this PC has (Story 6.5's visibility rule), sorted by name, with a running flag per game and an overall one. `game_launch{id}` starts a game in the player's session through a `GameHost`, after checking it against the library and the process allow-list. The app draws a `game-tile` grid, polls the list while a session is on screen (the shell's "game running" signal, passed into every tile's `GameMedia`), and activates a tile with Enter or a click. The Hub records `game.launched` through a new device intent.

## Boundaries & Constraints

**Always:** the service decides (library membership, allow-list, running or not); the app only asks, naming a game id and never a path; a game never starts twice; a failure is logged to the Hub as `kiosk.log{error, game_launch_failed}` and the player sees one info toast; nothing starts while the PC is locked or has no session.

**Never:** a path, URI or argument from the app; a launch of an `app` entry (Story 6.28); a new permission; a second mutation path (the Hub record is an intent through `apply()`).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Tab opens | 3 library games here | 3 tiles, 140×196, radius lg, name centred on `surface`, sorted by name |
| No games | none on this PC | "No games here yet — ask at the counter." |
| Not answered yet | service silent | neither tiles nor the empty line |
| Enter / click | tile focused | `game_launch{id}`; service starts it; `game.launched` queued for the Hub |
| Already running | tile again | `already_running{pid}`; the shell brings that window forward; no second copy |
| Running | any game up | tiles show "Running"; every tile's media gets `gameRunning`; ends when the last exits |
| Not in library / not allowed / will not start | refused | `failed`; `kiosk.log` error; HUD toast "Couldn't start <name> — ask at the counter." |
| Locked / no session | request | `FORBIDDEN`, nothing starts |
| Hub unreachable | any | still launches; the `game.launched` waits in the queue |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{kiosk,intent,scope}.rs` -- `KioskGameLaunched`, `IntentKind::KioskGameLaunched` (`game.launched`, device actor, paired scope).
- `crates/arena-core/src/apply/kiosk_log.rs`, `crates/arena-store/src/sessions.rs` -- one `device_events` row `game_launched` (JSON detail with game, name, session, player), `live_session_on_device`.
- `crates/arena-api/src/lan/session.rs` -- the device event rate limit covers it.
- `crates/arena-kiosk-svc/src/{games,games_windows,intents,app,runtime,host}.rs` -- `games`/`game_launch` requests, `GameHost`, the Windows host (`CreateProcessAsUserW`, link handler from the registry), the running monitor.
- `apps/kiosk/src-tauri/src/{main,bridge,games}.rs`, `ipc-names.json` -- `games`, `game_launch` commands, `game-launch-failed` event, window to front.
- `apps/kiosk/src/lib/{games.ts,GameTile.svelte,Games.svelte,Shell.svelte,Hud.svelte,bridge.ts}`, `routes/+page.svelte`, `packages/ui/src/messages/en.json` -- the tab, polling, toast.
- `docs/kiosk-accessibility.md` -- the tab.

## Tasks & Acceptance

- [x] `game.launched` intent (proto, apply, store lookup, scope, rate limit, proptest op, generated types)
- [x] Kiosk service: `games`, `game_launch`, allow-list and library checks, running tracking, Windows host, fake host
- [x] Tauri shell: commands, outcome mapping, window to front, failure event
- [x] Kiosk app: `game-tile` grid, empty state, polling and the running signal, HUD toast
- [x] Tests: Rust (proto, apply, proptest, kiosk-svc games, shell bridge), Playwright (`games.spec.ts`, HUD toast, shell)

## Spec Change Log

- The Hub's record is a `device_events` row (`game_launched`) rather than a new table: Story 6.27's play hours can read it or add a table then.
- The app polls `games` every 3 s (and on tab change and after a launch) instead of the service pushing a `game_running` message; same signal, fewer moving parts.

## Verification

- `cargo test -p arena-proto` 235 pass; `-p arena-store` and `-p arena-kiosk-core` pass; `-p arena-core --lib` 1143 pass (new `game_launched_writes_one_info_event...`); `proptest_apply` passes (new `GameLaunch` op; it also failed once on an existing `Session` op sequence — `DeviceUnassigned` after `Scan` — which does not involve this change and passed on rerun); `-p arena-api --test lan` 71 pass; `-p arena-kiosk-ui --bins` 31 pass.
- `cargo test -p arena-kiosk-svc --lib` -- all 14 new/related tests pass (`games::tests`, `games_windows::tests`); the same 14 lockdown/pipe/DPAPI/config/`procs::the_app_path_is_re_read_from_the_config` tests fail as before (Windows privileges on this host).
- `cargo clippy -D warnings` clean on proto, store, core, api, kiosk-svc, kiosk-ui; `cargo fmt --check` clean.
- `pnpm --filter @arena/kiosk check` -- 0 errors; Playwright kiosk suite 246 pass (10 new in `games.spec.ts`, 1 new in `hud.spec.ts`); the 4 `theme.spec.ts` tests fail as before (doubled drive-letter path).
- `pnpm check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` pass; `check:generated` lists the new generated types until they are committed.

Not run: a real Hub-to-kiosk round trip, the Tauri shell, `CreateProcessAsUserW` and the registry link handler against a real game or launcher on Windows, `SetForegroundWindow` on a real game window.

- Review (6-23): added an in-flight start claim in `games::launch` so a double click cannot start a game twice.
