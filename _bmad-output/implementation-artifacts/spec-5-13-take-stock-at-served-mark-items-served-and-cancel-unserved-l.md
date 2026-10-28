---
title: 'Story 5.13: Take stock at served, mark items served, and cancel unserved lines into waste'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '8dcfc80bd09e622fe7917108c6e4810b5a086f69'
baseline_revision: '8dcfc80bd09e622fe7917108c6e4810b5a086f69'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-12-attach-an-order-to-a-running-session-so-it-settles-with-the.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Stock is taken the moment a product is placed on a session bill, so a cancelled or not-yet-made kitchen item distorts stock, and there is no way to mark a line served or cancel it.

**Approach:**
- Setting `inventory.take_stock_at` (`served` default, `order`) in Settings → Inventory. Placed lines carry a `served` flag; new table `stock_holds (id, order_line_id, item_id, qty, created_at, released_at)`; available stock = usable on hand minus open holds.
- In `served` mode a placement serves non-kitchen lines at once (stock taken as in 5.12) and leaves kitchen-flagged lines unserved with one hold per tracked item (a bundle's parts × qty), no stock move. In `order` mode stock is taken at placement as before; kitchen lines are still unserved until marked.
- New intents `order.line_served {line_id}` (takes the stock of the line's open holds, closes them, marks served) and `order.line_cancel {line_id, made}` with a required reason. Cancel is refused for served lines. `made = false` closes the holds (or, in `order` mode, returns stock with move reason `return`); `made = true` records waste `cancelled_order` at cost. The line leaves the bill and the order total shrinks. Walk-up sales stay fully served.
- The tile sheet's order section lists placed lines with Mark served and Cancel (reason, "Was it already made?"). A session bill cannot settle while it has unserved lines.

## Boundaries & Constraints

**Always:**
- Permission `pos.sell` for both intents; neither takes `base_rev`. Cancel reasons: `changed_mind`, `mistake`, `other`; `made` is required.
- Available stock for sale checks (`plan_stock`, catalogue availability) = on hand − expired lots − open holds. A refused write changes nothing.
- A line is "held" when it has open holds. An unserved line without holds took its stock at placement (order mode) or tracks no stock.
- Waste for a cancelled line that was made: reason `cancelled_order`, valued at the average cost. In `order` mode the stock first goes back through a `return` move into a new lot, then the waste move takes it again; in `served` mode the hold is closed and waste takes the quantity (capped at the stock on hand).
- Undo of a placement (5.12) deletes the placement's open holds; it is refused `UNDO_NOT_ALLOWED` once a `return` move exists for the placement (an order-mode line was cancelled).
- `validate_settle` of a session order refuses `VALIDATION_FAILED` while any line is unserved ("Mark served or cancel unserved items first").
- Existing and counter-sale lines are served (`served` default 1). `pos.sale` takes stock at settle in both modes.

**Never:** Undo twins for serve and cancel, kitchen tickets or KOT (Epic 7), void of an unsettled order (5.23), partial-quantity cancel, refunds, invoice numbers.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Served mode, kitchen line | Burger (kitchen, tracked) ×2 placed | line unserved, hold of 2, no stock move, available −2 | none |
| Served mode, plain line | Cola ×1 | served, `sale` move, no hold | none |
| Order mode, kitchen line | Burger ×2 | stock −2 now, line unserved, no hold | none |
| Mark served (held) | line with open hold | `sale` move for the hold qty, hold closed, line served | `OUT_OF_STOCK` if stock changed |
| Mark served (order mode) | unserved, no holds | line served, no stock change | none |
| Mark served again | served line | refused | `VALIDATION_FAILED line_id` |
| Cancel, not made, served mode | held line | hold closed, line deleted, total reduced, no stock move | none |
| Cancel, not made, order mode | unserved line | `return` move, line deleted | none |
| Cancel, made | either mode | waste `cancelled_order` at cost, line deleted | none |
| Cancel served line | served line | refused | `VALIDATION_FAILED line_id` |
| Availability | on hand 5, open hold 4 | a sale of 2 is refused | `OUT_OF_STOCK` |
| Settle with unserved | session order with unserved line | refused | `VALIDATION_FAILED` |
| Undo placement | held lines | lines and holds removed | `UNDO_NOT_ALLOWED` after a cancel in order mode |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0049_stock_holds.sql` -- NEW: `order_lines.served/served_at/served_by`, `stock_holds` table (+ `rev/created_at/updated_at`), setting seeded by core defaults. `src/branch.rs` -- `Table::StockHolds`, `RowKey::StockHolds`. `src/sessions.rs` -- `OrderLineRow` gains `id`, `served`; `NewProductLine.served`; line readers; `src/stock.rs` -- hold readers/writers, `open_hold_totals`.
- `crates/arena-proto/src/{pos.rs,intent.rs,lib.rs,session.rs,stock.rs}` -- `OrderLineServed`, `OrderLineCancel`, kinds, `ReasonCode::{ChangedMind,Mistake}`, `BillItem.line_id/served`, `KEY_INVENTORY_TAKE_STOCK_AT`.
- `crates/arena-core/src/apply/pos_sale.rs` -- `load_held` subtracts holds; `plan_stock` takes a defer predicate and returns holds. `apply/order_place.rs` -- hold creation, undo deletes holds. NEW `apply/order_line.rs` -- serve and cancel. `apply/stock.rs` -- `return_to_stock`, `consume_for`. `apply/{mod,validate,mutate,guards}.rs` -- wiring. `apply/session.rs::validate_settle`. `settings_defaults.rs` -- key, default, rule. `snapshot.rs` -- bill items carry line id and served. `catalog.rs` -- availability less holds.
- `crates/arena-api/src/ui_auth.rs`, `ui_events.rs` -- results and events for the two intents.
- `apps/hub/src/lib/TileSheet.svelte`, `tile-actions.svelte.ts`, `routes/settings/inventory/+page.svelte` (+ nav), `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`; regenerate TS in `packages/ui/src/generated`.
- Tests: `crates/arena-core/src/apply/tests/order_line.rs` (NEW, model `tests/order_place.rs`), `crates/arena-store` migration test, `apps/hub/tests/*.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store` -- migration 0049, `stock_holds` table plumbing, line/hold readers and writers -- schema
- [x] `crates/arena-proto` -- payloads, kinds, reasons, setting key, bill item fields; regenerate TS -- wire contract
- [x] `crates/arena-core` -- setting default and rule, holds in availability and placement, `order_line.rs`, settle gate, snapshot, catalogue availability -- domain
- [x] `crates/arena-core` tests -- every matrix row; proptest ops where the generator allows -- proof
- [x] `crates/arena-api` -- results and events -- API
- [x] `apps/hub` -- Settings → Inventory, order section with Mark served and Cancel flow, messages, labels; tests -- Hub surface
- [x] Verification gate

**Acceptance Criteria:**
- Given "Served" mode, when lines are placed, then non-kitchen lines are served at placement and kitchen lines stay unserved with a hold; with "Order" stock is taken at placement.
- Given an unserved kitchen line, when staff press Mark served, then `order.line_served` takes stock, closes the hold and marks it served.
- Given an unserved line, when staff cancel it answering "Was it already made?", then No releases the hold (or returns stock as `return`) and Yes records waste "Cancelled order" at cost; the line leaves the bill and the audit records reason and staff.
- Given a served line, then cancel is not offered and the server refuses it.
- Given a walk-up sale, then all its lines are served at settle in either mode.

## Spec Change Log

## Review Triage Log

## Design Notes

- Mode is read at placement; a line's held state follows its holds, so a later setting change never strands a line.
- Order-mode cancel returns stock into a fresh no-expiry lot at the average cost, as the sale override does, because the line's share of the placement's lot draw is not recorded per line.

## Verification

**Commands:**
- `cargo test` on `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay`; `cargo clippy ... -D warnings`; `cargo fmt --check`
- `pnpm --filter hub check`; Playwright in `apps/hub`; `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `check:generated`
