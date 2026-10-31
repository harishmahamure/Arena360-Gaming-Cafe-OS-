---
title: "Story 10.8: Account -> Subscription shows each branch's plan, invoices and payment method"
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '24e7ea83ceffa47dd8ca2faac25db9af35693691'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-7-every-subscription-charge-gets-a-gst-tax-invoice-from-arena.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Owner app's Account -> Subscription page shows only a branch's tier and flags and says "Plan changes are available soon". An owner cannot see what a branch pays, when it renews, how it is paid, or find an invoice.

**Approach:** Three tenant-scoped cloud routes (plans, invoice list, signed PDF link) and a rebuilt Owner page: a card per branch with a state badge, add-ons, the locked-book price of the selected cycle, next renewal and payment method; a Free card with an Upgrade button; and an invoice list filterable by branch and financial year.

## Boundaries & Constraints

**Always:** every amount comes from the price book locked onto the branch's subscription, never the live one; GST is shown beside the amount on band IN only; money uses the plan's currency; a branch or invoice outside the owner's tenant answers `FORBIDDEN` and is never read; the PDF link is a presigned GET valid `PRESIGN_TTL_S` (600 s); a credit note is shown negative with its original number; one `h1`, each card a labelled region, state changes announced once in a polite live region; one column under 768 px and two above.

**Never:** no change-plan, cancel, refund or payment-method change (10.9, 10.10, 10.14, 10.15); no UPI handle read or stored; no change to the webhook transitions, the licence or the heartbeat wire shapes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Paid branch | active Basic monthly, card 4242 | card: Active, Basic · Monthly, add-ons, price `+ GST` per month, renewal date and amount, "Card ending 4242" |
| Free branch | no open subscription, or cancelled/ended/refunded/abandoned | "Free · 1 branch · ads on the Android Hub" and an Upgrade button to the Story 10.4 checkout for that branch |
| States | pending, pending_review, renewal_needs_approval, grace, cancel_at_period_end, lapsed | Awaiting payment, Checking payment, Needs approval, Grace, Cancels on <date>, Lapsed |
| Monthly / Yearly | tab changed | every price swaps in place, `?cycle=` in the URL, renewal unchanged |
| Bank mandate then UPI | renewal paid another way | bank name replaced; UPI shows no detail |
| Invoices | tenant has 3 documents | newest first; filter by branch and FY; credit note negative; "PDF being prepared" until rendered |
| Another tenant | their branch id or invoice id | `FORBIDDEN` |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0038_subscription_payment_detail.sql` -- `subscriptions.mandate_detail`, `billing_invoices.original_number` (guarded by the immutability trigger); `control.rs` registers it.
- `crates/arena-store/src/control/{subscription,billing,invoice}.rs` -- `Subscription.mandate_detail`, `SubscriptionChange.mandate_detail` (kept only with a method), `Invoice.original_number`, `invoices_of_tenant`, `invoice_years_of_tenant`, `invoice_by_id`.
- `crates/arena-cloud/src/billing.rs`, `webhook.rs` -- `PaymentInfo.detail` (card last four, mandate bank) carried by `Decision.mandate_detail`.
- `crates/arena-proto/src/subscription.rs` (+ generated TS) -- `PlanState`, `PlanPrice`, `PaymentMethodView`, `BranchPlan`, `AccountSubscription`, `InvoiceKind`, `InvoiceRow`, `InvoiceList`, `InvoiceDownload`.
- `crates/arena-cloud/src/account_plan.rs`, `api/account_billing.rs` -- plan cards, invoice rows and the three routes.
- `apps/owner/src/{lib/subscription.ts,lib/SubscriptionCard.svelte,lib/InvoicesPanel.svelte,routes/account/subscription/+page.svelte}`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-cloud/tests/billing_events.rs`, `billing.rs` and `account_plan.rs` unit tests, `apps/owner/tests/subscription.spec.ts`, `stub.ts`, `shell.spec.ts`, `offline.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0038, store reads and writes for the payment detail and the invoice list
- [x] Capture the card's last four / mandate bank in the webhook transition (never a UPI handle)
- [x] Proto types and generated TypeScript
- [x] `GET /v1/account/subscription`, `GET /v1/account/invoices`, `GET /v1/account/invoices/{id}/pdf`, tenant-scoped
- [x] Owner page: cards, state badges, cycle tabs in the URL, Free card with Upgrade, invoice list with filters and signed download, live region, responsive grid
- [x] Tests for every I/O row (cloud integration, Playwright on desktop and phone, axe in all three themes); README section

**Acceptance Criteria:**
- Given Account -> Subscription opens, then each branch shows a card with name, tier, cycle, state badge, add-ons with quantities, next renewal date and amount (+ GST on band IN) and the payment method.
- Given a Free branch, then it shows "Free · 1 branch · ads on the Android Hub" and an Upgrade button that opens the checkout for that branch.
- Given Invoices, then every invoice and credit note lists newest first, filterable by branch and FY, with a signed-URL Download PDF; a credit note is negative with its original number.
- Given the Monthly | Yearly control, then every displayed price swaps in place, is in the URL and uses the locked book.
- Given a request for another tenant's subscription or invoice, then it returns `FORBIDDEN`.

## Design Notes

`StatusBadge` is drawn for the Hub's dark tiles, so the card sets `--status-badge-background` to the card surface; the active state uses the `info` tone because the `ok` tone fails contrast in the light theme. The screen re-reads on window focus, so a payment finished in another tab shows (and is announced) on return. `original_number` is a nullable column so Story 10.15 only has to fill it.

## Verification

**Commands:**
- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud --all-targets -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud` -- pass apart from baseline failures
- `pnpm --filter @arena/owner check`; `npx playwright test` in `apps/owner` (desktop and phone)
- `pnpm run check:strings`, `check:generated` (after the generated files are committed), `check:hex`, `check:tier-lock`, `check:secrets`

## Auto Run Result

Status: review. fmt and clippy clean; `cargo test` passes except baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token` and the Windows-flaky `backup::a_hung_run_times_out_with_its_own_code`. `svelte-check` clean; the Owner Playwright suite passes on desktop and phone (727 tests) apart from `products.spec.ts` "an empty low-stock level clears it", which failed once in the full run and passes alone three times. `check:layers`, `check:tokens-used` and `format:check` fail on the baseline commit too (arena-api/arena-store edge, Hub tokens, two scripts).

Risks to know: `arena-store` sqld tests skip locally (no `sqld`); the signed PDF link needs `st.presigner` (the `ARENA_DATA_S3_*` variables) on the API role, else the route answers 503; a subscription row paid before this story has no `mandate_detail`, so a card shows "Card" until its next charge; "Pending" and "Checking payment" cards show prices from the pending subscription's book.
