---
title: 'Story 5.21: Coupons applied at settle'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: '3ca3f6c9e5c340162679130a873e4330877e4cc3'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-20-b2b-invoice-with-the-customer-s-gstin.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-29-dynamic-pricing-rules-by-weekday-hours-and-by-date.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A manager cannot run a promotion: there is no coupon, so staff discount by hand and the price advertised is not the price charged.

**Approach:** `coupon.upsert` (permission `catalog.edit`) stores coupons in `coupons` and `coupon_targets` (Settings → Coupons). `session.settle`, `pos.sale` and `plan.sell` take an optional `coupon` code; the Hub prices it inside the settle transaction (discount on eligible lines only, before tax, spread across rates by largest remainder), increments `coupons.used`, and stores `coupon_code` and `coupon_minor` on the order. `POST /ui/coupons/preview` prices a code for the settle bar, which shows a coupon line and the new total. Undo of any of the three settles gives the use back.

## Boundaries & Constraints

**Always:**

- A code is unique per branch, case-insensitive (stored upper-cased, 3 to 24 characters of `A-Z 0-9 - _`). A coupon is a percent (1 to 10000 basis points) or a fixed amount (above zero, up to `MAX_PRICE_MINOR`), with optional valid-from / valid-to (local `YYYY-MM-DD`, inclusive, branch timezone), an optional usage limit (at least 1), a minimum bill in minor units (0 = none) and targets (`group`, `plan` or `product` ids). Edits carry `base_rev` (`STALE`); a deactivated coupon stays listed.
- Eligible lines: all lines if the coupon has no targets; otherwise a time line (session) whose device group is a `group` target, a plan line whose plan is a `plan` target, a product line whose product is a `product` target. A bundle line is a product line. A coupon with targets and no eligible line is refused.
- Discount = percent of the eligible amount (half-up) or the fixed amount, never above the eligible amount. It is spread over the eligible lines' tax rates in proportion to their amounts (largest remainder in minor units, ties to the lower rate), taken off before tax is computed once per rate (`invoice::tax::compute`). A bill with no tax (session, plan sale) just has its total reduced. The minimum bill is checked against the bill total before the coupon.
- Refused with `COUPON_INVALID` and a reason in the message (`field: coupon`): unknown, inactive, not started, expired, used up (`used >= usage_limit`), not applicable to any line, bill under the minimum, bill with nothing to discount or nothing left to pay, or a bill partly paid. The bill is unchanged and the settle rolls back whole. Two settles racing for the last use are serialised by `apply()`: one wins, the other is `COUPON_INVALID`.
- `used` increments in the settle transaction. Undo (`session.settle.reverted`, `pos.sale.reverted`, `plan.sell.reverted`) decrements it, restores the order total (a session order) and clears `coupon_code` / `coupon_minor`. A coupon is not an irreversible effect.
- The receipt (print and reprint) prints a coupon line `Coupon <CODE>  -<amount>` after the items and before the tax lines. The settle bar shows the same line.

**Never:**

