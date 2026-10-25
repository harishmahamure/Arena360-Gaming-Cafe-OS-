---
title: 'Story 5.3: Products with categories, barcodes, images and the kitchen flag'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '1776e34'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-1-tax-settings-gst-registration-rates-inclusive-or-exclusive-a.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-2-tax-is-computed-once-per-rate-on-every-bill-in-minor-units.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-20-device-groups-with-six-swatches-held-at-the-hub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-1-kitchen-settings-at-the-hub-locked-on-free.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The counter has nothing to sell. Bills (5.2) need a product's price, tax rate and HSN; stock (5.5), bundles (5.4) and the kiosk Shop (Epic 6) need a product table and an availability feed.

**Approach:**
- A new Hub table `products` written only by `product.upsert` and `product.delete` (permission `catalog.edit`), with `base_rev`, soft delete and the change log like every other entity.
- Hub screen Inventory (the route `/inventory`, replacing its placeholder): add, edit, deactivate and delete products; a Counter's save opens `inline-pin`.
- `catalog.changed` carries each written product's `available` flag and reason, to the Hub UI stream and to paired kiosk sessions.
- The Hub status `setup` block reports `products_count`.

## Boundaries & Constraints

**Always:**
- **Hub migration** `0017_products.sql` (0016 is `floor_zones` on master; renumber on rebase if a sibling lands first): `products(product_id, name, category, price_minor, tax_rate_bp, hsn, track_stock, image, barcode NULL, kitchen, favourite, active, rev, deleted_at, created_at, updated_at)`; a unique index over `barcode` where `barcode IS NOT NULL AND active = 1 AND deleted_at IS NULL`. `products` joins the change-log tables (`Table::Products`).
- **Intents** (both `catalog.edit`, category `catalog`): `product.upsert{id?, name, category, price_minor, tax_rate_bp?, hsn, track_stock=true, image, barcode, kitchen, favourite, active=true}` and `product.delete{id}`. A create mints the id. An edit and a delete need `base_rev` equal to the row's `rev` (`STALE` otherwise, `VALIDATION_FAILED base_rev` when missing); an owner actor may omit it but a stale one is still `STALE`. A deleted row is never edited (`VALIDATION_FAILED id`).
- **Validation** (field named in `VALIDATION_FAILED`):
  - `name` 1 to 80 characters trimmed, no control characters; `category` up to 40 characters, may be empty.
  - `price_minor` 0 to 10^12; money stays integer minor units.
  - `tax_rate_bp` defaults to `TaxSettings::rates().0` (5.1/5.2) when absent; at most 2800 in GST mode (`tax_mode = gst_in`), at most 10000 otherwise.
  - `hsn`: in GST mode it must be 4, 6 or 8 digits else `HSN_REQUIRED` on field `hsn`; outside GST empty or the same shape else `VALIDATION_FAILED hsn`.
  - `barcode` trimmed, up to 48 visible ASCII characters without spaces; empty is stored as NULL. Another active, live product with the same barcode is `BARCODE_TAKEN` on `barcode`, only when the saved product is active; nothing is written.
  - `image` empty or a `data:image/jpeg;base64,` URI whose decoded bytes are at most 200 KB, start with the JPEG signature and whose SOF dimensions are 1 to 512 px on both sides; anything else is `VALIDATION_FAILED image`.
- **Availability** (`arena_proto::availability`): inactive or deleted is `available=false, reason=inactive`; a tracked product with quantity at most 0 is `out_of_stock`. Until Story 5.5 creates `stock`, a tracked product has quantity 0, so it is `out_of_stock`. Untracked products are available.
- **Feed:** a committed change to `products` pushes `catalog.changed{products:[{product_id, available, reason?}]}` for the written ids (a deleted or unknown id is `inactive`) on `/ui/events` and as `HubEvent::CatalogChanged` to paired LAN sessions only. Stock and expiry changes will push the same event from their stories.
- **Read:** `GET /ui/products` (any signed-in session) returns every live product with image, `quantity`, `available` and `reason`.
- **Setup:** `SetupStatus.products_count` counts active live products; it refreshes when `products` changes.
- **Hub UI** `/inventory`: form (name, category, price in the branch currency, tax rate with the default shown, HSN, barcode, image upload, track stock, kitchen, favourite, active), list with category filter, badges, Edit and Delete; image is shrunk in the browser to JPEG at most 512 px and 200 KB before it is sent, and a non-image or one that cannot be shrunk shows an inline error; field errors per field; STALE reloads; strings in `en.json`; targets at least 44 px; axe clean.
- **Tests:** Rust (apply: create, defaults, HSN, barcode, image, field checks, base_rev, soft delete, permissions, availability, count; proto: HSN, availability, image; registry and events pushes; `/ui/products`; replay fixture and the Story 1.8 proptest generator gain the intent); Playwright.

