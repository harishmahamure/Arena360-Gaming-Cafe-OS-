---
title: 'Story 6.36: The kiosk keeps the session running while the Hub is unreachable'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '673eef9958e90eaff7f18f7477a419ddc5b8fda2'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: []
deferred: []
---

## Intent

**Problem:** When the counter PC drops, the kiosk must never lock a player out; it must say it is reconnecting, carry on locally, and agree with the Hub once the link is back.

**Approach:** Most of the offline behaviour already existed (monotonic `lock_after_ms` deadline, local warning marks and time-up in `session::run`, the offline queue replayed oldest first by idempotency key, jittered 2–30 s back-off, every Hub-dependent control disabled with "Counter is reconnecting — try again shortly."). This story adds the missing parts: the `reconnecting-indicator` left of the volume control and on the expanded HUD, and the reconcile on reconnect. A new paired-only `session.sync` request returns the Hub's record of the PC's session (`SessionSync { session, lock }`); the kiosk replays its queue, applies the record (chip and HUD snap to the Hub's time, a Hub-side pause is followed, a session the Hub ended locks with its reason and runs the session-end cleanup), and only then reports the link `connected`, which is what removes the indicator.

## Tasks

- [x] Protocol: `session.sync` / `session.sync_result` frames and `SessionSync`; regenerate TS types
- [x] Store + core: `last_session_end`, `session_sync` (live session, paused, or why the last one ended)
- [x] Hub LAN: `session.sync` handler (paired devices only)
- [x] Kiosk service: replay → reconcile → `connected` on every connect (`hub_link::serve`, `session::reconcile`)
- [x] Kiosk UI: `ReconnectingIndicator` in the shell top bar (left of the volume control) and the expanded HUD; the stage pill steps aside while the shell is shown
- [x] Tests: 30-minute offline run on a paused clock, reconcile cases, `session.sync` over the LAN, Playwright for shell and HUD

## Acceptance Criteria

- Given the Hub is lost while the shell or HUD is visible, then a quiet "Reconnecting…" pill shows left of the volume control and on the expanded HUD, and nothing else locks.
- Given a running session and an unreachable Hub, then the chip and HUD keep counting on the monotonic clock, warnings fire at the marks, and the kiosk locks within 1 s of zero (the branch time-up action, Story 6.22).
- Given the Hub is unreachable, then login, sign-up, plan request, Extend, Place order, I'm done, Log out, Call staff, change password and saving PC settings are disabled with the reconnecting copy while launching, volume and Lock keep working (existing controls; unchanged).
- Given events made offline, then they replay in creation order and apply once by idempotency key (existing queue).
- Given the Hub returns, then the Hub's record wins: the chip and HUD snap to its time, a session it ended locks with its reason, and the indicator disappears only after this reconcile.

## Verification

- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-kiosk-core`; `cargo test -p arena-kiosk-svc --lib` (14 pre-existing environment failures: lockdown, named pipe, DPAPI, config/procs paths; identical without this change); `cargo fmt --check`; `cargo clippy -D warnings` on the lib targets
- `pnpm gen:types`, `pnpm --filter @arena/kiosk check`, Playwright kiosk `hud account-menu status`; `pnpm check:strings check:hex check:no-blur check:protocol format:check`

## Notes

- Hand-off: reconnecting to a promoted standby (Story 9.21) reuses `session::reconcile`.
- A session the kiosk already ended on its own clock (time up) is gone from the screen, so a later Hub answer for it changes nothing.
- The full "Hub process stopped for 30 minutes" run against a real Hub process is covered by the paused-clock service test plus the `session.sync` LAN test; the reference-hardware run is Story 6.38.

## Review Findings (code review, 2026-10-29)

- Fixed: `session_sync` returned `session: None, lock: None` when a live session ended between its two reads, which would have locked the PC as ended-by-staff; it now errors so the kiosk keeps its clock.
- Deferred (by design / out of scope): a PC locked on its own time-up ignores a later Hub extension (documented in Notes); the Hub's time is not applied while the player has pressed Lock (same as existing tick handling); `Connected` is reported even if `session.sync` fails or times out (the Hub's ticks then bring its time); no integration test of `hub_link::serve` ordering and no unit tests for the paused/forced/expired Hub mappings; no older-Hub capability gate.
