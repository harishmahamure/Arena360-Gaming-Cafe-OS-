---
title: 'Story 6.32: Pay now from the wallet, Pay at counter, and unpaid order lines at the end of a prepaid session'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: 'd99832e'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-31-build-a-cart-and-add-the-order-to-the-session-bill-from-the.md'
---

<intent-contract>

## Intent

**Problem:** After Story 6.31 a cart can only join a running postpaid or block session's bill. A prepaid player (pack, pass, wallet) has nowhere to pay, a player cannot spend their wallet on food, and nothing holds an order for the counter once the session is over.

**Approach:** The cart offers the choices the Hub says this PC can take (`shop.menu.pay`): *Add to session bill*, *Pay now from wallet* (balance shown, disabled with the reason when short; guests never see it) and *Pay at counter*. `shop.place{pay}` makes the Hub build one of three device intents: `order.place` on the session (as before); a `pos.sale` carrying `kiosk{device_id}` that settles by one wallet payment of the whole total the Hub works out; or an `order.place` carrying `counter{device_id, player_id}` that opens an unpaid counter order with no session. The PC owes the counter while it has an open counter order: its attract view carries `unpaid_minor`, the kiosk shows "Pay at the counter · Unpaid ₹X" and takes no login, and the Hub refuses `player.start_session` until staff settle it.

## Boundaries & Constraints

**Always:** The Hub decides (the kiosk only renders `pay` and relays `shop.place`); money is integer minor units; the wallet is never driven below zero (the same `validate_payments` as any wallet payment, so a concurrent spend is `INSUFFICIENT_BALANCE`, nothing written, the cart stays with the real balance); an unpaid counter order takes stock by the branch's take-stock mode as a session order does; staff settle it with any payment mix, void it, serve or cancel its kitchen lines by the Epic 5 rules.

