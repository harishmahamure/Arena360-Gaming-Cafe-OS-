---
title: 'Story 5.12: Attach an order to a running session so it settles with the bill'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: 'c9011e0ac4df89f8d1ebcf8d73b48000692f8169'
baseline_revision: 'c9011e0ac4df89f8d1ebcf8d73b48000692f8169'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-11-sell-at-the-counter-a-walk-up-sale-settles-and-takes-stock.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Staff cannot add food and drinks to a running player's session. The POS only sells walk-up carts, the tile sheet has no Order action, and a session bill only holds time segments.

**Approach:**
- Intent `order.place {session_id, lines[{product_id, qty}], stock_override?}` (permission `pos.sell`) and its twin `order.place.reverted {order_id, placement_id}`. A placement adds product `order_lines` to the session's open bill order (created at the first placement, kind `session`, status `open`, no payments), takes stock through `stock_moves` reason `sale` (lot order, expired lots never taken, bundle components × qty, the 5.11 `OUT_OF_STOCK` / `BUNDLE_SHORT` / `stock.override` rules reused), and treats every line as served. Each placement has its own `placement_id`, stored on its lines and stock moves.
- Session End appends its segment lines to that existing order instead of creating one, and Undo of End removes only the segment lines (the order stays while product lines remain). Settle covers segments and orders together.
- Hub: the tile sheet of an in-session PC gets Order, which opens POS attached to that session. The cart header reads "In session · <player> · <device>" with Change (attach any running session by player lookup or device, or Guest). The settle bar is replaced by "Add to session bill". The tile bill shows order lines after the segments. The kiosk session screen shows "Ordered: <n> items".

## Boundaries & Constraints

**Always:**
- Supported sessions: postpaid (not in a group bill) and block sessions. Any other session mode, an ended or paused-for-settle session is refused `VALIDATION_FAILED session_id`.
- Placement rules are those of `pos.sale` (lines 1 to 100, qty 1 to 999, no repeated product, active products, expired stock never sold, untracked products move no stock). A refused placement writes nothing. `order.place` takes no `base_rev`; the Undo takes the order's `rev`.
- Order amounts: each line is `qty × unit price` at the product's price; the order `subtotal_minor` and `total_minor` grow by the placed amount (and shrink on Undo), so settle, payments and cash events work unchanged. No tax is computed on session bills in this story (session bills carry none today); lines record `tax_rate_bp`.
- Undo (8 s, same staff who placed, no irreversible effect recorded on the order) puts every lot quantity of that placement back through `sale_reverted` moves and removes only that placement's lines. An earlier placement's lines and moves are untouched.
- End of a session with a placed order: totals = segments + order lines; `session.end.reverted` deletes segment lines only and subtracts them; the order is deleted only when no product lines remain.
- `bill.reverted` and `bill.settled` events keep working; add `order.placed` (UI event) and an `ordered_items` count on the kiosk `SessionScreen` (default 0), pushed at placement.

