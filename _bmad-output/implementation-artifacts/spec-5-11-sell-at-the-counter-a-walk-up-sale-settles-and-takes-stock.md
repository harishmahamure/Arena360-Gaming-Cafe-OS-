---
title: 'Story 5.11: Sell at the counter — a walk-up sale settles and takes stock'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '4e9fc3542278edbadbc5d16579abdcd6248fa89b'
baseline_revision: '4e9fc3542278edbadbc5d16579abdcd6248fa89b'
route: 'dispatch'
review_loop_iteration: 0
warnings: ['oversized']
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
---

<intent-contract>

## Intent

**Problem:** The POS screen builds a cart but its Settle button is disabled: no order, tax, payment, cash event or stock movement is ever written for a product sale.

**Approach:**
- Intent `pos.sale {lines[{product_id, qty}], payments, player_id?, credit_override?, stock_override?}` (permission `pos.sell`) and its twin `pos.sale.reverted {order_id}`. One transaction writes an `orders` row (kind `pos_sale`, settled, no session) with the Story 5.2 per-rate tax breakup (`tax_minor`, `tax_breakup`), `order_lines` (product, qty, unit price, rate), `payments` (the Story 5.10 shift gate and cash events apply), wallet/credit rows when a player is attached, and `stock_moves` reason `sale` (lot order, expired lots never taken; a bundle takes each component × qty).
- Short stock: a tracked plain product is refused `OUT_OF_STOCK`, a short bundle `BUNDLE_SHORT` naming the component. With `stock_override` (needs `stock.override`: the actor's own, or the elevating manager's) a `count_adjust` move first raises each short component to the needed quantity (note "Override at sale", `elevated_by`), so stock never goes below zero.
- Undo (8 s, same staff, no effect recorded): compensating `sale_reverted` cash rows (Story 5.10) and `sale_reverted` stock moves put every lot quantity back; payments, wallet rows, lines and the order are removed; the Hub restores the cart.
- Hub: the settle bar is live on POS with one Cash row prefilled for the whole total (only when a shift is open), Ctrl+Enter settles, wallet and credit appear once a player is attached, a refused short bundle offers an `inline-pin` override, the undo toast shows, and "Recent sales" lists the business day's walk-up sales (`GET /ui/pos/sales`) with a read-only detail.

## Boundaries & Constraints

