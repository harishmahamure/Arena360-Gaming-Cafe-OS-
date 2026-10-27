---
title: 'Story 3.26: Time-up on PCs — Grace, Then Clean Up or Staff Handles'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'fddafbe'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-23-session-timer-engine-with-time-warnings.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-25-hub-alerts-with-sound-and-a-persistent-badge.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A timed session ends the instant `ends_at` is reached: there is no grace for the player, and the branch's time-up mode ("Clean up" or "Staff handles") is stored on every session but never used.

**Approach:** `session.expire` (system only, no `base_rev`) now advances a timed session one step. An `active` session at `ends_at` enters `grace` for its snapshot `grace_min` (a grace of 0, or a session found past its grace after a restart, goes straight to the time-up action). A `grace` session whose grace has run out takes the time-up action: **Clean up** closes it out exactly as before (state `expired`, `ended_at = ends_at`, device freed, prepaid close-out); **Staff handles** closes the open segment at `ends_at` (no more time is charged), holds the session as `expired` with `ended_at` NULL, keeps the device in its session and raises a `time_up` alert in the same transaction. Staff end it with `session.end`, which now accepts a held `expired` session and bills a prepaid session no further than `ends_at`. The timer engine acts on a new `due_at` (`ends_at`, or the end of grace for a session in grace) and warns from `ends_at`. The kiosk screen tick carries `lock_after_ms` (now including grace), `grace_ms` and `time_up_mode`; the kiosk shows "Time's up" with the grace countdown and locks locally at zero with the mode's lock screen.

## Boundaries & Constraints

**Always:**
- A session expires at most once per step: `STALE` unless `active` with `ends_at <= issued_at`, or `grace` with `ends_at + grace <= issued_at`.
- A held session (`state = 'expired'`, `ended_at IS NULL`) counts as live: the one-live-session-per-device index, the live session list, the wallet reservation and the pack-in-use check all include it; a terminal `expired` row (clean-up) does not.
- Grace time is not charged: a prepaid session in `grace` or held `expired` ends at `min(now, ends_at)` when staff press End.
- No code comments beyond doc comments in the existing style (user rule).

**Never:**
- Extending out of grace (3.28), pass and block windows (3.27), console time-up (3.36), changing the alert service.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| End reached, grace 2 min | active, `ends_at <= now` | `grace`; segment open; device in session; tile "Grace m:ss" | No error |
| Grace over, Clean up | grace, `ends_at + grace <= now` | `expired`, `ended_at = ends_at`, device freed | No error |
| Grace over, Staff handles | same | held `expired`, segment closed at `ends_at`, `time_up` alert, device stays in session | No error |
| Grace 0 | active, due | straight to the time-up action | No error |
| Found past grace after restart | active, `now >= ends_at + grace` | time-up action at once | No error |
| Early / in grace | `now < ends_at` or `now < ends_at + grace` in grace | nothing written | `STALE` |
| Staff End on held / grace | held `expired` or `grace` | `ended`, prepaid billed to `ends_at` | No error |
| New start on a held device | held session on device | nothing written | refused (device busy) |

</intent-contract>

## Code Map

