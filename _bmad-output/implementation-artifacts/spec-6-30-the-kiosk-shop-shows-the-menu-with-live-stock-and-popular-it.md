---
title: 'Story 6.30: The kiosk Shop shows the menu with live stock and popular items'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '7832e9b9f47cb2dbd1a5d9af7585bceaaea7a587'
review_loop_iteration: 0
followup_review_recommended: false
warnings: []
deferred: []
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-29-home-shows-the-featured-game-with-a-hero-video-and-a-feature.md'
---

<intent-contract>

## Intent

**Problem:** A player cannot see what food and drink the café sells from their seat, and the kiosk has no Shop tab, no live stock and no way for the Hub to refuse kiosk orders on a branch without the feature.

**Approach:** A paired kiosk asks the Hub for `shop.menu` (licence flag `kiosk_orders`, active products and bundles with live availability, the top-3 sellers of the last 7 days) and for product photos one at a time (`shop.image`). The kiosk shows a Shop tab after Apps only when the Hub says orders are enabled, with a category rail, search and `shop-item-card`s, and re-asks every second while the tab is open so a Hub stock change shows within 2 s. A device-sent `order.place` on a branch without the flag is refused `FEATURE_NOT_IN_TIER`.

## Boundaries & Constraints

**Always:** The Hub decides (the kiosk only renders what `shop.menu` says); an unavailable card keeps its name and an "Out of stock" tag at full ink, dims only the photo and disables Add; money is integer minor units shown tabular; photos come only from the Hub (`shop.image`), never the internet; the reads change no state and are free of any intent.

