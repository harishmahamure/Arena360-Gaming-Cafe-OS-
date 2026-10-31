---
title: 'Story 10.3: Razorpay webhooks are verified, applied once, and issue the entitlement'
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '58ad36dc7104aeadd41a2021501995dcfd3564d2'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-2-a-branch-subscription-is-created-at-a-ppp-band-that-is-locke.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A branch can be subscribed (Story 10.2) but nothing hears that it was paid: no Razorpay event is verified, applied or remembered, so a paid branch stays Free, a repeated delivery could apply twice and a webhook that never arrives is never noticed.

**Approach:** `POST /v1/webhooks/razorpay` verifies the HMAC over the raw body, then records the event and applies its transition in one transaction keyed by the unique Razorpay event id. Activation writes the branch's `paid` entitlement in that same transaction, so the next heartbeat's licence carries it. A jobs-role reconcile reads unsettled subscriptions from Razorpay and feeds the same code path. A first payment from the wrong country is held for an operator.

## Boundaries & Constraints

**Always:** the signature is verified against `ARENA_RAZORPAY_WEBHOOK_SECRET` before the body is parsed; a bad one answers 400, is counted as a webhook failure and writes nothing; an event and its change commit together or not at all; a repeated Razorpay event id answers 200 and applies nothing; an event for a period no later than the one held never moves the period back or lifts `grace`; every applied change writes a `subscription_history` row (old state, new state, cause = event id or job, cloud time); the entitlement row changes (source `paid`, replacing `free` or `comp`) and the cloud never posts a command to change gates.