**Never:**
- Stock holds, serve/cancel (5.13), invoice numbers (5.14), receipts and print (5.15 to 5.19), coupons, discounts, voids, refunds (5.21 to 5.24), kitchen tickets, tax on session bills, orders on pack / pass / wallet / grouped sessions (deferred).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| First placement | postpaid session running, 2 × Cola 40.00, no order | open `session` order created with 2 lines, total 80.00, stock −2 | none |
| Second placement | order exists | lines appended, total grows, new `placement_id` | none |
| End after order | session 60 min at 100.00/h + Cola 80.00 | order has segment and product lines, total 180.00, single order | none |
| Undo of End | End undone within 8 s | session reopens, segment lines gone, product lines and order stay, total 80.00 | none |
| Undo of placement | within 8 s, same staff | that placement's lines and stock back, order total reduced; order removed if it was the only content and no segments | `UNDO_NOT_ALLOWED` if late, other staff, or an effect recorded |
| Plain short | Cola 1 left, qty 2 | refused, nothing written | `OUT_OF_STOCK` |
| Bundle short | Combo needs Chips ×2, 1 left | refused | `BUNDLE_SHORT` naming Chips |
| Bundle override | same with `stock_override` + manager | Chips raised by `count_adjust` ("Override at sale"), placement applies | `PERMISSION_DENIED` without `stock.override` |
| Unsupported session | pack / pass / wallet / grouped / ended session | refused | `VALIDATION_FAILED session_id` |
| Settle | ended session with placed order, payments = total | one settle covers both | `PAYMENT_SHORT` if not equal |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0048_session_orders.sql` -- NEW: `order_lines.placement_id`, `placed_by`, `placed_at`; `stock_moves.placement_id` (extend the 0047 no-update trigger). Use `ALTER TABLE ... ADD COLUMN`, not a rebuild.
- `crates/arena-store/src/sessions.rs` -- `insert_order`, `insert_order_line`, `insert_product_line` (~1194), `find_order_for_session`, `list_order_line_ids`, `effect_recorded`, `forward_intent_id`, `list_open_order_lines` (~556); add placement-scoped readers and a segment-only line reader. `src/stock.rs` -- `list_order_moves` (~245), add placement filter.
- `crates/arena-proto/src/pos.rs` -- `OrderPlace`, `OrderPlaceReverted`, view types; `intent.rs` (kind enum ~121, ALL ~272, `as_str` ~355, category ~457, requirement ~559 `PosSell`, payload enum ~1142, `kind()` ~1273, pinned table ~1581); `lib.rs` re-exports (~184); `session.rs` (~576 events, `SessionScreen` ~728 gets `ordered_items`); regenerate TS in `packages/ui/src/generated`.
- `crates/arena-core/src/apply/pos_sale.rs` -- reuse (make `pub(super)`): `load_held`, `check_lines`, `price`, `plan_stock`, the mutate loop (~326-409); new `apply/order_place.rs` for load/validate/mutate of both intents. `apply/stock.rs` -- `sale_restorable`, `restore_sale_moves` (take a move slice). `plan_sales.rs::delete_sale_rows`.
- `crates/arena-core/src/apply/session.rs` -- `mutate_end` (348-515: reuse the existing order via `find_order_for_session`, update totals, append segments, segments sort before product lines), `validate_end_reverted` (~1110) and `mutate_end_reverted` (~1147-1213: delete segment lines only, subtract, keep order if product lines remain), `validate_settle` (~641). `force_end.rs` shares `load_end_reverted_of`. `apply/mod.rs` (`Target` ~138-204, `load()` ~206-308), `validate.rs` (~261), `mutate.rs` (~376), `guards.rs` (`overriding_stock` ~54 add `OrderPlace`; no-op list ~195-230; `base_rev` ~300).
- `crates/arena-core/src/snapshot.rs` -- `bills()` (~135-226) adds order lines (new `BillItem` with label, qty, amount) and an ordered count; `session_screen.rs::session_screens()` fills `ordered_items`.
- `crates/arena-api/src/ui_auth.rs` -- `UiIntentResult` gets a `placed` outcome (`order_id`, `order_rev`, `placement_id`, `undo_until`, `total_minor`); do NOT route through `topping_up`; elevate list (~1480) adds `OrderPlace` for `pos.sell` and `stock.override`; push `SessionStarted`-style screen refresh after a placement. `ui_events.rs` -- `order.placed`, fix `bill_reverted` (~265) to fire when the Undo of End keeps the order (Delete of `order_lines` with a `sessions` Update).
- `crates/arena-kiosk-core/src/ipc.rs` (`PcSession` ~82), `crates/arena-kiosk-svc/src/{pc.rs,session.rs,app.rs}` (`ScreenSession`), `apps/kiosk/src/lib/bridge.ts` (~225-262), `apps/kiosk/src/routes/+page.svelte` (session block) -- thread `ordered_items`; show "Ordered: <n> items" (`data-testid="ordered"`) only when n > 0.
- `apps/hub/src/lib/TileSheet.svelte` (in-session section ~675-825: add Order button → `/pos?session=<id>`), `tile-actions.svelte.ts` (`permissionFor` ~259 map `order.place*` to `pos.sell`), `routes/floor/+page.svelte` (`unpaid()` ~49 and the open-order bill blocks ~242, ~533 must not show a settle bar for a running postpaid session), `lib/events.ts` (`order.placed`), `routes/pos/+page.svelte` (cart header ~445-470, `attachedSession`, "Add to session bill", override flow ~328-395, undo restore ~217-232), `lib/pos.ts`, `lib/undo.svelte.ts` (`order` kind, `offerUndo`), `lib/UndoDock.svelte`, `lib/audit/labels.ts`, `packages/ui/src/components/BillBreakdown.svelte` (product rows after segments), `packages/ui/src/messages/en.json` (keys: in-session header, Order, Add to session bill, Ordered, undo text; `check:strings`).
- Tests: `crates/arena-core/src/apply/tests/order_place.rs` (NEW, register in `tests.rs`; model `tests/pos_sale.rs`, fixtures `primary()`), update `tests/{session,extend,group_bill,prepaid,rate_flip,transfer}.rs` where they assume End creates / Undo deletes the order; `crates/arena-core/tests/proptest_apply.rs` (Ops `OrderPlace`, `OrderPlaceReverted`; End change count; running-session settle filter; invariant order total = sum of lines); `crates/arena-replay/src/lib.rs` fixtures; `crates/arena-store/tests` migration test for 0048; `ui_auth.rs` and `ui_events.rs` tests; `apps/hub/tests/{pos,tile-sheet,undo,session-end}.spec.ts` (+ `floor-fixture.ts`); `apps/kiosk/tests/pc-screen.spec.ts`.

## Tasks & Acceptance

**Execution:**
- `crates/arena-store` -- migration 0048, placement readers/writers, segment-only line reader, migration test -- schema for placements
- `crates/arena-proto` -- `order.place` / `order.place.reverted` payloads and kinds, `order.placed` event, `BillItem`, `SessionScreen.ordered_items`; regenerate TS -- wire contract
- `crates/arena-core` -- `order_place.rs`, reuse 5.11 stock planning, End appends to the existing order, Undo of End removes segments only, guards, snapshot bills and screen count -- domain
- `crates/arena-core` tests -- `order_place.rs` unit tests for every matrix row, updates to affected session tests, proptest ops and invariant, replay fixtures -- proof
- `crates/arena-api` -- placed outcome, elevate list, `order.placed` event, `bill_reverted` fix, kiosk screen refresh, tests -- API
- `crates/arena-kiosk-*`, `apps/kiosk` -- thread `ordered_items`, render "Ordered: <n> items", test -- kiosk surface
- `apps/hub` -- tile-sheet Order, POS session attach (header, Change chooser, Add to session bill, override PIN, Undo), bill breakdown rows, messages, labels -- Hub surface
- `apps/hub/tests` -- Playwright: Order from tile opens attached POS, place, second placement, Undo, short bundle override, bill shows order lines, no settle bar on running session; unit tests for helpers -- surface proof
- Verification gate.

**Acceptance Criteria:**
- Given an in-session tile's sheet, when staff press Order, then POS opens with the cart attached to that session and the header reads "In session · <player> · <device>" with Change; Change attaches any running session by player lookup or device.
- Given an attached cart, when staff press "Add to session bill", then `order.place` adds the lines to the session's open bill order (created at the first placement), `order.placed` is pushed and the kiosk session screen shows "Ordered: <n> items".
- Given lines placed, then tracked stock (and bundle components) is decremented at placement through `stock_moves` reason `sale` and every placed line is treated as served.
- Given the session bill, then order lines appear after the session segments, one settle covers both, and Undo of End removes only the segment lines while placed orders stay on the bill.
- Given a placement exceeding stock, then it is rejected `OUT_OF_STOCK` or `BUNDLE_SHORT` (with the Story 5.11 override for bundles).
- Given the placement's Undo toast, when `order.place.reverted` applies within 8 s, then the lines are removed and their stock returned.

## Spec Change Log

## Review Triage Log

## Design Notes

- Session bills carry no tax today; this story keeps that (the order total is the sum of line amounts) and records `tax_rate_bp` on lines so a later story can tax the combined bill. Pack / pass / wallet / grouped sessions are refused (their closing paths create their own order, `orders_one_per_session`); merging is deferred.
- `placement_id` (the intent id) scopes Undo and keeps a second placement's stock moves apart from the first. Moves already reverted are excluded.
- A running postpaid session whose order holds only product lines must not show a settle bar or be settle-able: `validate_settle` requires an ended session for non-block sessions.
- "Treated as served": lines are written complete, no `served` column until 5.13.

## Verification

**Commands:**
- `cargo test` on `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay`, `arena-kiosk-core` -- pass
- `cargo clippy ... -D warnings`, `cargo fmt --check` -- clean
- `pnpm --filter hub check`, `pnpm --filter kiosk check`; `pnpm exec playwright test` in `apps/hub` and `apps/kiosk`; `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `check:generated` -- pass
