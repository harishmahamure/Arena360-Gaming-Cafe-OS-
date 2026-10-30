---
title: "Story 8.23: Manage → Pricing — plans and rates, pricing rules and coupons"
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: 'af599ed68bb5127a7c0123cf91953d1fc85cf70b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-22-commands-queue-on-the-owner-s-device-while-it-is-offline.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Manage has a command preview and queue (8.21–8.22) but no form, so the owner cannot change a rate, plan, pricing rule or coupon from the Owner app.

**Approach:** Manage → Pricing gets a `tab-control` (Plans & rates · Pricing rules · Coupons, in the URL as `?tab=`) filled from each live paid branch's summary `catalogue`. Forms validate like the Hub and call `propose()` with `rate|plan|rule|coupon.upsert` and the row's `base_rev`. Branch rows carry the editable fields so the lists and forms can show and prefill them.

## Boundaries & Constraints

**Always:**
- The summary `CatalogueRow` gains an optional `fields` object (additive, no schema bump): the row as its upsert payload without `id`/`active` (rate: `group_id`, `price_per_hour_minor`; plan: `plan_kind`, `price_minor`, `minutes`, `validity_days`, `window_*_min`, `group_ids`; rule: `group_id`, `rule_kind`, `days_mask`, `start_min`, `end_min`, `date_*`, `adjust_*`, `priority`; coupon: `percent_bp`, `amount_minor`, `valid_*`, `usage_limit`, `min_bill_minor`, `targets`). Older summaries lack it: the row then lists by name only and cannot be edited.
- Ids are per branch, so an edit for one branch uses that branch's ids. "All branches" matches items by trimmed, case-insensitive name (a coupon by code, a rate by group name) and builds one payload per branch (`CommandDraft.variants`), each with its own `id`, `base_rev`, group ids and idempotency key; the preview post stays `all`.
- A branch where the item is missing by name is offered "Create here" (payload without `id`) or "Exclude"; Send waits until the owner chose for every such branch. A branch where a group the item names does not exist by name is excluded with the reason inline. Only branches with a variant and a chosen "Create here"/matched row are sent, one post each, offline each is queued.
- Field rules and error copy are the Hub's (`hub.plans.invalid.*`, `hub.rules.invalid.*`, `hub.coupons.invalid.*`, `hub.rates.*` keys); no MFA prompt (not sensitive).
- Rule status: `active now` (weekly: today's weekday and the minute in the window, in the branch zone; date: today within the dates), `scheduled`, `ended`, or `off` when inactive.
- The list shows the data age of each branch; the selected tab is `?tab=plans|rules|coupons` (default `plans`).

**Never:** a server change beyond the additive catalogue field, products (8.24), MFA (8.25), editing stock or coupon `used`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Edit a rate | one branch, group rate 60 -> 80 | preview, `rate.upsert {group_id, price_per_hour_minor: 8000}` | price below 1 or not a number: Hub's `hub.rates` error inline, no preview |
| Edit a plan, all branches | plan "Night pass" exists in b1, not b2 | b1 variant with b1 ids and `base_rev`; b2 asks Create here / Exclude | Send disabled until chosen |
| Group missing | rule group "VR" has no group by name at b2 | b2 excluded, reason shown | none |
| Rule row | weekly Fri 18:00–22:00, today Fri 19:00 | "Active now" | none |
| Old summary | row without `fields` | row listed, Edit hidden | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/summary.rs` -- `CatalogueRow.fields`; `packages/ui/src/generated/CatalogueRow.ts` regenerated (`pnpm run gen:types`).
- `crates/arena-store/src/summary.rs` -- `catalogue()` fills `fields` for rates, plans, rules, coupons (`list_plan_groups`, `list_coupon_targets`).
- `apps/owner/src/lib/pricing.ts` (new) -- catalogue reads per branch, name matching, validators, payload and variant builders, schedule text and rule status.
- `apps/owner/src/lib/commands.svelte.ts` -- `CommandDraft.variants/choices`, `confirm` posts per variant, `choose`.
- `CommandPreview.svelte` -- Create here / Exclude choices and excluded reasons.
- `PricingBoard.svelte` (tabs, branch pick, data age), `RatesPlansPanel.svelte`, `RulesPanel.svelte`, `CouponsPanel.svelte`, `PricingForms` per kind; `ManageBoard.svelte` hosts the board.
- `packages/ui/src/messages/en.json` -- `owner.pricing.*`; Hub's `hub.*` validation keys reused.
- Tests: `apps/owner/tests/pricing.spec.ts`; `stub.ts` catalogue fixtures; a Rust test for `fields`.

## Tasks & Acceptance

**Execution:**
- [x] Proto + store: `fields` on catalogue rows, regenerate types, Rust test.
- [x] `pricing.ts` and the draft `variants` / choices in `commands.svelte.ts` and the preview.
- [x] Tabs, lists, forms for rates, plans, rules, coupons; strings.
- [x] Playwright tests for every matrix row on desktop and phone; extend the 1.8 proptest generator is not needed (no new Hub intent).

**Acceptance Criteria:**
- [x] Given Manage → Pricing, when it opens, then a `tab-control` offers the three tabs, the selected tab is in the URL, and the lists come from each branch's catalogue with its data age.
- [x] Given a rate or plan edit, when Send is pressed, then the 8.21 preview opens with `rate.upsert`/`plan.upsert` and the row's `base_rev`, with no MFA prompt, and invalid fields show the Hub's copy.
- [x] Given Pricing rules, then each row shows name, schedule, branches and status; create/edit sends `rule.upsert`.
- [x] Given Coupons, then create/edit sends `coupon.upsert` with the Hub's fields.
- [x] Given "All branches" for an item present in some branches only, then the owner chooses Create here or Exclude before Send.

## Spec Change Log

## Review Triage Log

## Design Notes

- Staging proof of Journey 6 (Applied row, floor and attract within 1 minute) needs the staging environment and is outside this unattended run; the row flow itself is 8.21's.

## Verification

**Commands:**
- `cargo test -p arena-store -p arena-proto`, `pnpm run gen:types`, `pnpm run check:generated`, `check:strings`, `check:layers`, `check:protocol`, `format:check`; in `apps/owner`: `pnpm run check` and `pnpm exec playwright test` -- expected: clean

## Auto Run Result

Status: in-review. Implemented the additive catalogue `fields`, `pricing.ts`, per-branch variants with Create here / Exclude in the preview, and the Pricing tabs and forms. Verified: arena-proto/store/core tests, clippy, fmt, strings/layers/protocol/secrets/prettier, owner svelte-check, Playwright (438 passed). Not run: Journey 6 on staging. `check:tokens-used` fails on untouched `apps/hub` tokens.
