---
title: 'Story 5.4: Bundles priced on their own'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '635fb5c'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-3-products-with-categories-barcodes-images-and-the-kitchen-fla.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-2-tax-is-computed-once-per-rate-on-every-bill-in-minor-units.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The counter needs combos (burger + cola) sold at one price, while every component still counts in stock (5.5) and in the kiosk availability feed (Epic 6).

**Approach:**
- `product.upsert` (5.3) gains `bundle` and `components[{product_id, qty}]`. A bundle is a `products` row with `bundle = 1`, its own `price_minor`, `tax_rate_bp` and `hsn`, plus rows in a new change-log table `bundle_components`.
- A bundle never has its own stock: it is stored with `track_stock = 0` and 5.5 must never create a `stock` row for it. Its availability is derived from the components.
- The Inventory screen (5.3) gets a Bundle checkbox and a component editor; the list shows the components and "Component inactive: <name>".

## Boundaries & Constraints

**Always:**
- **Hub migration** `0019_bundles.sql` (0018 went to 2.13 and 0017 to 5.3): `products.bundle INTEGER NOT NULL DEFAULT 0 CHECK (bundle IN (0,1))` and `bundle_components(bundle_id, component_id, qty CHECK (qty > 0), PRIMARY KEY (bundle_id, component_id), CHECK (bundle_id <> component_id))`, both ids referencing `products`. `bundle_components` joins the change-log tables (`Table::BundleComponents`, unversioned like `role_perms`: it is versioned through its bundle's `rev`).
- **Intent shape:** `product.upsert` adds `bundle=false` and `components=[]` (both default). The product row is rewritten on every save, so its `rev` moves whenever its components change. The components are replaced as a set: each old row is deleted (the change carries the row as it was), then the new rows are inserted, after the product row in the same intent.
- **Validation** (field `components` unless stated; nothing is written on any failure):
  - not a bundle but `components` is not empty;
  - a bundle with 0 or more than 30 components;
  - `qty` outside 1 to 1000 (zero and negative included);
  - the same component listed twice, or the bundle as its own component;
  - a component that does not exist or is deleted;
  - a component that is itself a bundle;
  - field `bundle`: saving a product as a bundle while a live bundle already lists it as a component (that would nest).
  An inactive component is accepted at save (it makes the bundle unavailable, below).
- **Storage:** a bundle is stored with `track_stock = 0` whatever was sent. Un-bundling (`bundle=false`, no components) deletes its component rows and restores the sent `track_stock`. `product.delete` of a bundle soft-deletes the row and deletes its component rows. Deleting a component keeps the bundle's rows; the bundle then reads `component_inactive`.
- **Availability** (`arena_proto::bundle_quantity`, `arena_core::catalog`): a bundle's quantity is the minimum over its tracked components of `floor(component available / qty)` (negative available counts as 0); untracked components do not limit it; with no tracked component the bundle is available. Reasons in this order: the bundle inactive or deleted is `inactive`; any component inactive or deleted is `component_inactive`; quantity 0 with a tracked component is `out_of_stock`. Until Story 5.5, a tracked component has quantity 0 (`UNRECORDED_STOCK`), so a bundle with a tracked component is `out_of_stock`.
- **Feed:** `catalog.changed` for a written product also lists every live bundle that contains it, so a component's change reaches the kiosks as a bundle availability change.
- **Read:** `GET /ui/products` rows carry `bundle`, `components[{product_id, name, qty, active}]` and the derived `quantity`, `available`, `reason`.
- **Hub UI** `/inventory`: Bundle checkbox (hides Track stock), a component editor (select among non-bundle products other than the one being edited, quantity input, remove, add), client check that every row has a product and a whole quantity of at least 1 (`components` error shown inline); the list shows a Bundle badge, "Contains 2 × Cola, 1 × Burger", "Component inactive: <names>" and a "Not available" badge for any active product that is unavailable; strings in `en.json`; axe clean.
- **Tests:** Rust (apply: create, own price and tax, every refusal above with nothing written, replace, un-bundle, delete, nesting via conversion; availability and derived catalog entries; proto `bundle_quantity`; replay fixture and the Story 1.8 proptest generator gain a bundle); Playwright.

**Never:**
- A `stock` table or rows (5.5), selling or settling a bundle line (5.x POS stories), bundle pricing derived from components, nested bundles, per-component price overrides, CSV import.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected |
|----------|--------------|----------|
| Create | bundle, 2 live components, qty 2 and 1 | product + 2 component rows; own price and tax |
| Nested | component is a bundle | `VALIDATION_FAILED components`, nothing written |
| Self | bundle lists its own id | `VALIDATION_FAILED components` |
| Quantity | 0, negative, 1001, fractional (UI) | refused before or at the Hub |
| Convert | component product saved as a bundle | `VALIDATION_FAILED bundle` |
| Component deactivated or deleted | bundle shown | `component_inactive`, "Component inactive: <name>", not available |
| Tracked component, no stock yet | bundle | `out_of_stock`, quantity 0 |
| Only untracked components | bundle | available |
| Un-bundle | `bundle=false`, no components | component rows deleted |
| Delete bundle | `product.delete` | row soft-deleted, component rows deleted |

</frozen-after-approval>

## Code Map

- `crates/arena-store`: `migrations/0019_bundles.sql`, `products.rs` (`bundle`, component queries), `branch.rs` (`Table::BundleComponents`), `lib.rs`.
- `crates/arena-proto`: `catalog.rs` (`BundleComponent`, `ComponentView`, `bundle_quantity`, limits, `REASON_COMPONENT_INACTIVE`).
- `crates/arena-core`: `apply/products.rs`, `apply/mutate.rs`, `catalog.rs`.
- `crates/arena-replay`, `crates/arena-core/tests/proptest_apply.rs`: fixtures.
- `apps/hub`: `src/routes/inventory/+page.svelte`, `tests/products.spec.ts`; `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

- [x] Store, proto, apply, availability, feed, tests.
- [x] Hub page, messages, Playwright.
- [x] Verification commands from the loop state.

**Acceptance Criteria:** the Given/When/Then of Story 5.4 in `epics.md`. "Stock availability computed" is the derived quantity above over the 5.3 placeholder quantity (0 for tracked); real component quantities arrive with Story 5.5, which only has to replace `UNRECORDED_STOCK`.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter with the spec as claims, Verification Gap; all on Sonnet; data integrity). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/EC: a bundle whose component was deleted could not be saved or fixed from the UI (the select had no option for it, the save returned a generic error) | medium | P | the editor keeps the bundle's own components as options by name; Playwright |
| 2 | BH/EC: every bundle save deleted and re-inserted all unchanged component rows (2N changes, log churn) | medium | P | only changed or removed rows are deleted, only new or changed rows inserted; a price-only edit logs one change; test |
| 3 | BH: duplicate "Not available" and "Out of stock" badges | low | P | generic badge removed; the reason badges remain |
| 4 | EC: quantity typed with spaces refused | low | P | trimmed |
| 5 | VG: no upgrade test for 0018 to 0019, no check of the bundle row for a deleted bundle or a deleted component keeping its rows, nesting test passed on a deleted component, no 30 and 1000 edge, no stale bundle edit | high/medium | P | tests added |
| 6 | VG: `catalog.changed` expansion to parent bundles only tested at the reader | medium | P | SSE test; `/ui/products` test with the new fields |
| 7 | VG: replay only created a bundle | medium | P | fixture now replaces its components |
| 8 | VG: UI edit round trip, un-bundling, `bundle` field error, option list | medium | P | Playwright added; `errors.bundle` is now rendered |
| 9 | EC: all-untracked bundle reports `available` with `quantity` 0 | low | D | spec-defined; documented for 5.5 |
| 10 | BH/EC: nested bundles are checked only in `product.upsert`, not on replayed rows | low | D | no database trigger; revisit with catalog sync |
| 11 | BH: a deleted component reads "inactive" | low | D | wording only |
| 12 | BH: N+1 component reads | low | R | bounded by 30 components, local SQLite |
| 13 | EC: `bundle` error wins over `components` for a component product saved as an invalid bundle | low | R | both refusals are in the spec, no order promised |
| 14 | EC: fractional `qty` from a raw API caller is a deserialisation error | low | R | typed integer on the wire; the UI refuses it first |
| 15 | BH: replay of pre-0018 `products` log rows lacks `bundle` | low | D | unreleased schema; not exercised |