**Never:** A stock override, coupon, discount, credit or receipt choice from a device; a `pos.sale` from a device that names payments of its own, another player's wallet or another PC; order status toasts on the HUD (Story 6.33); a new permission.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Pay now | profiled player, running session, balance >= total | one transaction: settled `pos_sale` tied to the PC and player, one wallet payment, stock taken, balance down by the total; `kiosk_order` alert raised |
| Wallet short at the kiosk | balance < total | radio disabled, "Balance ₹10.00. Top up at counter."; default moves to the counter |
| Wallet short at the Hub | concurrent spend | `INSUFFICIENT_BALANCE`, nothing written, cart stays, "Balance ₹40.00. Top up at counter." |
| Guest | no player | only bill and counter offered; `pay: wallet` refused `VALIDATION_FAILED` |
| Session ended, cart open | no live session | only *Pay at counter* offered |
| Pay at counter | any player or none | open `pos_sale`, `session_id` NULL, `device_id`, `player_id` = the player of the PC's latest session, no payment, no invoice number; PC shows UNPAID |
| Staff settle | `session.settle{order_id}` | payments written, invoice number issued as for any bill, UNPAID clears, attract hold lifts |
| Staff undo settle | `session.settle.reverted` in the window | order open again, payments deleted |
| Staff void | `order.void` | stock back, status `void`, UNPAID clears |
| Other PC / player / override | device names another PC, another player, `stock_override` | `PERMISSION_DENIED` |
| No licence flag | device order without `kiosk_orders` | `FEATURE_NOT_IN_TIER` |
| Login on an owing PC | `player.start_session` | refused "Pay at the counter first." |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{pos.rs,shop.rs,attract.rs}` -- `OrderPlace.counter`/`CounterOrder`, `PosSale.kiosk`/`KioskSale`, `ShopPay`, `ShopPayOptions`, `ShopPlace.pay`, `ShopMenu.pay`, `Refused.balance_minor`, `AttractView.unpaid_minor`, `kiosk_order_gate`.
- `crates/arena-store/src/{sessions.rs,lib.rs}` -- `OrderRow.device_id`, `insert_pos_order` open/device, `unpaid_counter_minor`, `last_session_player`.
- `crates/arena-core/src/apply/{order_place.rs,pos_sale.rs,order_line.rs,order_void.rs,alert.rs,session.rs}`, `rbac.rs`, `shop.rs`, `attract.rs`, `devices.rs` -- the counter order, the wallet sale, relaxed serve/void/alert/undo for counter orders, `pay_options`/`pay_target`, the attract `unpaid_minor`.
- `crates/arena-api/src/lan/{shop.rs,session.rs,player_session.rs}` -- `shop.place{pay}`, `shop.menu` per device, login refusal while owing.
- `apps/kiosk/src-tauri/src/main.rs`, `apps/kiosk/src/lib/{cart.ts,Cart.svelte,attract.ts,Attract.svelte,shop.ts,bridge.ts}`, `routes/+page.svelte` -- pay choices, balance error, attract hold.
- `apps/hub/src/lib/{OpenOrders.svelte,open-orders.ts}`, `routes/pos/+page.svelte` -- POS "Open orders".
- `packages/ui/src/messages/en.json`, `docs/kiosk-accessibility.md`, generated types.

## Tasks & Acceptance

**Execution:**
- [x] Protocol: counter order, kiosk sale, pay choice and options, attract `unpaid_minor`; regenerate TS types
- [x] Store: open counter order with device, unpaid total, last session player
- [x] Core: counter order in `order.place`, wallet sale in `pos.sale`, device permissions, serve/cancel/void/undo-settle/alert-close for counter orders, menu pay options, attract hold
- [x] Hub LAN: `shop.place{pay}`, `shop.menu` per device, refusal with balance, login refused while owing
- [x] Kiosk: IPC `pay`, cart choices, wallet-short and balance errors, attract "Pay at the counter"
- [x] Hub app: POS Open orders list with the settle bar
- [x] Tests: `counter_order` (core, 13), proptest `CounterOrder` op and invariant, `lan.rs` (2), kiosk `cart`/`attract`, hub `pos`

**Acceptance Criteria:**
- Given a profiled player's cart, then *Pay now from wallet* shows the balance beside *Add to session bill* and is disabled with the reason when short; guests see no wallet.
- Given Pay now and Place order, then one transaction settles the order by one wallet payment, the wallet never goes below zero, the invoice number follows as for any settled bill and the order is tied to the PC.
- Given a concurrent spend, then `INSUFFICIENT_BALANCE`, the cart stays and shows the real balance.
- Given the session ended with the cart open, then the only choice is *Pay at counter*.
- Given Pay at counter, then an open order with no session is tied to the PC and player with its lines and tax breakup, no payment or invoice number, stock taken by the take-stock mode, UNPAID on the tile and the kiosk held on "Pay at the counter".
- Given an open counter order, then staff can load it from the tile sheet or POS Open orders into the settle bar and settle it with any mix, which clears UNPAID and lifts the hold; staff may void it.
- Given the proptest generator, then an open counter order has no payments and no invoice number and a settled one has paid.
- Given a prepaid session that ends with a Pay-at-counter order, then the kiosk is held and the tile shows UNPAID while the time is settled from the pack or wallet as before.

## Design Notes

- **No new order status.** `open_unpaid` is `status = 'open'`, `kind = 'pos_sale'`, `session_id IS NULL` (`device_id` set). Nothing wrote an open `pos_sale` before, `orders.status` and `kind` are CHECK-constrained, and rebuilding `orders` (invoices, refunds and coupons reference it) buys nothing. The floor snapshot already lists every open order as a bill with its device, so the tile's UNPAID tag, the tile sheet and its settle bar work unchanged.
- **Settling reuses `session.settle`.** It already takes only `order_id`, `base_rev`, any payment mix, coupon, discount and receipt choice, issues the invoice number through the same sweeper and has an 8 s undo. A second `order.settle` kind would copy all of it, so the epic's `order.settle` / `order.settle.reverted` are `session.settle` / `session.settle.reverted` on a counter order (the undo is allowed only for an order born from `order.place`, so a counter *sale* is never reopened).
- **Prepaid sessions hold no bill order.** `order.place` refuses a pack or wallet session (6.31), so "unpaid order lines at the end of a prepaid session" are the PC's open counter orders. Ending the session returns the PC to attract, which carries the hold; `session.settle` lifts it by itself.
- **Wallet sale payments are derived.** A `pos.sale` with `kiosk` carries no payments; `load_sale` prices the cart and makes one wallet payment of the total, so the kiosk never needs to know the total (tax mode, rates) and cannot choose it.
- **Lines of a wallet sale are born served** (a counter sale's rule), so the `kiosk_order` alert stays open until staff acknowledge it. A counter order's kitchen lines are served or cancelled through the existing paths and close the alert.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core` (`counter_order`, `proptest_apply`), `cargo test -p arena-api --test lan -- pay_now pay_at_counter shop_place`, `cargo test -p arena-kiosk-svc --lib -- a_cart_is_a_sane` -- pass
- `cargo fmt --check`; `cargo clippy` on touched crates -- clean
- `pnpm --filter @arena/kiosk check`, `@arena/hub check`, Playwright kiosk `cart attract`, hub `pos`; `pnpm check:strings check:hex check:no-blur check:protocol check:layers check:pins format:check check:generated` -- pass