- A coupon on a top-up, a credit collection or a zero-total bill; more than one coupon on a bill; the manual discount (Story 5.22), the owner `coupon.upsert` command (Story 8.23) and a delete intent.
- The customer's phone or name in a coupon row.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Percent, all lines | 10% on 2 lines (500 bp and 1800 bp), no targets | discount split over the two rates, tax per rate on the reduced amounts | none |
| Targeted | product target P, cart holds P and Q | only P discounted | none |
| Fixed above bill | 500.00 off a 300.00 bill | discount = 300.00, total 0 | none |
| Expired / not started | outside valid window | refused, bill unchanged | `COUPON_INVALID` |
| Used up | `used = usage_limit` | refused | `COUPON_INVALID` |
| Under minimum | total below `min_bill_minor` | refused | `COUPON_INVALID` |
| Not applicable | targets match no line | refused | `COUPON_INVALID` |
| Race | last use, two settles | first applies, second refused | `COUPON_INVALID` |
| Undo | within the window | `used - 1`, order total restored | none |
| Bad coupon edit | blank/duplicate code, both kinds, percent over 100%, to before from | nothing written | `VALIDATION_FAILED` + field |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0054_coupons.sql` -- `coupons`, `coupon_targets` (change-log tables), `orders.coupon_code/coupon_minor`; `src/coupons.rs` -- rows and queries; `src/branch.rs` -- `Table::Coupons/CouponTargets`, `RowKey`; `src/sessions.rs` (`set_order_coupon`, `NewPosOrder`), `src/outbox.rs` (`ReceiptHead.coupon`).
- `crates/arena-proto/src/pricing.rs`-style new `coupon.rs` -- `CouponUpsert`, `CouponTarget`, `CouponView`, `CouponPreviewRequest/Response`, limits; `error.rs` -- `ErrorCode::CouponInvalid`; `intent.rs` -- `coupon.upsert` (`CatalogEdit`); `session.rs`, `pos.rs`, `plans.rs` -- `coupon` on the three settle payloads.
- `crates/arena-core/src/apply/coupon.rs` -- upsert load/validate/mutate plus `price` (eligibility, discount, rate spread), `redeem_on_settle`, `release_on_revert`; `apply/{mod,validate,guards,mutate}.rs`; settle paths in `session.rs`, `pos_sale.rs`, `plan_sales.rs`; `invoice/tax.rs` untouched (called with the reduced amounts); `printer/receipt.rs` + `printer/job.rs` -- coupon line.
- `crates/arena-api/src/ui_coupons.rs` -- `GET /ui/coupons`, `POST /ui/coupons/preview`; router + `catalog.edit` for the list.
- Hub: `routes/settings/coupons/+page.svelte`, `lib/coupons.ts`, `routes/settings/+layout.svelte`, `lib/SettleBar.svelte` (coupon field, line, total), `tile-actions.svelte.ts`, `TileSheet.svelte`, `players/SellPlan.svelte`, `routes/pos/+page.svelte`, `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/coupon.rs`, `printer/receipt.rs`, store `migrations.rs`, API test, proptest `Op::Coupon`, Playwright `coupons.spec.ts` and `settle.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Store: migration 0054, change-log tables, queries, order coupon columns, receipt head
- [x] Protocol: types, `COUPON_INVALID`, `coupon.upsert`, `coupon` on the three settles, generated TS
- [x] Core: `coupon.upsert`; pricing; redeem at settle and release at undo for all three intents; receipt line
- [x] API: list and preview routes
- [x] Hub UI: Settings → Coupons page; settle-bar coupon field and line on POS, tile sheet and plan sale
- [x] Tests: Rust matrix, receipt golden, migration, API, proptest op, Playwright
- [x] Verification gate

**Acceptance Criteria:**
- Given Settings → Coupons, when a Manager creates a coupon, then `coupon.upsert` stores code, percent or amount, validity, limit, minimum and targets in `coupons` and `coupon_targets`.
- Given a bill in the settle bar, when staff enter a valid code, then the discount applies to eligible lines only, before tax, spread over rates by largest remainder, and the bill breakdown and receipt gain a coupon line.
- Given an expired, exhausted, unknown or not-applicable code, or a bill under the minimum, when applied, then it is refused `COUPON_INVALID` with the reason inline and the bill is unchanged.
- Given a settle with a coupon, when it commits, then `coupons.used` increments in the same transaction, the receipt shows code and amount, and Undo decrements it.
- Given two settles racing for the last use, when both reach `apply()`, then only one succeeds and the other is `COUPON_INVALID`.

## Spec Change Log

## Review Triage Log

## Design Notes

A session or plan order carries no tax today, so its coupon lowers `orders.total_minor` (and `subtotal_minor`) at settle and Undo adds `coupon_minor` back; a POS sale is inserted already net, with `coupon_minor` recording the cut. Largest remainder: share_i = floor(D * a_i / A), the leftover minor units go one each to the rates with the largest remainders `(D * a_i) mod A`.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy` (workspace except `arena-e2e`) `-D warnings`; `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay` -- expected: pass.
- `pnpm gen:types`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, Hub `svelte-check`, prettier; Playwright `coupons` and `settle` in `apps/hub` -- expected: pass.
