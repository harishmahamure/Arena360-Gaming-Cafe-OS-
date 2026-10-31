---
title: "Story 10.11: Renewals that need approval go to grace, then lapse, on cloud time"
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '329f8e8c'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-10-switch-billing-cycle-and-payment-method-with-card-or-enach-f.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** A renewal that needs the bank's approval, or whose charge fails, has no life in the cloud: the subscription stays `active`, the owner is never warned and a branch never lapses.

**Approach:** `active -> renewal_needs_approval -> grace -> lapsed` (AD-20) driven by Razorpay events and a 5-minute jobs sweep on cloud time. The entitlement is untouched until `lapsed`, when it becomes the Free tier; the next heartbeat carries a Free licence. The owner is told by web push and WhatsApp, and the Owner app shows a persistent banner.

## Boundaries & Constraints

**Always:** `grace_until` = period end + 7 days on cloud time; the cloud keeps signing the paid licence in grace; a charge for a later period returns the subscription to `active` with the period Razorpay reports (counted from the original renewal date); every move writes `subscription_history`; alert ids are deterministic so nothing is sent twice; `subscriptions_per_state` is logged each run; a staff `comp` entitlement is never overwritten by a lapse.

**Never:** no change to the licence or heartbeat wire shapes; nothing the Hub shows staff before the lapse applies; no Hub-side handling of the lower licence (10.12, 10.13); no cancel or refund (10.14, 10.15).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Needs approval | `subscription.renewal_needs_approval` on `active` | `renewal_needs_approval`, entitlement unchanged, alert "Your <branch> renewal needs approval" |
| Not approved by period end | sweep at `current_period_end` | `grace`, `grace_until` = end + 7 d |
| Charge failed after period end | `subscription.pending` | `grace` at once |
| Charge failed before period end | `subscription.pending` | marked (`renewal_failed_at`); sweep moves it at period end |
| Grace warnings | sweep on grace days 1-4, 5-6, 7 | one alert each for day 1, 5, 7; repeats send nothing |
| Paid in grace | `subscription.charged` for a later period | `active`, `grace_until` cleared, no lapse |
| Grace ends | sweep at `grace_until`, or `subscription.halted` | `lapsed`, entitlement `free`, next heartbeat Free licence |
| Long-dead period | sweep days after grace end | `grace` then `lapsed` in one run |
| New method in grace | Change payment method | open; Change plan stays "Settle the pending renewal first"; mandate authorised then its charge settles the renewal |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0041_subscription_lifecycle.sql` (`renewal_failed_at`; `grace_until` is from 0035), `src/control/billing.rs` (`Lifecycle`, `lifecycle_due`, `subscriptions_awaiting_payment`, `subscription_state_counts`), `plan_change.rs` (`propose_plan_change` lets a method change through in the unpaid states).
- `crates/arena-cloud/src/lifecycle.rs` (grace maths, `next_step`, `notice_for`, `sweep_once`, `run_job`), `webhook.rs` (`decide_lifecycle`, completion of a new mandate in the unpaid states), `plan_change.rs` (`gate_method`), `cycle_change.rs`, `api/webhook.rs` (alert on entering the state), `account_plan.rs`, `config.rs`/`backup.rs` (task `subscription-lifecycle`).
- `crates/arena-proto/src/{owner_alert,subscription}.rs` (+ generated TS): `RenewalNeedsApproval`, `RenewalGrace`, `BranchPlan.grace_until`.
- `apps/owner/src/lib/{RenewalBanner.svelte,SubscriptionCard.svelte,shell.svelte.ts,subscription.ts}`, `routes/+layout.svelte`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-cloud/tests/billing_events.rs` (Story 10.11 section), `lifecycle.rs` unit tests, `tests/config.rs`, `apps/owner/tests/renewal.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0041 (`grace_until` was already added by 0035) and store reads/writes for grace, failed-renewal mark and lapse
- [x] Webhook/reconcile decisions: needs approval, charge failed, halted; a paid renewal returns to `active`
- [x] `subscription-lifecycle` jobs task: grace, lapse, owner alerts on days 1/5/7, per-state counts
- [x] Owner alerts `renewal_needs_approval` and `renewal_grace` (push + WhatsApp)
- [x] Payment-method change open in `renewal_needs_approval` and `grace`
- [x] Owner app: persistent banner on every screen, card sentence, "Change payment method" while unpaid
- [x] Tests for every I/O row, README and deploy notes

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud --all-targets -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud --features arena-store/control`
- `pnpm --filter @arena/owner check`; `npx playwright test` in `apps/owner`
- `pnpm run check:strings`, `check:generated` (after the generated files are committed), `check:hex`, `check:tier-lock`, `check:secrets`

## Auto Run Result

Status: review. fmt and clippy clean; `cargo test` passes except the baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token` and the timing-sensitive `backup::a_hung_run_times_out_with_its_own_code` (passes alone). `svelte-check` clean; the whole owner Playwright suite passes (new `renewal.spec.ts`; `shell.spec.ts` now expects the shell's extra `/account/subscription` read).

Risks to know: Razorpay has no hosted "approve" call, so the banner's "Approve" leads to the branch's card, which explains that the bank approves the debit and offers the new-method route; the real event names for a bank-approval renewal (`subscription.renewal_needs_approval`) and a failed charge (`subscription.pending`) are exercised against the fake only; an `active` subscription whose period passes with no `pending` event is not moved (the webhook or reconcile must report the failure); the reconcile does not raise alerts (the lifecycle sweep does, within 5 minutes); `check:generated` passes only once the regenerated files are committed.
