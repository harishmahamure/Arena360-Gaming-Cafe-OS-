---
title: 'Story 5.18: Cash drawer kicks on cash sales'
type: 'feature'
created: '2026-10-28'
status: 'in-review'
baseline_commit: 'ce6751d'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-17-choose-print-or-none-at-settle-and-print-through-the-side-ef.md'
---

## Intent

**Problem:** Counter staff open the drawer by hand when they take cash, so a drawer opening is not logged and a cash sale can still be undone after the drawer was opened.

**Approach:**

- Settings → Receipts has a switch "Cash drawer on printer" (`receipt.drawer`, default off).
- With it on, a settle (`session.settle`, `pos.sale`, `plan.sell`) that wrote a cash `sale` event records the irreversible effect "drawer kicked" and queues a `drawer` row on `side_effect_outbox`, inside the settle transaction. The effect closes Undo (`UNDO_NOT_ALLOWED`, and no Undo toast because `SettleOutcome.irreversible` is true); a correction is a Refund. No I/O runs in `apply()`.
- `ReceiptPrintTask` (arena-tasks) sends queued drawer rows first, then prints, through the same `Printer` adapter and the shared one-job lock: the ESC/POS pulse `ESC p 0 25 250`. It applies the system intent `drawer.kicked`, which writes a `drawer_open` `cash_events` row (amount 0, the sale's order, the staff who took the cash) against the shift the sale was taken on, or `drawer.kick_failed`, which raises the one open "Printer offline" alert. A failure never touches the sale.

## Boundaries & Constraints

**Always:**

- A settle with no cash row, or with the setting off, queues no kick and records no effect (its Undo stays open).
- The kick is independent of the receipt choice: Print and the drawer queue side by side; None still kicks.
- `drawer.kicked` and `drawer.kick_failed` are system-only, take no `base_rev`, and refuse `STALE` a row that is not queued and `VALIDATION_FAILED` a row that is not a `drawer` row (a print result cannot close a drawer row or the reverse). `drawer.kicked` is also `STALE` when no shift holds the sale's cash.
- A result touches the order row so the change log (and the audit) carries the intent kind.

**Never:**

- Wallet top-up and credit-collection cash (their settle bars take no receipt choice; the drawer is for sales), refunds (5.24), shift close (5.26), a manual "open drawer" button.
- I/O inside `apply()`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Cash sale, drawer on | settle with a cash row | effect `drawer_kicked`, one queued `drawer` row | none |
| Undo after | revert of that sale | refused | `UNDO_NOT_ALLOWED` |
| Non-cash / drawer off | UPI only, or setting off | no row, no effect, Undo works | none |
| Print + drawer | `receipt: print`, cash | one `print` and one `drawer` row | none |
| Kick sent | `drawer.kicked` | row `done`, `drawer_open` cash event (amount 0) on the sale's shift | none |
| Kick fails | refused / no printer set up | `drawer.kick_failed`, row `failed`, one `printer_offline` alert, no cash event | the sale stays settled |
| Result twice | closed row | refused | `STALE` |

## Code Map

- `crates/arena-proto/src/receipt.rs` -- `KEY_RECEIPT_DRAWER`, `DEFAULT_RECEIPT_DRAWER`, `DrawerKicked`, `DrawerKickFailed`; `intent.rs` -- kinds `drawer.kicked`, `drawer.kick_failed` (system, Money category, `IntentKind::ALL` 88); `shift.rs` -- `CASH_DRAWER_OPEN`.
- `crates/arena-store/src/{outbox.rs,shifts.rs,lib.rs}` -- `KIND_DRAWER`, `shift_of_order_sale`, `Store::queued_drawers`.
- `crates/arena-core/src/settings_defaults.rs` -- the default, the `Bool` rule, keyed field; `src/apply/receipt_print.rs` -- `queue_on_settle` now runs `queue_drawer` for every settle and `queue_print` for Print; drawer results in `load`/`validate`/`mutate`, shared `fail_row`; wired in `mod.rs`, `mutate.rs`, `validate.rs`. `src/printer/escpos.rs` -- `drawer_kick()`. `src/snapshot.rs` -- `queued_drawers`.
- `crates/arena-tasks/src/receipts.rs` -- the task drains drawers then prints and picks the result intent by row kind.
- `apps/hub/src/lib/receipt.ts`, `routes/settings/receipt/+page.svelte` (switch), `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/receipt.rs` (5 new), `arena-tasks` `receipts.rs` (3 new), `escpos.rs`, `proptest_apply.rs` (`PosSale.drawer`, `assert_drawers_hold`), `tests/receipts.spec.ts`.

## Tasks & Acceptance

- [x] Protocol: setting key, two result intents, `drawer_open` cash kind.
- [x] Store: `drawer` outbox reads, the shift of a sale's cash.
- [x] Core: queue the kick and its irreversible effect at a cash settle; `drawer.kicked` / `drawer.kick_failed`.
- [x] Task: ESC/POS pulse through the shared printer and lock; result intents.
- [x] Hub UI: Settings → Receipts switch, strings, audit labels.
- [x] Tests: Rust matrix, task tests with the recorded fake printer, proptest op and invariant, Playwright.
- [x] Verification gate.

**Acceptance Criteria:**

- Given Settings → Receipts with "Cash drawer on printer" on, when a settle with a cash row commits, then a `drawer` outbox row is queued and the task sends the pulse; `drawer.kicked` is recorded with a `drawer_open` cash event against the shift.
- Given the drawer was kicked for a settle, then Undo is rejected `UNDO_NOT_ALLOWED` and no Undo toast shows; a correction is a Refund.
- Given a settle with no cash row, or the setting off, then no kick is sent.
- Given the printer is offline, when a kick fails, then the sale stays settled, `drawer.kick_failed` is recorded and the "Printer offline" alert is raised.

## Verification

- `cargo test` for `arena-proto`, `arena-store`, `arena-core` (844 lib tests and `proptest_apply` at 200 cases), `arena-api`, `arena-replay`, `arena-tasks` pass; `cargo clippy -D warnings` on the touched crates and `cargo fmt --check` are clean; `pnpm gen:types`, `check:strings`, `check:protocol`, prettier and Hub `svelte-check` pass; Playwright `receipts` and `settings` pass.
- Failures that are not from this story: `arena-tasks` `a_printer_that_stops_reading_times_out` and `trusted_time_is_cloud_time_plus_monotonic_elapsed` (also fail on the baseline, as noted in 5.17).

## Notes for later stories

- The drawer is for sales only; 5.24 refunds that pay cash back and 5.25 cash expenses may want a kick of their own (a new outbox payload, same `drawer` kind and results).
- `drawer_open` cash events carry amount 0, so the 5.26 variance maths must ignore them (they are a log, not money).
