---
title: 'Story 6.31: Build a cart and add the order to the session bill from the seat'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '097d490'
review_loop_iteration: 1
followup_review_recommended: false
warnings: []
deferred: []
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-30-the-kiosk-shop-shows-the-menu-with-live-stock-and-popular-it.md'
---

<intent-contract>

## Intent

**Problem:** A player can see the Shop (Story 6.30) but cannot order: Add does nothing, there is no cart, and nothing reaches the session bill, the Hub's alerts or the tile.

**Approach:** Add puts the item in a `cart` panel (right side, "Your order · To <PC> · <zone>"). A step up asks the Hub for the live menu first; every menu item now carries `max_qty` (stock less expired lots and open holds, a bundle's tightest part), so a step past it is refused inline. Place order sends `shop.place{lines}`; the Hub finds the session on the paired PC, builds the `order.place` itself (device actor, never a stock override) and applies it by the Epic 5 rules, so the lines join the open bill and stock follows the branch's take-stock mode. A committed order raises a `kiosk_order` alert for the PC (sound, header badge, tile tag `ORDER`) that closes by itself once no line of the bill is unserved.

## Boundaries & Constraints

**Always:** The Hub decides (the kiosk only checks against what `shop.menu` says and relays `shop.place`); money is integer minor units shown tabular; a short line keeps the cart and is marked with the reason; no override on the kiosk; Place order is disabled with "Counter is reconnecting — try again shortly." when the Hub link is down or did not answer.

**Never:** Pay now from the wallet, Pay at counter or any prepaid-session handling (Story 6.32); order status on the HUD (6.33); a new intent kind or permission; a `stock_override` from a device; a session or player id from the kiosk.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Add | card Add, item available | cart opens with the line, total, tax note, "Add to session bill" |
| Step past stock | qty would exceed `max_qty` | refused, qty unchanged, "Only n left." inline |
| Last step down | qty 1, step down | line removed; "Cart's empty." |
| Place, ok | postpaid/block session running, flag set | lines join the bill; cart clears with "Order placed."; `kiosk_order` alert raised |
| Place, short | `OUT_OF_STOCK` / `BUNDLE_SHORT` with `lines[i]` | cart stays; the product's line marked; nothing written |
| Place, no flag | device order, no `kiosk_orders` | refused `FEATURE_NOT_IN_TIER` (6.30 gate) |
| Place, other PC / override | device names another PC's session or `stock_override` | `PERMISSION_DENIED` |
| Hub unreachable | link down or no answer | Place disabled / "Counter is reconnecting…"; cart kept |
| All lines served | last unserved line served or cancelled | the PC's open `kiosk_order` alert is acknowledged |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{shop.rs,frame.rs,alert.rs,lib.rs}` -- `ShopPlace`, `ShopPlaceResult`, frames `shop.place(_result)`, `ShopItem.max_qty`, `AlertKind::KioskOrder`.
- `crates/arena-core/src/{catalog.rs,shop.rs,devices.rs,rbac.rs}`, `apply/{alert.rs,order_line.rs}` -- `Status.sellable`, `shop::order_target`, `device_places_own_order`, `close_kiosk_order`.
- `crates/arena-api/src/lan/{shop.rs,session.rs}` -- `shop.place` for paired devices.
- `crates/arena-kiosk-svc/src/{player_session.rs,app.rs}`, `apps/kiosk/src-tauri/{src/main.rs,src/bridge.rs,ipc-names.json}` -- IPC `shop_place`.
- `apps/kiosk/src/lib/{cart.ts,Cart.svelte,Shop.svelte,Shell.svelte,bridge.ts,shop.ts}`, `routes/+page.svelte`.
- `apps/hub/src/lib/alerts.svelte.ts`, `packages/ui/src/{components/AlertBadge.svelte,messages/en.json,generated/*}`, `docs/kiosk-accessibility.md`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol: `shop.place` frames, `max_qty`, `kiosk_order` alert kind; regenerate TS types
- [x] Core: sellable quantity on the menu, device order permission on its own session, alert auto-close, tests
- [x] Hub LAN: `shop.place` builds `order.place`, raises the alert, names the short product; `tests/lan.rs`
- [x] Kiosk service and Tauri: `shop_place` IPC relay with a sane-cart check, test
- [x] Kiosk app: cart rules, panel, Shop cart button, page wiring, strings, a11y doc, `cart.spec.ts`
- [x] Hub app: `order` tile tag, alert label, spec

**Acceptance Criteria:**
- Given Add on a card, then the cart opens as a right-side panel with ±1 steppers, bundle components indented, the total in `hud-numeric` with "Inclusive of applicable taxes"; empty reads "Cart's empty."
- Given a stepper change past availability, then it is refused with the reason inline after a live stock check with the Hub.
- Given a running session, then the cart offers "Add to session bill" with "Payable at the counter when your session ends" and Place order.
- Given Place order, then `order.place` applies through the Story 5.12 path: lines join the open bill, stock follows the take-stock mode, the cart clears with a confirmation.
- Given a committed kiosk order, then the Hub raises an alert (sound, badge) and the tile shows ORDER until every line is served.
- Given `OUT_OF_STOCK` / `BUNDLE_SHORT`, then the cart stays and the short line is marked; no override.
- Given the Hub unreachable, then Place order is disabled with "Counter is reconnecting — try again shortly."

## Design Notes

`max_qty` is `Option` and omitted for untracked items. The ORDER tag and sound reuse the Story 3.25 alert machinery (one open alert per PC, the 30 s sound loop, tile tag). A kiosk order of only non-kitchen items has every line born served, so nothing auto-closes its alert: staff acknowledge it (it is the prompt to bring the items). A kitchen line closes it when served or cancelled.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-core -p arena-api` (`kiosk_order`, `shop`), `cargo test -p arena-kiosk-svc --lib -- shop cart`, `cargo test -p arena-kiosk-ui` -- pass
- `cargo fmt --check`; `cargo clippy -p arena-proto -p arena-core -p arena-api --all-targets -D warnings`; `cargo clippy -p arena-kiosk-svc -D warnings` -- clean
- `pnpm --filter @arena/kiosk check`, `@arena/hub check`, Playwright kiosk `cart shop shell home`, hub `alerts`, `pnpm check:strings check:hex check:no-blur check:protocol check:layers check:pins format:check` -- pass

## Auto Run Result

Status: review

- **Summary:** Add builds a cart panel; Place order sends `shop.place`; the Hub applies a device `order.place` on the PC's own session (postpaid or block), raises a `kiosk_order` alert (ORDER tile tag, sound, list row) and closes it when the bill has no unserved line.
- **Verification:** proto, core (7 new), api (3 new lan), kiosk-svc cart test and `arena-kiosk-ui` pass; kiosk Playwright `cart` (14), `shop`, `shell`, `home` (58) and hub `alerts` (24) pass; fmt, clippy (touched crates), svelte-check, strings/hex/blur/protocol/layers/pins/prettier pass.
- **Residual risks:** a prepaid (pack/wallet) session is refused `VALIDATION_FAILED` and the cart says "ask at the counter" until Story 6.32; `arena-kiosk-svc` has 14 environmental test failures (file-ownership checks need admin) and 3 older `clone_on_copy` clippy lints in `launchers/tests.rs`, all unchanged by this story; `check:tokens-used` fails on hub-only tokens as before; `check:generated` only reports the new uncommitted types; the real Tauri shell and a real Hub were not exercised; no review pass has run.

## Code Review Result

Status: done (4 reviewers: blind, edge cases, verification gaps, acceptance).

- **Fixed:** Place order stayed disabled after an unanswered placement (a successful menu poll now clears it); steps during an in-flight placement were wiped with the cart (steps are ignored while busy); a refused or unanswered first Add left a mute empty cart (reason and "reconnecting" now show there); a product dropped from the menu showed no mark (refresh marks it unavailable); a 51st distinct product is refused with the limit; tests added for cancel-closes-alert, partial-serve-keeps-alert, the 50-line cap and the dropped product.
- **Deferred (accepted risk, not blocking):** no idempotency key on `shop.place` (a retry after a lost reply can double an order); the alert is raised in a second intent after `order.place`; non-kitchen-only orders keep their alert until staff acknowledge it (by design, see Design Notes); `order.unplace` does not close the alert; per-cart (not per-line) stock sums when a bundle and its component share stock; the Hub does not check `shop.place` product ids against the menu (order.place still validates them); cart-reset-on-session-end and poll-refresh have no page-level Playwright test.
