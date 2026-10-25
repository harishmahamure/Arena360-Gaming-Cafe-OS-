---
title: 'Story 5.5: Stock-in with expiry lots and weighted average cost'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '8cecad6'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-4-bundles-priced-on-their-own.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-3-products-with-categories-barcodes-images-and-the-kitchen-fla.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-2-tax-is-computed-once-per-rate-on-every-bill-in-minor-units.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Stock levels, stock value and expiry tracking need real numbers. Until now every tracked product counts as quantity 0 (`UNRECORDED_STOCK`), so nothing tracked can be sold and no cost is known.

**Approach:**
- New intents `stock.in` and `stock.in.reverted` (permission `stock.in`, held by Counter, Manager, Owner). A stock-in appends a `stock_moves` row, adds a `stock_lots` row, raises `stock.qty` and moves `stock.avg_cost_minor` to the weighted average of the quantity on hand and the new lot, in integer minor units, half-up.
- Three new change-log tables: `stock` (one row per tracked product, created at its first stock-in; an absent row reads as 0), `stock_lots`, `stock_moves` (append-only, enforced by triggers).
- A pure decrement planner (`consume_plan`) and a store-level `decrement` that later stories (5.6 waste, sales, write-offs) call: earliest expiry first, lots without expiry last, never below zero, lots used recorded on the move.
- The 5.3 placeholder is replaced: `Product.quantity` and `catalog.changed` availability read `stock.qty`; bundles read their components' real quantities (5.4). A stock write pushes `catalog.changed` for its product (and, through 5.4, its bundles).
- Hub: Inventory → Stock-in page with an 8 s Undo.

## Boundaries & Constraints

**Always:**
- **Hub migration** `0020_stock.sql` (0019 is 5.4; renumber on rebase if a sibling took 0020). Tables:
  - `stock(product_id PK -> products, qty >= 0, avg_cost_minor >= 0, rev, created_at, updated_at)`;
  - `stock_lots(lot_id PK, product_id, qty_in > 0, qty_remaining 0..qty_in, unit_cost_minor >= 0, expiry TEXT NULL 'YYYY-MM-DD', received_at, rev, created_at, updated_at)`;
  - `stock_moves(move_id PK, product_id, reason, delta <> 0, before_qty, before_avg_cost_minor, unit_cost_minor, supplier, lot_id NULL, lots_used JSON text, staff_id, at, rev, created_at, updated_at)` with triggers that abort any delete and any update of a business column (only the `rev`/`updated_at` stamp may touch a row).
  All three join the change log (`Table::Stock`, `StockLots`, `StockMoves`).
- **Intents:** `stock.in {product_id, qty, unit_cost_minor, supplier = "", expiry = null}`; `stock.in.reverted {move_id}`. Both require `stock.in`. No `base_rev`.
- **Weighted average** (`arena_proto::weighted_avg_cost`): `round_half_up((old_qty * old_avg + in_qty * in_cost) / (old_qty + in_qty))`, computed in `i128`; with `old_qty = 0` the result is the new cost whatever `old_avg` was. Golden: 10 at 2000 and 30 at 2400 gives 2300. The result never leaves `[min(old_avg, in_cost), max(old_avg, in_cost)]`.
- **Validation** (field in brackets; nothing is written on any failure, `VALIDATION_FAILED`):
  - `product_id`: no such live product, a bundle, or a product with `track_stock = 0`;
  - `qty`: outside 1 to 1 000 000, or the stock would pass 100 000 000;
  - `unit_cost_minor`: negative or above `MAX_PRICE_MINOR` (0 is allowed: free stock);
  - `supplier`: more than 80 characters or a control character (trimmed; empty allowed);
  - `expiry`: not a real `YYYY-MM-DD` calendar date, or before today's business day in the branch's locale (today itself is allowed).
