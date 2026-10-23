## Epic 10: Subscriptions and tiers

Owners pay for, upgrade, cancel and lapse a subscription per branch without disruption, and a Free Android Hub can migrate to Windows when it goes paid.

> Notes: this epic builds on the Epic 1 `apply()`, change log, typed actors, inline PIN (Story 1.13) and audit; on the Epic 2 cloud (`arena-cloud` roles, `gz_control`, owner sign-in), the resumable wizard (Story 2.7), step 2 and its `entitlements` table, licence signing, the `arena-proto` tier table, the shared pricing table and the audited `ops grant-comp` command (Story 2.8), Hub pairing that refuses a Windows Hub on Free (Story 2.10), the heartbeat that re-signs the licence from the current entitlement and returns `cloud_time` (Story 2.11), licence verification, the entitlement API and `trusted_now` (Story 2.12), the device cap (Story 2.19) and Account → Branches (Story 2.30); on Epic 3 sessions and their invariants; on the Epic 4 Free limits (Story 4.9) and "Upgrading removes ads within one heartbeat, and a lapse brings them back only after expiry" (Story 4.15), which this epic now drives from real billing instead of comp grants; and on Epic 5 GST settings and money formatting. From Epic 8 it uses, by title: "The Owner app shell — navigation, themes, install and Free limits" (Account → Subscription route), "On Free, the Owner app shows only Subscription, pairing and support" (owner-side gates read `gz_control` entitlements), "Step-up MFA for sensitive commands", "Owner alerts by web push and WhatsApp, and the Alerts tab", "The cloud sends player and owner messages through a swappable messaging adapter, exactly once" and "A Cloudflare Tunnel for entitled branches, run by the Windows Hub" (reacts to the `tunnel` flag). From Epic 9 it uses, by title: "Upgrading from Free uploads a full snapshot before the first batch" (fires on the first paid licence; not rebuilt here), "One-tap full database export to a folder or USB drive" (the `.a360` format reused for the Android → Windows move), "Restore a branch from an export file as a new epoch", "Owner replaces a failed Hub and the new PC receives the branch key" (KEK release reused for the move) and "The cloud issues epoch counters by compare-and-set on the branch fence". **Decided (spine AD-10, AD-20, AD-29, AD-44):** Razorpay only; one Razorpay subscription per branch; monthly and yearly (yearly = 10× monthly, paid tiers only); renewals over ₹15,000 incl. GST use card or eNACH, never UPI Autopay; states `active → renewal_needs_approval → grace (7 days) → lapsed`; webhooks terminate in the cloud only and are applied idempotently by event id; paid features reach the Hub **only** through the licence token, and billing never sends gate-changing commands; a lapse or downgrade never ends running sessions or open orders and over-cap devices go read-only newest pairing first; 7-day refunds are issued by Arena 360 staff via Razorpay. **Admin UI is Epic 11:** this epic delivers the cloud refund capability and an audited `arena-cloud ops` path; Epic 11's admin app calls the same service. **Partner referral code:** not captured here — no FR in FR178–183 needs it; Epic 11 owns it (AD-19: immutable after first payment, which Epic 11 enforces against `subscriptions.first_paid_at` from this epic). **Owner-login add-on (phase split, decided):** FR178 is MVP and lists the "additional owner login ₹50" add-on, but the feature it pays for (FR161) is v1.1 (Epic 14). This epic delivers every other FR178 clause at MVP and delivers the owner-login clause as price-book data only: it is priced in the price book (Story 10.1), shown with its price and "Available soon", and cannot be bought in MVP. The sale itself ships in v1.1 with "The owner-login add-on can be bought for a branch" (Epic 14). The FR178 text should tag that clause [v1.1] to match FR161. **Deferred (spine):** Razorpay Subscriptions coverage for Malaysia, Singapore and the US must be confirmed before the first non-India tenant; the MY/SG/US bands are built and tested against the Razorpay fake and test mode, and are switched on per band by a control-plane flag. Every Hub story adds its new intents to the Story 1.8 `proptest` generator.

### Story 10.1: The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans

As an Arena 360 operator,
I want every price kept once in a versioned price book and mirrored to Razorpay plans,
So that the wizard, the Owner app and every invoice charge the same amount, and a later price change never touches existing subscribers.

**Covers:** FR178 (prices), FR181 (bands), AD-20, AD-34 (adapter with recorded fake and manual mode), AD-7 (minor units, ISO 4217), AD-9, UX-DR93

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `price_books` (id, version, effective_from, status `draft|live|retired`) and `price_book_items` (price_book_id, band `IN|MY|SG|US`, currency, item `basic|premium|addon_devices_20|addon_tunnel|addon_owner_login`, cycle `monthly|yearly`, amount_minor, gst_applicable, razorpay_plan_id)
**And** the seed live book is:

| Band | Currency | Basic / month | Premium / month | +20 devices | Tunnel | Owner login |
| --- | --- | --- | --- | --- | --- | --- |
| IN | INR | ₹200 | ₹1,999 | ₹50 | ₹200 | ₹50 |
| MY | MYR | RM 23 | RM 229 | RM 6 | RM 23 | RM 6 |
| SG | SGD | S$8 | S$79 | S$2 | S$8 | S$2 |
| US | USD | $6 | $59 | $1.50 | $6 | $1.50 |

with every yearly amount exactly 10× monthly, Free absent (it is never billed), and `gst_applicable = 1` only for band IN

**Given** a book in `draft`
**When** an operator runs the audited `arena-cloud ops price-book publish --id <id> --reason <text>`
**Then** the Razorpay adapter creates one Razorpay plan per (band, item, cycle) with the book's amount and currency, stores each `razorpay_plan_id`, marks the book `live` and the previous book `retired`; a Razorpay error leaves the draft unpublished and nothing half-written

