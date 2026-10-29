---
title: 'Story 6.28: The Apps tab'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '416623f6c479abe77dc347663de57f5f319e421e'
review_loop_iteration: 0
followup_review_recommended: false
warnings: []
deferred:
  - summary: >-
      "Already open" is known only for apps this service started, so an app that hands over to a running instance and exits (Discord, Chrome, Spotify) never shows open and a second click starts another copy.
    evidence: |-
      apps_of and launch_app_in read the pid the service started via launcher_pid. Settling it needs matching running processes by image name for library apps, as launcher_open does with find_process.
    location: >-
      crates/arena-kiosk-svc/src/games.rs (launch_app_in)
    severity: medium
  - summary: >-
      Apps outside the 6.12 sign-out targets (for example a music player with a login) have no end or sign-out step at session end beyond the generic programs step.
    evidence: |-
      cleanup.rs names fixed targets (steam, epic, riot, discord, chrome, edge, firefox); the story's own criterion names only Discord and a browser.
    location: >-
      crates/arena-kiosk-svc/src/cleanup/signout.rs
    severity: low
  - summary: >-
      The apps and app_launch dispatch arms in app.rs have no service-level test.
    evidence: |-
      The sibling games, game_launch, launchers and most_played arms have none either; the handlers and the Playwright stubs are tested on both sides.
    location: >-
      crates/arena-kiosk-svc/src/app.rs
    severity: low
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-27-play-hours-per-game-and-the-most-played-here-carousel.md'
---

<intent-contract>

## Intent

**Problem:** The Apps pill in the player shell is an empty placeholder: a player cannot open the café's Discord, a browser or music player the way they open a game.

**Approach:** The kiosk service lists the library's `kind: app` entries this PC has (`apps`) and starts one (`app_launch{id}`) through the same allow-list check as a game, but with no booster, no play tracking and no `game.launched`: a start queues a `kiosk.log` info `app_launched` for the Hub. The Apps tab reuses the Games tab's `filter-rail`, search and tile grid, without launcher cards or a carousel.

## Boundaries & Constraints

**Always:** the allow-list is checked before anything starts (same `spec_of` / `allowed` as games); an app that is already running is brought forward, not started twice; the tab lists only apps installed on this PC; an app launch is never a game play; activating a tile uses the service only (no new permission, no second mutation path — the record is an existing `kiosk.log` intent).

