---
title: "Story 10.9: Change tier and add-ons mid-cycle with proration"
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '2684c60d'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-8-account-subscription-shows-each-branch-s-plan-invoices-and-p.md'
warnings: []
deferred:
  - "Only the changed items should re-price against the live book; the whole change is priced from the subscription's locked book."
  - "A held change applies on the new subscription's first charge only; the reconcile job does not read held changes."
---

<intent-contract>

## Intent

**Problem:** An owner cannot move a paid branch up or down a tier, or add and remove add-ons, after paying.

**Approach:** A stored `subscription_changes` row per change; an upgrade is prorated and paid through Razorpay and applied by the charge webhook, a downgrade is scheduled for the period end, and an upgrade over the UPI limit is held until a card or eNACH mandate exists. The Owner card gets Change plan, a summary, and Keep current plan.

## Boundaries & Constraints

**Always:** amounts from the subscription's locked price book; `(new daily rate - old daily rate) x days left` in minor units with GST on band IN; the entitlement changes only when the charge is confirmed (upgrade) or the period ends (downgrade); a second change on a stale `rev` fails `STALE` before any provider call; only `active` subscriptions change.

**Never:** no cycle switch or payment method change (10.10); no cancel or refund (10.14, 10.15); no change to the licence or heartbeat wire shapes; no UPI handle read or stored.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Upgrade | Basic to Premium, 30 of 31 days left | prorated charge + GST, new renewal total; entitlement changes on the charge webhook; prorated invoice |
| Downgrade | 2 packs to 1 | `Changes on <date>. No partial refunds.`; entitlement unchanged until the renewal charge |
| Keep current plan | scheduled or held change | provider moved back or mandate cancelled; card has no pending change |
| Over the UPI limit | UPI mandate, renewal over Rs 15,000 | held; Checkout (card, eNACH) for a new subscription; nothing charged until it exists |
| Unsettled renewal | renewal_needs_approval, grace, pending_review | `Settle the pending renewal first` |
| Two devices | second change on an old `rev` | `STALE`, owner sees the current plan |
| Tunnel | Premium / Android Hub | `Included` / `Windows Hub only` |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0039_subscription_changes.sql`, `src/control/plan_change.rs`, `billing.rs` (`SubscriptionChange.plan_change`).
- `crates/arena-cloud/src/plan_change.rs` (proration, plan, confirm, withdraw, completion), `billing.rs` (`change_plan`, `create_mandate`, fake), `webhook.rs` (completion in `handle`), `invoice.rs` (`for_proration`), `account_plan.rs`, `api/subscription.rs`, `api/webhook.rs`.
- `crates/arena-proto/src/subscription.rs` (+ generated TS).
- `apps/owner/src/lib/{ChangePlan.svelte,SubscriptionCard.svelte,subscription.ts,razorpay.ts}`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-cloud/tests/billing_events.rs`, `plan_change.rs` unit tests, `apps/owner/tests/plan-change.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0039 and store reads/writes for changes
- [x] Proration, direction, hold rules and the preview/confirm/withdraw routes
- [x] Razorpay adapter: plan change and mandate subscription (fake and real)
- [x] Webhook completion: entitlement, tier/add-ons, prorated invoice, ops mismatch task, replaced subscription cancelled
- [x] Owner UI: Change plan, summary, pending change with Keep current plan, Checkout for a new mandate
- [x] Tests for every I/O row and README section

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud --all-targets -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud` (`--features control` for arena-store)
- `pnpm --filter @arena/owner check`; `npx playwright test` in `apps/owner`
- `pnpm run check:strings`, `check:generated` (after the generated files are committed), `check:hex`, `check:tier-lock`, `check:secrets`

## Auto Run Result

Status: review. fmt and clippy clean; `cargo test` passes except the baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token` and the Windows-flaky `backup::a_hung_run_times_out_with_its_own_code`. `svelte-check` clean; the Owner Playwright suite passes (746 tests, desktop and phone) apart from `shell.spec.ts` "a refused session goes to sign-in and clears the cache", which failed once in the full run and passes alone (also on the baseline).

Risks to know: the real Razorpay `PATCH /subscriptions/{id}` proration and the `addons` upfront charge of a mandate subscription are exercised against the fake only; a held change completes on the new subscription's `subscription.charged` event only; pricing is from the locked book (changed items are not re-priced against the live book).

## Code Review Result

Status: done. No blocking findings. fmt, clippy, `cargo test` (arena-proto, arena-store, arena-cloud), `svelte-check`, the string/hex/tier-lock/secrets/generated checks and `plan-change.spec.ts` (18 tests) pass; the only failing test is the baseline `binary::unreachable_at_start...`.

Non-blocking, left as follow-ups:
- A change stuck in `charging` (the prorated charge never confirmed) cannot be kept out and blocks a new change until the next renewal charge completes it.
- A held change's mandate completion records the `cancel_subscription` ops task, so a proration amount mismatch on that path is only logged (one ops task per event).