**Given** a published book
**When** anyone tries to change an amount in it
**Then** the API and ops command refuse with `PRICE_BOOK_IMMUTABLE`; prices change only by publishing a new version

**Given** the shared pricing table in the UI package (Story 2.8)
**When** the web apps build
**Then** it is generated from `GET /v1/pricing?band=` against the live book (with a committed snapshot for offline builds), and a CI check fails if the committed snapshot differs from the seed

**Given** the Razorpay adapter
**When** tests run
**Then** they use the recorded fake (plans, subscriptions, payments, refunds, webhooks); `billing.provider = manual` makes every Razorpay call a no-op that writes an ops task, so a Razorpay outage never blocks the rest of the cloud

**Given** a band whose `band_enabled` flag is off (MY, SG, US until Razorpay coverage is confirmed)
**When** pricing is requested for it
**Then** the API returns `BAND_NOT_AVAILABLE` and the wizard shows "Paid plans aren't available in your country yet — Free is" with Free still selectable

### Story 10.2: A branch subscription is created at a PPP band that is locked to it

As a café owner,
I want my branch's price fixed by my billing country when I subscribe,
So that I pay the local price I signed up for and a later price-table change never reprices me.

**Covers:** FR179 (one Razorpay subscription per branch, autopay, card/eNACH over ₹15,000), FR181, AD-20, AD-9, AD-40, NFR14, NFR23

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `subscriptions` (id, tenant_id, branch_id unique among non-ended rows, band, currency, price_book_id, tier, cycle, add_ons JSON with quantities, state, razorpay_subscription_id, mandate_method `upi_autopay|card|emandate`, current_period_start/end, first_paid_at, cancel_at_period_end, created_at, rev)

**Given** an owner calling `POST /v1/branches/:id/subscription {tier, cycle, add_ons, billing_country}` for a branch they own
**When** the branch has no live subscription
**Then** the cloud derives the band from `billing_country` (IN → IN, MY → MY, SG → SG, US → US; any other country refused `BAND_NOT_AVAILABLE`), copies the live `price_book_id` onto the row, creates the Razorpay subscription for the matching plans with add-ons, stores it in state `pending`, and returns the checkout parameters (key id, subscription id, allowed methods, amount, tax, total)