**Never:** the booster, `game.launched`, `game.play_recorded` or `GameState.running` for an app; an app counted in the "game running" signal; launcher cards or the carousel on the Apps tab.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| List | library has games and apps | `apps` answers only `kind: app` entries, sorted by name, with `running` |
| Launch | app in library and allowed | started, `kiosk.log info app_launched` queued, no play, no `game.launched` |
| Already open | app running | `already_running{pid}`, nothing queued |
| Not allowed / not here / will not start | allow-list refuses, no path, host error | `failed` + `kiosk.log error game_launch_failed` |
| A game id sent to `app_launch` | id of a `kind: game` entry | `failed` (not in this PC's apps) |
| No session / locked | any | `apps` empty; `app_launch` refused |
| Empty | no apps on this PC | "No games here yet — ask at the counter." |
| Filter / search | category or tag, text | same as Games |

</intent-contract>

## Code Map

- `crates/arena-kiosk-svc/src/games.rs` -- `cards` (games only today) gains a kind; new `list_apps`, `app_launch` / `launch_app_in`, reusing `spec_of`, `allowed`, `load_visible`, and the `launcher_pid` / `begin_launcher` / `launcher_started` claim maps (ids cannot collide with `steam|epic|riot`); `GameState.running` is untouched.
- `crates/arena-kiosk-svc/src/app.rs` -- dispatch `apps` and `app_launch`.
- `apps/kiosk/src-tauri/{ipc-names.json,src/main.rs,src/bridge.rs}` -- commands `apps`, `app_launch` (like `games`, `game_launch`).
- `apps/kiosk/src/lib/{bridge.ts,games.ts,Games.svelte,Shell.svelte}`, `src/routes/+page.svelte` -- `loadApps`, `launchApp`, `Games` gets a `kind` ("games" | "apps") that hides launchers/carousel and swaps the title, search label and empty strings.
- `packages/ui/src/messages/en.json`, `docs/kiosk-accessibility.md`, `apps/kiosk/tests/apps.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Kiosk service: `apps` list, `app_launch`, `app_launched` log, dispatch, Rust tests
- [x] Tauri shell: commands, ipc names
- [x] App: bridge, `Games` reused for apps, Apps tab wiring, strings, a11y doc
- [x] Tests: Playwright `apps.spec.ts`; run the I/O matrix cases

**Acceptance Criteria:**
- Given library entries of kind app, when the Apps tab renders, then it shows a `filter-rail` of the apps' categories, search and a tile grid, without launcher cards or a carousel.
- Given an app tile, when activated, then the app opens through the service after the allow-list check and is recorded as an app launch, not counted in game hours.
- Given no apps on this PC, when the tab renders, then it shows "No games here yet — ask at the counter."
- Given an app that keeps saved logins (Discord, a browser), when the session ends, then the Story 6.12 cleanup covers it (unchanged; those targets are ended and signed out by `signout_*` / `reset_*`).

## Design Notes

"Recorded as an app launch": a `kiosk.log` info row `app_launched` (detail `<name> (<app id>)`) written by the Hub as a `device_events` row at the launch time on the device. Unlike `game.launched` it does not name the player and session; no report reads app launches yet, and the existing intent keeps the change free of a new protocol kind. `game_plays` are written only by `game.play_recorded`, which an app never sends.

## Verification

**Commands:**
- `cargo test -p arena-kiosk-svc --lib games` -- expected: pass
- `cargo fmt --check && cargo clippy -p arena-kiosk-svc --lib -- -D warnings` -- expected: clean
- `pnpm --filter @arena/kiosk check`, Playwright `apps.spec.ts` and `games.spec.ts`, `pnpm check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` -- expected: pass

## Review Triage Log

### 2026-10-29 — Review pass
- verdicts: 20 findings — high 0, medium 3, low 14, false 3, maybe-false 0 (blind hunter 9, edge-case hunter 9, verification gap 2; intent audit was descriptive)
- findings:
  - `[medium]` `[patch]` `COMMANDS: [&str; 39]` in `bridge.rs` held 41 names, so the kiosk shell's test build failed — length set to 41; the shell crate's tests pass (31).
  - `[medium]` `[patch]` a slow `apps` answer could repopulate the list after the session ended — per-run `live` flag in +page.svelte, as for the carousel.
  - `[medium]` `[defer]` "already open" only for apps this service started (hand-over apps) — in `deferred`.
  - `[low]` `[defer]` apps outside the 6.12 sign-out targets keep a login — in `deferred`; Discord and browsers, which the story names, are covered.
  - `[low]` `[defer]` `apps` / `app_launch` dispatch untested — in `deferred`, same as the sibling arms.
  - `[false]` `[reject]` wrong empty-state copy — the story's criterion gives exactly "No games here yet — ask at the counter."
  - `[false]` `[reject]` the 6.12 cleanup claim is unbacked — the story says cleanup already covers Discord and browsers; this story does not change it.
  - `[false]` `[reject]` build break is the only gap in the Tauri command list — fixed above, nothing else missing.
  - `[low]` `[reject]` library id equal to `steam|epic|riot`, `log_event` failure after a start, `game_launch_failed` shared by apps, copied launch/start code, stringly kind, repeated ternaries, unvalidated toast name, double-click and stale-flag nits, no player/session in the record, Hub retention of `app_launched`, browser arguments, extra tests — ids are ULIDs; the rest follow the game and launcher patterns or need more than a direct fix; the record choice is in Design Notes.

## Auto Run Result

Status: done

- **Summary:** the Apps pill shows the library's apps this PC has with the Games tab's filter rail, search and tile grid (no launcher cards, no carousel). The kiosk service answers `apps` and `app_launch{id}` with the same `spec_of` and allow-list as a game, no booster, no play tracking and no `game.launched`; a start queues `kiosk.log info app_launched`.
- **Files:** `crates/arena-kiosk-svc/src/{games,app}.rs` and `games/tests.rs` (service side); `apps/kiosk/src-tauri/{ipc-names.json,src/main.rs,src/bridge.rs}` (commands); `apps/kiosk/src/lib/{bridge,games}.ts`, `Games.svelte`, `Shell.svelte`, `routes/+page.svelte` (tab); `packages/ui/src/messages/en.json`, `docs/kiosk-accessibility.md`; `apps/kiosk/tests/apps.spec.ts`.
- **Review:** 2 patches applied, 3 deferred, the rest rejected with reasons above. Follow-up review recommended: false.
- **Verification:** `cargo fmt --check` clean; `cargo clippy -p arena-kiosk-svc --lib -D warnings` clean; `cargo test -p arena-kiosk-svc --lib games` 32 pass; kiosk shell crate tests 31 pass; `pnpm --filter @arena/kiosk check` 0 errors; Playwright apps, games, launchers, most-played and shell specs pass (full kiosk suite 279 pass, 4 `theme.spec.ts` fail as before); `check:strings`, `check:hex`, `check:no-blur`, `check:layers`, `check:pins`, `check:protocol` and prettier pass.
- **Residual risks:** the real Tauri shell, a real Windows start and a Hub round trip were not exercised; see `deferred`.
