---
title: 'Story 3.23: Session Timer Engine with Time Warnings'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: '6dc7835'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-22-sell-a-pack-or-pass-at-the-counter-and-see-owned-plans-on-th.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Nothing watches a session's end. A timed session (one with `ends_at`) never stops, no one is warned that time is running out, and a Hub restart would forget every deadline.

**Approach:** A pure `TimerEngine` (`arena-core::session_timer`) keeps a min-heap of end times and a per-session warning cursor. The Hub task `SessionTimerTask` (`arena-tasks`) reads active sessions through `SnapshotReader::timed_sessions`, ticks every 1 s with the injected `Clock`, and re-syncs on every applied change to `sessions` and every 30 s. A due session is ended by the new system-only intent `session.expire` (the `session.end{reason: expired, actor: system}` of the epic): no `base_rev`, `STALE` unless the session is `active` with `ends_at <= issued_at`, so it applies at most once. It closes the open segment at `ends_at`, sets `state='expired'`, `end_reason='expired'`, `ended_by='hub'`, frees the device, and writes no bill (prepaid time is paid for). Postpaid sessions are tracked by the branch's next closing time, warned only and never expired. Warnings at 10, 5 and 1 min go to the kiosk as the `session.warning` event (not persisted; a new or moved end time starts after the marks already passed, so a restart repeats nothing). An expiry also pushes a `TimesUp` lock to the kiosk, on top of the local lock from `lock_after_ms`, which now counts down to `ends_at`. The Hub tile counts the time left for a timed session and shows a warning border and ⚠ from 10 min.

## Boundaries & Constraints

**Always:**
- The engine holds no database handle and no clock; ticking never runs inside `apply()` and expiry goes through the writer queue like any intent.
- Overdue sessions end at once in `ends_at` order after a restart, each once.
- A paused session is not tracked; resume moves `ends_at` (already done by `fold_pause`) and the next sync re-inserts it.
- No code comments beyond doc comments in the existing style (user rule).

**Never:**
- Grace, clean-up or staff-handles at time-up (3.26), starting prepaid sessions (3.24), alerts with sound (3.25), console warnings (3.35).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Due | active, `ends_at <= now` | `expired`, segment closed at `ends_at`, device freed, `TimerEvent::Expired` | No error |
| Second expire | already expired | nothing written | `STALE` |
| Early / paused | `ends_at` in future or state paused | nothing written | `STALE` |
| No end time | `ends_at` null | nothing written | `VALIDATION_FAILED` on `session_id` |
| Not the system | owner or staff actor | nothing written | `PERMISSION_DENIED` |
| Warnings | 20 min session | 10, 5, 1 once each | none |
| Restart at 3 min left | session found with 3 min left | only the 1 min warning | none |

</intent-contract>

## Code Map

- `crates/arena-proto`: `session.rs` (`SessionExpire`, `SESSION_WARNING_MARKS_MIN`, `SnapshotSession.ends_at`), `intent.rs` (`session.expire`), `frame.rs` (`session.warning`).
- `crates/arena-store`: `sessions.rs` (`expire_session`).
- `crates/arena-core`: `session_timer.rs` (engine), `apply/session_expire.rs`, `session_screen.rs` (`timed_sessions`, `lock_after_ms` to `ends_at`), `snapshot.rs`.
- `crates/arena-tasks/src/session_timer.rs`, `crates/arena-hub/src/lib.rs` (spawn and relay), `crates/arena-api/src/lan/{mod,registry}.rs` (`push_timer_event`).
- Kiosk: `arena-kiosk-core/src/ipc.rs`, `arena-kiosk-svc/src/{pc,session,hub_link,app}.rs`, `apps/kiosk/src/{lib/bridge.ts,routes/+page.svelte}`.
- Hub: `packages/ui/src/components/FloorTile.svelte`, `apps/hub/src/routes/floor/+page.svelte`, `packages/ui/src/messages/en.json`, `apps/hub/tests/session-timer.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol: `session.expire`, `session.warning`, `SnapshotSession.ends_at`, generated types
- [x] `session.expire` apply logic and store query
- [x] `TimerEngine` with heap, warning marks, overdue order and a 500-session test on injected time
- [x] `SessionTimerTask`, Hub wiring and kiosk push (warning event, `TimesUp` lock)
- [x] Kiosk session screen shows the warning; kiosk countdown uses `ends_at`
- [x] Hub tile time left, warning border and ⚠ glyph
- [x] Tests: apply matrix, engine, task, proptest op, Playwright spec

**Acceptance Criteria:**
- Given sessions with `ends_at`, when the Hub service runs, then the engine keeps a min-heap of end times, ticks every 1 s and is rebuilt from active sessions at start.
- Given `ends_at` is reached, then `session.expire` applies once with the system actor and the kiosk locks within 2 s.
- Given 10, 5 or 1 min remain, then the kiosk is warned and the tile shows a warning border and ⚠ from 10 min; warnings are not persisted or resent for marks already passed.
- Given a paused session resumes, then its end time moves by the paused duration and is re-inserted.
- Given the Hub restarts with overdue sessions, then they end at once in `ends_at` order, none twice.
- Given 500 sessions with random end times, then each ends within 1 s of `ends_at` and `apply()` is never blocked.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test session-timer floor pause` in `apps/hub`.

## Auto Run Result

Status: done
Verified: fmt, clippy (including `arena-kiosk-svc`), hub and kiosk check, string/token/contrast/protocol/layer checks and prettier pass; arena-core (engine, apply matrix, proptest), arena-tasks (timer task) and arena-api tests pass; `session-timer.spec.ts` passes on desktop and phone. Unrelated failures as before: arena-tasks (`trusted_time_is_cloud_time_plus_monotonic_elapsed`, `a_printer_that_stops_reading_times_out`), arena-cloud timing tests, and 14 privileged Windows tests in `arena-kiosk-svc` (lockdown, DPAPI, pipe). `session-end` "End works by keyboard" on phone failed once in a parallel run and passes alone.
Left for later: grace and clean-up at time-up (3.26), the prepaid session start that sets `ends_at` (3.24), alerts (3.25). The proptest op covers refusals only; the success path is covered by the apply tests.

## Code Review (2026-10-27)

Four layers ran (blind, edge cases, verification gaps, acceptance). No acceptance violations. Fixed: permanent expiry refusals (`VALIDATION_FAILED`, `PERMISSION_DENIED`) are no longer retried every tick; timer event channel depth raised to 1024 and dropped events are logged in the Hub relay; audit label added for `session.expire`. Left as follow-ups (not blocking): grace and `time_up_mode` at time-up (3.26), kiosk warning pipeline and `push_timer_event` integration tests, paused timed tile shows elapsed time.