- `crates/arena-proto`: `session.rs` (`SnapshotSession.grace_ends_at`, `SessionScreen.grace_ms`), `pc_command.rs` (`LockReason::TimesUpSeeCounter`).
- `crates/arena-store`: `migrations/0037_held_expired_sessions.sql` (live index), `sessions.rs` (`LiveSession.grace_min`, `grace_session`, `hold_session_expired`, live predicates), `player_packs.rs`.
- `crates/arena-core`: `apply/session_expire.rs` (grace, close, hold + alert), `apply/session.rs` (End accepts held, caps prepaid at `ends_at`), `session_timer.rs` (`due_at`, `TimerEvent::Expired.held`), `session_screen.rs`, `snapshot.rs`.
- `crates/arena-tasks/src/session_timer.rs` (outcome from the applied change tables), `crates/arena-api/src/lan/registry.rs` (lock reason by mode).
- Kiosk: `arena-kiosk-core/src/ipc.rs` (`PcSession.grace_in_ms/grace_ms`), `arena-kiosk-svc/src/{pc,session,app}.rs`, `apps/kiosk/src-tauri/src/bridge.rs`, `apps/kiosk/src/{lib/bridge.ts,routes/+page.svelte,routes/+layout.svelte}`.
- Hub: `apps/hub/src/routes/floor/+page.svelte` (tile "Grace m:ss" / "Time up", warning border), `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol fields, lock reason, generated types, catalogue strings
- [x] Migration for held sessions in the live index; store predicates, `grace_session`, `hold_session_expired`
- [x] `session.expire` grace / clean-up / staff-handles; `session.end` of a held or grace session
- [x] Timer engine `due_at`, task outcome events, kiosk lock reason push
- [x] Kiosk service, IPC, Tauri bridge and screen: grace countdown, local lock by mode
- [x] Hub tile grace countdown and time-up display
- [x] Tests: apply matrix, engine, task, Hub and kiosk Playwright

**Acceptance Criteria:**
- Given a prepaid session reaches `ends_at`, then it enters `grace` for its grace period, the tile shows "Grace m:ss" and the kiosk shows "Time's up" with the grace countdown.
- Given grace ends in Clean up mode, then the session ends as expired and the kiosk locks within 2 s.
- Given grace ends in Staff handles mode, then the session becomes `expired` with no further time charged, the kiosk holds "Time's up — see the counter.", a TIME UP alert is raised and the session ends only when staff press End.
- Given the Hub is unreachable, then the kiosk locks at `lock_after_ms` (which includes grace) with the mode's lock screen and writes no state.
- Given the Hub restarts in grace or past time-up, then the heap is rebuilt and overdue sessions are processed at once in order, none twice.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo clippy -p arena-kiosk-svc --lib -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test session-timer floor` in `apps/hub`; `pnpm exec playwright test pc-screen` in `apps/kiosk`.

## Auto Run Result

Status: done
Verified: fmt, clippy (workspace without `arena-kiosk-svc`, and its lib), hub and kiosk svelte-check, string/token/contrast/protocol/layer checks and prettier pass; arena-core (new grace, clean-up and staff-handles apply tests, engine test), arena-tasks (staff-handles task test) and arena-api tests pass; hub `session-timer` and `floor` and kiosk `pc-screen` Playwright specs pass. Unrelated failures as before: arena-tasks (`trusted_time_is_cloud_time_plus_monotonic_elapsed`, `a_printer_that_stops_reading_times_out`), arena-kiosk-svc integration tests do not lint (`discovery.rs`, existing).
Decisions: one `session.expire` intent drives both steps (no new intent kind); the grace is not a new column (`ends_at + grace_min`); the kiosk receives no grace event, it derives the phase from `lock_after_ms` and `grace_ms` in the tick; the "Time's up" alert is raised inside the hold transaction so a crash cannot lose it.
Left for later: Undo of an End on a held session reopens it as `active` past its `ends_at` (the engine then expires it again); extend out of grace (3.28); end-to-end kiosk test on a real Windows kiosk.

## Code Review (2026-10-27)

Four layers (blind, edge-case, verification-gap, acceptance) ran on the implementation commit.
- Fixed: the kiosk tick reported a full `grace_ms` for a held `expired` session and for one whose lock is capped by branch closing time, so a resynced kiosk showed a wrong "Grace ends in" countdown. `session_screens` now reports `grace_ms = 0` unless the session's own end drives the lock; the held-session test asserts it.
- Deferred (low or by design, not blocking): `held` is inferred from the tables a close wrote (a hold with no open segment and an already-open alert would emit no lock event; not reachable for a running session); the TIME UP alert is acked by staff on its own, not by End (3.25 design); Undo of an End on a held session (already listed above); extra tests for a prepaid End in grace, a held pack/wallet session, the registry `held` mapping and the bridge `grace_in_ms` arithmetic.
- Verified: arena-core tests, fmt and clippy (excluding the Unix-only `arena-e2e` and existing non-linting `arena-kiosk-svc` integration tests) are green; arena-tasks/arena-kiosk-svc failures are the existing Windows-environment ones.