**Never:** no grace, lapse, downgrade or cancel handling (10.11-10.14); no refund or invoice (10.7, 10.15); no checkout UI (10.4); no tier change or proration (10.9); no reject path for a review (refund belongs to 10.15); no secret, signature or provider body in an error or log.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Bad signature | wrong secret, missing, not hex, body tampered after signing | 400 `BAD_REQUEST`, nothing written | `webhook_failures` +1 |
| Not an event | signed non-JSON body, or no `X-Razorpay-Event-Id` | 400, nothing written | `webhook_failures` +1 |
| Authenticated then charged | `pending` subscription | `active`, period from Razorpay, `first_paid_at`, entitlement `paid` with tier and add-ons | none |
| Charged then authenticated | same, reversed | same end state; `authenticated` alone changes nothing | none |
| Activated alone | `pending` | `active`, entitlement; a later charge dates `first_paid_at` | none |
| Repeat | same event id, even another body | 200 `duplicate`, no new rows | none |
| Two at once | same event id in parallel | exactly one applies | unique id decides |
| Foreign first payment | band IN, card `international` (or country US) | `pending_review`, no entitlement, `first_paid_at` set, open `subscription_review` ops task, owner text from `GET .../subscription` | alert post to `ARENA_ALERT_WEBHOOK` if set |
| Review cleared | `ops subscription review --approve` | `active`, entitlement, `ops_audit` row, task closed, history cause `ops:review:<operator>` | `NOT_IN_REVIEW`, `UNKNOWN_SUBSCRIPTION` |
| Renewal | `grace` or `renewal_needs_approval`, later `current_end` | `active`, period forward | none |
| Old renewal | `current_end` not later than held | recorded, nothing changes | none |
| Unknown subscription | Razorpay id we do not hold | recorded with no subscription, 200 `unmatched` | none |
| Missed webhook | provider says `active`, paid | reconcile applies it with source `reconcile`; same state next run is a no-op | provider failure leaves it for the next run |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0036_billing_events.sql` -- `billing_events` (unique `razorpay_event_id`), `subscription_history`, `subscriptions.payment_country`; registered in `control.rs`.
- `crates/arena-store/src/control/billing.rs` -- `apply_billing_event` (event + change in one transaction; `Duplicate` on the unique id, `Conflict` on a stale `rev`), `apply_subscription_change` (operator path), history and event reads, `subscription_by_razorpay_id`, `subscriptions_to_reconcile`. `subscription.rs` gains `payment_country`.
- `crates/arena-cloud/src/webhook.rs` -- `WebhookSecret` (sign/verify, constant time), `WebhookMetrics`, `parse`, `from_remote`, pure `decide`, `handle`, `approve_review`, `alert_ops`; `api/webhook.rs` is the route.
- `crates/arena-cloud/src/billing.rs` -- `PaymentOrigin`/`PaymentInfo`/`RemoteSubscription`, `payment_info_from`, `fetch_subscription` on the fake (`set_remote`), the Razorpay client (subscription, latest invoice, payment) and `Manual`.
- `crates/arena-cloud/src/reconcile.rs` -- `billing-reconcile` jobs task; `config.rs`, `backup.rs` register it.
- `crates/arena-cloud/src/ops.rs`, `config.rs`, `main.rs` -- `ops subscription review --id --approve --reason [--operator]`.
- `crates/arena-cloud/src/api/subscription.rs` -- `GET /v1/branches/{id}/subscription` (`SubscriptionStatus`, review message); `arena-proto` type and generated TS.
- `crates/arena-cloud/src/licence.rs` -- `claims_for` applies the entitlement's add-ons (`devices_20:<n>` raises the device limit by 20 a pack, `tunnel` adds the flag).
- `deploy/README.md` -- webhook secret, review command, reconcile schedule.

## Tasks & Acceptance

**Execution:**
- [x] `0036_billing_events.sql` + registration, store `billing.rs` -- AC1, AC3, AC9
- [x] `webhook.rs` signature, parse, decide, handle; `POST /v1/webhooks/razorpay`; auth state secret and metrics -- AC2, AC3, AC4, AC6, AC9
- [x] Country check, `pending_review`, ops task, `ops subscription review`, owner status text -- AC5
- [x] Reconcile job and provider fetch (fake, Razorpay, manual) -- AC7
- [x] Licence add-ons in claims; heartbeat proof; no command posted -- AC8
- [x] Tests: `tests/billing_events.rs`, `webhook.rs`/`billing.rs` unit tests, `tests/config.rs`, `tests/licence.rs`, `tests/backup.rs` and `arena-store` table lists

**Acceptance Criteria:**
- Given `gz_control`, when the migration runs, then `billing_events` and `subscription_history` exist with the story's columns.
- Given `POST /v1/webhooks/razorpay` with a bad signature, then it answers 400, counts a webhook failure and writes nothing.
- Given a verified event, when it is handled, then it is recorded and applied in one transaction and a repeated event id answers 200 and applies nothing.
- Given `authenticated` and `charged` (or `activated`) for a `pending` subscription in any order, then it is `active` with Razorpay's period, `first_paid_at` is set on the first charge and the entitlement becomes `paid` with tier and add-ons.
- Given a first payment whose country does not match the band, then the subscription is `pending_review`, no entitlement is issued, an ops task is open and the owner reads the checking-your-payment text; the audited review command activates it through the same write.
- Given a later renewal charge, then the period moves forward and `renewal_needs_approval` or `grace` return to `active`; an older event never moves the period back.
- Given a webhook that never arrives, when the 10-minute reconcile runs, then it applies the same transitions through the same path with source `reconcile`.
- Given the next heartbeat after activation, then the licence carries the paid claims and no command was posted.
- Given every transition, then a history row records old state, new state, cause and cloud time, and handling time and failures appear in the webhook metrics.

## Spec Change Log

## Review Triage Log

## Design Notes

The decision (`decide`) is a pure function of the subscription row and the parsed event; `handle` reads the row, decides, and writes event + change guarded by the row's `rev`, retrying when it moved. A duplicate is detected by the unique event id inside that transaction, so two parallel deliveries cannot both apply. An event with no matching subscription is still recorded (a repeat is then a no-op); the reconcile covers a payment whose row we did not hold yet.

The payment country is read from the payment entity: UPI, bank mandates and net banking are India-only, a card carries `card.country` when Razorpay sends one, otherwise `card.international`. A card Razorpay calls international cannot be placed, so it fits any band but IN. Anything Razorpay does not say counts as a match: only a payment known to be from elsewhere is held. The first payment's origin is kept on the row so a later event without a payment is still checked.

The entitlement keeps `until` empty: a paid branch stays entitled until lapse handling (10.11) says otherwise, so a late webhook cannot drop a branch to Free. `entitlements.issued_at` keeps its first value across renewals. Add-ons are stored as `devices_20:<n>` and `tunnel`. The wizard's `tier` step is marked done in the activation transaction, as `grant-comp` does, so Story 10.4 only has to observe it.

## Verification

**Commands:**
- `cargo test -p arena-cloud --test billing_events --test config --test licence --test backup`, `cargo test -p arena-cloud --lib`, `cargo test -p arena-proto`, `cargo test -p arena-store --features control --test control` -- expected: pass
- `cargo fmt --all --check`, `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-api -p arena-kiosk-core --all-targets -- -D warnings` -- expected: clean

## Auto Run Result

Status: review (implemented and verified; the code review is the next pass)

Implemented: migration 0036 (`billing_events`, `subscription_history`, `subscriptions.payment_country`), store module with the atomic event+change write, `POST /v1/webhooks/razorpay`, `WebhookSecret` verification, `decide`/`handle`, country review with `ops subscription review`, `GET /v1/branches/{id}/subscription`, the `billing-reconcile` jobs task and provider fetch, add-on-aware licence claims, webhook metrics, deploy README note, regenerated TS (`SubscriptionStatus`).

Verification: `cargo test -p arena-cloud` (lib 150, billing_events 16, config 26, licence 13, backup 22, others green), `arena-proto` (363), `arena-store --features control` (control 24, lib 215); `cargo fmt --check` and clippy `-D warnings` clean. Known unrelated failure: `binary.rs::unreachable_at_start_retries_then_exits_1_without_logging_the_token` (Windows connect timeout, listed in spec 10.1). `pnpm check:generated` reports the new generated files only because they are uncommitted; `check:deploy` needs `kubectl`; `format:check` flags two untouched scripts.

Residual risks: the real Razorpay client's subscription/invoice/payment reads have no HTTP-stub test; Razorpay does not expose a card issuer country on every plan, so a card without `country` is judged on `international` alone; the ops alert post is best effort (the ops task row is the record); the webhook metrics are in process and logged, their OpenTelemetry export waits for the Story 2.3 exporter; an event for an `abandoned` subscription (paid after the 24 h sweep) is recorded but grants nothing and needs an operator.
