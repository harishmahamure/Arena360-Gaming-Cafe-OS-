---
title: 'Story 3.11: End a postpaid session and see the bill to the minute'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'da323fb7dc818e1bebd30880560186a8fe577e88'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-9-start-a-postpaid-session-for-a-player-or-a-guest.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-10-the-pc-unlocks-to-a-session-screen-when-a-session-starts.md'
warnings:
  - oversized
deferred: []
---

<intent-contract>

## Intent

**Problem:** A session can start but never end. There is no bill, no way to free the PC, and the PC never locks to "Pay at counter".

**Approach:** `session.end{session_id}` (permission `session.end`, `base_rev` = the session `rev`) closes the session and its open segment, writes an open `orders` row with one `order_lines` row per segment, frees the device and locks the PC with `pay_at_counter`. A pure bill engine in `arena-core` computes the lines for both the End and the live `bill-breakdown` the tile sheet shows from the snapshot.

## Boundaries & Constraints

**Always:**
- Engine (`billing::bill`): per segment minutes = ceil((to − from − paused) / 60 000), `to` = `now` for an open segment; the session snapshot minimum applies to the first segment's minutes; the total is rounded up to the snapshot rounding unit (0 or 1 = none) by adding the difference to the last segment; amount = round-half-up(minutes × rate_per_hour / 60) in integer minor units; subtotal = sum of line amounts. Session `paused_total_ms` is taken off the last segment (saturating at 0). Golden: 41 min at ₹100/hr, rounding 15, minimum 30 = 45 min = 7500; 10 min = 30 min = 5000.
- `session.end` checks, nothing written on refusal: session missing `NOT_FOUND`; `base_rev` absent `VALIDATION_FAILED` on `base_rev`; `base_rev` ≠ session `rev` `STALE` (an ended session's `rev` has moved, so an older one is `STALE`); session not `active`, `paused` or `grace` `VALIDATION_FAILED` on `session_id`; `mode` not `postpaid` `VALIDATION_FAILED` on `session_id`.
- Writes in one transaction: session `state='ended'`, `ended_at`, `end_reason='staff'`; open segment `to_at`; `orders` row (`status='open'`, `session_id`, `player_id`, `guest_no`, `device_id`, `subtotal_minor`, `total_minor` = subtotal, `business_day` and `fy` from the session, `rev`); one `order_lines` row per segment in order (`session_segment_id`, `device_id`, `from_at`, `to_at`, `minutes`, `rate_minor_per_hour`, `amount_minor`, `position`); device back to its `prior_state` or `online`. Migration `0026_orders.sql`; `Table::Orders` and `Table::OrderLines` in the change log.
- Idempotent through the existing key: a retry returns the first outcome and no second order. Unique index on `orders(session_id)` as a backstop.
- Snapshot `bills`: one entry per live session (computed at snapshot time) and per open order of an ended session, each with `session_id`, `device_id`, `order_id` (null while live), `lines`, `subtotal_minor`, `paid_minor` (0 until 3.12), `remaining_minor`. `SnapshotSession` gains `rev`. No name, phone or email in any change row.
- Hub `run_intent` pushes `pc_command` lock with reason `pay_at_counter` to the PC's kiosk after End applies, keyed by the intent key. A later `session.start` unlocks it as in 3.10.
- Hub UI: an in-session tile sheet shows `bill-breakdown` (lines "<device> · <from>–<to> · <n> min × <rate>/hr" with amount, then Subtotal, "Paid so far", "Remaining", tabular figures), refreshed every 10 s, and an End button (permission `session.end`, PIN-elevation path like the other jobs). A free tile with an open order shows "UNPAID <amount>". Strings in `en.json`; `pnpm gen:types`; audit labels for `session.end`; remove `SessionEnd` from `USED_BY_LATER_STORIES`.
- Proptest generator and replay fixture include `session.end`; the one-live-session-per-device invariant still holds.

**Never:**
- Payments, `session.settle`, "Paid so far" other than 0 (3.12); pause, force end, undo (3.14, 3.15, 3.34); prepaid modes and ends_at (3.23+); tax (Epic 5); a kiosk persisted lock across restart; the receipt.
- Code comments (user rule).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| End | active postpaid, 41 min, ₹100/hr, 15/30 | 1 open order, 1 line 45 min 7500, device `online`, PC locked "Pay at counter" | none |
| Short | 10 min | 1 line 30 min 5000 | none |
| Double End | same key twice | first outcome, one order | none |
| Stale | ended session, older `base_rev` | refused | `STALE` |
| No base_rev | `session.end` without it | refused | `VALIDATION_FAILED` |
| Unknown | no such session | refused | `NOT_FOUND` |

</intent-contract>

## Code Map

- `crates/arena-core/src/billing.rs` -- `read()` rules; add the pure `bill()` engine and `BillLine` types.
- `crates/arena-core/src/apply/session.rs` -- `load/validate/mutate` for `SessionStart`; add the End trio (separate `EndTarget`).
- `crates/arena-core/src/apply/{mod,guards,mutate,validate}.rs` -- add `Target::SessionEnd`, wire load, `base_rev`, validate, mutate.
- `crates/arena-store/src/sessions.rs`, `branch.rs`, `lib.rs`, `migrations.rs`, `migrations/0026_orders.sql` -- new tables, `Table`/`RowKey` variants, session/segment/order readers and writers.
- `crates/arena-proto/src/{session,intent,lib,permission}.rs` -- `SessionEnd`, bill types, `IntentKind::SessionEnd` (39 → 40 in `ALL`), snapshot `bills`.
- `crates/arena-core/src/snapshot.rs` -- fill `bills` and `rev`.
- `crates/arena-api/src/ui_auth.rs` (`run_intent`), `lan/registry.rs`, `lan/mod.rs` -- push the lock with reason.
- `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-replay/src/lib.rs` -- generator and fixture.
- `packages/ui/src/components/BillBreakdown.svelte`, `index.js`, `messages/en.json`; `apps/hub/src/lib/{TileSheet.svelte,tile-actions.svelte.ts,audit/labels.ts}`, `apps/hub/src/routes/floor/+page.svelte`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-core` `billing::bill` with golden unit tests
- [x] `arena-store` migration 0026, tables, readers and writers
- [x] `arena-proto` `SessionEnd`, bill types, intent kind and snapshot fields; `pnpm gen:types`
- [x] `arena-core` `session.end` apply (validate, mutate), snapshot bills; apply tests
- [x] `arena-api` lock push with `pay_at_counter`
- [x] Hub UI: `BillBreakdown`, End job, UNPAID line, strings, audit labels
- [x] Proptest, replay, Playwright specs

**Acceptance Criteria:**
- Given an in-session tile sheet, when it opens, then it shows the live bill lines, Subtotal, Paid so far 0 and Remaining.
- Given End is pressed, when `session.end` applies, then the order and lines exist, the device is free, the kiosk locks with "Pay at counter" and the tile shows "UNPAID <amount>".
- Given End is retried with the same key or sent against an ended session with an older `base_rev`, then no second bill exists and the stale one is `STALE`.

## Spec Change Log

## Review Triage Log

### Review Findings

- [x] [Review][Patch] Live in-session snapshot bill had no test [crates/arena-core/src/apply/tests/session.rs] — fixed: asserts live bill (45 min, 7500) and that the ended order carries the same lines
- [x] [Review][Defer] A paused session's in-progress pause is billed as play time [crates/arena-core/src/billing.rs] — deferred: pause lands in 3.15; paused_total_ms is the spec'd input until then
- [x] [Review][Defer] paused_total_ms is taken off the last segment only [crates/arena-core/src/billing.rs] — deferred: spec'd in Design Notes; per-segment pause records arrive in 3.15/3.31
- [x] [Review][Defer] Pay-at-counter lock is fire-and-forget; an offline kiosk stays unlocked [crates/arena-api/src/ui_auth.rs] — deferred: persisted kiosk lock is out of scope (spec Never), same as 3.10
- [x] [Review][Defer] Open orders are unbounded in the snapshot and the sheet shows no itemised bill for an unpaid order [crates/arena-core/src/snapshot.rs] — deferred: settlement and the order view land in 3.12

#### Rejected

- Replayed End re-pushes the lock: low; same pattern as session.start replay, needs a retry after a new session starts on the same PC, fix adds a branch.
- "Session already ended" shown for every session.end VALIDATION_FAILED: low; base_rev is always sent and only postpaid sessions exist, so unreachable from the UI.
- Empty-segments session writes a zero order: false; session.start always writes a segment.
- UNPAID hidden for offline or ungrouped tiles: low; floor tiles are grouped and an ended device returns to its prior state.
- Device state restore overwrites maintenance: false; a device in session cannot be set to maintenance (busy refusal).
- No confirmation on End: low; End is gated by permission and PIN elevation, spec'd as a single button.
- Quadratic order-line scan, wire `rev` without serde default, proptest change count, rounding wording: low; no everyday harm.

## Design Notes

Paused time has no per-segment record before 3.15 and 3.31, so the engine takes the session total and applies it to the last segment. The live bill is computed by the Hub at snapshot time, so the UI never re-implements the engine; the sheet re-fetches the snapshot every 10 s while open on an in-session device.

## Verification

**Commands:**
- `cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace` -- expected: pass
- `pnpm run check:protocol && pnpm run check:strings`; `pnpm --filter @arena/hub check` -- expected: pass
- `playwright test tests/session-end.spec.ts tests/session-start.spec.ts tests/tile-sheet.spec.ts` in `apps/hub` -- expected: pass
