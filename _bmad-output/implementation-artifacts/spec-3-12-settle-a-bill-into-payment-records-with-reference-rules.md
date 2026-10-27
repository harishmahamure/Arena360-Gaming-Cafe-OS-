---
title: 'Story 3.12: Settle a bill into payment records with reference rules'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: '852310c1690f2d7fffba4672e1177339855b864d'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-11-end-a-postpaid-session-and-see-the-bill-to-the-minute.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An ended session leaves an open `orders` row that can never be paid. There is no payment record, the order never closes, the tile keeps showing UNPAID and the PC stays on "Pay at counter".

**Approach:** `session.settle{order_id, payments[]}` (permission `pos.sell`, `base_rev` = the order `rev`) writes one `payments` row per method (cash, UPI, card), marks the order settled in one transaction, and the Hub UI stream emits `bill.settled{order_id}`. The Hub reads the outcome (`settled_at`, `undo_until`) back for the caller and returns the PC to its idle locked screen.

## Boundaries & Constraints

**Always:**
- Payments sum must equal `orders.total_minor` exactly, else `PAYMENT_SHORT` on `payments` and nothing is written. Cash change is the client's job (it sends cash = remainder). An empty list is valid only for a zero total.
- A UPI row needs a `reference` of exactly 4 ASCII digits and a card row a non-blank reference of at most 64 characters, else `REFERENCE_REQUIRED` on `payments[i].reference`. A cash row carries no reference. References are checked before the sum. Each amount is above 0.
- Checks, nothing written on refusal: order missing `NOT_FOUND` on `order_id`; `base_rev` absent `VALIDATION_FAILED` on `base_rev`; `base_rev` ≠ order `rev` `STALE`; order already `settled` `STALE`; order `void` `VALIDATION_FAILED`.
- One transaction: a `payments` row per row (`order_id`, `method`, `amount_minor`, `reference`, `staff_id` = actor, `position`, `business_day` and `fy` of the settle moment), order `status='settled'`, `settled_at`, `settled_by`. Migration `0027_payments.sql`; `Table::Payments` in the change log. No invoice number, no receipt, no tax, no shift check.
- Idempotent through the existing key (the first outcome comes back, no second payment). Money is integer minor units.
- The snapshot bill carries `order_rev` (the order `rev`, null while live) so a client can send `base_rev`.
- Hub: `/ui/intent` replies `{applied, settled?}`; `settled` is `{order_id, settled_at, undo_until = settled_at + 8000}` read from the order row (so a replay returns the same). After a fresh settle, if the device has no live session, push `pc_command` lock with no reason (idle locked screen) keyed by the intent key. UI SSE emits `bill.settled` `{order_id}`; the Hub UI reloads the snapshot on it, so UNPAID clears.
- New error codes `PAYMENT_SHORT` (409) and `REFERENCE_REQUIRED` (422). Proptest generator and replay fixture include `session.settle`; audit label added; `pos.sell` leaves the reserved list.

**Never:**
- The settle bar and payment split UI (3.13); Undo and `session.settle.reverted` (3.14); wallet, credit, packs (3.16+); receipts, invoice numbers, tax, shift binding (Epic 5); a persisted kiosk lock.
- Code comments (user rule).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Cash | open ₹75.00, cash 7500 | 1 payment, order settled, outcome has `undo_until` | none |
| Split | cash 5000 + UPI 2500 ref `1234` | 2 payments in order, order settled | none |
| Short | sum 7000 vs 7500 | refused, nothing written | `PAYMENT_SHORT` |
| Over | sum 8000 vs 7500 | refused | `PAYMENT_SHORT` |
| UPI ref | `123` or `12a4` | refused naming the row | `REFERENCE_REQUIRED` |
| Card ref | blank | refused naming the row | `REFERENCE_REQUIRED` |
| Retry | same key twice | first outcome, one set of payments | none |
| Second settle | settled order, any `base_rev` | refused | `STALE` |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{session,error,intent,permission,lib}.rs` -- `SessionSettle`, `PaymentRow`, `PaymentMethod`, `SettleOutcome`, `BillSettledEvent`, two error codes, `IntentKind::SessionSettle` (40 → 41 in `ALL`), `SnapshotBill.order_rev`.
- `crates/arena-store/migrations/0027_payments.sql`, `migrations.rs`, `src/{branch,sessions,lib}.rs` -- `payments` table, `orders.settled_at/settled_by`, `Table::Payments`, `RowKey::Payments`, order and payment readers and writers.
- `crates/arena-core/src/apply/{session,mod,guards,validate,mutate}.rs` -- the settle trio (`SettleTarget`).
- `crates/arena-core/src/snapshot.rs`, `devices.rs` -- `order_rev`; `settlement()` reader for the Hub.
- `crates/arena-api/src/{ui_auth,ui_events}.rs`, `error` maps in `arena-cloud` and `arena-kiosk-core` -- outcome, idle lock, `bill.settled`, HTTP status of the new codes.
- `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-replay/src/lib.rs`, `src/apply/tests/session.rs` -- generator, fixture, tests.
- `apps/hub/src/lib/{events.ts,tile-actions.svelte.ts,audit/labels.ts}`, `apps/hub/src/routes/+layout.svelte`, `packages/ui/src/messages/en.json` -- reload on `bill.settled`, refusals, audit label.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` types, intent kind, error codes, `order_rev`; `pnpm gen:types`
- [x] `arena-store` migration 0027, `Table::Payments`, readers and writers
- [x] `arena-core` `session.settle` apply and snapshot `order_rev`; apply tests
- [x] `arena-api` outcome, idle lock, `bill.settled` event, status mapping; tests
- [x] Proptest, replay fixture
- [x] Hub UI: event hook, refusal strings, audit label

**Acceptance Criteria:**
- Given an open order, when `session.settle` applies with payments summing to the total, then payments and the settled order exist, `bill.settled` is emitted and the outcome carries `settled_at` and `undo_until`.
- Given payments that miss the total, or a UPI row without 4 digits, or a card row without a reference, then the intent is refused `PAYMENT_SHORT` or `REFERENCE_REQUIRED` and nothing is written.
- Given a settle commits, then the kiosk returns to its idle locked screen when the device is free, and the snapshot no longer lists the bill.
- Given a retry with the same key or a second settle, then no payment is duplicated and the second settle is `STALE`.

## Spec Change Log

## Review Triage Log

## Design Notes

The outcome is read from the committed order row rather than from `Applied`, so a replayed key returns identical `settled_at`. The PC lock-clear is skipped when a new session already started on the device, so settling an old bill never locks a PC in use.

## Verification

**Commands:**
- `cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace` -- expected: pass
- `pnpm run check:generated && pnpm run check:protocol && pnpm run check:strings`; `pnpm --filter @arena/hub check` -- expected: pass
