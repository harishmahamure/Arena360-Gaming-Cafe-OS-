---
title: 'Story 5.26: Close a shift with a blind count and a signed variance'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
baseline_commit: 'a589c926fea9ec8dbbae32906804afc425fc7d6c'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-25-cash-deposits-and-expenses.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A shift can be opened and its cash moves recorded (Stories 5.10, 5.25), but never closed, so the drawer is never reconciled and the next shift's float is whatever the last shift opened with.

**Approach:** `shift.close {counted_minor, deposit?, reason?, note?}` (permission `shift.close`) stores the counted cash, the expected cash the Hub works out from `cash_events`, and the signed variance (counted - expected), and closes the shift. Staff count first: `GET /ui/shift/count?counted_minor=` reveals expected cash, difference and breakdown only for a stated count. A difference over the branch threshold (setting `shift.variance_threshold_minor`, default 100.00, Settings → Shift) needs an approver holding the new permission `shift.override_variance` (the inline manager PIN) and a reason.

## Boundaries & Constraints

**Always:**

- `shift.close` takes no `base_rev`, needs an open shift (`SHIFT_NOT_OPEN`) and a count of 0 to 100,000,000 minor units (`VALIDATION_FAILED`, field `counted_minor`). The variance is always recomputed inside the transaction; the client's reveal is a preview.
- |difference| ≤ threshold closes with no PIN and no reason. Above it: no approver holding `shift.override_variance` (the elevating member, else the actor, else the owner) is `VARIANCE_OVER_THRESHOLD` (new, HTTP 409, its message carries no amounts); then `reason` is required (counting_error, unrecorded_expense, change_float_mismatch, other) and `other` needs a note of 1 to 140 characters. Reason, note and approver are stored only for an over-threshold close.
- An optional deposit at close (amount 1..counted, slip reference 1..40 characters) is written after the count as a `deposit` `cash_events` row; the next shift's float prefill is `next_float_minor` = counted - deposit (`last_opening_float` reads it once a shift is closed).
- The reveal returns the expected cash only together with the count it was asked for; the readout shows a warning border and "Over threshold", never red.

**Never:**

- Returning the expected cash without a count, or in a `shift.close` error message; changing `cash_events` (still append-only); closing a shift twice.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Within threshold | float 1,000.00, counted 900.00 (100.00 short) | shift closed, variance -10000, next float 90000 | none |
| Over, Counter alone | 100.01 short | refused | `VARIANCE_OVER_THRESHOLD` |
| Over, manager PIN, reason | 100.01 short, reason other + note | closed; reason, note, approver, both actors in the change log | none |
| Over, no reason | approver present | refused | `VALIDATION_FAILED` `reason` |
| Deposit at close | counted 9,000, deposit 7,000 | deposit row after the count, next float 2,000 | deposit > counted: `VALIDATION_FAILED` `deposit` |
| No shift / second close | | refused | `SHIFT_NOT_OPEN` |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0059_shift_close.sql` -- `shifts` gains `counted_minor`, `expected_minor`, `variance_minor`, `variance_reason`, `variance_note`, `closed_by`, `approver_id`, `next_float_minor`; `src/shifts.rs` -- `close_shift`, `shift_breakdown`, `shift_count`, `variance_threshold`, `last_opening_float` (next float once closed).
- `crates/arena-proto/src/shift.rs` -- `ShiftClose`, `CloseDeposit`, `VarianceReason`, `ShiftCount`, `ShiftBreakdown`, `KEY_SHIFT_VARIANCE_THRESHOLD`; `intent.rs` -- kind `shift.close`; `permission.rs` -- `shift.override_variance`; `error.rs` -- `VARIANCE_OVER_THRESHOLD`.
- `crates/arena-core/src/apply/shift.rs` -- `validate_close`, `mutate_close`, `can_override`; `settings_defaults.rs` -- the threshold default and rule (seeded into upgraded branches by `branch.seed_defaults`, like the new permission).
- `crates/arena-core/src/snapshot.rs` + `crates/arena-api/src/ui_shift.rs` -- `GET /ui/shift/count`.
- Hub: `lib/CloseShiftPanel.svelte`, `lib/DenominationCounter.svelte`, `lib/VarianceReadout.svelte`, `lib/shift.ts`, `lib/shift-count.ts`, `routes/shift/+page.svelte`, `routes/settings/shift/+page.svelte`, `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/shift_close.rs` (8), store migration test, `ui_auth` reveal test, proptest `Op::ShiftClose`, replay seed, Playwright `shift-close.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] Store: migration 0059, close/reveal helpers, next-float prefill
- [x] Protocol: `shift.close`, `shift.override_variance`, `VARIANCE_OVER_THRESHOLD`, generated TS
- [x] Core: validate and mutate the close; threshold setting; approver check
- [x] API: `GET /ui/shift/count` reveal
- [x] Hub UI: denomination counter, variance readout, close panel, Settings → Shift
- [x] Tests: Rust matrix, migration, API, proptest op, replay, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Close shift opens, then one row per INR note (₹500…₹10), a coins total, a Single total toggle and a running total show, with the readout blind ("Expected cash shows after you enter the count."); a currency with no notes counts as Single total only.
- Given a confirmed count, then Counted, Expected and Difference show 32px apart with "Counted ₹4,850.00. Expected ₹4,990.00. Difference −₹140.00 short." and a Breakdown of float, cash sales, cash refunds, deposits and expenses.
- Given |difference| ≤ threshold, `shift.close` closes with no PIN; above it the readout shows the warning border and "Over threshold" and the close needs the inline PIN plus a reason, else `VARIANCE_OVER_THRESHOLD`.
- Given an over-threshold close, the shifts row carries variance, reason, closer and approver, and the change log records both actors.
- Given a deposit at close, it is a `deposit` cash event after the count and the next float is counted minus the deposit.
- Given a close within the threshold using Single total, it completes in under 10 s from the first tap.

## Spec Change Log

## Review Triage Log

## Design Notes

The close stores its result on the `shifts` row (an update change) rather than a new table, so the change log, replay and the owner alert (Epic 8) read one row. The reveal is a separate read endpoint because the close intent itself must work without it; the Hub recomputes everything at apply time, so a stale preview cannot weaken the threshold. Amounts are minor units; the default threshold `10000` is 100.00 in a two-decimal currency.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay`, `arena-kiosk-core`; `pnpm gen:types`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `shift-close.spec.ts cash-moves.spec.ts shift.spec.ts pos.spec.ts audit.spec.ts` in `apps/hub`.

## Review Findings (code review, 2026-10-28)

Patched: the PIN elevation for `shift.close` now needs `shift.override_variance` on the approver; a stale reveal is dropped after `VARIANCE_OVER_THRESHOLD`; reveal errors map to specific messages; a signed-out session during the PIN step returns to sign-in; misplaced doc comments (`intent.rs`, `arena-store/src/lib.rs`) and the replay comment fixed; core test added for an approver without the override permission.
Deferred (see `deferred-work.md`): blind count is a UI convention, deposit at close needs only `shift.close`, count cap.
