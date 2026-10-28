---
title: 'Story 5.22: Manual discount with manager PIN and reason'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: '07294c26d40daaba305f609e79fca41da5e0e458'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-21-coupons-applied-at-settle.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Staff can only discount a bill through a coupon, so a one-off courtesy (regular customer, service issue, staff meal) is either not given or given outside the system with no approver or reason on record.

**Approach:** `session.settle`, `pos.sale` and `plan.sell` take an optional `discount` (`ManualDiscount`: a percent or an amount, a reason, a note for `other`). The Hub prices it inside the settle transaction after the coupon, before tax, spread over the tax rates by largest remainder as for a coupon, and writes `discount_minor`, `discount_reason`, `discount_note` and `discount_by` (the approving staff member) on the order. `discount.manual` is checked on the actor or on the manager whose PIN approved the intent (`elevated_by`). `POST /ui/discount/preview` prices it for the settle bar, which shows a Discount line and the new total. Undo of a session settle gives the amount back to the bill.

## Boundaries & Constraints

**Always:**

- A discount is a percent (1 to 10000 basis points, the Hub offers 1–100%) or an amount (above zero, up to `MAX_PRICE_MINOR`), never both. The reason is `regular_customer`, `service_issue`, `staff_meal` or `other`; `other` needs a note of 1 to 200 plain characters (the Hub picker allows 140). The note is stored only for `other`.
- The coupon applies first. The discount's base is what is left of each tax rate after the coupon's shares; a percent is half-up of that base, an amount above it (or above the bill after the coupon) is refused. The discount is taken off before tax is computed once per rate; a bill with no tax (session, plan sale) just has its total reduced.
- Refused with `VALIDATION_FAILED` and field `discount`, `discount.percent_bp`, `discount.amount_minor` or `discount.note`: both or neither of percent and amount, out-of-range values, a missing or bad note, an amount over the bill, nothing left to discount, a part-paid bill. The bill is unchanged and the settle rolls back whole. A coupon problem stays `COUPON_INVALID`.
- Permission: `discount.manual` on the actor, or on the elevating manager (`PERMISSION_DENIED` otherwise); the owner always may. `POST /ui/elevate` items take `discount: true` (only for the three settle kinds) so the PIN only matches a manager who holds it. The approving staff id is stored as `discount_by`; the actor stays `settled_by`.
- The receipt (print and reprint) prints `Discount  -<amount>` after the coupon line and before the tax lines. The settle bar shows the same line.
- A session settle's Undo restores the order total and clears the discount columns; a counter or plan sale is reverted whole (the order goes with its columns).

**Never:**

- A discount on a top-up, a credit collection or a group bill; more than one discount on a bill; a discount without a reason; a discount PIN or ticket kept after one settle (one approval, one settle).
- The customer's phone or name in a discount row.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Percent | 10% on a 40.00 sale | 4.00 off, total 36.00, columns set | none |
| After coupon | 10% coupon then 10% discount on 40.00 | coupon 4.00, discount 3.60, total 32.40 | none |
| Two rates | coupon then 10% on 5% and 18% lines | shares by largest remainder on what is left per rate | none |
| Counter, no permission | `discount` with no PIN | nothing written | `PERMISSION_DENIED` |
| Counter + manager PIN | elevated by a manager with `discount.manual` | applied, `discount_by` = manager | none |
| Amount over bill | 40.01 off 40.00 | refused | `VALIDATION_FAILED` `discount.amount_minor` |
| Other, no text | reason `other`, blank note | refused | `VALIDATION_FAILED` `discount.note` |
| Undo (session) | within the window | total restored, columns NULL | none |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0055_manual_discount.sql` -- `orders.discount_minor/discount_reason/discount_note/discount_by`; `src/coupons.rs` -- `set_order_discount`, `clear_order_discount`; `src/outbox.rs` -- `ReceiptHead.discount`.
- `crates/arena-proto/src/discount.rs` -- `ManualDiscount`, `DiscountReason`, `DiscountPreviewRequest/Preview`; `discount` on `SessionSettle`, `PosSale`, `PlanSell`; `ElevationItem.discount`.
- `crates/arena-core/src/apply/discount.rs` -- pricing after the coupon (`quote`, `price`, `Adjust`), `record`, `release_on_revert`, `preview`; `coupon.rs` (optional coupon on `CouponBill`, `spread` shared); `pos_sale.rs` (`price_adjusted`), `session.rs`, `plan_sales.rs`; `guards.rs` (`manual_discount`); `printer/{receipt,job}.rs` -- Discount line.
- `crates/arena-api/src/ui_coupons.rs` -- `POST /ui/discount/preview`; `ui_auth.rs` -- route and `discount` on elevation items.
- Hub: `lib/discount.ts`, `lib/DiscountEntry.svelte` (value, reason, in-place PIN), `lib/SettleBar.svelte` (Discount line, total, ticket handling), `TileSheet.svelte`, `tile-actions.svelte.ts`, `players/SellPlan.svelte`, `routes/pos/+page.svelte`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/discount.rs`, `printer/receipt.rs` (+ goldens), store `migrations.rs`, API test, proptest `Op::Discount` and `assert_discounts_hold`, Playwright `settle.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Store: migration 0055, order discount columns and helpers, receipt head
- [x] Protocol: `ManualDiscount`, preview types, `discount` on the three settles, elevation item flag, generated TS
- [x] Core: pricing after the coupon, record at settle, restore at Undo, `discount.manual` guard, receipt line
- [x] API: `/ui/discount/preview`, `discount` on `/ui/elevate` items
- [x] Hub UI: Discount entry (percent or amount, reason picker, inline PIN), settle-bar line and total on POS, tile sheet and plan sale
- [x] Tests: Rust matrix, receipt golden, migration, API, proptest op, Playwright
- [x] Verification gate

**Acceptance Criteria:**
- Given the settle bar, when staff choose Discount and enter a percentage (1–100) or an amount (at most the bill after the coupon), then a reason picker (Regular customer · Service issue · Staff meal · Other with text) and, without `discount.manual`, an `inline-pin` open in place.
- Given a valid PIN and reason, when the settle applies, then the discount is taken before tax across rates as for a coupon, stored on the order with reason and approver, shown as a Discount line on the bill and receipt, and visible in the audit (the order row image and `elevated_by`).
- Given a discount and a coupon, when both apply, then the coupon applies first and the discount to the remainder.
- Given no reason, or a PIN without the permission, when submitted, then nothing is applied and the reason is shown inline.

## Spec Change Log

## Review Triage Log

## Design Notes

The coupon and the discount share one pricing step (`discount::price`): the coupon's per-rate shares are subtracted from the lines' per-rate amounts, and the discount is spread over what is left. `Adjust::shares()` merges both into the negative tax lines of `price_net`. The settle bar's PIN ticket is single use: the bar hands it to the settle and forgets it, and asks again if the settle is retried or the ticket is older than 45 s (the Hub keeps it 60 s). The audit lists the discount through the order row's change (the columns) and the intent's `elevated_by`; no new audit label was needed because no new intent kind was added.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy -D warnings --all-targets` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay`, `arena-tasks`, `arena-hub`; `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-replay`, `arena-api` -- expected: pass. (`arena-kiosk-svc`'s `discovery` test target and two `arena-tasks` tests, `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `a_printer_that_stops_reading_times_out`, fail on a clean checkout too.)
- `pnpm gen:types`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, Hub `svelte-check`, prettier; Playwright `settle`, `coupons`, `pos`, `plan-sales`, `credit`, `group-bill`, `elevation` in `apps/hub` -- expected: pass.
