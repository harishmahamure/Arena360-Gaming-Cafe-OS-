---
title: "Story 10.10: Switch billing cycle and payment method, with card or eNACH for renewals over Rs 15,000"
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '83cee7412ced848bb0ab9d11d0010e94780e6bda'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-9-change-tier-and-add-ons-mid-cycle-with-proration.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** An owner cannot move a paid branch between monthly and yearly billing, or change the card or bank mandate that pays for it.

**Approach:** Razorpay cannot change a subscription's period, so a cycle switch and a new payment method are each a new Razorpay subscription the owner authorises in Checkout, stored as a held `subscription_changes` row (`subject` `cycle` or `method`) on the same `subscriptions` row. The webhook applies it on authorisation or first charge and retires the old subscription.

## Boundaries & Constraints

**Always:** amounts from the subscription's locked price book; yearly = 10x monthly + GST on band IN; credit = unused days of the paid month, GST on the net; nothing changes (or is charged) until the owner authorises; the old subscription is ended only after that; Checkout methods come from `allowed_methods` for the new renewal total; a stale `rev` fails `STALE` before any provider call; only `active` subscriptions change.

**Never:** no change to the licence or heartbeat wire shapes; no entitlement write for a cycle or method change; no UPI handle read or stored; no cancel or refund (10.14, 10.15); no grace/lapse handling (10.11).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Monthly to yearly | active monthly, 30 of 31 days left | summary: yearly price, credit, net today, renewal a year out; Checkout; first charge applies cycle, invoices the net, cancels the old subscription |
| Yearly to monthly | active yearly | `Switches to monthly on <date>. No partial refunds.`; authorising ends the old one with its cycle; first monthly charge applies it |
| Over the UPI limit | UPI mandate, new renewal over Rs 15,000 | Checkout offers card and eNACH only; cancelling Checkout leaves the old cycle |
| Change payment method | any active subscription | Checkout with the methods the renewal allows; authorising replaces the mandate, old cancelled after |
| Abandoned or failed | Checkout closed | previous method stays; card says `Payment method unchanged` |
| Replaced subscription webhook | event on the old Razorpay id after a switch | recorded in `billing_events`, state unchanged |
| Two devices | second change on an old `rev` | `STALE` |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0040_subscription_change_subject.sql`, `src/control/plan_change.rs` (`ChangeSubject`, `cycle`, `authorise_only`, lookup includes `charging`).
- `crates/arena-cloud/src/cycle_change.rs` (`plan_cycle`, `confirm_cycle`, `confirm_method`), `plan_change.rs` (`completion`, `gate_state`, `with_change`, `cycle_line`), `webhook.rs` (`handle`), `billing.rs` (`cancel_at_cycle_end`), `api/subscription.rs`, `api/webhook.rs` (`retire`).
- `crates/arena-proto/src/subscription.rs` (+ generated TS).
- `apps/owner/src/lib/{SwitchCycle.svelte,SubscriptionCard.svelte,subscription.ts}`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-cloud/tests/billing_events.rs`, `plan_change.rs` unit tests, `apps/owner/tests/cycle-change.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0040 and store reads/writes for subject and cycle
- [x] Cycle switch preview/confirm and payment-method routes
- [x] Webhook completion: cycle, mandate replacement, authorisation of a switch to monthly, invoice, retire old subscription
- [x] Provider: end a subscription with its paid cycle (fake and real)
- [x] Owner UI: Switch to yearly/monthly, Change payment method, pending-change wording, Keep current plan rules
- [x] Tests for every I/O row and README section

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud --all-targets -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud` (`--features arena-store/control`)
- `pnpm --filter @arena/owner check`; `npx playwright test` in `apps/owner`
- `pnpm run check:strings`, `check:generated` (after the generated files are committed), `check:hex`, `check:tier-lock`, `check:secrets`

## Auto Run Result

Status: review. fmt and clippy clean; `cargo test` passes except the baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`. `svelte-check` clean; `cycle-change.spec.ts` (7 tests, desktop and phone), `plan-change.spec.ts` and `subscription.spec.ts` pass.

Risks to know: the real Razorpay `addons` upfront charge on a future-start subscription and `cancel_at_cycle_end` are exercised against the fake only; the method and card digits of a new mandate come from the authorising event's payment when present; changing the method in `renewal_needs_approval`/`grace` is left to Story 10.11; a yearly-to-monthly switch whose `authenticated` event is missed is applied by the first monthly charge, which then cancels the old subscription at once.
