---
title: 'Story 3.27: Pass and Block Sessions with Fixed Windows'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_revision: 'aa48908'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-24-start-a-prepaid-session-on-a-pack-or-the-wallet.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-26-time-up-on-pcs-grace-then-clean-up-or-staff-handles.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** `session.start` refuses `mode: pass` and `mode: block`. A player who bought a night pass (3.22) cannot play on it, and a block plan (3.21) cannot be started for a player or a guest, so neither has the fixed window end the timer engine (3.23) and time-up (3.26) already honour.

**Approach:** `session.start{mode: pass, pack_id}` plays an owned pass (`player_packs.kind = 'pass'`), and `session.start{mode: block, plan_id}` starts a block plan for a player or a guest. Both are checked against the plan's window, read on the wall clock of the branch time zone (minutes from local midnight, wrapping midnight when the end is before the start), and both set `ends_at` to the window end. A pass session closes like a pack session with a settled zero bill and takes nothing off the pass. A block opens its bill at the start (one order line at the block price, unpaid) so it can be settled now or at the end through `settle-bar`; ending it adds no second bill. The Hub start area lists owned passes and the group's blocks (blocks also for a guest).

## Boundaries & Constraints

**Always:**
- Pass checks, in order: owned by the player and kind `pass` (`NOT_FOUND` on `pack_id`); group not allowed -> `PLAN_NOT_FOR_GROUP`; `expires_at <= now` -> `PACK_EXPIRED`; already on a live session -> `VALIDATION_FAILED` on `pack_id`; now outside the window -> `PASS_OUTSIDE_WINDOW` on `pack_id` naming the window ("This pass plays 22:00–06:00."). A pass needs a player.
- Block checks: `plan_id` of an active `block` plan (`NOT_FOUND` on `plan_id`); group not allowed -> `PLAN_NOT_FOR_GROUP`; now outside the start window -> `PASS_OUTSIDE_WINDOW` on `plan_id` naming it ("This block starts 22:00–06:00."). A block takes a guest or a player; `plan_id` on any other mode, or `pack_id` on a block, is `VALIDATION_FAILED`.
- Pass and block sessions take no group rate (segment rate 0, `rate_id` null, no `NO_RATE`).
- The window is wall-clock in the branch tz in effect at the start; the business-day start does not move it. `ends_at` for a 22:00–06:00 window is 06:00 the next day from a 23:30 start and 06:00 the same day from a 02:15 start. The start minute is inside, the end minute is outside.
- Pass close-out is the shared prepaid close-out (staff End or expiry): zero bill settled at once, `minutes_left` stays NULL, the pass is reusable. Block End or expiry only ends the session (staff End charges nothing more and refunds nothing; a held or in-grace block ends at `min(now, ends_at)`). Neither can be Undone and End reports `irreversible: true`.
- New code `PASS_OUTSIDE_WINDOW` is registered everywhere `PLAN_NOT_FOR_GROUP` is; `SessionStart.plan_id` is optional. No code comments beyond doc comments in the existing style (user rule).

**Never:**
- Extend (3.28), pause of a pass or block, dynamic pricing (3.29–3.31), transfer (3.32), kiosk-originated starts (Epic 6), receipts or invoice numbers (Epic 5), changing the timer engine or grace logic.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pass before midnight | 22:00–06:00 pass, start 23:30 | `ends_at` = 06:00 next day | No error |
| Pass after midnight | same pass, start 02:15 | `ends_at` = 06:00 same day | No error |
| Pass outside window | start 14:00, 21:59 or 06:00 | nothing written | `PASS_OUTSIDE_WINDOW`, window named |
| Pass other group / expired / busy / pack id / guest | | nothing written | `PLAN_NOT_FOR_GROUP` / `PACK_EXPIRED` / `VALIDATION_FAILED` / `NOT_FOUND` / `VALIDATION_FAILED` |
| Block for a guest | start 23:45, price 20000 | session ends 06:00; open order with one 20000 line | No error |
| Block outside window, other group, unknown or non-block plan | | nothing written | `PASS_OUTSIDE_WINDOW` / `PLAN_NOT_FOR_GROUP` / `NOT_FOUND` |
| Settle a running block bill | `session.settle` on its order | order settled; expiry later adds no order | No error |
| End a block early | 30 min in | session `ended`, the one order stays as billed | No error |
| Block time-up in Staff handles | `ends_at` reached | held `expired`; End ends it at `ends_at` | No error |
| End or expire a pass session | | zero-bill order settled, device freed | No error |

