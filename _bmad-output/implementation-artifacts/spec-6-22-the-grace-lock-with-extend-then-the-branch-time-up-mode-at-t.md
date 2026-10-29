---
title: 'Story 6.22: The grace lock with Extend, then the branch time-up mode at the kiosk'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '9e4d361'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-21-extend-from-the-kiosk-with-live-prices.md'
warnings: []
deferred:
  - 'A full-monitor scrim window with click-through everywhere but the Extend button: the grace card is the HUD window sized to its content, so the game gets every click and key outside the card. A per-pixel hit-test region is a later refinement.'
  - 'Persisting which programs were suspended across a kiosk-service restart: the service remembers only that it froze the PC (`pc.frozen`) and resumes every unprotected program of the player on the next unlock or session start.'
---

<intent-contract>

## Intent

**Problem:** At zero the kiosk has a grace countdown in the shell, but a player in a game sees nothing, cannot extend without leaving it, and a "Staff handles" branch only covers the screen while the game keeps running behind the lock.

**Approach:** The HUD window draws a solid "Time's up" grace card (countdown in `kiosk-display` digits, wrap-up copy, Extend as `button-warning`, Ctrl+Shift+E key caps) for as long as the grace runs; it never takes focus and, being only as large as the card, leaves the rest of the screen to the game. Over a fullscreen-exclusive game nothing is drawn: the taskbar flashes and the warning sound plays once at grace start. When the grace ends the service already performs the branch mode locally (Clean up: cleanup and attract; Staff handles: "Time's up — see the counter."). New: in Staff handles the lock also suspends the player's unprotected programs, and an unlock, a staff Extend (session start push) or End (cleanup) resumes or ends them.

## Boundaries & Constraints

**Always:** the card is solid (no blur), appears on the kiosk's own monotonic clock so it shows with the Hub unreachable, and goes when an Extend lands; the suspend uses the enforcer's safety rules (never the kiosk app, its WebView2, the service or protected System32 images); nothing is written for a local time-up; Extend on the card shows only for a profiled player on prepaid time (as 6.21).

**Never:** focus taken by the card; an overlay over a fullscreen-exclusive game; a new frame, intent, permission or `LockReason`; a kill on a Staff-handles lock (the game is only suspended).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Grace starts | prepaid, `remaining 0`, grace 2 min | card "Time's up", `1:59`…, "Wrap up your round. This PC locks when the timer hits 0:00.", Extend + key caps; warning toast gone; chime |
| Guest / pass | no balance | same card, no Extend, no key caps |
| Extend lands | `remaining_ms` > 0 | card gone, "More time added. GG." |
| Ctrl+Shift+E | card showing | panel opens, focus on the extend options |
| Fullscreen-exclusive | grace starts | no window; taskbar flash + beep once |
| No grace | `grace_ms` 0 | no card |
| Clean up | grace ends | lock `TimesUp`, cleanup, attract (6.11) |
| Staff handles | grace ends | lock `TimesUpSeeCounter`, player programs suspended |
| Staff Extend | session start push | lock lifted, programs resumed |
| Staff End | `PayAtCounter` lock | cleanup ends the suspended programs |
| Hub unreachable | deadline passes | same local action, no state written |

</intent-contract>

## Code Map

- `apps/kiosk/src/lib/{hud.ts,Hud.svelte}`, `packages/ui/src/messages/en.json` -- `graceLeft`, grace card, `player.grace.*`.
- `crates/arena-kiosk-core/src/hud.rs`, `apps/kiosk/src-tauri/src/{hud,main}.rs` -- `grace_flash`, `Hud::note_grace`, flash at grace start.
- `crates/arena-kiosk-svc/src/procs/{mod,fake,windows}.rs` -- `ProcessKiller::set_suspended`, `Inner::freeze`, `Ctx::freezer`.
- `crates/arena-kiosk-svc/src/{pc,runtime}.rs` -- freeze on `TimesUpSeeCounter`, resume on unlock.
- `crates/arena-kiosk-svc/src/session.rs` -- existing deadline lock (hold) kept; new `grace_start` timer tells the app the grace began on the kiosk clock.
- `docs/kiosk-accessibility.md` -- grace in fullscreen-exclusive games.

## Tasks & Acceptance

- [x] Grace card on the HUD (countdown, copy, Extend, key caps), Playwright
- [x] Taskbar flash and sound at grace start over a fullscreen-exclusive game
- [x] Suspend/resume of the player's programs in the service, behind a `ProcessKiller` method with a fake
- [x] Staff-handles lock suspends; unlock, session start and cleanup resume or end
- [x] Tests: Rust (core, kiosk-svc), Playwright (`hud.spec.ts`)

## Spec Change Log

- The epic's scrim is the card's own solid backing: no second full-monitor window is drawn (see `deferred`).

## Verification

- `cargo test -p arena-kiosk-core` -- 74 pass; `cargo test -p arena-kiosk-ui --bins` -- 30 pass (new `a_grace_flashes_once_and_only_over_an_exclusive_game`)
- `cargo test -p arena-kiosk-svc --lib` -- new tests pass (suspend and resume, staff End after the hold, Clean up never freezes, a program that will not freeze, `grace_start`); the same 14 lockdown/pipe/DPAPI/config/`procs::the_app_path_is_re_read_from_the_config` tests fail as before (Windows privileges, confirmed on the stashed baseline)
- `cargo clippy -D warnings` clean on kiosk-core, kiosk-svc (`--lib --bins`) and the Tauri shell; `cargo fmt --check` clean
- `pnpm --filter @arena/kiosk check` -- 0 errors; Playwright kiosk suite 236 pass (49 in `hud.spec.ts`, 10 new), the 4 `theme.spec.ts` tests fail as before (doubled drive-letter path)
- `pnpm check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` -- pass

Not run: a real Hub-to-kiosk round trip, the Tauri shell, a real fullscreen-exclusive game (taskbar flash and beep), `NtSuspendProcess` against a real game on Windows.