- **Stock-in effect:** `stock` row inserted (qty, avg) or updated; `stock_lots` row with `qty_in = qty_remaining = qty`; `stock_moves` row `reason = stock_in`, `delta = +qty`, `before_qty`, `before_avg_cost_minor`, `lot_id`, `lots_used = []`, `staff_id = actor.id`, `at = now`. Written in that order: stock, lot, move.
- **Decrement** (`arena_proto::consume_plan`, `arena_core::decrement`): lots with stock are taken earliest `expiry` first (ISO dates compare as text), lots without expiry last, ties by `received_at` then `lot_id`; `None` (nothing written) when `qty <= 0` or `qty` exceeds the sum of the lots; otherwise the plan sums to exactly `qty` and no lot is over-drawn. `decrement` lowers `stock.qty` and the lots, leaves `avg_cost_minor` unchanged, appends a move with `delta = -qty` and `lots_used = [{lot_id, qty}]`, and refuses (`OUT_OF_STOCK` is 5.6's code; here `VALIDATION_FAILED qty`) when `stock.qty` or the lot sum is short. No intent calls it yet; 5.6 does.
- **Undo** (`stock.in.reverted`), under the Story 3.14 rules as far as this story can honour them: the move must exist and be a `stock_in` (`VALIDATION_FAILED move_id` otherwise; `NOT_FOUND` for an unknown id is not used, the field error is); the actor must be the staff member who made the stock-in (`PERMISSION_DENIED`); `now - move.at <= 8000` ms (`EXPIRED`); the move must still be the latest move of its product and its lot must be untouched (`qty_remaining = qty_in`), else `STALE`. Effect: the lot is deleted, `stock` returns to `before_qty` and `before_avg_cost_minor` exactly (no reverse arithmetic), and a compensating `stock_moves` row `reason = stock_in_reverted`, `delta = -qty` is appended. A second revert finds a later move and fails `STALE`.
- **Read:** `GET /ui/products` rows carry the real `quantity` and a new `avg_cost_minor`; a bundle's quantity comes from `bundle_quantity` over its tracked components' real quantities.
- **Feed:** `catalog.changed` (LAN and UI) lists the product of every `stock` row a commit wrote; 5.4 adds parent bundles.
- **Hub UI** `/inventory/stock-in`: product select (active-or-not tracked non-bundle products), quantity, unit cost (locale exponent), supplier, optional expiry date; field errors inline; after success a status line with an Undo button for 8 s (counts down, disappears); the product list shows on hand and average cost. Linked from `/inventory`. Strings in `en.json`; axe clean.
- **Tests:** unit and boundary tests for the average (golden, zero stock, rounding ties, extremes, monotone bounds), date validation, `consume_plan`; proptest properties on the average and on random stock-in/decrement sequences (conservation, never negative, lot order); apply tests for every refusal with nothing written, the undo rules, append-only triggers, bundles and the catalog placeholder replaced; migration upgrade 0019 to 0020; replay reproduces the new tables; Story 1.8 proptest generator gains `stock.in` and `stock.in.reverted`; Playwright.

**Never:**
- A stock row for a bundle or an untracked product, selling or settling (5.x POS), waste and adjustment intents (5.6), expiry alerts and reports (later), edits to a lot after the fact, negative stock, supplier master data, stock-in by CSV.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected |
|----------|--------------|----------|
| Golden | 10 on hand at 2000, stock-in 30 at 2400 | qty 40, avg 2300, 2 lots |
| First stock-in | no `stock` row, 5 at 150 | row created: qty 5, avg 150 |
| Depleted then restocked | qty 0, old avg 900, 4 at 100 | avg 100 |
| Rounding tie | 1 at 1, 1 at 2 | avg 2 (1.5 rounds up) |
| Free stock | cost 0 | accepted; avg falls |
| Past expiry | yesterday | `VALIDATION_FAILED expiry`; today accepted |
| Bad date | `2026-02-30`, `26-1-1` | `VALIDATION_FAILED expiry` |
| Bundle / untracked / deleted / unknown | any | `VALIDATION_FAILED product_id` |
| Qty 0, negative, 1 000 001; total over 100 000 000 | any | `VALIDATION_FAILED qty` |
| Decrement across lots | lots A (exp 1 Jan, 3), B (exp 1 Feb, 5), C (no expiry, 10); take 7 | A 3, B 4; C untouched |
| Decrement more than held | take 19 of 18 | refused, nothing written |
| Undo in time | by same staff, nothing since | lot gone, qty and avg as before, compensating move |
| Undo late / other staff / after another move / twice | | `EXPIRED` / `PERMISSION_DENIED` / `STALE` / `STALE` |

</frozen-after-approval>

## Code Map

- `crates/arena-store`: `migrations/0020_stock.sql`, `stock.rs` (rows and queries), `branch.rs` (`Table::Stock`, `StockLots`, `StockMoves`, `RowKey`), `migrations.rs`, `lib.rs` (`Store::stock`).
- `crates/arena-proto`: `stock.rs` (payloads, `weighted_avg_cost`, `consume_plan`, `valid_date`, limits), `intent.rs` (kinds, requirement, payloads), `catalog.rs` (`Product.avg_cost_minor`), `lib.rs`.
- `crates/arena-core`: `apply/stock.rs` (load, validate, mutate, `decrement`), `apply/{mod,mutate,validate,guards}.rs`, `catalog.rs` (real quantities).
- `crates/arena-api`: `lan/registry.rs` (`product_ids` includes `stock`), fixtures.
- `crates/arena-replay`, `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-core/tests/stock_props.rs`.
- `apps/hub`: `src/routes/inventory/stock-in/+page.svelte`, link on `inventory/+page.svelte`, `tests/stock-in.spec.ts`; `packages/ui/src/messages/en.json`; regenerated `packages/ui/src/generated`.

## Tasks & Acceptance

- [ ] Proto math and payloads, with tests first.
- [ ] Store migration, rows, change-log tables.
- [ ] Apply: stock.in, stock.in.reverted, decrement; catalog; feed.
- [ ] Property, replay and proptest-generator coverage.
- [ ] Hub page, messages, Playwright.
- [ ] Verification commands from the loop state.

**Acceptance Criteria:** the Given/When/Then of Story 5.5 in `epics.md`, as refined above. The "Story 3.14 rules" are implemented for this intent alone (same staff, 8 s, nothing since); the shared undo toast and guard arrive with 3.14.

## Spec Change Log

## Review Triage Log