**Given** the renewal debit (tier + add-ons for the cycle, plus 18% GST on band IN) exceeds ₹15,000 (or the band's converted equivalent)
**When** the subscription is created
**Then** the allowed methods are card and eNACH only, and the response says "Renewals over ₹15,000 need a card or bank mandate"; at or below it UPI Autopay, card and eNACH are all offered (verified with Premium yearly ₹19,990 + GST = ₹23,588.20 → no UPI, Basic yearly → UPI allowed)

**Given** a subscription created on price book v1
**When** book v2 with higher prices is published later
**Then** renewals, prorations and invoices for that subscription keep using v1's amounts until the owner changes tier, cycle or add-ons, which re-prices only the changed items at the then-live book (and is shown before confirm)

**Given** a branch that already has a live subscription, a branch the session doesn't own, or a tier `free`
**When** the endpoint is called
**Then** it returns `SUBSCRIPTION_EXISTS`, `FORBIDDEN` or `VALIDATION` and Razorpay is not called

**Given** a `pending` subscription with no payment
**When** 24 h pass
**Then** a jobs-role sweep cancels it at Razorpay and marks it `abandoned`, freeing the branch to start again

### Story 10.3: Razorpay webhooks are verified, applied once, and issue the entitlement

As an Arena 360 operator,
I want every Razorpay event checked and applied exactly once, with a sweep for anything missed,
So that a paid branch is entitled the moment its payment lands and never twice or never.

**Covers:** FR179 (activation, charges), FR6 (paid-tier entitlement issue), AD-20, AD-6 (webhooks terminate in the cloud), AD-10, AD-29, NFR11, NFR25

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `billing_events` (id, razorpay_event_id unique, type, subscription_id nullable, payload_hash, source `webhook|reconcile`, received_at, applied_at) and `subscription_history` (id, subscription_id, old_state, new_state, cause, cloud_time)

**Given** `POST /v1/webhooks/razorpay`
**When** a request arrives
**Then** the `X-Razorpay-Signature` HMAC is verified against the webhook secret from SOPS before anything is parsed; a bad signature returns 400, is counted in the `webhook_failures` metric and writes nothing

**Given** a verified event
**When** it is handled
**Then** it is inserted into `billing_events` (razorpay event id unique, type, payload hash, received_at, applied_at) and applied in the same transaction; a duplicate event id returns 200 and applies nothing

**Given** `subscription.authenticated` followed by `subscription.charged` (or `subscription.activated`) for a `pending` subscription
**When** they are applied in any order
**Then** the subscription becomes `active` with the period from Razorpay, `first_paid_at` is set on the first charge, and the branch's `entitlements` row becomes source `subscription` with the tier and add-ons (replacing `free` or `comp`), written in the same transaction

**Given** the first successful charge applied for a `pending` subscription
**When** Razorpay reports the payment method's country (card issuer or bank) and it does not match the subscription's band
**Then** the subscription moves to `pending_review` instead of `active`, no entitlement is issued, ops are alerted and the owner sees "We're checking your payment — this usually takes a working day"; an operator clearing the review with the audited `arena-cloud ops subscription review --id <id> --approve --reason <text>` activates it through the same path, writing the entitlement then

**Given** a later `subscription.charged` for a renewal
**When** it is applied
**Then** the period moves forward and the state returns to `active` from `renewal_needs_approval` or `grace`; an out-of-order older event never moves the period back

**Given** a webhook that never arrives
**When** the jobs role runs its 10-minute reconcile
**Then** it fetches every subscription in `pending`, `renewal_needs_approval` or `grace` from Razorpay and applies the same transitions through the same code path, recording `source: reconcile`

**Given** the next heartbeat after activation
**When** the cloud re-signs the licence (Story 2.11)
**Then** the licence carries the paid claims, and the cloud never posts a command to change gates

**Given** every transition
**When** it is applied
**Then** a `subscription_history` row records old state, new state, cause (event id or job) and cloud time, and webhook handling time and failures appear in the observability metrics

### Story 10.4: Wizard step 2 takes payment through Razorpay Checkout

As a café owner,
I want to pick Basic or Premium, monthly or yearly, pay once and have the step finish by itself,
So that setting up a paid branch takes one payment and no waiting around.

**Covers:** FR6 (paid path completes when the entitlement is issued), FR178 (prices shown GST-exclusive, yearly 10×, add-ons), FR179 (autopay mandate), AD-20, NFR15, UX-DR24, UX-DR25, UX-DR84, UX-DR93, UX-DR103, UX-DR113

**Acceptance Criteria:**

**Given** wizard step 2 with billing enabled (the Story 2.8 flag turned on)
**When** the owner picks Basic or Premium
**Then** Continue is enabled, the card shows the price for the selected Monthly · Yearly tab in the branch band's currency with "+ 18% GST" on band IN only, yearly shows "2 months free", and an "Add-ons" panel lists +20 devices (quantity 0–10), Tunnel (Basic only; "Included" on Premium) and Owner login ("Available soon", not selectable)

**Given** the owner confirms
**When** the summary opens
**Then** it shows tier, cycle, add-ons, subtotal, GST 18% (band IN) and total, the renewal date, the refund line "Full refund within 7 days of your first payment. After that, cancel anytime; no partial refunds.", and a "Pay and continue" primary button

**Given** "Pay and continue"
**When** Razorpay Checkout opens with the Story 10.2 parameters
**Then** only the allowed methods are offered, the mandate is set up as part of payment, and closing Checkout returns to the summary with nothing changed

**Given** a successful payment
**When** Checkout returns
**Then** the page shows "Confirming your payment…" and, as soon as the Story 10.3 entitlement is written (server-sent events with a 5 s polling fallback), step 2 is marked done and the wizard moves to step 3 with focus on its heading, announced once

**Given** no confirmation within 2 minutes
**When** the wait ends
**Then** the page shows "Your payment is still being confirmed. We'll move you on as soon as it lands." and the wizard can be closed and resumed; it advances whenever the entitlement is issued

**Given** a failed or declined payment
**When** Checkout reports it
**Then** the summary stays with "Payment didn't go through — nothing was charged. Try again or use another method." and the same subscription can be retried

**Given** step 3 for a paid branch
**When** it opens
**Then** the Windows Hub download is the primary choice and the Android link remains available

### Story 10.5: Upgrade links and Add branch reuse the same checkout

As a café owner on Free, or adding a branch,
I want every "Upgrade" link and "Add branch" to take me straight to the same plan picker and payment,
So that I can go paid from wherever I meet a limit without starting the wizard again.

**Covers:** FR6 (paid path from Upgrade), FR178 (tiers and add-ons shown), FR179, AD-20, UX-DR24, UX-DR93, UX-DR113

**Acceptance Criteria:**

**Given** the `tier-lock` "Upgrade" links that earlier epics show hidden or as "Available soon" while billing is off
**When** this story ships with billing enabled
**Then** every one of them is shown and active, and a CI check fails if any `tier-lock` still renders the "Available soon" placeholder for a tier that can be bought

**Given** a `tier-lock` "Upgrade" link anywhere (Hub via owner link, Owner app) for a branch the owner owns
**When** the owner follows it
**Then** the Story 10.4 tier picker, add-ons panel, summary and Checkout open in the Owner app for that branch, pre-set to the tier the lock named, and success returns them to where they came from with the lock lifted once the entitlement is written

**Given** Account → Branches "Add branch" on a tenant with at least one paid branch
**When** the owner adds a branch
**Then** the new branch is created and the same picker and Checkout open for it, with Free not offered while the tenant already has a Free branch

**Given** an Upgrade link for a branch that already has a live subscription
**When** it is followed
**Then** it opens Account → Subscription for that branch instead of a new checkout, and no second subscription is created (`SUBSCRIPTION_EXISTS`)

**Given** Checkout closed or the payment failed from one of these entry points
**When** the owner returns
**Then** they come back to the page they started from with nothing changed, and the Story 10.4 failure text is shown

**Given** a branch the session doesn't own
**When** an Upgrade link names it
**Then** the API returns `FORBIDDEN` and no checkout opens

### Story 10.6: A paid licence reaches the Hub within one heartbeat and staff see the plan

As a café owner,
I want the Hub to switch to my paid plan as soon as I've paid, without a restart,
So that what I bought works straight away at the counter.

**Covers:** FR182 (upgrade within one heartbeat), FR14 (Windows Hub pairs on paid), AD-10, AD-29, AD-44, NFR11, NFR22, UX-DR24

**Acceptance Criteria:**

**Given** the tier table in `arena-proto`
**When** the cloud signs a licence from an entitlement with source `subscription`
**Then** claims follow the tier — Basic `limits {devices: 60 + 20×n, users: unlimited, branches: unlimited}`, `upload_interval_s = summary_interval_s = 900`, `retention_days = 365`, flags `owner_app`, `kiosk_orders`, `kitchen`, `standby`; Premium devices `200 + 20×n`, intervals 300, `retention_days = 1095`, plus `tunnel`, `live_view` — and the Tunnel add-on on Basic adds `tunnel` and `live_view`; `n` is the +20 devices quantity

**Given** a branch whose entitlement just became paid
**When** the Hub's next heartbeat returns the new licence
**Then** the Hub verifies and swaps it without a restart, every gate (`limit()`, `has_flag()`) reads the new claims for the next intent, `tier-lock`s on the Hub lift, and an audit row records the old and new tier with `actor{kind: system}`

**Given** the Hub header and System screen
**When** the tier changes
**Then** System shows "Plan: Basic · Monthly" (or the current tier and cycle) and the licence `exp` in branch time, and a one-time `hub-label` notice "Plan updated to Basic" appears in the alert list; nothing on the floor moves or loses focus

**Given** a branch with a paid entitlement
**When** a `platform: windows` Hub pairs (Story 2.10)
**Then** pairing is accepted; the `FEATURE_NOT_ENTITLED` refusal applies only to Free

**Given** the Hub's heartbeat interval (60 s when live)
**When** a test measures activation-to-gate
**Then** the paid licence is in force on the Hub within 70 s of the entitlement being written, verified on Windows and Android

**Given** a paid licence cached when the WAN drops
**When** the Hub runs offline
**Then** paid claims hold until `exp` on `trusted_now`; moving the wall clock forward or back does not change that

### Story 10.7: Every subscription charge gets a GST tax invoice from Arena 360

As a café owner,
I want a proper tax invoice for every payment, with GST shown separately,
So that I can claim input tax credit and keep my books straight.

**Covers:** FR179 (invoices itemise GST), FR178 (invoices), AD-7, AD-9, UX-DR96, NFR16

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `billing_invoices` (id, number, fy, subscription_id, branch_id, kind `invoice|credit_note`, currency, lines JSON, subtotal_minor, cgst_minor, sgst_minor, igst_minor, total_minor, buyer GSTIN, place of supply, issued_at, pdf_key) with `number` unique

**Given** a `subscription.charged` applied by Story 10.3
**When** the charge is for band IN
**Then** an invoice numbered `A360/<FY>/<seq:06>` (FY Apr–Mar in Asia/Kolkata, gap-free per FY) is issued with one line per item (tier, add-ons with quantity, cycle and period), subtotal, and GST computed once on the subtotal at 18% and rounded half-up to paise: CGST 9% + SGST 9% when the branch's state matches Arena 360's registered state, else IGST 18%

**Given** the branch has a GSTIN (Story 2.7 / 5.1)
**When** the invoice is issued
**Then** it shows the buyer's legal name, GSTIN and state code, Arena 360's GSTIN, address and SAC 998314, and the place of supply

**Given** a charge on band MY, SG or US
**When** the invoice is issued
**Then** it is in the band's currency with no GST lines and the note "Export of services — zero-rated under LUT"

**Given** an issued invoice
**When** the jobs role renders it
**Then** a PDF is written to the cloud's billing prefix in R2 (never under a branch prefix), the owner gets an "Invoice ready" message through the messaging adapter, and the invoice is immutable thereafter

**Given** the same charge event delivered twice
**When** it is applied
**Then** exactly one invoice exists for it

### Story 10.8: Account → Subscription shows each branch's plan, invoices and payment method

As a café owner,
I want one screen that shows what each branch is on, when it renews and every invoice,
So that I know exactly what I'm paying for and can find any invoice.

**Covers:** FR178 (Subscription per branch, invoices, payment method), FR181 (band shown), AD-40, NFR15, UX-DR24, UX-DR25, UX-DR90 (Account → Subscription), UX-DR93, UX-DR113, UX-DR116 (Subscription is spine-only)

**Acceptance Criteria:**

**Given** Account → Subscription in the Owner app
**When** it opens
**Then** each branch shows a `card` with branch name, tier, cycle, state as a `status-badge` (Active · Needs approval · Grace · Cancels on <date> · Lapsed · Free), add-ons with quantities, next renewal date and amount (+ GST on band IN), and the payment method (type and last 4 or bank)

**Given** a Free branch
**When** its card renders
**Then** it shows "Free · 1 branch · ads on the Android Hub" and an "Upgrade" primary button that opens the Story 10.4 checkout for that branch

**Given** "Invoices"
**When** the owner opens it
**Then** it lists every invoice and credit note across branches, newest first, filterable by branch and FY, each with number, date, branch, total and a "Download PDF" link served by a short-lived signed URL; a credit note is shown as a negative amount with its original invoice number

**Given** the Monthly · Yearly `tab-control`
**When** the owner views prices on this screen
**Then** it swaps every displayed price in place and is reflected in the URL, using the band's locked price book (Story 10.2), not the live one

**Given** a phone (< 768 px) and a desktop
**When** the screen renders
**Then** the cards stack in one column on phone and two on desktop with full parity, and all money uses the band's currency formatting

**Given** a screen reader
**When** the screen loads or a state changes
**Then** the screen has one `h1`, each card is a labelled region, and state changes are announced once in a polite live region

**Given** a request for another tenant's subscription or invoice
**When** it reaches the API
**Then** it returns `FORBIDDEN` and nothing is read

### Story 10.9: Change tier and add-ons mid-cycle with proration

As a café owner,
I want to move a branch up or down a tier or add and remove add-ons whenever I need,
So that I only pay for what I use, and an upgrade starts working today.

**Covers:** FR179 (mid-cycle changes prorated), FR178 (add-ons), FR182 (upgrade within one heartbeat), AD-20, AD-29, UX-DR93, UX-DR103

**Acceptance Criteria:**

**Given** an `active` subscription
**When** the owner chooses "Change plan" and picks a higher tier, or adds an add-on or raises the device add-on quantity
**Then** the summary shows the prorated charge for the rest of the period (new daily rate − old daily rate × days left, in minor units, + GST on band IN) and the new renewal amount, and confirming updates the Razorpay subscription immediately

**Given** the prorated charge succeeds
**When** Razorpay confirms it
**Then** the entitlement and licence change at once (applied within one heartbeat by Story 10.6), a Story 10.7 invoice is issued for the prorated amount, and the cloud logs any difference between its computed and Razorpay's charged amount to ops

**Given** a lower tier, an add-on removed or the device quantity lowered
**When** the owner confirms
**Then** the change is scheduled for the period end with the text "Changes on <date>. No partial refunds.", the card shows the pending change with "Keep current plan" to withdraw it, and the entitlement is unchanged until then

**Given** Tunnel on a Premium branch, or on a branch whose Hub is Android
**When** the add-ons panel renders
**Then** Tunnel shows "Included" on Premium, and on Android shows "Windows Hub only" and cannot be added

**Given** a change that takes the renewal debit over ₹15,000 incl. GST on a UPI Autopay mandate
**When** the owner confirms
**Then** the change is held and Razorpay Checkout opens with card and eNACH only to authorise a new mandate; nothing is charged and the plan is unchanged until that mandate exists, and cancelling Checkout leaves everything as it was

**Given** a subscription in `renewal_needs_approval`, `grace` or `pending_review`
**When** the owner opens "Change plan"
**Then** it is disabled with the inline reason "Settle the pending renewal first"

**Given** two changes submitted at once from two devices
**When** they reach the API
**Then** the second fails `STALE` on the subscription `rev` and the owner sees the current plan

### Story 10.10: Switch billing cycle and payment method, with card or eNACH for renewals over ₹15,000

As a café owner,
I want to switch a branch between monthly and yearly and change how I pay,
So that I can save with yearly billing and keep renewals going when my card or bank changes.

**Covers:** FR178 (billing-cycle switch, payment method), FR179 (card/eNACH over ₹15,000), AD-20, UX-DR25, UX-DR93

**Acceptance Criteria:**

**Given** an `active` monthly subscription
**When** the owner switches to Yearly
**Then** the summary shows the yearly amount (10× monthly + GST on band IN), a credit for the unused days of the current month, the net charged today, and the new renewal date one year out; on confirm the cloud replaces the Razorpay subscription with the yearly plan, keeps the same `subscriptions` row and band, and the entitlement never lapses during the swap

**Given** an `active` yearly subscription
**When** the owner switches to Monthly
**Then** the switch is scheduled for the end of the paid year with "Switches to monthly on <date>. No partial refunds."

**Given** a cycle switch whose new renewal debit exceeds ₹15,000 incl. GST
**When** the current mandate is UPI Autopay
**Then** Checkout opens with card and eNACH only to authorise the new mandate before the switch applies, and cancelling Checkout leaves the old cycle unchanged

**Given** "Change payment method"
**When** the owner opens it
**Then** Razorpay's mandate update flow opens with the methods allowed for the current renewal amount, and on success the card shows the new method; the old mandate is cancelled only after the new one is authorised

**Given** a mandate authorisation that fails or is abandoned
**When** the owner returns
**Then** the previous method stays in force and the card says "Payment method unchanged"

**Given** a webhook for the replaced Razorpay subscription arriving after a cycle switch
**When** it is applied
**Then** it is recorded in `billing_events` but cannot change the state of the current subscription

### Story 10.11: Renewals that need approval go to grace, then lapse, on cloud time

As a café owner,
I want clear warnings and a week's grace when a renewal can't go through,
So that a payment hiccup never surprises me or switches off my branch overnight.

**Covers:** FR179 (renewal states), FR182 (lapse path), AD-20 (`active → renewal_needs_approval → grace → lapsed`), AD-29, AD-44, NFR22, NFR25, UX-DR96, UX-DR103

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `grace_until` (nullable, cloud time) to `subscriptions`

**Given** a renewal debit that needs customer authentication (a card or eNACH debit over ₹15,000, or a pre-debit notice the bank requires approved)
**When** Razorpay reports it
**Then** the subscription moves to `renewal_needs_approval`, the entitlement is unchanged, and the owner gets a web push and WhatsApp alert "Your <branch> renewal needs approval" with a link to approve in the Owner app

**Given** a renewal charge that fails (`subscription.pending`), or `renewal_needs_approval` not approved by the period end
**When** the period ends
**Then** the subscription moves to `grace` with `grace_until = period end + 7 days` on cloud time, the cloud keeps signing the paid licence, and the owner is alerted on day 1, day 5 and day 7 of grace

**Given** a branch in `grace`
**When** the owner pays (approve, retry or new method) and Razorpay confirms the charge
**Then** the subscription returns to `active` with the next period counted from the original renewal date, and no lapse ever happened

**Given** `grace_until` passes on cloud time, or Razorpay reports `subscription.halted`
**When** the jobs role runs (every 5 minutes)
**Then** the subscription moves to `lapsed`, the entitlement becomes source `free` with Free claims, and the next heartbeat delivers a Free licence

**Given** a Hub offline with a paid licence
**When** its branch lapses in the cloud
**Then** the Hub keeps the paid claims until the cached licence's `exp` on `trusted_now`, then applies Free limits; winding the PC clock back does not extend it

**Given** an owner opening the Owner app while a branch is in `renewal_needs_approval` or `grace`
**When** any screen loads
**Then** a persistent warning-coloured banner names the branch, the state and the date, with "Approve" or "Update payment method"; the Hub shows nothing to staff until the lapse applies

**Given** each state change
**When** it is applied
**Then** it is in `subscription_history`, and the observability metrics count subscriptions per state

### Story 10.12: A lapse or downgrade never ends a running session, and over-cap devices go read-only

As a counter staff member,
I want a plan drop to leave every running session alone and to tell me plainly which devices are over the limit,
So that players keep playing and I know which PCs I can still start.

**Covers:** FR182 (lapse never ends live sessions; over-cap devices read-only, newest pairing first), AD-29, AD-2, AD-44, NFR10, NFR22, UX-DR24, UX-DR97, UX-DR103

**Acceptance Criteria:**

**Given** a Hub that receives a lower licence (lapse to Free, Premium → Basic, or add-ons removed)
**When** it applies the licence
**Then** a system intent `entitlement.applied` recomputes device standing in one transaction: non-revoked devices counted by Story 2.19's rules are ordered by `paired_at`, the oldest up to `limits.devices` stay active, and the rest are marked `over_cap` (newest pairing first); an audit row lists each device marked

**Given** a device with an active, paused or grace session when it becomes `over_cap`
**When** the session continues
**Then** extend, pause, transfer away and end all work as before; only when the session ends does the device lock to "This PC is over the plan's device limit — ask staff."

**Given** an `over_cap` device
**When** staff try to start a session on it, or transfer a session onto it
**Then** `apply()` rejects with `DEVICE_OVER_CAP` and the tile shows `tier-lock` "Over plan limit" with Upgrade; the tile stays visible on the floor

**Given** a device is revoked or the plan rises again
**When** the next licence or revocation is applied
**Then** standing is recomputed and the oldest `over_cap` devices become active first, without staff action

**Given** a paid licence held offline past its `exp`
**When** `trusted_now` passes `exp`
**Then** the same recompute runs against Free limits; winding the wall clock back never undoes or delays it

**Given** the `proptest` generator
**When** licence changes are interleaved with session intents
**Then** no session is ever ended or changed by `entitlement.applied`, and the count of active devices never exceeds `limits.devices` for new starts

### Story 10.13: A lapse or downgrade keeps open orders, signed-in staff and the main Hub working

As a café manager,
I want open orders, signed-in staff and the Hub itself to carry on when the plan drops,
So that a lapse changes what we can start next, not what is already happening.

**Covers:** FR182 (lapse never ends open orders), AD-29, AD-2, AD-11, NFR10, UX-DR24, UX-DR103

**Acceptance Criteria:**

**Given** open kiosk orders or kitchen tickets when `kiosk_orders` or `kitchen` drops
**When** the lower licence applies
**Then** open orders and tickets still move to served or are cancelled as usual and the Kitchen display keeps showing them until the last open ticket is served; new kiosk orders are refused `FEATURE_NOT_ENTITLED`, the kiosk Shop tab hides ordering, and new KOTs are not created

**Given** more active staff than `limits.users` after the change
**When** the licence applies
**Then** anyone signed in stays signed in; new sign-ins are allowed for the Owner and the most recently active users up to the cap, others see "This branch's plan allows <n> users — ask the owner."

**Given** a Windows Hub whose branch lapses to Free
**When** the Free licence applies
**Then** it keeps running at Free limits with no ads and no lock

**Given** a standby Hub when the `standby` flag drops
**When** the standby receives the licence
**Then** it stops following and shows "Standby needs a paid plan"; the main Hub is unaffected and promote is refused `FEATURE_NOT_ENTITLED`

**Given** the plan rises again
**When** the next licence applies
**Then** kiosk ordering, KOTs, the user cap and standby following return without restarting anything

**Given** the `proptest` generator
**When** licence changes are interleaved with order and staff intents
**Then** no open order is ever ended or changed by `entitlement.applied`

### Story 10.14: Cancel keeps the plan to the end of the period, then the branch drops to Free

As a café owner,
I want to cancel a branch's plan and keep what I've paid for until the period ends,
So that cancelling is simple and fair, and I know exactly when it stops.

**Covers:** FR180 (cancel keeps the plan to period end, then Free; no partial refunds), FR178 (cancel), AD-20, AD-29, UX-DR93, UX-DR103, UX-DR112

**Acceptance Criteria:**

**Given** an `active` subscription more than 7 days after its first payment
**When** the owner chooses "Cancel plan"
**Then** a panel shows the policy text "Your plan stays active until <period end>. After that this branch moves to Free. No partial refunds." and what changes on Free (users 3, devices 20, no owner dashboards, no cloud backup, ads on the Android Hub), and asks for step-up MFA before "Cancel plan" completes

**Given** the cancellation is confirmed
**When** the cloud applies it
**Then** Razorpay is told to cancel at cycle end, `cancel_at_period_end = 1`, the card shows "Cancels on <date>" with "Keep my plan", and the entitlement and licence are unchanged

**Given** "Keep my plan" before the period end
**When** the owner confirms
**Then** the cancellation is withdrawn at Razorpay and the subscription stays `active` with the same renewal date

**Given** the period end on cloud time for a cancelled subscription
**When** the jobs role runs
**Then** the subscription becomes `ended` (no grace), the entitlement becomes source `free`, and the next heartbeat delivers a Free licence handled by Stories 10.12–10.13

**Given** the tenant has another branch already on Free
**When** a second branch's cancellation ends
**Then** that branch still runs at Free limits, shown as "Lapsed — Free limits" with "Resubscribe", and the one-Free-branch rule for new branches is unchanged

**Given** a cancelled branch
**When** the owner resubscribes later
**Then** a new subscription is created at the then-live price book for the tenant's band (Story 10.2)

### Story 10.15: Full refund within 7 days of the first payment, issued through the ops path

As a café owner who changed my mind,
I want my first payment refunded in full if I cancel within 7 days,
So that I can try a paid plan without risk.

**Covers:** FR180 (full refund within 7 days of first payment; no partial refunds), AD-20 (refunds by admin via Razorpay), AD-21 (admin actions audited), AD-19 (refund visible to commission), UX-DR93, UX-DR96

**Acceptance Criteria:**

**Given** an `active` subscription within 7 days (on cloud time) of `first_paid_at`
**When** the owner chooses "Cancel plan"
**Then** the panel offers "Cancel and get a full refund of <amount>", stating the branch moves to Free at once, and after step-up MFA a `refund_requests` row (subscription, payment id, amount, requested_at, status `requested`) is written and the owner sees "Refund requested — usually processed within 2 working days"

**Given** a request in `requested`
**When** an Arena 360 operator runs the audited `arena-cloud ops refund approve --request <id> --reason <text>` (the service Epic 11's admin app will call)
**Then** the Razorpay adapter issues a full refund of every payment on that subscription, cancels it immediately, and the request moves to `issued`; a Razorpay error leaves it `requested` with the error shown to the operator

**Given** `refund.processed` from Razorpay
**When** it is applied
**Then** the subscription becomes `refunded`, the entitlement becomes source `free` (the next heartbeat delivers a Free licence), a credit note `A360/<FY>/CN<seq:06>` reversing each invoice is issued, and the owner is told "Refund of <amount> processed"

**Given** a refund request after the 7-day window, a second request, or any partial amount
**When** it reaches the API or ops command
**Then** it is refused `REFUND_NOT_ALLOWED`; after 7 days the cancel panel shows only the Story 10.14 flow

**Given** an operator rejecting a request with `ops refund reject --reason`
**When** it completes
**Then** the owner sees the reason and the subscription stays `active`

**Given** any refund action
**When** it is recorded
**Then** the admin audit holds operator, reason and amounts, and the refund is stored against the subscription so partner commission can be computed net of refunds

### Story 10.16: Owner starts moving a Free Android Hub to a Windows Hub after upgrading

As a café owner who has just upgraded,
I want to move my branch from the Android tablet to a Windows PC with a code,
So that I get standby and Tunnel without retyping anything.

**Covers:** FR183 (start of the move), AD-10, AD-22, AD-23, UX-DR85, UX-DR103

**Acceptance Criteria:**

**Given** a paid branch whose Hub is Android
**When** the owner opens its Subscription card or Account → Branches
**Then** a "Move to a Windows Hub" action explains the steps (install on the PC, type the code, confirm on the tablet) and that sessions keep running; on Free the action is `tier-lock` "Available on Basic"

**Given** the owner starts the move
**When** the cloud creates it
**Then** a `hub_migrations` row (branch, from_hub, state `awaiting_windows`, created_at) and a `pairing_codes` row with purpose `migrate` are written, and the Story 2.9 `pairing-code` card shows the 8-character code with the Windows download

**Given** a fresh Windows Hub install
**When** it pairs with a `migrate` code
**Then** pairing returns the token and licence as usual, no `branch.bootstrap` is queued, the Android Hub's token stays valid, and the Windows Hub opens a "Waiting for your Android Hub" screen serving no devices; the migration moves to `awaiting_handover`

**Given** `awaiting_handover`
**When** the Android Hub's next heartbeat arrives
**Then** the response carries `migration {id, target_hub_name}` and the Android Hub shows staff a persistent "Move this branch to <PC name>" item in its alert list

**Given** a code that is wrong, expired or used, or a `migrate` code typed into an Android install
**When** pairing is attempted
**Then** the Story 2.10 errors apply unchanged, and an Android install is refused `FEATURE_NOT_ENTITLED` for a `migrate` code

**Given** the owner cancels the move before handover
**When** the cloud applies it
**Then** the Windows Hub's token is revoked, the migration is `cancelled`, and the Android Hub clears the alert at its next heartbeat

### Story 10.17: The Android Hub hands over a final snapshot and steps down

As a café manager at the counter,
I want to confirm the move on the tablet and see it hand everything to the new PC,
So that nothing written on the tablet is left behind.

**Covers:** FR183 (snapshot export with no data loss), AD-1, AD-8 (Android→Windows snapshot format in `arena-proto`), AD-11, AD-42, AD-30, NFR9, NFR10, UX-DR66

**Acceptance Criteria:**

**Given** the "Move this branch to <PC name>" item on the Android Hub
**When** an Owner or Manager opens it and confirms with `inline-pin`
**Then** the Android Hub shows a 6-digit handover code and the Windows Hub's "Waiting" screen asks for it; both are on the same LAN (found by mDNS or typed IP)

**Given** the Windows Hub enters the correct code
**When** the handover starts
**Then** the Android Hub stops accepting new intents (`HUB_HANDING_OVER`, kiosks show their Epic 6 reconnecting state and count down on `lock_after_ms`), waits for the writer queue to drain, and writes a final snapshot in the Story 9.4 `.a360` format with `purpose: migrate`, last `seq` and epoch in the header

**Given** the snapshot
**When** it is transferred
**Then** the Windows Hub pulls it over the LAN on the device-token TLS channel and checks sha256, branch id and last `seq`; on success the Android Hub enters "This branch has moved to <PC name>" (read-only, serving nothing) and reports `migration.handed_over` with the last `seq` in its next heartbeat

**Given** the LAN transfer fails, or the tablet and PC cannot see each other
**When** the manager chooses "Use a file instead"
**Then** the Android Hub writes the same snapshot to its backup folder (Story 4.7) for copying to the PC, and stays stopped until the Windows Hub confirms import or the manager chooses "Cancel move"

**Given** "Cancel move" before the Windows Hub confirms import
**When** the manager confirms with PIN
**Then** the Android Hub resumes as main with the same epoch and nothing set aside

**Given** a wrong handover code 5 times
**When** the sixth attempt arrives
**Then** the code is invalidated and a new one shown, as for kiosk pairing

### Story 10.18: The Windows Hub imports the snapshot as the next epoch

As a café owner,
I want the new Windows Hub to carry on exactly where the tablet stopped,
So that the move loses no data and no session.

**Covers:** FR183 (import with no data loss), AD-11, AD-38 (epoch by fence CAS), AD-24 (KEK release), AD-29, NFR8, NFR9, NFR10, UX-DR103

**Acceptance Criteria:**

**Given** a verified `purpose: migrate` snapshot on the Windows Hub (pulled over the LAN or picked from a file)
**When** it imports
**Then** it loads the database, migrates the schema forward if older, claims the next epoch through "The cloud issues epoch counters by compare-and-set on the branch fence" with `fork = {snapshot epoch, last seq}`, and nothing is set aside because the snapshot is the final state

**Given** the branch KEK
**When** the Windows Hub needs it to read `player_keys`
**Then** the owner approves "Release branch key to <PC name>" with step-up MFA as in "Owner replaces a failed Hub and the new PC receives the branch key", and the Hub stores it in DPAPI

**Given** the Windows Hub opens its writer
**When** it reports `migration.imported` in its heartbeat
**Then** the cloud revokes the Android Hub's token, marks the migration `done`, records the Windows Hub as the branch's Hub, and the Android Hub at its next heartbeat (or at once on the LAN) shows "This branch has moved" permanently

**Given** kiosks paired to the Android Hub
**When** the Windows Hub is serving
**Then** each kiosk shows "Pair again" and pairs with a Hub code (Story 2.18); its running session is found by device and resumes

**Given** sessions that were running during the move
**When** the Windows Hub serves the kiosks
**Then** every session continues with the same start time, balance and remaining time, and the time the Hub was unreachable is counted as normal session time

**Given** row counts and checksums of the Android database at the final `seq` and of the Windows database after import
**When** they are compared
**Then** every branch row matches, and the first batch on a paid branch follows "Upgrading from Free uploads a full snapshot before the first batch" if it has not yet run

### Story 10.19: Kiosks follow the new Windows Hub without pairing again

As a café manager,
I want the kiosks to move to the new Windows Hub by themselves,
So that I don't walk to every PC and pair it again after the move.

**Covers:** FR183 (move without disruption), AD-42 (paired-device class, pinned Hub certificate), AD-25, AD-12, NFR8, NFR11, UX-DR98, UX-DR103

**Acceptance Criteria:**

**Given** a handover that has started (Story 10.17), with the Windows Hub sending its LAN address and certificate fingerprint to the Android Hub along with the handover code
**When** the Android Hub stops accepting intents
**Then** it sends each connected kiosk `hub.successor {address, cert fingerprint, branch_id, expires_at = now + 24 h}` signed with its own pinned key, and each kiosk stores it beside its current pin

**Given** a kiosk holding a valid `hub.successor`
**When** its current Hub reports "This branch has moved" or stays unreachable
**Then** the kiosk connects to the successor address, accepts only the named fingerprint, presents its existing device token, and on success replaces its pin and drops the old one

**Given** the Windows Hub (after Story 10.18 import)
**When** a kiosk presents a device token from the imported database
**Then** it is accepted under the paired-device class, and the kiosk's running session resumes with its Epic 6 reconnect-and-reconcile

**Given** a `hub.successor` with a bad signature, a different branch id, an expired `expires_at`, or a fingerprint mismatch at connect
**When** the kiosk checks it
**Then** it is ignored, the kiosk keeps its old pin, and an audit row is written at the Hub it next reaches

**Given** a kiosk that was offline during the handover
**When** it comes back
**Then** it falls back to "Pair again" (Story 10.18)

**Given** a move with 20 kiosks on the test LAN
**When** the import completes
**Then** every online kiosk is connected to the Windows Hub within 30 s without re-pairing

### Story 10.20: Billing, tier changes and the Hub move proven end to end

As an Arena 360 operator,
I want the full subscription life cycle and the Android → Windows move tested before every release,
So that billing never stalls a café and no plan change breaks a running branch.

**Covers:** FR178–FR183, AD-20, AD-29, AD-44, NFR1, NFR10, NFR22

**Acceptance Criteria:**

**Given** the Razorpay fake with a controllable cloud clock
**When** the billing suite runs
**Then** it walks each band through create → activate → upgrade with proration → cycle switch → renewal needs approval → grace → recovery, grace → lapse, cancel → period end → Free, and 7-day refund → credit note, asserting states, entitlements, licences and invoice totals at each step, including duplicate and out-of-order webhooks

**Given** Razorpay test mode on staging
**When** the nightly run executes
**Then** a Basic monthly UPI Autopay, a Premium yearly card and a Premium yearly eNACH subscription are created, charged and refunded, and UPI is never offered for the Premium yearly plan

**Given** the internal 10-PC test café with 10 live sessions and 2 open kitchen tickets
**When** the branch is downgraded from Premium to Basic and then lapsed to Free
**Then** no session or ticket ends, the newest-paired devices beyond 20 become over cap only after their sessions end, and ads appear on the Android Hub only after the paid licence's `exp` on `trusted_now`

**Given** a Free Android Hub with 30 days of seeded history and 5 live sessions
**When** it upgrades and moves to a Windows Hub over the LAN, and again by file
**Then** both moves finish with checksums equal at the final `seq`, every session continues, and the kiosks reconnect without re-pairing

**Given** the release pipeline
**When** a release candidate is built
**Then** these suites run and block the release on any failure

