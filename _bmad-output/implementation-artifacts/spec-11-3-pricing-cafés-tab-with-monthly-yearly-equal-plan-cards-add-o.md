---
title: 'Story 11.3: Pricing — Cafés tab with Monthly · Yearly, equal plan cards, add-ons and FAQ'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '97d7b3c1'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/mockups/key-web-pricing.html'
deferred:
  - 'The Partners tab shows "Partner programme details coming soon" until Story 11.12 replaces it with the programme terms.'
  - '"Choose <tier>" links to `<start URL>?tier=<free|basic|premium>`. Reading that parameter is Story 11.8 (Get started hands off to sign-up); until then the onboarding app ignores it.'
  - 'The comparison table follows the `arena-proto` tier table, not the mockup: Basic backs up every 15 minutes (900 s) and Premium every 5 minutes (300 s), where the mockup says 5 and 1. Basic is shown with the Tunnel as an add-on, Premium with it included.'
  - 'Add-ons are listed per branch per month only (as the story asks); the yearly add-on prices are in the snapshot but are not shown.'
---

<intent-contract>

## Intent

**Problem:** `/pricing` does not exist (Home links to it and gets the 404), so a prospective owner cannot see what a branch costs, what each tier limits, or the refund rule.

**Approach:** A static Astro page. Both billing cycles are in the HTML and `<html data-pricing-tab / data-pricing-billing>` (set before first paint from the URL, then by two small Svelte `TabControl` islands) chooses the visible panel and prices, so the page is complete with scripts off and the URL carries the choice. Every amount and limit is read from the generated snapshots (`pricing.snapshot.ts` for the price book, new `tiers.snapshot.ts` for the `arena-proto` tier table); the message catalogue holds words and `{placeholders}` only.

## Boundaries & Constraints

**Always:** amounts from `@arena/ui/pricing` (`planPrice`, `addOnPrice`, `monthsFree`) formatted with Indian digit grouping; limits from `tierRow`; tokens only; three cards of equal size and weight, none emphasised; one primary "Get started" on the page; `?tab=` and `?billing=` kept in step with the selection through `history.replaceState`.

**Never:** a typed price in `en.json` or markup; rewarded ads anywhere; `backdrop-filter`; a page that scrolls sideways at 390 px.

## I/O & Edge-Case Matrix

| Scenario | State | Expected Output / Behavior |
|----------|-------|---------------------------|
| Open | `/pricing` | Cafés and Monthly selected; URL becomes `?tab=cafes&billing=monthly` |
| Deep link | `?tab=partners` / `?billing=yearly` | opens on Partners / Yearly with no flash of the other panel |
| Bad value | `?tab=x&billing=y` | Cafés and Monthly |
| Arrow keys | focus on a tab | move, select and update the URL (wrap) |
| Yearly | selected | Basic ₹2,000/branch/year + 18% GST, Premium ₹19,990, each "2 months free"; Free unchanged; polite live-region announcement |
| 390 px | any | cards in one column in order; table scrolls in its own region, row headers pinned; page never scrolls sideways |
| No JavaScript | scripts off | Cafés, Monthly, full content |

</intent-contract>

## Code Map

- `apps/marketing/src/pages/pricing.astro`, `styles/pricing.css`, `components/PricingTabs.svelte`, `components/BillingTabs.svelte`, `lib/pricing-page.ts` (view model), `lib/pricing-state.ts` (URL and `<html>` state), `lib/price.ts` (`rupees` now exported).
- `packages/ui/src/pricing.ts` (`addOnPrice`, `monthsFree`, `tierRow`), `tiers.snapshot.ts` (generated), `components/LinkButton.svelte` (`params`), `messages/en.json` (`site.pricing.*`).
- `crates/arena-proto/src/tier.rs`: `tier_snapshot_matches_the_tier_table` writes/checks the snapshot (`UPDATE_TIERS_SNAPSHOT=1 cargo test -p arena-proto tier_snapshot`); `.prettierignore` lists it.
- Tests: `apps/marketing/tests/pricing.spec.ts` (new), `a11y.spec.ts` and `pseudo.spec.ts` (now cover `/pricing`), `scripts/marketing-site.test.mjs` (every displayed amount is a snapshot amount; no typed amount in the catalogue).

## Tasks & Acceptance

**Execution:**
- [x] `tab-control` Cafés · Partners under the title; selection in the URL; arrow keys; Partners placeholder
- [x] One "Get started" above a Monthly · Yearly `tab-control` (in the URL); three equal cards (Free, Basic, Premium) with "Choose <tier>" to Get started with the tier
- [x] Yearly swaps prices, "2 months free" on paid tiers, announced to screen readers
- [x] Amounts from the generated pricing table; CI check fails if a displayed amount is not in the price-book snapshot
- [x] Comparison table from the `arena-proto` tier table (generated snapshot, CI-checked) and add-ons per branch per month + GST; no rewarded ads
- [x] FAQ as a disclosure list with the exact refund text
- [x] 390 px layout: one column, table scrolls in its own container with the capability column pinned

**Acceptance Criteria:**
- Given Pricing, then Cafés is selected and in the URL, arrows move between tabs, and Partners says coming soon (pricing.spec.ts).
- Given the Cafés tab, then one Get started sits above Monthly · Yearly, followed by three equal cards with the prices and "Choose <tier>" links (pricing.spec.ts).
- Given Yearly, then prices swap, "2 months free" shows on paid tiers and the change is announced (pricing.spec.ts).
- Given the build, then every amount shown is in the price-book snapshot and none is typed in the catalogue (`scripts/marketing-site.test.mjs`); the tier snapshot fails `cargo test` when the table changes.
- Given the table and add-ons, then limits equal the tier table and add-ons read +20 devices ₹50, Tunnel ₹200, owner login ₹50 — available soon (pricing.spec.ts).
- Given the FAQ, then the refund answer is the exact text in a disclosure list (pricing.spec.ts).
- Given 390 px, then cards stack and only the table scrolls, first column pinned (pricing.spec.ts, a11y.spec.ts, pseudo.spec.ts).

## Verification

- `pnpm --filter @arena/marketing test` (66 passed, including axe in light, dark and hc at 390 and 1440 px and the pseudo-locale overflow check for `/pricing`), `node --test scripts/marketing-site.test.mjs` (9 passed), `cargo test -p arena-proto tier_snapshot`, `pnpm run check:strings`, `check:tokens-fresh`, `check:contrast`, `check:hex`, `check:no-blur`, `check:no-remote`, `check:pins`, `pnpm --filter @arena/onboarding check` (consumes the shared UI package), `prettier --check` on the changed files, `tsc --noEmit` in `apps/marketing` (no errors in `src` or `tests`).
- Not caused by this story: `check:layers` reports `arena-tasks -> arena-replay` at the baseline.
