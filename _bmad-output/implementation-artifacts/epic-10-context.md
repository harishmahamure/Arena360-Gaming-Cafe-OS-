# Epic 10 Context: Subscriptions and tiers

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Owners pay for, upgrade, cancel and lapse a subscription per branch without disruption, and a Free Android Hub can migrate to Windows when it goes paid. Billing lives only in the cloud (Razorpay); paid features reach the Hub only through the signed licence token, so an upgrade applies within one heartbeat and a lapse or downgrade never ends a running session or open order. The epic also delivers the Android-to-Windows Hub move with no data loss.

## Stories

- Story 10.1: The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans
- Story 10.2: A branch subscription is created at a PPP band that is locked to it
- Story 10.3: Razorpay webhooks are verified, applied once, and issue the entitlement
- Story 10.4: Wizard step 2 takes payment through Razorpay Checkout
- Story 10.5: Upgrade links and Add branch reuse the same checkout
- Story 10.6: A paid licence reaches the Hub within one heartbeat and staff see the plan
- Story 10.7: Every subscription charge gets a GST tax invoice from Arena 360
- Story 10.8: Account -> Subscription shows each branch's plan, invoices and payment method
- Story 10.9: Change tier and add-ons mid-cycle with proration
- Story 10.10: Switch billing cycle and payment method, with card or eNACH for renewals over Rs 15,000
- Story 10.11: Renewals that need approval go to grace, then lapse, on cloud time
- Story 10.12: A lapse or downgrade never ends a running session, and over-cap devices go read-only
- Story 10.13: A lapse or downgrade keeps open orders, signed-in staff and the main Hub working
- Story 10.14: Cancel keeps the plan to the end of the period, then the branch drops to Free
- Story 10.15: Full refund within 7 days of the first payment, issued through the ops path
- Story 10.16: Owner starts moving a Free Android Hub to a Windows Hub after upgrading
- Story 10.17: The Android Hub hands over a final snapshot and steps down
- Story 10.18: The Windows Hub imports the snapshot as the next epoch
- Story 10.19: Kiosks follow the new Windows Hub without pairing again
- Story 10.20: Billing, tier changes and the Hub move proven end to end

## Requirements & Constraints

