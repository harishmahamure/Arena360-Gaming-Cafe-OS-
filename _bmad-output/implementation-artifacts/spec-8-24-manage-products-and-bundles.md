---
title: "Story 8.24: Manage → Products and bundles"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: 'ed809dc85c64a11819755182a2cfebc9843c9f1a'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-23-manage-pricing-plans-and-rates-pricing-rules-and-coupons.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The owner can change prices from Manage → Pricing but not the menu: a product or bundle cannot be added or changed from the Owner app, and the summary lists them by name only.

**Approach:** Manage gets two areas (Pricing · Products, `?area=`). Products lists each live paid branch's products and bundles from the summary `catalogue` (category, price, tax, active, kitchen, barcode), searchable by name or barcode. Forms validate like the Hub and call `propose()` with `product.upsert` and the row's `base_rev`; a tracked product's low-stock level is its own `stock.threshold` command. Branch rows carry the editable fields, as in 8.23.

## Boundaries & Constraints

**Always:**
- Product and bundle `CatalogueRow.fields` (additive, no schema bump) is the upsert payload without `id`, `active`, `image` and `kitchen_notes`, plus `low_stock_qty`; a bundle adds `components` (`product_id`, `qty`). Flags are booleans. Older summaries lack `fields`: the row lists by name only and cannot be edited.
- Ids are per branch: forms hold names (a bundle's components too); `resolve()` (8.23) builds one payload per branch with that branch's own ids, `base_rev` and idempotency key. A branch missing a component by name is left out with the reason inline, and a branch where another active item has the barcode is left out likewise.
- The picture is resized in the browser to a JPEG of at most 512 px and 200 KB and sent in the payload. The summary holds no images, so an owner `product.upsert` **without** `image` keeps the stored one (`IMAGE_KEEP` in `arena_core::commands`, swapped in `owner_wins`); `""` removes it. `ProductUpsert.image` gains `#[serde(default)]`.
- A product command may carry a picture: the cloud's payload cap for `product.upsert` is 288 KB (`PRODUCT_COMMAND_PAYLOAD_MAX`, other kinds stay 16 KB) and `POST /v1/commands` takes bodies of that size.
- `stock.threshold` is an owner command kind (handled, Basic, not sensitive): `{product_id, low_stock_qty | null}`, no `base_rev`. Only tracked non-bundle products have a level.
- Stock quantities are never editable; no MFA prompt.

**Never:** stock-in, waste or any quantity edit; editing kitchen notes; MFA (8.25).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Edit a product | Cola 50 -> 60 | preview, `product.upsert {id, …, price_minor: 6000}` with the row's `base_rev`, no `image` | bad price/tax/HSN/barcode: Hub copy inline, no preview |
| Bundle, All branches | b3 lacks "Noodles" | b3 left out with `owner.pricing.excluded.missing`; b1 sent | none |
| New item that exists | "COLA" | each branch with it is left out ("already has") | none |
| Barcode taken | other active item has it | branch left out with the reason | none |
| Picture | 1600×1200 PNG | JPEG ≤ 512 px, ≤ 200 KB in `payload.image` | not a picture: `hub.products.invalid.image` |
| Low-stock level | tracked Cola -> 6 / empty | `stock.threshold {product_id, low_stock_qty: 6 \| null}` per branch | untracked/missing: left out |
| Old summary | row without `fields` | listed, no Edit | none |

</intent-contract>

## Code Map

- `crates/arena-store/src/summary.rs` -- `product_rows`, `to_flags`: `fields` for products and bundles (components via `list_components`).
- `crates/arena-proto/src/{owner_commands,commands,catalog,summary}.rs` -- `stock.threshold` kind, `command_payload_max`, `image` default, doc.
- `crates/arena-core/src/commands.rs` -- `HANDLED_COMMAND_KINDS` (14), `IMAGE_KEEP` in `command_intent` / `owner_wins`.
- `crates/arena-cloud/src/api/commands.rs` -- per-kind payload cap, body limit on the post route.
- `apps/owner/src/lib/products.ts` (new) -- reads, search, validators, `proposeProduct`, `proposeThreshold`; `image.ts` (new) -- `shrinkImage`.
- `CatalogueScope.svelte` (new, extracted from `PricingBoard`) -- branch pick, data ages, reads; `ProductsBoard`, `ProductsPanel`, `ProductForm`, `ThresholdForm`; `ManageBoard` hosts the area tabs; `PricingRow` gains `more`.
- `packages/ui/src/messages/en.json` -- `owner.products.*`, `owner.manage.*`; the Hub's `hub.products.*` / `hub.lowstock.*` copy is reused.
- Tests: `apps/owner/tests/products.spec.ts`; Rust tests in `arena-core` (summary fields, image keep + threshold command) and `arena-cloud` (picture payload size).

## Tasks & Acceptance

**Execution:**
- [x] Store/proto/core: product `fields`, `stock.threshold` owner kind, image keep, payload cap; Rust tests.
- [x] `products.ts`, `image.ts`, `CatalogueScope` extraction, products board/panel/forms, Manage areas, strings.
- [x] Playwright tests for every matrix row on desktop and phone.

**Acceptance Criteria:**
- [x] Given Manage → Products, then products and bundles are listed per branch with category, price, tax rate, active and kitchen flags, searchable by name or barcode.
- [x] Given a product edit, when sent, then `product.upsert` goes through the preview with `base_rev`, no MFA prompt, and stock quantities are never editable (the low-stock level is its own `stock.threshold`).
- [x] Given a bundle edit, then components must exist at each target branch, otherwise that branch is left out with the reason inline.
- [x] Given a picture, then it is resized to at most 512 px and 200 KB in the browser before it is sent in the payload.

## Spec Change Log

- The epic lists the low-stock threshold in the product edit, but `product.upsert` has no such field and a new product has no id to key a second command by. It is a separate `stock.threshold` action on a tracked row (new owner kind) rather than a field of the product form.
- The 16 KB command payload cap and 64 KB body limit could not carry a 200 KB picture; both are raised for `product.upsert` only.

## Review Triage Log

- Code review (2026-10-30): no blocking findings. clippy, Rust tests, owner svelte-check, strings, layers, prettier and all 474 Playwright tests pass. Two unrelated arena-cloud process-timing tests (backup hung-run, unreachable-at-start) flaked under full-suite load; `a_hung_run` passes alone.

## Design Notes

- Tax rate: empty means the branch default; the Owner app checks 0–100 %, so a GST branch's 28 % cap and its HSN requirement are enforced by the Hub (the row then reads "Couldn't apply at branch").
- A product that is a component of a bundle cannot become one; the Hub refuses it the same way.
- The fetch page of a Hub's mailbox is up to 50 commands, so many pictures in the mailbox make one large response; not changed here.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-api`, `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-api --all-targets -- -D warnings`, `pnpm run gen:types`, `check:strings`, `check:layers`, `check:protocol`, `format:check`; in `apps/owner`: `pnpm run check` and `pnpm exec playwright test` -- expected: clean

## Auto Run Result

Status: in-review. Implemented product/bundle `fields`, the Products area (list, search, product and bundle forms, picture, low-stock level), `stock.threshold` as an owner kind, image keep and the larger payload cap for product commands. Not run: staging proof of the end-to-end owner flow.