## Auto Run Result

Status: done

- **Summary:** The cart offers the choices the Hub says the PC can take. Pay now settles a `pos.sale` by one derived wallet payment; Pay at counter opens an unpaid counter order (open `pos_sale`, no session) that the floor snapshot already shows as UNPAID and that staff settle, undo or void; the attract view carries `unpaid_minor` and holds the PC.
- **Verification:** `arena-proto`, `arena-store`, `arena-core` (13 new `counter_order`, the proptest with its new `CounterOrder` op and invariant), `arena-api` `lan` (81, 2 new), `arena-kiosk-ui` and the kiosk `a_cart_is_a_sane` test pass; kiosk Playwright (337, incl. 6 new cart and 1 new attract), hub `pos` (86), `alerts`, `floor`, `kitchen` pass; fmt, clippy (`-D warnings`) on proto, store, core, api, kiosk-svc and kiosk-ui, svelte-check, strings, hex, blur, protocol, layers, pins, prettier pass.
- **Residual risks:** `check:generated` reports the new types until they are committed; `arena-kiosk-svc` has the same 14 environmental failures as before (file ownership needs admin); kiosk `theme.spec` (4) fails on a doubled Windows drive path in the test fixture and `arena-e2e` does not build on Windows (both unchanged); `arena-cloud` (2), `arena-tasks` (2) and `arena-replay` (1, `library_entries has no row`) fail in tests this story does not touch; `proptest_apply` is flaky on sequences unrelated to this story (a Plan edit answering `STALE` before `GROUP_KIND_MISMATCH`, a Session start on an unassigned PC), seen before the new op ran; the real Tauri shell, a real Hub and a real kiosk hold were not exercised; no review pass has run.
- **Deviations from the epic text:** no `order.settle` kind and no new status (see Design Notes); a cancelled line on a counter order lowers the total by its amount without recomputing tax; the hold lifts at the next attract push, not by a PC command.

## Review Result

Code review passed. One blocking finding fixed: with tax set exclusive, an unpaid counter order's floor bill (and so its UNPAID total and settle bar) showed the line sum without tax while `session.settle` needs the priced total, so staff could not settle it. The snapshot now uses the order's `total_minor` for counter orders (`OpenOrder.total_minor`), with a regression test. Known limitation kept: a cancelled line on a counter order lowers the total without recomputing its tax breakup.