- Razorpay only; one Razorpay subscription per branch; monthly and yearly (yearly = exactly 10x monthly, paid tiers only). Free is never billed. Prices are shown GST-exclusive; 18% GST applies to band IN only (SAC 998314). MY/SG/US invoices are zero-rated exports with no GST lines.
- Price is fixed by billing country (PPP band IN/MY/SG/US) and locked to the subscription's price book version; later price-book publishes never reprice existing subscribers until the owner changes tier, cycle or add-ons (only changed items re-price). Published books are immutable (`PRICE_BOOK_IMMUTABLE`). MY/SG/US are built and tested against the Razorpay fake/test mode and switched on per band by a control-plane flag (`BAND_NOT_AVAILABLE` otherwise; Free stays selectable).
- Renewal debits over Rs 15,000 incl. GST (or the band's equivalent) must use card or eNACH, never UPI Autopay (e.g. Premium yearly Rs 19,990 + GST = Rs 23,588.20 -> no UPI). Mid-change that crosses this threshold on a UPI mandate holds the change until a new mandate exists.
- Subscription states: `pending`, `pending_review` (payment country mismatches band), `active`, `renewal_needs_approval`, `grace` (7 days on cloud time), `lapsed`, plus `ended`, `refunded`, `abandoned`. Pending with no payment is abandoned after 24 h.
- Cancel keeps the plan to period end then Free (no partial refunds, step-up MFA). Full refund only within 7 days of first payment, whole amount only (`REFUND_NOT_ALLOWED` otherwise), issued by Arena 360 staff via an audited ops path; refund produces a credit note and is stored so partner commission is computed net.
- Downgrade, lapse or add-on removal: running sessions, open orders/tickets and signed-in staff are never ended; only new starts follow the new limits. Over-cap devices go read-only, newest pairing first. A Windows Hub on lapse keeps running at Free limits with no ads and no lock. Ads return on the Android Hub only after the paid licence's `exp` on `trusted_now`.
- Tier claims: Basic devices 60 + 20n, intervals 900 s, retention 365 d, flags `owner_app`, `kiosk_orders`, `kitchen`, `standby`; Premium devices 200 + 20n, intervals 300 s, retention 1095 d, adds `tunnel`, `live_view`; Tunnel add-on on Basic adds `tunnel` and `live_view`. Owner-login add-on is price-book data only ("Available soon", not purchasable in MVP; the sale ships in Epic 14).
- Invoices: `A360/<FY>/<seq:06>`, FY Apr-Mar in Asia/Kolkata, gap-free per FY, one invoice per charge event, immutable, PDFs in the billing R2 prefix (never a branch prefix). CGST+SGST when buyer state matches Arena 360's, else IGST; rounding half-up to paise. Credit notes `A360/<FY>/CN<seq:06>`.
- Android-to-Windows move loses no data: checksums match at final `seq`, running sessions keep start time/balance/remaining time, online kiosks reach the new Hub within 30 s on a 20-kiosk test LAN.

## Technical Decisions

- Webhooks terminate in the cloud only: HMAC `X-Razorpay-Signature` verified (secret from SOPS) before parsing, applied idempotently by unique `razorpay_event_id` in one transaction with the entitlement write; out-of-order events never move a period back. A 10-minute jobs-role reconcile fetches `pending`, `renewal_needs_approval` and `grace` subscriptions and applies the same code path (`source: reconcile`). Every transition writes `subscription_history`. A 5-minute jobs sweep applies lapse/period-end on cloud time.
- Billing never sends gate-changing commands; the entitlement row (source `subscription|free|comp`) changes in `gz_control` and the heartbeat re-signs the licence. Hub swaps the licence without restart (within 70 s on a 60 s heartbeat), emits a system intent `entitlement.applied` that recomputes device standing in one transaction, and audits old/new tier with `actor{kind: system}`. Offline paid claims hold to `exp` on `trusted_now`; wall-clock changes do not matter.
- `gz_control` tables: `price_books`, `price_book_items` (band, currency, item, cycle, `amount_minor`, `gst_applicable`, `razorpay_plan_id`), `subscriptions` (unique branch among non-ended; `rev` for `STALE` optimistic concurrency; `grace_until`, `first_paid_at`, `cancel_at_period_end`), `billing_events`, `subscription_history`, `billing_invoices`, `refund_requests`, `hub_migrations`, `pairing_codes` purpose `migrate`. Money in minor units, ISO 4217.
- Razorpay adapter has a recorded fake (plans, subscriptions, payments, refunds, webhooks); `billing.provider = manual` turns calls into no-ops that write ops tasks so a Razorpay outage never blocks the cloud. Operator actions are audited `arena-cloud ops` commands (`price-book publish`, `subscription review`, `refund approve|reject`) that Epic 11's admin app will call as the same service. Pricing UI table is generated from `GET /v1/pricing?band=` with a CI-checked committed snapshot.
- Cycle switch replaces the Razorpay subscription but keeps the same `subscriptions` row, band and entitlement; webhooks for the replaced Razorpay subscription are recorded but cannot change current state. Old mandate cancelled only after the new one is authorised.
- Move to Windows: Android Hub shows a 6-digit handover code (5 wrong tries invalidates it); stops intents (`HUB_HANDING_OVER`), drains the writer, writes a final `.a360` snapshot (`purpose: migrate`) in the `arena-proto` format; Windows Hub pulls it over the device-token TLS channel (or file fallback), verifies sha256/branch id/last `seq`, claims the next epoch via the fence compare-and-set with `fork = {snapshot epoch, last seq}`, and gets the branch KEK by owner-approved release (step-up MFA, stored in DPAPI). Android token is revoked only when the Windows Hub reports `migration.imported`. Kiosks get a signed `hub.successor {address, cert fingerprint, branch_id, expires_at = +24 h}` and re-pin; kiosks offline during handover fall back to "Pair again".
- Every new Hub intent is added to the Story 1.8 `proptest` generator; licence changes interleaved with session/order/staff intents must never end or alter a session or order.

## UX & Interaction Patterns

- Wizard step 2: Monthly/Yearly `tab-control`, price in the band currency (+ 18% GST on IN only), yearly labelled "2 months free", Add-ons panel (+20 devices qty 0-10, Tunnel Basic-only / "Included" on Premium / "Windows Hub only" on Android, Owner login "Available soon"). Summary shows subtotal, GST, total, renewal date and the 7-day refund line, then "Pay and continue". Closing Checkout changes nothing; after payment "Confirming your payment..." via SSE with 5 s polling fallback; 2-minute timeout message lets the wizard be closed and resumed; step 3 prefers Windows Hub download for paid branches.
- All `tier-lock` Upgrade links open the same picker/Checkout pre-set to the locked tier and return the owner to where they came from; a branch with a live subscription goes to Account -> Subscription instead. CI fails if any buyable `tier-lock` still shows "Available soon".
- Account -> Subscription: per-branch `card` with `status-badge` (Active, Needs approval, Grace, Cancels on <date>, Lapsed, Free), invoices list with signed-URL PDF download, prices from the branch's locked book. One column on phone, two on desktop, one `h1`, polite live-region announcements. A persistent warning banner shows in the Owner app during `renewal_needs_approval` or `grace`; the Hub shows staff nothing until lapse applies.
- Hub: System shows "Plan: Basic - Monthly" and licence `exp` in branch time; one-time "Plan updated" `hub-label` alert; floor never moves or loses focus. Over-cap tiles stay visible with `tier-lock` "Over plan limit"; a running session continues and the device locks with "This PC is over the plan's device limit - ask staff." only after it ends. Move uses `inline-pin` and persistent "Move this branch to <PC name>" alert item.

## Cross-Story Dependencies

- Builds on Epic 2 (wizard step 2 and billing flag from Story 2.8, pairing 2.10, heartbeat/licence signing 2.11, `trusted_now` 2.12, device cap 2.19, Account -> Branches 2.30), Epic 4 (Free limits and ad removal/return), Epic 8 (Owner app shell, step-up MFA, alerts/messaging adapter, Tunnel reacting to the `tunnel` flag) and Epic 9 (first-paid-licence full snapshot upload, `.a360` export format, restore as a new epoch, KEK release, fence compare-and-set from Story 9.3).
- Within the epic: 10.1 -> 10.2 -> 10.3 is the billing spine; 10.4, 10.5, 10.9, 10.10 and 10.14 build on the 10.2/10.3 checkout and webhook path; 10.6 delivers the licence that 10.12, 10.13 and the proptest assertions rely on; 10.7 hooks into 10.3 charges and 10.15 credit notes; 10.11 and 10.14 both end in lapse/Free handled by 10.12-10.13; 10.16 -> 10.17 -> 10.18 -> 10.19 is the Hub-move chain; 10.20 is a release-blocking proof suite over everything.
- Feeds Epic 11 (admin app calls the refund/review/publish services; partner referral code is immutable after `first_paid_at`; commission net of refunds) and Epic 14 (owner-login add-on sale).