**Never:**
- A `stock` table or quantities (5.5), bundles (5.4), CSV import, kitchen-tier gating of the stored flag (Epic 7 registers its intents with the gate), kiosk Shop UI, a category entity (category is a product attribute).
- Heartbeat or wizard-step changes in the cloud (Story 2.28 owns the `setup` heartbeat block).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected |
|----------|--------------|----------|
| Create | Manager, valid | row stored, `catalog.changed` |
| GST, no HSN | `hsn` empty or 5 digits | `HSN_REQUIRED` field `hsn`, nothing written |
| Barcode taken | other active product holds it | `BARCODE_TAKEN`, nothing written |
| Barcode of inactive product | holder inactive or deleted | accepted; reactivating the holder is then `BARCODE_TAKEN` |
| Big image | PNG, 513 px, over 200 KB | `VALIDATION_FAILED image` |
| Stale edit | `base_rev` behind | `STALE` |
| Deactivate | `active=false` | leaves the grid feed as `inactive`; kept on past orders |
| Delete | `product.delete` | `deleted_at` set, barcode freed |
| Counter | no `catalog.edit` | `PERMISSION_DENIED`; UI opens `inline-pin` |

</frozen-after-approval>

## Code Map

- `crates/arena-store`: `migrations/0016_products.sql`, `products.rs`, `branch.rs` (`Table::Products`).
- `crates/arena-proto`: `catalog.rs`, `intent.rs`, `error.rs` (`HSN_REQUIRED`, `BARCODE_TAKEN`), `operating.rs` (`products_count`), `frame.rs` (`HubEvent::CatalogChanged`).
- `crates/arena-core`: `apply/products.rs`, `catalog.rs`, `setup.rs`, `snapshot.rs`, `devices.rs`.
- `crates/arena-api`: `ui_auth.rs` (`/ui/products`), `ui_events.rs`, `lan/registry.rs`.
- `apps/hub`: `src/routes/inventory/+page.svelte`, `src/lib/products.ts`, `catalog.svelte.ts`, `events.ts`, `tests/products.spec.ts`.

## Tasks & Acceptance

- [x] Store, proto, apply, availability, setup count, feed, route, tests.
- [x] Hub page, messages, Playwright.
- [x] Verification commands from the loop state.

**Acceptance Criteria:** the Given/When/Then of Story 5.3 in `epics.md`, except as recorded in the decision log: wizard step 6 completing by itself is the cloud half of Story 2.28 (the Hub reports `products_count` locally), and the kiosk-side use of `catalog.changed` is Epic 6.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter with the spec as claims, Verification Gap; all on Sonnet; data integrity). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/EC: after STALE the form kept the old `rev`, so every retry was STALE | medium | P | the reload re-opens the edit with the fresh product; Playwright |
| 2 | BH/EC: a successful Delete of another row wiped an open edit | medium | P | reset only for an upsert or the deleted row itself |
| 3 | BH/EC: a lost response then a second Save minted a new key and duplicated the product | medium | P | key kept per payload until it applies or fails |
| 4 | EC: owner with a stale `base_rev` overwrote newer data | medium | P | stale is `STALE` for every actor; owner may still omit; test |
| 5 | BH/EC: unreadable `branch.tax` silently meant non-GST (HSN optional, 100% cap) | medium | P | `INTERNAL`, nothing written |
| 6 | BH/EC: `catalog.changed` read the whole catalog with images under the shared reader lock | medium | P | per-id read |
| 7 | EC: an edit without `tax_rate_bp` reset the stored rate to the default | low | P | keeps the stored rate; test |
| 8 | EC: kiosk queue treated `HSN_REQUIRED` and `BARCODE_TAKEN` as permanent | low | P | moved to the retryable arm |
| 9 | BH/EC: overlapping loads, vanished filter, failed image pick kept the old image, price ignored symbol position, tax hint shown without settings, `ImageBitmap` leak, barcode message wording | low | P | fixed |
| 10 | VG: setup count refresh, Counter ticket, LAN fan-out wiring, real JPEG encoder output, 512 px height, inactive-holder barcode, edit writes all fields, unique index, limits at the edge, Playwright stale reload, noisy and broken pictures, filter, badges, push reload | high/medium | P | tests added |
| 11 | BH/EC: images ride in the change log and in `GET /ui/products` (up to 270 KB a change) | medium | D | spec-mandated storage; a thumbnail or side table is a later optimisation |
| 12 | BH: a tracked product reads out of stock at creation | low | R | the story's own rule until Story 5.5 records stock |
| 13 | EC: validator checks the JPEG header, not that it decodes | low | D | only the Hub UI and kiosk render it |
| 14 | VG: 44 px target sizes, axe on error and PIN states, malformed `branch.tax` apply test | low | D | covered by the shared components |
| 15 | EC: no catch-up for a kiosk offline during `catalog.changed` | low | D | Epic 6 reads the products on connect |
| 16 | BH/EC: barcodes and categories are case-sensitive | low | R | consistent between index and check |
