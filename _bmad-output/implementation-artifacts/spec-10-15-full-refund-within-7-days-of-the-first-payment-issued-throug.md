---
title: "Story 10.15: Full refund within 7 days of the first payment, issued through the ops path"
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: 'ca70215614db83807271597b1eb74b2a202f09a4'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-14-cancel-keeps-the-plan-to-the-end-of-the-period-then-the-bran.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** An owner who regrets a paid plan has no way to get the first payment back, and Arena 360 has no audited way to give it.

**Approach:** Inside 7 days of the first payment the cancel panel offers "Cancel and get a full refund". After step-up MFA a `refund_requests` row is written and nothing else moves. An operator runs `arena-cloud ops refund approve|reject`; approving refunds every payment at Razorpay and cancels the subscription at once. `refund.processed` then issues a credit note per invoice, makes the subscription `refunded` and the branch Free.

## Boundaries & Constraints

**Always:** step-up before a request is written; cloud time for the window; whole amount only; one request per subscription; every operator decision writes `ops_audit` in the same transaction; each payment refunded at most once (retry-safe); credit notes `A360/<FY>/CN<seq:06>` in their own gap-free sequence, in the transaction that records `refund.processed`.

**Never:** no partial refund; no change to the licence or heartbeat wire shapes; no Hub-side handling (Stories 10.12, 10.13 cover the Free licence); no admin UI (Epic 11).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| No fresh step-up | refund request | `MFA_REQUIRED`, nothing written |
| Request | `active`, inside 7 days, whole amount | 204; row `requested`; plan, Razorpay and licence untouched; card "Refund requested — usually processed within 2 working days" |
| Outside the window, second request, part of the amount, pending plan change | request | `REFUND_NOT_ALLOWED`; the offer is gone after 7 days |
| Stale `rev` | request | `STALE` |
| Approve | `requested` | every payment refunded, subscription cancelled at once, request `issued`, `ops_audit` |
| Razorpay error | approve | request stays `requested` with `last_error`; the retry skips payments already refunded |
| `refund.processed` (not the last payment) | one payment | payment processed, one credit note |
| `refund.processed` (last payment) | | subscription `refunded`, entitlement `free`, request `processed`, one `refund_processed` alert |
| Repeat / unknown payment | event | recorded, nothing else |
| Reject | `requested` | request `rejected`, `ops_audit`, owner reads the reason, plan stays `active` |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0043_subscription_refund.sql` (`refund_requests`, `refund_payments`, `billing_invoices.payment_id`), `src/control/refund.rs`, `src/control/invoice.rs` (credit-note issue, `invoices_of_subscription`, `invoice_of_payment`), `src/control/billing.rs` (`event_stmt`).
- `crates/arena-cloud/src/refund.rs` (offer, request, approve, reject, `handle_processed`, notify), `billing.rs` (`refund_payment` on fake and Razorpay, `PaymentInfo.id`), `webhook.rs` (`refund.processed` parse and dispatch, `payment_id` on invoices), `invoice.rs` (credit-note PDF), `ops.rs` / `config.rs` / `main.rs` (`ops refund`), `api/subscription.rs` (route, refund guard), `account_plan.rs` (`refund_offer`, `refund`), `owner_alerts.rs`.
- `crates/arena-proto` (`RequestRefund`, `RefundOffer`, `RefundView`, `RefundStatus`, `ErrorCode::RefundNotAllowed`, `OwnerAlertKind::RefundProcessed`, generated TS).
- `apps/owner/src/lib/{CancelPlan.svelte,SubscriptionCard.svelte,subscription.ts}`, `packages/ui/src/{components/Button.svelte,messages/en.json}`.
- Tests: `crates/arena-cloud/tests/billing_events.rs` (Story 10.15 section), `tests/config.rs`, `tests/backup.rs`, `apps/owner/tests/refund.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0043 and the store: requests, per-payment refunds, credit-note issue, payment ids on invoices
- [x] Cloud: offer and request with step-up, approve (retry-safe) and reject with audit, `refund.processed` handling
- [x] Razorpay adapter refund call (fake, real, manual)
- [x] Credit note PDF, no "Invoice ready" alert for it; "Refund processed" owner alert
- [x] `arena-cloud ops refund approve|reject`
- [x] Owner app: refund offer in the cancel panel, status lines on the card
- [x] Tests for every I/O row; README

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-api -p arena-kiosk-core --all-targets --features arena-store/control -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud --features arena-store/control` -- all pass except baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`
- `pnpm --filter @arena/owner check`; `playwright test tests/refund.spec.ts tests/cancel.spec.ts` pass
- `pnpm run check:strings`, `check:hex`, `check:tier-lock`, `check:secrets`; `check:generated` passes once the regenerated files are committed

## Auto Run Result

Status: in-review. Risks to know: the real Razorpay refund call (`POST /payments/{id}/refund` with the whole amount) is exercised against the fake only; a refund request needs every invoice of the subscription to name its payment (set from the charge event from this story on), so an older invoice without a payment id makes the plan ineligible for the owner's offer and `ops refund approve` refuses it (`NO_PAYMENT`) for a manual refund; a `subscription.cancelled` webhook is still not acted on; `RefundProcessed` is raised for a Free branch on purpose (the refund itself leaves it Free); the disk filled during verification, so `target/debug/incremental` was deleted and tests ran with `CARGO_INCREMENTAL=0`.

### Review Findings

Code review (2026-10-31): no blocking findings. fmt, clippy `-D warnings`, cargo tests (only the baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token` fails), owner svelte-check, string/hex/generated checks and the refund/cancel Playwright specs all pass.

- [x] [Review][Defer] If `refund.processed` for the last payment arrives after the subscription is already `refunded`, the request is not moved to `processed` (the completion statements are skipped) [crates/arena-cloud/src/refund.rs:~540] — deferred: unreachable today (only this handler sets `refunded`); settle if another path ever does.