</intent-contract>

## Code Map

- `crates/arena-proto`: `session.rs` (`SessionStart.plan_id`), `error.rs` (`PassOutsideWindow`), registered in `arena-api/src/ui_auth.rs` status mapping, `arena-cloud/src/api/error.rs`, `arena-kiosk-core/src/queue.rs`.
- `crates/arena-core`: new `windows.rs` (`PlayWindow::ends_at`, `label`, golden tests across midnight, zones and day starts), `locale::tz_at`, `apply/prepaid.rs` (start reads/checks/plan for pass and block, `open_block_bill`, `end_block`, pass in the shared close-out), `apply/session.rs` (`validate`, `mutate`, `mutate_end`).
- Tests: `apply/tests/prepaid.rs` (matrix rows), proptest `Op::WindowStart`, existing refusal tests updated.
- Hub: `lib/players/StartOptions.svelte` (passes and blocks, guest blocks), `lib/tile-actions.svelte.ts` (`StartChoice`, `plan_id`, refusals), `lib/TileSheet.svelte`, `packages/ui/src/messages/en.json`, generated types, `apps/hub/tests/prepaid-start.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol field and error code, registered everywhere, generated types
- [x] `windows::PlayWindow` with golden tests (midnight wrap, zone, business-day start) and `locale::tz_at`
- [x] Start validation, `ends_at` and block bill for pass and block; pass in the shared close-out; block End
- [x] Apply tests for every matrix row; proptest `WindowStart`
- [x] Hub start options for passes and blocks, refusal text, Playwright specs

**Acceptance Criteria:**
- Given an owned unexpired pass for the group, when staff start it inside the window, then `ends_at` is the window end in branch local time, across midnight (23:30 on 22:00–06:00 -> 06:00 next day).
- Given now is outside the pass window, when start is attempted, then it is refused `PASS_OUTSIDE_WINDOW` naming the window.
- Given a block for the group whose start window contains now, when staff start it for a player or guest, then the session ends at the window end whenever it started, and a bill at the block price opens at once, settleable now or at the end.
- Given a block outside its start window, when start is attempted, then it is refused with the window shown.
- Given a pass or block reaching its end, when the timer ticks, then Story 3.26's grace and time-up mode apply.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test prepaid-start session-start` in `apps/hub`.

## Auto Run Result

Status: review
Verified: fmt, clippy, hub and kiosk check, string/token/contrast/protocol/layer checks and prettier pass; arena-core (526 lib tests incl. new pass/block matrix and `windows` goldens, proptest with `WindowStart`) pass; `prepaid-start` and `session-start` Playwright specs pass on desktop and phone. Unrelated timing failures as before: arena-cloud (`a_hung_run_times_out_with_its_own_code`, `unreachable_at_start_retries_then_exits_1_without_logging_the_token`), arena-tasks (`trusted_time_is_cloud_time_plus_monotonic_elapsed`, `a_printer_that_stops_reading_times_out`). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: one code `PASS_OUTSIDE_WINDOW` covers pass and block; a block's start window and its play window are the plan's single window; a block's order line spans start to `ends_at` at rate 0 so the price is the line amount; windows are wall-clock in the branch tz, not business-day relative; the Hub does not pre-compute window validity, the Hub's refusal text names it.
Left for later: replay coverage of window starts; extend of pass and block sessions (3.28); the "All day" window wording of Epic 6.