**Never:** a cart, an order intent or a payment path (Stories 6.31 to 6.33); a new permission; a Shop route or menu read that works without the flag; showing inactive or deleted products.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Basic+ branch | flag `kiosk_orders` set | `enabled: true`; pill nav shows Shop after Apps |
| Free branch | flag absent | `enabled: false`, no items; no Shop pill; a stale Shop tab falls back to Home |
| Out of stock | tracked product at 0, expired, or bundle component short/inactive | card dims photo, "Out of stock" tag, Add disabled |
| Bundle | components listed | card lists "2 × Cola" style components |
| Popular | top 3 by units sold in 7 days (void orders excluded) | `popular-badge` "POPULAR" on those cards |
| Stock change at the Hub | next poll | card state flips within 2 s |
| Nothing available | no items, or none available | "Kitchen's closed right now." |
| Search / category | "Search food & drinks" text, category chip | filter the cards; empty result "Nothing matches." |
| Device order intent, no flag | `order.place` from a `device` actor | refused `FEATURE_NOT_IN_TIER` before the permission check |
| Hub unreachable | menu read fails | the last good menu stays; Shop pill state unchanged |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{shop.rs,frame.rs,lib.rs,error.rs,intent.rs}` -- `ShopMenu`, `ShopItem`, `ShopComponent`, `ShopImageRequest/Result`, frames `shop.menu(_result)` / `shop.image(_result)`, `FLAG_KIOSK_ORDERS`, `ErrorCode::FeatureNotInTier`, `kiosk_order_gate`.
- `crates/arena-store/src/{lib.rs,products.rs}` -- `units_sold(since, limit)` over `order_lines` of non-void orders.
- `crates/arena-core/src/{shop.rs,devices.rs,lib.rs}` -- `shop::menu` / `shop::image` over `catalog::status` (reuse), `DeviceDirectory::{shop_menu,shop_image}`.
- `crates/arena-core/src/apply/{guards.rs,mod.rs}` -- `guards::kiosk_orders` before `guards::permission`.
- `crates/arena-api/src/lan/{shop.rs,session.rs,mod.rs}` -- paired-device dispatch (same shape as `games.most_played` / `player.extend`).
- `crates/arena-kiosk-svc/src/{player_session.rs,app.rs}`, `apps/kiosk/src-tauri/src/{main.rs,bridge.rs}` -- IPC `shop_menu`, `shop_image` forwarded to the Hub.
- `apps/kiosk/src/lib/{shop.ts,Shop.svelte,ShopItemCard.svelte,shell.ts,Shell.svelte,bridge.ts}`, `routes/+page.svelte` -- tab, rules, cards, polling.
- `packages/ui/src/messages/en.json`, `packages/ui/src/generated/*` (regenerated), `docs/kiosk-accessibility.md`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol: shop types, frames, flag, error code, `kiosk_order_gate`; regenerate TS types
- [x] Store and core: `units_sold`, `shop::menu`/`image`, directory methods, apply guard, tests
- [x] Hub LAN: `shop.menu` and `shop.image` frames for paired devices, test in `tests/lan.rs`
- [x] Kiosk service and Tauri: `shop_menu` / `shop_image` IPC, tests
- [x] Kiosk app: Shop tab gated by `enabled`, filter rail, search, cards, badge, empty state, 1 s poll, strings, a11y doc, `shop.spec.ts`

**Acceptance Criteria:**
- Given a branch with `kiosk_orders`, when the shell renders, then `pill-nav` shows Shop after Apps; on a Free branch Shop is absent and a Shop tab is replaced by Home.
- Given the Shop tab, when it renders, then it shows a `filter-rail` of categories with "All", a "Search food & drinks" field and a card per active product and bundle (photo, name, tabular price, Add).
- Given an unavailable product or a bundle with a short component, when its card renders, then the photo dims, the name and "Out of stock" stay at full ink and Add is disabled; a Hub change shows within 2 s.
- Given a bundle, when its card renders, then it lists its components.
- Given the top 3 products by units sold in the last 7 days, when the menu renders, then they carry a "POPULAR" badge.
- Given no available products, when the Shop renders, then it shows "Kitchen's closed right now."
- Given an `order.place` from a device actor on a branch without `kiosk_orders`, when it reaches `apply()`, then it is rejected `FEATURE_NOT_IN_TIER`.

## Design Notes

Only active, non-deleted products are listed; one that becomes inactive leaves the menu on the next poll, while stock, expiry and bundle shortages keep it listed as unavailable. The menu carries `has_image`; the card asks `shop.image` once per product (kept for the session view) so a menu frame stays small (photos can be 200 KB each). `catalog.changed` already reaches paired services but is not forwarded to the app; the app re-asks `shop.menu` every second while the Shop tab is open (the Hub read is a few indexed queries), 30 s otherwise to learn `enabled`.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core --lib shop` and `cargo test -p arena-api --test lan shop`, `cargo test -p arena-kiosk-svc --lib shop` -- expected: pass
- `cargo fmt --check`, `cargo clippy` on the touched crates `-D warnings` -- expected: clean
- `pnpm --filter @arena/kiosk check`, Playwright `apps/kiosk` `shop.spec.ts`, `pnpm check:strings check:hex check:no-blur check:protocol format:check` -- expected: pass

## Auto Run Result

Status: done

- **Summary:** Paired kiosks ask the Hub for `shop.menu` (licence flag, active products and bundles with live availability, top-3 sellers of 7 days) and `shop.image` (one photo at a time). The kiosk shows a Shop pill after Apps only when enabled, with a category rail, search, cards, POPULAR badge and the closed-kitchen line; it re-asks every second while Shop is open. A device `order.place` without `kiosk_orders` is refused `FEATURE_NOT_IN_TIER` before the permission check.
- **Verification:** proto, store, core (5 new shop tests) and api (3 new lan tests) pass; kiosk-svc shop test passes; fmt and clippy clean; kiosk `check` 0 errors; Playwright `shop`, `home`, `shell` 44 pass; strings, hex, blur, protocol, layers, pins and prettier pass. `theme.spec.ts` fails as before this story (Windows path); `check:generated` only reports the new uncommitted types.
- **Residual risks:** Add is a no-op until Story 6.31; photos are not re-fetched if changed mid-session; the menu read is uncached at 1 s per open Shop tab; the real Tauri shell and real Hub photos were not exercised; Story 6.31's kiosk order tests need `kiosk_orders` set; no review pass has run yet.

## Code Review Result

Reviewed the implementation commit (proto, store, core, Hub LAN, kiosk service, kiosk app). No blocking findings. Checks: shop tests (proto/store/core/api/kiosk-svc) pass, clippy `-D warnings` clean on non-test targets, `svelte-check` 0 errors, prettier clean. Pre-existing clippy `clone_on_copy` lints in `arena-kiosk-svc/src/launchers/tests.rs` (untouched by this story) remain.
