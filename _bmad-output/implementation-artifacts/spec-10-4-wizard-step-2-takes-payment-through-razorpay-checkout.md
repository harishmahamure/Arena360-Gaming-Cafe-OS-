---
title: 'Story 10.4: Wizard step 2 takes payment through Razorpay Checkout'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '731bca67228e469ba46b8c98b6dc2d616b533d7b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-2-a-branch-subscription-is-created-at-a-ppp-band-that-is-locke.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-3-razorpay-webhooks-are-verified-applied-once-and-issue-the-en.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Wizard step 2 still disables Basic and Premium: an owner cannot choose a cycle or add-ons, see what they will be charged, open Razorpay Checkout, or have the wizard move on once the Story 10.3 webhook writes the entitlement.

**Approach:** With billing on, the tier step lets the owner pick a paid plan, cycle and add-ons, shows a server-made summary, opens Checkout with the Story 10.2 parameters, then waits ("Confirming your payment…") for the wizard to leave step 2, by server-sent events with a 5 s polling fallback and a 2-minute message. A closed or failed Checkout changes nothing and retries the same subscription.

## Boundaries & Constraints

**Always:** the amounts on the summary come from the cloud (the branch's live price book, GST on band IN only, never from the browser); the band comes only from the branch country; a retry of an unpaid `pending` subscription with the same plan, cycle and add-ons answers the same Checkout parameters, a different choice cancels it at the provider and starts a new one; only the allowed methods are offered, never wallet, netbanking, pay-later or EMI; step 2 is marked done only by the 10.3 entitlement write, never by the browser; the announcement of step 3 is made once and focus lands on its heading; Free keeps its one-click Continue.

**Never:** no upgrade-link entry points or Add branch (10.5); no licence delivery or plan display on the Hub (10.6); no invoices (10.7); no tier change or proration (10.9); no new Owner-login purchase (quantity stays 0, shown "Available soon"); no secret or provider body in an error or log; no change to the 10.3 webhook transitions.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pick paid, billing on | Basic, band IN | Continue enabled; price in INR with "+ 18% GST"; yearly "2 months free"; add-ons panel | none |
| Band off sale | country has no band or band off | paid cards disabled with the band-unavailable reason; Free still selectable | none |
| Summary | tier, cycle, add-ons | server quote: subtotal, GST 18% (IN), total, renewal date, refund line, "Pay and continue" | `BAND_NOT_AVAILABLE` text |
| Pay | summary | subscription created, Checkout opens with key, subscription id and allowed methods | 503 shows generic error, nothing charged |
| Closed Checkout | dismissed | back to the summary, nothing changed | none |
| Declined | `payment.failed` | summary with "Payment didn't go through — nothing was charged. Try again or use another method."; Pay again reuses the same pending subscription | none |
| Paid | handler returns | "Confirming your payment…"; when the wizard leaves step 2, step 3 opens, heading focused, announced once | none |
| Slow | no change in 2 min | "Your payment is still being confirmed. We'll move you on as soon as it lands."; still advances later | none |
| Retry, other choice | pending row, different plan | old one cancelled at the provider and abandoned, new checkout | provider failure 503, old row kept |
| Paid via PUT | billing on, `PUT /wizard/tier` paid | refused | 400 validation |

</intent-contract>

## Code Map

- `crates/arena-proto/src/tier.rs`, `subscription.rs` -- `TierChoices.band`, `TierUnavailable::BandNotAvailable`; new `SubscriptionQuote`; regenerate `packages/ui/src/generated`.
- `crates/arena-cloud/src/api/wizard_tier.rs` -- `tier_choices` band and reasons; paid `PUT` refusal when billing is on.
- `crates/arena-cloud/src/subscription.rs` -- `create` retry/replace of a `pending` row, `checkout_of`, `make_quote` (renewal date by chrono).
- `crates/arena-cloud/src/api/subscription.rs` -- `POST /branches/{id}/subscription/quote`.
- `crates/arena-cloud/src/api/webhook.rs` -- send the branch id on `wizard_tx` after an applied event (SSE wake-up).
- `apps/onboarding/src/lib/TierStep.svelte` (+ `AddOnsPanel.svelte`, `CheckoutSummary.svelte`, `razorpay.ts`, `api.ts`, `wizard.svelte.ts`, `routes/setup/+layout.svelte`) -- picker, summary, Checkout loader, confirming/slow states, streaming flag.
- `packages/ui/src/messages/en.json`, `apps/onboarding/tests/stub.ts`, `tests/tier.spec.ts`, `tests/checkout.spec.ts`, `crates/arena-cloud/tests/{subscription,tier,webhook-related}.rs` -- tests.
- Read-only: `apps/onboarding/src/lib/InstallHubStep.svelte` (already Windows-first for paid, Android link kept), `packages/ui/src/pricing.ts` (snapshot lines per band).

## Tasks & Acceptance

**Execution:**
- [x] proto `TierChoices.band`, `BandNotAvailable`, `SubscriptionQuote`, TS regeneration -- AC1, AC2
- [x] `wizard_tier.rs`, `subscription.rs`, `api/subscription.rs`, `api/webhook.rs` backend changes -- AC1-AC6
- [x] onboarding picker, add-ons panel, summary, Checkout loader, confirming/slow states, messages -- AC1-AC7
- [x] Tests: cloud `subscription`/`tier` integration tests, onboarding Playwright specs with a stubbed `window.Razorpay`

**Acceptance Criteria:**
- Given step 2 with billing on and the band on sale, when Basic or Premium is picked, then Continue is enabled, the card shows the selected cycle's price in the band currency with "+ 18% GST" on IN only and "2 months free" on yearly, and the Add-ons panel offers +20 devices 0-10, Tunnel (Basic only, "Included" on Premium) and Owner login ("Available soon", not selectable).
- Given Continue, then the summary shows tier, cycle, add-ons, subtotal, GST, total, renewal date, the refund line and "Pay and continue".
- Given "Pay and continue", then Checkout opens with the 10.2 parameters and only the allowed methods; closing it returns to the summary with nothing changed.
- Given a successful payment, then "Confirming your payment…" shows and, once the entitlement is written (events, 5 s polling fallback), step 3 opens with its heading focused and announced once.
- Given no confirmation within 2 minutes, then the still-confirming text shows and the wizard still advances when the entitlement lands.
- Given a failed payment, then the summary shows the failure text and Pay again reuses the same subscription.
- Given step 3 for a paid branch, then Windows download is primary and the Android link remains.

## Spec Change Log

## Review Triage Log

Code review (4 layers). Patched: (1) `replace()` could cancel at the provider a subscription already paid in Checkout whose webhook had not landed (reload then pick another plan); it now reads the provider's state first and refuses with `SUBSCRIPTION_EXISTS` unless the subscription is still `created` with no charge (test added). (2) Paid plan cards showed made-up amounts (`₹0`, or IN prices when billing is on and the band is missing); they now show no price in those cases.

Deferred / accepted: reload after paying still shows step 2 (spec residual risk; needs a persisted confirming state, a later story); allowed-methods `display.blocks` shape unverified against live checkout.js; test gaps (non-IN band on sale, poll with a live silent stream, checkout.js loader failure path); 422 vs 400 for the paid PUT (documented). Rejected: `renews_at` fallback (chrono cannot fail for realistic timestamps), loader timeout, unused `onboarding.summary.loading` key (cosmetic).

## Design Notes

The 10.3 activation already marks the `tier` step done, so the wizard's `current` moves to step 3 by itself; the browser only watches `GET /wizard`. The webhook route wakes `/wizard/events` subscribers; the reconcile job runs in another process so the 5 s poll covers it. Prices on the cards come from the committed pricing snapshot for the band (CI-checked against the book); the summary always uses the server quote.

## Verification

**Commands:**
- `cargo test -p arena-cloud --test subscription --test tier --test billing_events`, `cargo test -p arena-cloud --lib`, `cargo test -p arena-proto` -- expected: pass
- `cargo fmt --all --check`, `cargo clippy -p arena-proto -p arena-store -p arena-cloud --all-targets -- -D warnings` -- expected: clean
- `pnpm --filter @arena/onboarding check`, `pnpm --filter @arena/onboarding test` -- expected: pass

## Auto Run Result

Status: review (implemented and verified; the code review is the next pass)

Implemented: `TierChoices.band` and `TierUnavailable::BandNotAvailable`; `QuoteSubscription`/`SubscriptionQuote` and `POST /v1/branches/{id}/subscription/quote` (live book, or the waiting row's own book for the same choice; renewal date by chrono); `create` retries an unpaid `pending` subscription with the same choice (same Checkout) and cancels-then-abandons it for another choice (provider failure keeps the old row); the band is the branch country's alone (a request naming another country is `BAND_NOT_AVAILABLE`); paid `PUT /wizard/tier` with billing on is `VALIDATION_FAILED` (422); an applied webhook event wakes the wizard stream. Onboarding: band-aware cards with "+ 18% GST" on IN and "2 months free", `AddOnsPanel`, `CheckoutSummary`, `razorpay.ts` loader/options (default method blocks off), confirming and slow states, layout poll while confirming (5 s, also with the stream up).

Verification: `cargo test -p arena-cloud -p arena-proto` (lib 151, subscription 24, tier 14, billing_events 17, proto 364) pass; fmt and clippy `-D warnings` clean; `pnpm --filter @arena/onboarding check` clean; Playwright onboarding suite 462 passed (desktop, phone, unit). Known unrelated failures: `binary.rs::unreachable_at_start_retries_then_exits_1_without_logging_the_token` (Windows connect timeout) and `account.spec.ts` "copies and downloads" backup codes (clipboard line endings on Windows); `pnpm check:generated` reports the new generated files only because they are uncommitted.

Residual risks: `window.Razorpay` is stubbed in tests, never the real checkout.js (its `config.display.blocks` option shape is from Razorpay's docs, untested against the live script); reopening the wizard after paying but before the webhook lands shows step 2 again (Pay again would reopen Checkout for the already-paid subscription); the paid `PUT` refusal is 422 `VALIDATION_FAILED` like every other validation error, not 400.