**Always:**
- Server totals are authoritative: lines are summed per rate and tax computed once per rate (`invoice::tax::compute`, `inclusive` from `branch.tax`; GST mode uses each product's `tax_rate_bp`, simple mode the branch `rate_bp`). The payments must equal the total (`PAYMENT_SHORT`). The Hub ports `compute` and shows the same figure.
- Lines: 1 to 100, qty 1 to 999, no repeated product; every product must exist, be active and not deleted (`VALIDATION_FAILED lines[i]`). An untracked product moves no stock. Stock usable = on hand minus expired lots.
- A refused sale writes nothing. `pos.sale` takes no `base_rev`.
- An Undo after an irreversible effect, by another staff member, or past 8 s is `UNDO_NOT_ALLOWED`.

**Never:**
- Session attach (5.12), serve/hold stock (5.13), invoice numbers (5.14), receipts, print, drawer (5.15–5.19), coupons, discounts, voids, refunds (5.21–5.24), a kitchen ticket.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected |
|----------|--------------|----------|
| Walk-up cash | 2 × Cola 40.00, shift open, cash 80.00 | order settled, 1 payment, `sale` cash event, stock −2 (lot order) |
| Mixed | cash + UPI | one cash event for the cash row only |
| No shift, cash | cash row | `SHIFT_NOT_OPEN`, nothing written |
| Plain short | Cola 1 left, qty 2 | `OUT_OF_STOCK` |
| Bundle short | Combo needs Chips ×2, 1 left | `BUNDLE_SHORT` naming Chips |
| Override | same, `stock_override` + manager | Chips raised +1 (`count_adjust`, "Override at sale"), sale applies, Chips 0 |
| Override, no right | counter staff, no elevation | `PERMISSION_DENIED` |
| Expired lot | only expired stock | `OUT_OF_STOCK` |
| Exclusive tax | 5% exclusive | total = subtotal + tax; payments must match |
| Undo | within 8 s | cash `sale_reverted`, stock restored to the same lots, order gone |
| Undo late / other staff | | `UNDO_NOT_ALLOWED` |

</intent-contract>

## Code Map

- `crates/arena-proto`: new `pos.rs` (`PosSale`, `PosSaleLine`, `PosSaleReverted`, `PosSaleView`, move constants), `intent.rs`/`lib.rs`/`error.rs` (`BUNDLE_SHORT`), generated TS in `packages/ui/src/generated`.
- `crates/arena-store`: `migrations/0047_pos_sales.sql` (orders kind + tax columns, order_lines product columns, stock_moves `order_id`/`note`/`elevated_by`), `sessions.rs`/`stock.rs`/`wallet.rs` helpers, `list_pos_sales`.
- `crates/arena-core`: new `apply/pos_sale.rs`; `apply/{mod,validate,mutate,guards}.rs` wiring; `apply/stock.rs` (`consume_for_sale`, `restore_sale_moves`); `snapshot.rs` reader `pos_sales`.
- `crates/arena-api/src/ui_auth.rs`: `topping_up` includes `PosSale` (settled outcome for the Undo toast), `GET /ui/pos/sales`, status maps for `BUNDLE_SHORT` (also `arena-cloud`, `arena-kiosk-core`).
- Reuse: `session::{validate_payments, insert_payments, record_payer_txns, load_payer}`, `shift::{gate, reverse_sales}`, `plan_sales.rs` as the shape of an order-creating intent and its Undo.
- `apps/hub`: `lib/pos.ts` (tax port, sale lines), `lib/SettleBar.svelte` (prefill, Ctrl+Enter), `lib/undo.svelte.ts` (`pos` kind), `routes/pos/+page.svelte`, `lib/pos-sales.ts`, `packages/ui/src/messages/en.json`, `tests/pos.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: payloads, kinds, `BUNDLE_SHORT`, view types; regenerate TS.
- [x] Store: migration 0047 and helpers.
- [x] Core: `pos.sale`, `pos.sale.reverted`, override, tax; guards (`stock.override`).
- [x] Core tests (`apply/tests/pos_sale.rs`), proptest op, replay/fixtures.
- [x] API: settled outcome, `GET /ui/pos/sales`, status maps.
- [x] Hub: live settle bar on POS, override PIN, undo + cart restore, Recent sales, messages.
- [x] Hub tests: Playwright (sale, mix, no shift, override, undo restore, recent sales, < 10 s) and unit tests (tax port).
- [x] Verification gate.

**Acceptance Criteria:** the Given/When/Then of Story 5.11 in `epics.md`.

## Spec Change Log

## Review Triage Log

## Design Notes

- The override's `count_adjust` raise is kept when the sale is undone: it corrects the stock record, which is not the sale's to take back.
- An order line carries its product's name in `label`; a bundle is one line (the bundle's own price and rate); its components appear only as stock moves.
- Invoice numbers arrive in 5.14, so Recent sales shows "Pending" for every sale until then.

## Verification

**Commands:**
- `cargo test` on `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay` -- pass
- `cargo clippy ... -D warnings`, `cargo fmt --check` -- clean
- `pnpm --filter hub check`; `pnpm exec playwright test` in `apps/hub`; `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `check:generated` -- pass

## Auto Run Result

Status: review

- **Change:** `pos.sale` / `pos.sale.reverted` (migration 0047, `apply/pos_sale.rs`, stock helpers in `apply/stock.rs`), `GET /ui/pos/sales`, and the live POS settle bar with override PIN, Undo with cart restore and Recent sales on the Hub.
- **Verification (all pass):** `cargo fmt --check`; `cargo clippy -D warnings` on proto, store, core, api, replay; `cargo test` on proto (176), store (55), core (752 lib plus integration and proptest), api (242), replay; `pnpm --filter hub check` (0 errors); `check:strings`, `check:layers`, `check:protocol`, `check:hex`; Playwright `pos`, `settle`, `shift` specs (129 passed), full suite 1400 passed by the implementation run. Every I/O matrix row has a passing test.
- **Pre-existing failures on this Windows host** (unrelated): `arena-e2e` is unix-only, `arena-kiosk-svc` discovery/pipe tests, `arena-cloud` backup hung-run test.
- **Residual risks:** a plain product short of stock is refused `OUT_OF_STOCK` even with an override (the spec allows the override for bundles only); every per-line validation error uses the field `lines[i]`; the `orders` rebuild in 0047 is covered by a migration test but has not run against a production database; the code review pass (step 4) is still to run.

## Code Review (2026-10-28)

Fixed in review: `credit.manage` now guards `credit_override` on `pos.sale`; a plain line that shares a product with a bundle is refused `OUT_OF_STOCK` and never raised by an override; `bill.settled` is emitted for a walk-up sale (reader `is_pos_sale`); an Undo restores the cart only when the POS screen is showing and the cart is empty; Recent sales rows list their payment methods. Tests added for the first two. Deferred (not blocking): nested bundles, tax-settings load failure UX, unbounded `GET /ui/pos/sales`.
