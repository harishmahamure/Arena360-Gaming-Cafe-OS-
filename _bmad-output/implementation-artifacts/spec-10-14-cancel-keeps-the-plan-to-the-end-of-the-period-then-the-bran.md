---
title: "Story 10.14: Cancel keeps the plan to the end of the period, then the branch drops to Free"
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '032a41d0c41534f04e7004f9ab1344c3eb994a5a'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-11-renewals-that-need-approval-go-to-grace-then-lapse-on-cloud.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** An owner cannot cancel a paid branch plan, so a plan they no longer want keeps renewing.

**Approach:** Cancel sets `cancel_at_period_end` and tells Razorpay to stop at cycle end; nothing else moves. The jobs sweep ends the subscription (`ended`, no grace) when the period passes on cloud time and the entitlement becomes Free. "Keep my plan" withdraws it.

## Boundaries & Constraints

**Always:** step-up before a cancel completes; entitlement and licence unchanged until the period end; stale `rev` fails `STALE` before any provider call; every move writes `subscription_history`; a comp entitlement is not overwritten.

**Never:** no change to the licence or heartbeat wire shapes; no partial refund; no full refund path (10.15); no Hub-side handling (10.12, 10.13).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| No fresh step-up | cancel on `active` | `MFA_REQUIRED`, nothing written or sent |
| Cancel | step-up fresh, matching `rev` | 204; Razorpay told cycle-end; flag set; card "Cancels on <date>"; licence unchanged |
| Keep my plan | flagged, before the end | Razorpay cancellation withdrawn; flag cleared; same renewal date |
| Period end | sweep at `current_period_end` | `ended`, entitlement `free`, next heartbeat Free licence, card "Lapsed — Free limits" + Resubscribe |
| Unpaid renewal / pending change / stale `rev` | cancel | refused / `STALE` |
| Resubscribe | after `ended` | new subscription (migration 0042 frees the open slot) |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0042_subscription_cancel.sql`, `src/control/billing.rs` (`Lifecycle.cancel_at_period_end`, `lifecycle_due`), `src/control/subscription.rs` (`OPEN_STATES`).
- `crates/arena-cloud/src/cancel.rs` (`cancel`, `keep`), `api/subscription.rs` (routes, step-up gate), `billing.rs` (`keep_after_cancel`, fake `kept()`), `lifecycle.rs` (`Step::End`), `account_plan.rs` (`ended` reads Lapsed).
- `crates/arena-proto/src/subscription.rs` (`CancelPlan`, + generated TS).
- `apps/owner/src/lib/{CancelPlan.svelte,SubscriptionCard.svelte,subscription.ts}`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-cloud/tests/billing_events.rs` (Story 10.14 section), `lifecycle.rs` unit test, `apps/owner/tests/cancel.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0042; store flag write and sweep query
- [x] Cloud cancel/keep with step-up, provider cancel and withdraw (fake and real)
- [x] Sweep ends cancelled subscriptions to `ended`/Free
- [x] Owner app: cancel panel, Keep my plan, Lapsed line and Resubscribe
- [x] Tests for every I/O row; README

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud --all-targets --features arena-store/control -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud --features arena-store/control` -- all pass except baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`
- `pnpm --filter @arena/owner check`; owner Playwright suite passes (one `replace-hub` run flaked and passed on rerun)
- `pnpm run check:strings`, `check:hex`, `check:tier-lock`, `check:secrets`; `check:generated` passes once the regenerated files are committed

## Auto Run Result

Status: in-review. Risks to know: Razorpay's withdrawal of a cycle-end cancel (`POST /subscriptions/{id}/cancel_scheduled_changes`) is exercised against the fake only; a cancel inside 7 days of the first payment is accepted here (Story 10.15 adds the refund route and panel); a `subscription.cancelled` webhook is not acted on (the sweep ends the plan on cloud time); the "Resubscribe" button reuses the Upgrade link to the wizard checkout.

## Review Result

Reviewed 2026-10-31. Fixed: the real Razorpay withdraw call used a non-existent path (`cancel_update`); it now uses the documented `cancel_scheduled_changes`. Still unverified against live Razorpay: whether that endpoint also withdraws a cycle-end subscription cancel (tested against the fake only). Clippy, fmt and arena-cloud tests green apart from known-flaky `backup` timing and baseline `binary` tests.
