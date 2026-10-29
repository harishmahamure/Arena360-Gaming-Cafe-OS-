---
title: 'Story 7.2: Kitchen notes on order lines from the POS and the kiosk cart'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: 'a85c722'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-1-kitchen-settings-at-the-hub-locked-on-free.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A kitchen item can only be ordered as is; the counter and the player cannot say "Less sugar" or "No onion".

**Approach:**
- A Manager keeps up to 8 preset notes (40 characters each) on a kitchen product, saved with `product.upsert`.
- The Hub POS cart toggles those presets and adds one free-text note (60 characters) on a kitchen line; the kiosk cart toggles presets only.
- The notes ride on the order line (`PosSaleLine.kitchen_notes`) and are stored on `order_lines.kitchen_notes`; they never touch price, tax or stock.
- The notes part of `product.upsert`, `order.place` and `pos.sale` is registered with the Story 7.1 gate.

## Boundaries & Constraints

**Always:**
- **Preset list.** `ProductUpsert.kitchen_notes: Option<Vec<String>>`: `None` keeps the stored list, `Some` replaces it. At most 8 distinct notes, each trimmed, 1 to 40 characters, no control characters, only on a `kitchen` product. A product that is not `kitchen` holds none. Over 40 characters is `FIELD_TOO_LONG` (new code, HTTP 422, deterministic for the kiosk queue) with `field: kitchen_notes`; the other faults are `VALIDATION_FAILED`.
- **Line notes.** `PosSaleLine.kitchen_notes: Vec<String>` (default empty, omitted when empty), used by `pos.sale`, `order.place` and the kiosk `shop.place`. Allowed only on a `kitchen` product; at most 9 distinct notes per line, each 1 to 60 characters (`FIELD_TOO_LONG` on `lines[i].kitchen_notes`). Staff may add one note that is not a preset of the product; a `device` actor (the kiosk) may send presets only.
- **Storage.** Migration `0068_kitchen_notes.sql`: `products.kitchen_notes TEXT NOT NULL DEFAULT '[]'` and `order_lines.kitchen_notes TEXT NULL` (JSON array of trimmed strings, `NULL` when none). Both ride the change log as plain columns.
- **Same product twice.** A sale or order may repeat a product when the notes differ; identical notes (same set) are `VALIDATION_FAILED` on the later line. Stock planning already sums by product, so the lines share one stock check. The Hub POS cart and the kiosk cart key a line by product and sorted notes, and merge lines whose notes become equal, adding quantities.
- **Gate.** `kitchen_gate` returns `New` for a `product.upsert` with a non-empty `kitchen_notes`, and for an `order.place` or `pos.sale` whose any line has notes. On a branch without the `kitchen` flag that is `TIER_LOCKED` and nothing is written; the same order without notes, and a product edit that leaves the list alone or empties it, are accepted.
- **Kiosk menu.** `ShopItem.kitchen_notes` carries a product's presets only when the branch has the `kitchen` flag. The Hub `Product` view always carries the list.
- **Hub UI.** Inventory form: a "Kitchen notes" list (add, remove, 40-character inputs with a counter, at most 8) shown for a kitchen product on a branch with the flag; without the flag the form sends no list. POS cart: a kitchen line on such a branch has a Notes button that opens preset chips (`aria-pressed`) and one free-text field (60 characters, counter); chosen notes show indented under the line. Without the flag the cart sends no notes.
- **Kiosk UI.** A cart line whose item has presets has a Notes button opening preset chips only; chosen notes show indented under the line.
- Strings in `en.json`; targets at least 44 px.

**Never:**
- A change to price, tax, stock, a bundle's parts or the coupon and discount maths because of a note.
- Free text on the kiosk.
- A kitchen ticket (Story 7.3).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Save presets | Basic, kitchen product, `["  Less sugar ", "No ice"]` | stored trimmed | N/A |
| Ninth preset | 9 notes | rejected | `VALIDATION_FAILED` `kitchen_notes` |
| Long preset | 41 characters | rejected | `FIELD_TOO_LONG` `kitchen_notes` |
| Free, preset list | `product.upsert` with notes | rejected, nothing written | `TIER_LOCKED` |
| Edit, no list | `kitchen_notes: None` | stored list kept | N/A |
| Order with notes | Basic, staff, preset + free text | stored on the line, total unchanged | N/A |
| Over-long note | 61 characters | rejected | `FIELD_TOO_LONG` `lines[i].kitchen_notes` |
| Plain product | notes on a non-kitchen product | rejected | `VALIDATION_FAILED` |
| Second free note | staff, two non-preset notes | rejected | `VALIDATION_FAILED` |
| Kiosk free text | device, non-preset note | rejected | `VALIDATION_FAILED` |
| Free, order | notes on `order.place`; same order without notes | first rejected, second accepted | `TIER_LOCKED` |
| Same product, same notes | two lines, equal note sets | rejected | `VALIDATION_FAILED` `lines[i]` |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/{kitchen.rs,pos.rs,catalog.rs,shop.rs,error.rs}` -- limits, gate arms, `kitchen_notes` fields, `FIELD_TOO_LONG`.
- `crates/arena-store/{migrations/0068_kitchen_notes.sql,src/products.rs,src/sessions.rs}` -- columns, `ProductRow.kitchen_notes`, `NewProductLine.kitchen_notes`; `serde_json` is now a plain dependency.
- `crates/arena-core/src/apply/{kitchen_notes.rs,products.rs,pos_sale.rs,order_place.rs}`, `catalog.rs`, `shop.rs` -- validation, storage, views.
- `apps/hub/src/{lib/pos.ts,routes/pos/+page.svelte,routes/inventory/+page.svelte}`, `apps/kiosk/src/{lib/cart.ts,lib/Cart.svelte,routes/+page.svelte}`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-core/src/apply/tests/kitchen_notes.rs`, `apps/hub/tests/kitchen-notes.spec.ts`, `pos.unit.spec.ts`, `apps/kiosk/tests/cart.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: limits, gate arms, fields, `FIELD_TOO_LONG` with its HTTP and queue mappings, generated types.
- [x] Store: migration 0068, product and line columns.
- [x] Core: preset and line validation, storage, views, Rust tests.
- [x] Hub: product form notes list, POS cart notes, merge rules, Playwright and unit tests.
- [x] Kiosk: cart notes chips, merge rules, Playwright and unit tests.

**Acceptance Criteria:**
- Given a kitchen product on a Basic branch, when a Manager saves up to 8 presets of up to 40 characters, then they are stored and audited by the change log.
- Given a kitchen line in the Hub POS cart or the kiosk cart, when notes are chosen, then they show under the line, travel on the intent and are stored on the `order_lines` row without changing the amount.
- Given two lines of one product with different notes, then they stay apart; with equal notes they merge.
- Given a Free branch, then no notes control is offered and an intent carrying notes is `TIER_LOCKED`.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-replay -p arena-cloud -p arena-kiosk-core --locked`; clippy with `-D warnings` on the same crates.
- `pnpm gen:types`; `pnpm --filter @arena/hub check`, `@arena/kiosk check`; Hub `kitchen-notes`, `pos`, `products` specs and kiosk `cart`, `shop` specs with a unique `ARENA_PW_PORT`; `check:strings`, `check:layers`, `check:protocol`, `format:check`.

## Spec Change Log

## Review Triage Log

Not yet reviewed (story in `review`).
