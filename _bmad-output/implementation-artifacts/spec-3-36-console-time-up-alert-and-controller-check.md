---
title: 'Story 3.36: Console Time-Up Alert and Controller Check'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_revision: 'f5e9695'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-35-console-sessions-timed-from-the-hub-with-warnings-on-the-til.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A console session that reaches its end time follows the PC grace and time-up mode, so nobody is alerted at the counter, and nothing records whether the controllers came back after any console session.

**Approach:** A console has no grace and always waits for staff: `session.start` snapshots `grace_min = 0` and `staff_handles` for a console, and `session.expire` holds a console session `expired` (no more time charged) with a `time_up` alert until staff press End. When a console session has ended, staff must record `controller.check` (`returned` or `missing`) before the console takes another session. `missing` raises a `controller_missing` alert for the manager. The Hub tile shows a "Check controllers" warning badge and the sheet offers Returned and Missing; the Sessions screen lists the result.

## Boundaries & Constraints

**Always:**
- `controller.check` needs the `session.end` permission, takes no `base_rev`, applies once per session (`STALE` after), and only to an ended console session (`VALIDATION_FAILED` otherwise).
- `session.start` and `session.transfer` onto a console with an unchecked ended session are refused `CONTROLLERS_UNCHECKED` ("Mark controllers first").
- Sessions that ended before migration 0042 are marked `na`, so no existing console is blocked.
- Undoing an End clears the controller check columns.
- No code comments beyond doc comments. Every new string is in `packages/ui/src/messages/en.json`.

**Never:**
- A new permission, WhatsApp/SMS alerts (Epic 8), the Pi/QR flow (Epic 12).

## Code Map

- `crates/arena-store/migrations/0042_controller_check.sql`, `src/sessions.rs` -- check columns, `set_controller_check`, `unchecked_console_session`, `list_unchecked_consoles`.
- `crates/arena-proto` -- `ControllerCheck`, `ControllerResult`, `IntentKind::ControllerCheck`, `AlertKind::ControllerMissing`, `ErrorCode::ControllersUnchecked`, `SnapshotDevice.controller_check`, `SessionListRow.controller_check`.
- `crates/arena-core/src/apply/controller_check.rs` -- load, validate, mutate; `session.rs`, `transfer.rs` -- start/transfer gate; `session_expire.rs` -- console has no grace and holds.
- `apps/hub` -- floor tile badge and "Time's up", tile sheet Returned/Missing and disabled Start, Sessions "Controllers" column.

## Tasks & Acceptance

**Execution:**
- [x] Store -- migration, queries, reopen clears the check
- [x] Proto -- intent, result, alert kind, error code, snapshot and list fields, generated types
- [x] `arena-core` -- `controller.check`, start/transfer gate, console time-up hold with no grace
- [x] `arena-core` tests and proptest op
- [x] Hub -- tile badge, sheet buttons, disabled Start, Sessions column, alert label and tag, audit label
- [x] Playwright `console.spec.ts`, `controllers-list.spec.ts`

**Acceptance Criteria:**
- Given a prepaid console session reaching `ends_at`, then there is no grace, the session becomes `expired` with no further charge, and the tile shows "Time's up" with a TIME UP badge until acknowledged.
- Given staff press End, then `session.end` applies as for any session and Undo works.
- Given a console session has ended, then the tile shows a "Check controllers" warning badge, the sheet offers Returned and Missing, and Start is disabled with "Mark controllers first".
- Given Returned or Missing, then `controller.check` logs the result with staff and time and clears the badge; Missing also raises a `controller_missing` alert.
- Given the Sessions screen, then it shows Returned or Missing for the session.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test -p arena-core`, `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `check:strings`, Hub Playwright suite (all pass). `arena-tasks` (heartbeat, LAN printer timeout) and `arena-cloud` (backup, binary) tests fail in this Windows environment independent of this story.

</intent-contract>
