---
title: 'Story 5.9: POS screen — product grid, search or scan, and cart'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: '703880d1bcf4ae232da8952bd91cc2f365740f24'
baseline_revision: '703880d1bcf4ae232da8952bd91cc2f365740f24'
route: 'dispatch'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
---

<intent-contract>

## Intent

**Problem:** The POS rail destination is a placeholder, so counter staff cannot find a product or build a cart.

**Approach:** A Hub-only screen (no new intents or Rust changes) over `GET /ui/products`: `pos-product-grid` on the left (category strip with ★ Favourites first, one "Search or scan barcode" field, `hub-tile` product tiles), the `cart` on the right 40px apart, the `settle-bar` pinned under it, and below 840px a `phone-cart-bar` that opens cart + settle bar as a full-height sheet.

## Boundaries & Constraints

**Always:**
- A product that is inactive, out of stock or expired renders a disabled tile with a tag ("Inactive", "Out of stock", "Expired") at full ink and cannot be added.
- A search filters every product by name, case-insensitively, whatever category is picked. Ctrl+K focuses the field when focus is not in a text field (on POS it wins over the Players lookup).
- A code ending in Enter in the field, or a burst of printable keys under 50 ms apart ending in Enter while focus is on POS outside a text field, adds the product with that exact barcode (+1) and clears the field; an unknown code shows "No product with barcode <code>." and adds nothing.
- ± steppers check availability live: a step up past stock on hand (a bundle counts its components, together with lines using the same product) or past 999 per line is refused with the reason under the line; a step down is always allowed. Bundle components show indented under the bundle.
- The cart header shows "Walk-up" or the attached player and a "Change" link opening `player-lookup`. An empty cart shows "Cart's empty." and the settle bar shows no total.

**Never:**
- Creating orders, taking payment, stock movement, tax breakup, session attach, shifts (Stories 5.10–5.13), persisting the cart across screens or reloads, a settle action.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Tap | available tile | line +1, total updates | none |
| Unavailable tile | out of stock / expired / inactive | disabled, tag shown | cannot be added |
| Scan, field | `8902` + Enter | product +1, field cleared | none |
| Scan, burst | 4 keys 5 ms apart + Enter, focus outside a text field | product +1 | slow typing adds nothing |
| Unknown code | `0000` + Enter | nothing added | "No product with barcode 0000." |
| Step past stock | qty 3 of 3, + | qty stays 3 | "Only 3 Cola left." under the line |
| Bundle | combo of Cola×1 + Chips×2 | parts shown ×qty; refused when a part runs short | reason under the line |
| Phone | narrower than 840px, cart has items | bar "N items · total · View"; opens sheet; Esc/Back closes with cart kept | none |

</intent-contract>

## Code Map

- `apps/hub/src/lib/pos.ts` -- pure rules: tags, category/search filter, barcode lookup, `step` availability check, totals, `ScanBurst`.
- `apps/hub/src/routes/pos/+page.svelte` -- the screen; cart state is page-local; loads `loadProducts()`, reloads on `catalog.version`.
- `apps/hub/src/routes/+layout.svelte` -- Ctrl+K focuses `[data-pos-search]` before `[data-player-lookup]`.
- `apps/hub/src/lib/players/PlayerLookup.svelte` -- reused for "Change".
- `packages/ui/src/messages/en.json` -- `hub.pos.*`.
- `apps/hub/tests/pos.spec.ts`, `pos.unit.spec.ts`; `shell.spec.ts` no longer treats POS as a placeholder.

## Tasks & Acceptance

**Execution:**
- [x] `apps/hub/src/lib/pos.ts` -- availability, filter, barcode, burst rules
- [x] `apps/hub/src/routes/pos/+page.svelte` -- grid, search/scan, cart, settle bar, phone bar and sheet
- [x] `apps/hub/src/routes/+layout.svelte` -- Ctrl+K on POS
- [x] `packages/ui/src/messages/en.json` -- messages
- [x] Tests -- Playwright (desktop and phone) for every matrix row, unit tests for the rules, shell spec updated

**Acceptance Criteria:**
- [x] Given POS on the rail, when it opens, then the grid, category strip (★ Favourites first), search field, cart 40px apart and settle bar show.
- [x] Given an inactive, out-of-stock or expired product, when the grid renders, then its tile is disabled with a readable tag.
- [x] Given typing in the search field, when characters are entered, then the grid filters by name; Ctrl+K focuses the field.
- [x] Given a keyboard-wedge scan (Enter, or a fast burst outside a text field), then the product is added and the field cleared; an unknown code is reported.
- [x] Given a cart with lines, when the steppers are used, then availability is checked live, bundle parts show, and the header shows Walk-up or the player with Change.
- [x] Given an empty cart, then "Cart's empty." shows and the settle bar has no total.
- [x] Given a window under 840px with items, then `phone-cart-bar` opens the sheet and Esc/Back closes it with the cart kept.

## Spec Change Log

## Review Triage Log

## Design Notes

- Product prices are summed as listed; the per-rate tax breakup and the real settle action arrive with Story 5.11, so the settle bar's button is disabled with its reason.
- Favourites is the default tab when any product is a favourite, else All. A search ignores the picked category so a product is never hidden by it.
- Cart state is page-local: a cart is an unsaved draft until Story 5.11 makes it an order.

## Verification

**Commands:**
- `pnpm --filter hub check` -- 0 errors, 0 warnings
- `pnpm exec playwright test` in `apps/hub` -- full suite pass (1335 passed, 61 skipped)
- `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `format:check` -- pass; `check:tokens-used` fails on pre-existing `--ad-reserved` in `floor/+page.svelte` only

## Auto Run Result

Status: review

- **Change:** the POS screen (grid, category strip, search or scan, cart with live availability, settle-bar region, phone cart bar and sheet); Ctrl+K targets the POS search on POS.
- **Files:** `apps/hub/src/lib/pos.ts`, `apps/hub/src/routes/pos/+page.svelte`, `apps/hub/src/routes/+layout.svelte`, `packages/ui/src/messages/en.json`, `apps/hub/tests/{pos.spec,pos.unit.spec,shell.spec}.ts`.
- **Residual risks:** the settle button is disabled until Story 5.11; a cart is lost on navigation; availability is checked against the last loaded product list (the server re-checks at sale in 5.11). The story was built by the main agent directly, not through the build-auto subagent/review loop.
