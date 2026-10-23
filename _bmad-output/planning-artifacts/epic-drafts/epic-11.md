## Epic 11: Marketing site, partner programme and admin operations

Prospects find and sign up through the public site, partners earn and track commission, and Arena 360 staff curate the catalogue, approve partners and support branches.

> Draft notes: this epic builds on the Epic 1 scaffold (Story 1.1: the Astro 7 marketing skeleton, the partner-portal and admin SvelteKit skeletons on Workers static assets, the shared UI package and message catalogue), tokens with the contrast gate (Story 1.2) and shared base components (Story 1.3); on the Epic 2 cloud and sign-up (Stories 2.1–2.3: `arena-cloud` roles, `gz_control`, per-IP and per-phone rate limits, the messaging adapter used for OTP, telemetry scrubbing), the `arena-proto` tier table, licence signing and the audited `arena-cloud ops grant-comp` command (Story 2.7), heartbeats (Story 2.10), the Hub pairing that refuses a Windows Hub on Free (Story 2.9) and `onboarding_progress` (Stories 2.6, 2.30); on the Hub `tracing` ring buffer and `PRAGMA integrity_check` (Story 1.4) and Settings → System (Story 2.25); on the Epic 4 ad metrics (`ad_impressions`); on the Epic 6 catalogue tables, manifest API and `arena-cloud catalog import` (Story 6.1 — this epic adds only the curation UI and the screening gate; the manifest's `flash_screened` filter already exists); on Epic 8 by title: "The cloud sends player and owner messages through a swappable messaging adapter, exactly once" (partner WhatsApp and email go through it), "The cloud accepts owner command kinds, checks them, and fans "all branches" out" and "The Hub applies owner commands exactly once, and the owner's change wins" (the `diagnostics.request` command kind is added to that catalogue), "Presign issues batch and summary upload URLs under the branch's own R2 prefix" (diagnostics upload) and "The builder serves audit queries from a local cache" (support audit search with a recorded reason already enforced at the API); and on Epic 10 by title: "The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans" (generated pricing table used by the Pricing page), "Razorpay webhooks are verified, applied once, and issue the entitlement" (`subscriptions.first_paid_at`), "Every subscription charge gets a GST tax invoice from Arena 360" (`billing_invoices`, GST-exclusive `subtotal_minor`, credit notes) and "Full refund within 7 days of the first payment, issued through the ops path" (the refund service and `arena-cloud ops refund approve|reject`, which the admin app calls). **Decided (spine AD-15, AD-17, AD-18, AD-19, AD-21, AD-40):** marketing is Astro rendering shared Svelte components, deployed to Cloudflare Workers static assets (not Pages); partner portal at `partners.arena360.cloud` is a separate identity with its own login and API scope and never reads tenant operational data; the payout job runs on the 1st in Asia/Kolkata business time, pays 10% of referred tenants' paid GST-exclusive subscription revenue net of refunds, rolls balances under ₹500 over, deducts 2% TDS, claws refunds back in the next payout, and staff transfer manually and mark paid; KYC (PAN, bank, optional GSTIN) is encrypted and admin-read only; the admin app sits behind Cloudflare Access (staff SSO) and every admin action is written to `admin_audit` in `gz_control` with `actor.kind = admin`; support reads branch data only through the diagnostics bundle and the audit builder with a recorded reason. **One audit store:** the `admin_audit` table already written by the `arena-cloud ops` commands, the catalogue import (Story 6.1) and the audit builder (Story 8.35) is reused; admin-app actions and `ops` commands call the same services so CLI and UI never diverge. **Out of this epic:** the in-browser demo Hub, Compare, Status and changelog pages (Epic 14, v1.1), automated payout transfers (spine Deferred), and partner co-branding (logo-only branch customisation stays as is). English only (NFR16).

### Story 11.1: Marketing site shell — navigation, footer and light and dark themes

As a prospective café owner,
I want a fast public site that reads well on my phone and laptop, in light or dark,
So that I can find what Arena 360 does and how to start without fighting the page.

**Covers:** FR196 (site shell), AD-15, NFR15, NFR16, NFR26, UX-DR5, UX-DR6, UX-DR11, UX-DR13, UX-DR17, UX-DR19, UX-DR20, UX-DR91 (header, phone nav, footer), UX-DR113, UX-DR114

**Acceptance Criteria:**

**Given** the Astro 7.4 + `@astrojs/svelte` 9.1 project from Story 1.1
**When** it builds
**Then** every page is prerendered to static HTML and deployed to Cloudflare Workers static assets at `arena360.cloud` (staging at `staging.arena360.cloud`), with interactive parts (theme toggle, phone nav, tab controls) as Svelte islands from the shared UI package, and Sora served from the site itself with no third-party font or CDN fetch

**Given** a desktop width (≥ 1024 px)
**When** any page opens
**Then** an 80 px header shows the wordmark (link to Home), Product · Pricing · Downloads · Help · FAQ and one `button-primary` "Get started", with the content on the web gutter (56 px) and section gap (128 px) tokens

**Given** a phone width (< 768 px)
**When** any page opens
**Then** the header is 64 px with a hamburger and "Get started" always visible; the hamburger opens a full-screen nav (Product · Pricing · Downloads · Help · FAQ) that traps focus, closes on Esc or a link, and returns focus to the hamburger; the header stays sticky while scrolling

**Given** any page
**When** it renders
**Then** the footer holds links to Legal (Terms, Privacy, Data processing, Ad policy), Help, FAQ, Downloads and `theme-toggle` System · Light · Dark; System follows `prefers-color-scheme` live, a chosen mode is remembered per device, and an inline head script applies it before first paint so the page never flashes the wrong theme

**Given** light mode
**When** a page renders
**Then** it uses the "Cool paper" tokens, links in body text are underlined in `accent-text-light`, and full-bleed media sections keep the dark tokens and scrim

**Given** any page
**When** an axe scan runs in both themes at 390 px and 1440 px
**Then** it has `header`, `nav`, `main` and `footer` landmarks, one `h1`, labelled controls, ≥ 44 px targets, a skip-to-content link, zero serious or critical violations, and no horizontal scroll at 390 px with strings lengthened by 30%

**Given** the site build
**When** CI runs
**Then** every user-facing string comes from the message catalogue (the hard-coded-string lint passes), each page has a title, meta description, canonical URL and Open Graph tags, `sitemap.xml` and `robots.txt` are generated, and an unknown path returns a 404 page in the same shell with links to Home and Help

### Story 11.2: Home tells the install, pair, play story

As a prospective café owner,
I want the first page to tell me plainly what I get and how fast I can start,
So that I believe it runs my café offline and costs less than what I use now.

**Covers:** FR196 (Home), NFR26, UX-DR11, UX-DR91 (Home sections), UX-DR103 (player register), UX-DR115 (key-web-marketing-home), UX-DR116 (hero on `web-hero`, light theme)

**Acceptance Criteria:**

**Given** Home at ≥ 1024 px
**When** it opens
**Then** the hero shows the promise "Run your café offline. See it from anywhere. Pay less than you do now." in `web-hero` 64/200 on the left in the scrim zone, a short supporting line and the "Get started" primary, and a dark Hub floor-view product shot on the right; below 1024 px the title uses `web-display` and the shot stacks under the copy

**Given** the rest of Home
**When** it scrolls
**Then** sections follow in order: "Install. Pair. Play." three steps (install the Hub, pair your PCs with a code, start a session), "What you get" three cards (works with the internet down, owner app from anywhere, per-branch price with Free to start), a closing "Get started" band, then the footer, with sections 128 px apart (64 px on phone)

**Given** a hero background video
**When** the page loads
**Then** it is muted with no audio track, loops only while visible, stops when `prefers-reduced-motion` is set, and falls back to its still cover and then solid `bg` if it cannot play; no element uses `backdrop-filter` or live blur

**Given** the light theme
**When** Home renders
**Then** the hero and media bands stay dark with the scrim while the step and card sections use "Cool paper"

**Given** the page on a mid-range phone over a throttled 4G profile
**When** a Lighthouse CI run executes
**Then** Largest Contentful Paint is under 2.5 s and the performance and accessibility scores are each at least 90, and the build fails otherwise

### Story 11.3: Pricing — Cafés tab with Monthly · Yearly, equal plan cards, add-ons and FAQ

As a prospective café owner,
I want every price shown plainly with GST stated and the refund rule up front,
So that I know exactly what I'd pay before I sign up.

**Covers:** FR196 (Pricing, Cafés), FR178 (displayed prices), FR180 (refund FAQ), UX-DR25, UX-DR91 (Pricing), UX-DR93, UX-DR103 (money/legal register), UX-DR115 (key-web-pricing), UX-DR116 (toggle, GST note, add-ons, refund FAQ, no rewarded ads)

**Acceptance Criteria:**

**Given** Pricing
**When** it opens
**Then** a `tab-control` Cafés · Partners sits under the title with Cafés selected, the selected tab is in the URL (`/pricing?tab=cafes`), arrow keys move between tabs, and Partners shows "Partner programme details coming soon" until Story 11.11

**Given** the Cafés tab
**When** it renders
**Then** one primary "Get started" sits above a Monthly · Yearly `tab-control` (also reflected in the URL), followed by three equal tier `card`s with no emphasised plan: Free "₹0 · 1 branch · ads on the Android Hub", Basic "₹200/branch/month + 18% GST", Premium "₹1,999/branch/month + 18% GST", each card's secondary action "Choose <tier>" leading to Get started with the tier pre-selected

**Given** Yearly is selected
**When** prices swap
**Then** Basic shows "₹2,000/branch/year + 18% GST" and Premium "₹19,990/branch/year + 18% GST", each with "2 months free", Free is unchanged, and the change is announced to screen readers

**Given** the prices
**When** the site builds
**Then** every amount comes from the Epic 10 generated pricing table for band IN (never typed into page copy), and a CI check fails if a displayed amount differs from the price book snapshot

**Given** the comparison table and add-ons
**When** they render
**Then** the table lists each tier's limits from the `arena-proto` tier table (branches, devices, users, owner dashboards, cloud backup and retention, standby, Tunnel, kitchen display, ads), and add-ons list "+20 devices ₹50", "Tunnel for remote devices ₹200" and "Additional owner login ₹50 — available soon", each per branch per month + 18% GST; no rewarded ads appear anywhere

**Given** the Pricing FAQ
**When** it renders
**Then** it answers what happens offline, how cancellation works and the refund rule in the exact text "Full refund within 7 days of your first payment. After that, cancel anytime: your plan stays active to the end of the paid period, then the branch drops to Free. No partial refunds.", as an accessible disclosure list

**Given** a visitor at 390 px
**When** the page renders
**Then** the cards stack in one column in the same order and the comparison table scrolls inside its own container with the tier column pinned, so the page itself never scrolls sideways

### Story 11.4: Product pages for Hub, Kiosk, Console and Owner with demo videos

As a prospective café owner,
I want to see each part of Arena 360 working before I install anything,
So that I can judge whether it fits my counter, my PCs and my consoles.

**Covers:** FR196 (Product), NFR26, UX-DR5 (media stays dark), UX-DR91, UX-DR113, UX-DR116 (spine-only marketing page)

**Acceptance Criteria:**

**Given** Product
**When** it opens
**Then** it has an overview and one section each for Hub, Kiosk, Console and Owner, reachable as anchors (`/product#hub` etc.) from an in-page link list, each with an illustrated walkthrough of three to five steps and a 90-second demo video

**Given** a demo video
**When** the page loads
**Then** it shows its poster still and does not play until pressed; it plays with sound only after the press, has WebVTT English captions on by default and a text transcript link, and is served from the CDN under the shared `marketing/` R2 prefix

**Given** a video that fails to load or a browser that cannot play it
**When** the section renders
**Then** the poster still remains with "Video unavailable — read the walkthrough below", never a broken player

**Given** the Console section
**When** it renders
**Then** it says plainly that console time is run from the Hub tile at launch and that the TV and QR self-start arrive with Pi support, without promising a date

**Given** keyboard and screen-reader users
**When** they operate a video
**Then** play, pause, captions and volume are native or labelled controls reachable by Tab, and pressing play moves no focus

### Story 11.5: Downloads with installers, system requirements and checksums

As a café owner setting up,
I want to download the right installer and check it is genuine,
So that I install the real Arena 360 on hardware that will run it.

**Covers:** FR196 (Downloads), AD-32 (signed installers), NFR17, NFR18

**Acceptance Criteria:**

**Given** the signed-installer prefix in R2 (AD-32)
**When** the release pipeline publishes a release
**Then** it also writes `releases/manifest.json` (product, channel `stable`, version, file name, size, SHA-256, signature URL, published_at), and the Downloads page reads it at build time and again in the browser so a new release appears without a site redeploy

**Given** Downloads
**When** it opens
**Then** it lists Windows Hub, Windows Kiosk (installer for each gaming PC), Android Hub (a Google Play link, not an APK) and, when present in the manifest, the Raspberry Pi image, each with version, size, release date, SHA-256 in a copyable monospace field and a "How to verify" disclosure showing the PowerShell `Get-FileHash` command

**Given** a product missing from the manifest (for example the Pi image before console TV support ships)
**When** the page renders
**Then** its row reads "Coming soon" with no download link

**Given** the system requirements section
**When** it renders
**Then** it states Kiosk: Windows 10 1803 to 11, 64-bit, 4 GB RAM, WebView2 Evergreen; Windows Hub: Windows 10 or later; Android Hub: Android 9+, 3 GB RAM, 20 GB free; "Windows 7 is not supported"; any ESC/POS printer; and links to the Help centre compatibility lists

**Given** the manifest fetch fails in the browser
**When** the page is open
**Then** the build-time list stays shown with "Showing the list from <date>", and no link points to a file whose checksum is not shown

### Story 11.6: Help centre with setup guides, troubleshooting and compatibility lists

As a café owner or installer,
I want setup steps, fixes for common problems and a list of hardware that is known to work,
So that I can get unstuck without waiting for support.

**Covers:** FR196 (Help centre), NFR18 (compatibility list, "tested device" badge), UX-DR113, UX-DR116 (spine-only marketing page)

**Acceptance Criteria:**

**Given** the Help centre
**When** it opens
**Then** it shows three categories — Setup, Troubleshooting, Hardware compatibility — and a search box, with articles authored as Markdown content collections in the repo, each with a title, summary, last-reviewed date and stable slug

**Given** the launch content set
**When** the site builds
**Then** Setup holds at least: install the Hub (Windows, Android), pair a kiosk, set up a printer and cash drawer, add a standby PC, invite staff; Troubleshooting holds at least: kiosk can't find the Hub, printer offline, "Hub slow", branch shows offline in the owner app, restoring from an export; and the build fails if an internal link points to a missing slug

**Given** Hardware compatibility
**When** it renders
**Then** printers and HDMI-CEC TVs are listed from a versioned data file (brand, model, connection USB/LAN/Bluetooth or CEC, result) with a "Tested device" badge on rows verified by the Arena 360 test rig, a note that CEC is best-effort and a smart plug is the fallback, and the table is filterable by brand and connection

**Given** a search term
**When** the visitor types
**Then** a static search index built at deploy returns matching article titles and summaries within 200 ms with no server call, and "No articles match — try Troubleshooting or contact support" when nothing matches

**Given** any article
**When** it ends
**Then** it shows the support channel for the visitor to use (community and help centre on Free, email within 24 h on Basic, chat within 4 h on Premium) without asking for any personal data on the page

### Story 11.7: FAQ and Legal pages

As a prospective café owner,
I want straight answers on billing, data, ads and refunds, and the terms I'm agreeing to,
So that nothing in the contract surprises me later.

**Covers:** FR196 (FAQ, Legal), FR180 (refund text), FR201 (policy version), UX-DR103 (money/legal register), UX-DR113

**Acceptance Criteria:**

**Given** FAQ
**When** it opens
**Then** it groups answers under Billing, Data ownership, Offline, Ads and Refunds as an accessible disclosure list, with the refund answer using the same text as the Pricing FAQ (one catalogue key), and the page emits `FAQPage` structured data

**Given** the Data ownership answers
**When** they render
**Then** they state that the full branch database and every report can be exported at any time on every tier, that branch data lives on the café's own Hub, and that cloud data is hosted in India

**Given** Legal
**When** it opens
**Then** it lists Terms of service, Privacy policy, Data processing terms and Ad policy, each on its own page with a version number, an effective date and a change history list

**Given** the Privacy policy
**When** it builds
**Then** its version is read from the same `arena-proto` policy-version constant that kiosk sign-up consent rows record, and CI fails if the page version and the constant differ

**Given** the Ad policy
**When** it renders
**Then** it states that ads appear only on the Free Android Hub at the counter, never on player screens, exclude adult, gambling and alcohol categories, and are removed on upgrade

### Story 11.8: Get started hands off to sign-up, and funnel events are counted without personal data

As the Arena 360 team,
I want to know how many visitors go from the site to sign-up and on to a live branch, without tracking anyone,
So that we can measure the launch funnel and fix where people drop off.

**Covers:** FR196 (Get started), NFR27 (funnel instrumentation), NFR13, NFR23, AD-40

**Acceptance Criteria:**

**Given** "Get started" anywhere on the site
**When** it is pressed
**Then** the visitor lands on the onboarding sign-up route from Story 2.3, carrying any `utm_source`, `utm_medium`, `utm_campaign` and selected tier as query parameters, and the marketing `/get-started` URL itself redirects there so it can be linked

**Given** `gz_control`
**When** the migration runs
**Then** it adds `product_events` (id ULID, name, at, anonymous session id, tenant_id nullable, branch_id nullable, page path, utm source/medium/campaign, referrer domain) with no IP, user agent, phone, email or name column, and a retention job deletes rows older than 13 months

**Given** the public `POST /v1/events` endpoint
**When** the site or onboarding app sends an event
**Then** only allow-listed names are stored (`site_page_view`, `get_started_click`, `signup_started`, `otp_verified`, `signup_completed`, `pricing_tab_changed`, `partner_apply_started`), unknown names and extra fields are dropped, and the per-IP Cloudflare limit applies

**Given** the anonymous session id
**When** it is created
**Then** it is a random id kept in `sessionStorage` (no cookie, no cross-site identifier), discarded when the tab closes, and linked to a `tenant_id` only by the `signup_completed` event sent from the authenticated onboarding session

**Given** a browser sending Global Privacy Control or Do Not Track
**When** any page loads
**Then** no event is sent

**Given** the event endpoint is down or slow
**When** a visitor navigates
**Then** events are sent with `navigator.sendBeacon` and failures are dropped silently; navigation and sign-up never wait on them

**Given** telemetry export
**When** the CI privacy test sends a sign-up with known values
**Then** none of those values appear in `product_events`, OpenTelemetry traces or Sentry

### Story 11.9: Admin app behind Cloudflare Access, with every action audited

As an Arena 360 staff member,
I want one internal app that only staff can reach, where everything I do is recorded,
So that we can run the business safely and answer for every change we make.

**Covers:** FR193 (staff SSO), FR194 (every admin action audited), AD-21, AD-40, AD-9, NFR11, NFR14, NFR25

**Acceptance Criteria:**

**Given** the admin SvelteKit app from Story 1.1
**When** it deploys
**Then** it is served from Workers static assets at `admin.arena360.cloud` behind a Cloudflare Access application that allows only the staff identity provider group, with staging behind a separate Access application

**Given** any `/v1/admin/*` request at the cloud
**When** it arrives
**Then** the cloud verifies the `Cf-Access-Jwt-Assertion` against the Access team certificates and the admin audience, and returns 401 if it is missing, expired or for another audience; a request that bypasses Cloudflare reaches no admin handler

**Given** `gz_control`
**When** the migration runs
**Then** it adds `admin_staff` (email, display name, roles subset of `catalogue|partners|finance|support|superadmin`, active, added_by, added_at), and a valid Access identity with no active `admin_staff` row gets 403 with "Your account isn't set up for admin. Ask a superadmin." and the denial is audited

**Given** the existing `admin_audit` table
**When** the migration runs
**Then** it gains, additively where missing, `staff_email`, `role_used`, `action`, `target_kind`, `target_id`, `reason`, `summary` (no secrets, no KYC values, no player data) and `request_id`, and an update or delete on `admin_audit` is refused by a trigger

**Given** any admin endpoint that changes state or reveals sensitive data
**When** it succeeds or fails
**Then** one `admin_audit` row with `actor{kind: admin, id: <staff email>}` is written in the same transaction as the change, and an endpoint without an audit call fails a CI check that lists every admin route against its audit action

**Given** the admin app
**When** a staff member opens it
**Then** it shows a navigation of Catalogue · Partners · Payouts · Billing · Support · Metrics · Audit · Staff with sections they lack a role for hidden, follows the light and dark themes, and meets the web screen-reader baseline

**Given** Audit
**When** it opens
**Then** it lists `admin_audit` rows newest first, filterable by staff, action, target and date, paged 50 at a time and exportable to CSV, and only `superadmin` can add or deactivate staff from Staff (itself audited)

### Story 11.10: A reseller or installer applies with phone OTP and sees "Under review"

As a reseller or installer,
I want to apply in a couple of minutes and see where my application stands,
So that I know I'm in the queue and don't have to chase anyone.

**Covers:** FR188, AD-18, AD-23, AD-40, NFR13, NFR23, UX-DR18, UX-DR92 (Under review tracker), UX-DR103, UX-DR113, UX-DR115 (key-web-partner-portal, state C)

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `partners` (id ULID, name, phone E.164 unique, email, city, business name, type `reseller|installer`, cafés served, status `applied|approved|rejected|suspended`, applied_at, decided_at, decided_by, rejection reason) and `partner_sessions`, entirely separate from `owners` and owner sessions

**Given** the partner portal (SvelteKit, Workers static assets) at `partners.arena360.cloud/apply`
**When** an applicant opens it
**Then** it shows text fields for name, phone (default +91), email, city, business name, type (Reseller / Installer) and cafés served (number), validated on submit and blur, with one primary "Send code"

**Given** a valid form
**When** "Send code" is pressed
**Then** a 6-digit code is sent by SMS through the messaging adapter with the same expiry, attempt and per-phone limits as owner sign-up, and a correct code creates the `partners` row in `applied`, issues a partner session cookie scoped to `partners.arena360.cloud`, and shows the status page

**Given** an `applied` partner
**When** they open the portal
**Then** they see only "Under review" with a three-step tracker (Submitted ✓ → Review (current) → Approved) and the text "We'll message you on WhatsApp when you're approved.", and no referral, café or payout data

**Given** a phone already used by a partner
**When** the form is submitted
**Then** the code is still sent (no enumeration) and a correct code signs in to the existing partner instead of creating a second one

**Given** a partner session cookie
**When** it calls any owner, tenant, branch or admin endpoint
**Then** the cloud returns 403 `FORBIDDEN`, and an owner session calling `/v1/partner/*` gets the same; the two scopes share no cookie, token or table

**Given** a returning partner
**When** they sign in
**Then** they sign in with phone OTP at `partners.arena360.cloud/sign-in`, and logs and telemetry carry the partner id only, never phone or email

### Story 11.11: Pricing — Partners tab with terms, a worked payout example and Apply

As a reseller or installer,
I want to see exactly what I'd earn and when I'd be paid before applying,
So that I can decide whether the programme is worth my time.

**Covers:** FR196 (Pricing, Partners), FR191 (terms displayed), UX-DR25, UX-DR91 (Partners tab), UX-DR93, UX-DR103 (money/legal register), UX-DR115 (key-web-pricing frames 3–4)

**Acceptance Criteria:**

**Given** the Pricing page
**When** the Partners tab is selected (or `/pricing?tab=partners` is opened)
**Then** it replaces the Story 11.3 placeholder with the programme terms: "10% of subscription revenue for every month the café you refer stays active and paying", leads not exclusive by territory, and who can apply (resellers and installers)

**Given** the worked payout example
**When** it renders
**Then** it shows, from the same catalogue values as the portal: three referred cafés on Basic monthly (₹600 GST-exclusive revenue) → ₹60 commission, "Pending — below ₹500 minimum, rolls over"; and ten cafés on Premium monthly (₹19,990) → ₹1,999 commission, TDS 2% ₹39.98, payout ₹1,959.02 by the 10th of the next month

**Given** payout terms
**When** they render
**Then** they state: monthly bank transfer by the 10th for the previous month; ₹500 minimum, a lower balance rolls over; TDS deducted at 2% (subject to tax advice), shown on each payout; GST-registered partners upload a GST invoice; KYC (PAN, bank details, optional GSTIN) after approval and before the first payout; refunds after a payout are recovered from the next one

**Given** the Partners tab
**When** "Apply" is pressed
**Then** it goes to `partners.arena360.cloud/apply` (Story 11.10) and a `partner_apply_started` event is sent under Story 11.8's rules

### Story 11.12: Staff approve or reject partners, and approval goes out on WhatsApp

As an Arena 360 partner manager,
I want to review applications and approve or reject them with a reason,
So that only real resellers and installers get a referral code.

**Covers:** FR189 (approval by WhatsApp with portal link), FR194 (partner approval, audited), AD-18, AD-21, AD-27/AD-34 (messaging adapter), UX-DR92, UX-DR96 (partner approval message), UX-DR115 (key-web-partner-portal, state B)

**Acceptance Criteria:**

**Given** a staff member with the `partners` role
**When** they open Partners → Applications
**Then** they see `applied` partners oldest first with name, business, type, city, cafés served and applied date, and can open one to Approve or Reject

**Given** Approve
**When** it is confirmed
**Then** the partner becomes `approved`, a unique referral code (8 characters, uppercase letters and digits without 0/O/1/I, e.g. `IMRN4K7Q`) and link `arena360.cloud/r/<code>` are generated, `decided_by` and `decided_at` are stored, and one `admin_audit` row is written

**Given** an approval
**When** it is committed
**Then** a WhatsApp message "Your Arena 360 partner application is approved. Log in: https://partners.arena360.cloud/sign-in" is sent once through the messaging adapter (SMS fallback), keyed on the partner id so a retry never sends twice, and a send failure shows on the application as "Message not sent — Resend" without undoing the approval

**Given** Reject
**When** it is confirmed
**Then** a reason is required, the partner becomes `rejected`, the portal shows "Your application wasn't approved." with the reason, and no message is sent beyond that portal status

**Given** an approved partner signing in
**When** the portal opens
**Then** the tracker is replaced by a referral card showing the code and link with "Copy link" and "Copy code", and a banner "KYC needed before first payout" linking to KYC

**Given** a staff member with the `partners` role
**When** they suspend an approved partner with a reason
**Then** the partner becomes `suspended`, the referral link stops attributing new sign-ups, existing attributions and accrued balances stay, and the action is audited

### Story 11.13: An approved partner submits KYC, stored encrypted and readable only by admin

As an approved partner,
I want to give my PAN and bank details once, securely,
So that I can be paid without sending documents over chat.

**Covers:** FR189 (KYC before first payout), FR194 (KYC review, audited), AD-19 (KYC encrypted, admin-read only), AD-22, NFR13, UX-DR18, UX-DR92 (KYC)

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `partner_kyc` (partner_id, status `submitted|verified|rejected`, ciphertext, key version, PAN last 4, account last 4, IFSC, gst_registered flag, submitted_at, reviewed_by, reviewed_at, rejection reason), where PAN, legal name, account number and GSTIN live only inside the AES-256-GCM ciphertext under a KYC data key held in SOPS-managed secrets, never in logs or telemetry

**Given** an approved partner on KYC
**When** they submit legal name, PAN, bank account number (entered twice), IFSC and optional GSTIN
**Then** PAN must match `AAAAA9999A`, IFSC `AAAA0XXXXXX`, a GSTIN must be 15 characters with a valid check digit and contain the given PAN, the two account numbers must match, and errors show per field without clearing the others

**Given** submitted KYC
**When** the partner views it later
**Then** the portal shows only "PAN ••••<last 4>", "Account ••••<last 4> · <IFSC>", "GST registered: yes/no" and the status; no endpoint returns the full values to a partner session

**Given** a staff member with the `partners` or `finance` role
**When** they press "Show KYC" on a partner
**Then** a reason is required, the decrypted values are shown for that view only, and the reveal is written to `admin_audit`; they can then mark the KYC Verified or Rejected (reason required), and a rejected partner sees the reason and can resubmit

**Given** a partner who changes their bank details after verification
**When** they resubmit
**Then** the KYC returns to `submitted`, payouts are held until it is verified again, and the previous ciphertext is kept for the audit history

### Story 11.14: A referral link or code is captured at owner sign-up and locked after the first payment

As a partner,
I want every café that signs up through my link or code to be credited to me and to stay credited,
So that I'm paid for the cafés I bring in.

**Covers:** FR190, AD-19 (URL/cookie → onboarding → tenant, immutable after first payment), AD-18, NFR13

**Acceptance Criteria:**

**Given** a visitor opening `arena360.cloud/r/<code>` or any marketing URL with `?ref=<code>`
**When** the code belongs to an `approved` partner
**Then** a first-party cookie `a360_ref` (code only, 30 days, Secure, SameSite=Lax) is set on `arena360.cloud` and its subdomains and the visitor lands on Home; an unknown, rejected or suspended code sets nothing and the page still loads

**Given** the onboarding sign-up from Story 2.3
**When** it opens
**Then** it shows an optional "Partner code" field prefilled from `a360_ref` (editable), and an entered code is checked on submit with "We don't recognise that code" for an invalid one, which never blocks sign-up

**Given** sign-up completes with a valid code
**When** the tenant is created
**Then** `tenants.referral_partner_id` and `referral_captured_at` are set, and a `referral_attributions` history row is written

**Given** a tenant with no first payment yet (no `subscriptions.first_paid_at` on any of its branches)
**When** the owner adds or changes a partner code under Account → Subscription, or staff change it in the admin app with a recorded reason
**Then** the attribution is updated and the history row records who changed it and when

**Given** a tenant where any branch has `first_paid_at`
**When** anyone tries to set, change or clear the referral
**Then** the API returns `REFERRAL_LOCKED` for owners and staff alike, and the admin app shows the field read-only with "Locked after first payment on <date>"

**Given** a code whose partner phone equals the signing-up owner's phone
**When** sign-up completes
**Then** no referral is recorded (self-referral) and the owner sees no error

### Story 11.15: Partner portal home — referral card, metrics and referred cafés

As a partner,
I want to see my link, how many of my cafés are paying and who they are,
So that I can follow up with the cafés I brought in.

**Covers:** FR192 (referral link/code, referred cafés, no tenant operational data), AD-18, UX-DR81, UX-DR92 (home, empty state), UX-DR100 (partner empty state), UX-DR113, UX-DR115 (key-web-partner-portal, state A)

**Acceptance Criteria:**

**Given** an approved partner on the portal home
**When** it opens
**Then** it shows the referral card (link and code with Copy buttons), a metric row with active paying cafés and commission rate "10%", a referred-cafés table, and navigation Referred cafés · Payouts · Referral link · KYC, in staff mood up to about 1200 px wide, in light and dark

**Given** the referred-cafés table
**When** it renders
**Then** each row shows café name, city, number of branches, plan status per café (Free, Paying, Grace, Lapsed, Refunded) and referred since, from a partner-scoped API that reads only `tenants`, `branches` and `subscriptions` state in `gz_control`

**Given** the partner API
**When** it builds a response
**Then** it never includes owner name, phone, email, GSTIN, revenue figures per café, branch data, R2 files or any Owner API data, and a contract test fails if the DTO gains a field outside the allow-list

**Given** no referred cafés
**When** the home opens
**Then** it shows "No referred cafés yet." with the referral link and Copy button

**Given** a partner who is `suspended`
**When** they sign in
**Then** they see the suspension notice and their payouts only, with the referral card hidden

### Story 11.16: Commission accrues on paid revenue net of refunds

As a partner,
I want each payment from a café I referred to add 10% to my balance, and refunds to take it back,
So that my balance always matches what Arena 360 actually earned from my cafés.

**Covers:** FR191 (10% of paid subscription revenue net of refunds), AD-19 (ledgered, net of refunds, clawback), AD-7 (minor units), AD-9

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `partner_ledger` (id ULID, partner_id, kind `accrual|clawback|adjustment|payout|tds`, amount_minor (INR paise, signed), source invoice or credit-note id unique per kind, tenant_id, branch_id, business month in Asia/Kolkata, statement_id nullable, created_at), append-only with no update or delete

**Given** an Epic 10 invoice issued and paid in band IN for a branch whose tenant has `referral_partner_id`
**When** the invoice is recorded
**Then** one `accrual` line of 10% of `subtotal_minor` (GST-exclusive), rounded half-up to the paisa, is written for that partner, keyed on the invoice id so a replayed event never doubles it

**Given** an Epic 10 credit note reversing a referred invoice
**When** it is issued
**Then** one `clawback` line of the negative of that invoice's accrual is written, whether or not the accrual has already been paid out

**Given** an invoice for a tenant with no referral, a suspended partner's tenant attributed before suspension, or a non-IN band
**When** it is recorded
**Then** no line is written for the first and third; for the suspended partner the accrual is still written (attribution stays)

**Given** staff with the `finance` role
**When** they add an `adjustment` line
**Then** a reason is required, the amount is signed and the action is audited; adjustments are the only manual ledger write

### Story 11.17: The monthly payout job on the 1st writes each partner's statement with rollover, TDS and clawback

As an Arena 360 finance staff member,
I want each partner's statement worked out automatically on the 1st,
So that we pay the right amount by the 10th without spreadsheets.

**Covers:** FR191 (payout by the 10th, ₹500 rollover, 2% TDS, clawback after payout), AD-19 (monthly job, Asia/Kolkata, FY Apr–Mar), AD-7, NFR25

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `partner_statements` (id, partner_id, month, opening carry, accruals, clawbacks, adjustments, gross, tds, net, status `pending_minimum|ready|paid|carried_negative`, paid_at, bank reference, unique on (partner_id, month))

**Given** the jobs role
**When** 00:30 Asia/Kolkata on the 1st arrives
**Then** for every partner with ledger lines or a non-zero carry it computes gross = previous statement's carry + the previous month's accruals + clawbacks + adjustments, writes one statement per partner and month, and links the ledger lines to it

**Given** the job has already run for a month
**When** it runs again for that month (retry or manual rerun)
**Then** no statement or ledger line is added or changed

**Given** gross below ₹500.00 and above zero
**When** the statement is written
**Then** it is `pending_minimum` with no TDS, and its gross is carried into next month's opening carry

**Given** gross ≤ 0 (clawbacks exceed accruals, for example a refund after a paid month)
**When** the statement is written
**Then** it is `carried_negative`, nothing is payable, and the negative amount is the next month's opening carry

**Given** gross of ₹500.00 or more
**When** the statement is written
**Then** TDS is 2% of gross rounded to the paisa, net = gross − TDS, a `tds` ledger line is written, and the status is `ready`

**Given** the job
**When** it finishes or fails
**Then** it records counts and totals to metrics and logs, a failure alerts the ops channel, and a success posts "Statements for <month>: ready <n>, pending minimum <n>, carried negative <n>" to the ops channel

### Story 11.18: Payouts are held until KYC is verified and, for GST-registered partners, a GST invoice is attached

As an Arena 360 finance staff member,
I want statements held automatically when a partner's KYC or GST invoice is missing,
So that we never transfer money to unverified bank details or without the invoice the law needs.

**Covers:** FR189 (KYC before the first payout), FR191 (GST-registered partners upload an invoice or are held), AD-19

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it additively extends the `partner_statements` status with `held_kyc|held_invoice` and adds `invoice_key` (nullable) and `hold_reason`

**Given** a statement of ₹500.00 or more written by the payout job
**When** its status is decided
**Then** it is `held_kyc` if the partner's KYC is not `verified`, else `held_invoice` if the partner is GST-registered and `invoice_key` is empty, else `ready`

**Given** a `held_kyc` statement
**When** the partner's KYC becomes verified
**Then** the statement re-evaluates at once (to `held_invoice` or `ready`) without waiting for the next run

**Given** a `held_invoice` statement
**When** its `invoice_key` is set
**Then** it moves to `ready` at once

**Given** a `ready` statement not yet paid
**When** the partner resubmits KYC (Story 11.13 returns it to `submitted`)
**Then** the statement returns to `held_kyc` until the new KYC is verified

**Given** held statements
**When** the next month's job runs
**Then** they stay as they are (their amounts are not re-added to the next opening carry), the new month gets its own statement, and the ops summary adds "held KYC <n>, held invoice <n>"

### Story 11.19: Partner portal Payouts and GST invoice upload

As a partner,
I want to see every month's accrued, held and paid amounts with TDS, and upload my GST invoice,
So that I know exactly what's coming and nothing is held without me knowing why.

**Covers:** FR191 (GST-registered partners upload an invoice or are held), FR192 (accrued, pending, paid, TDS, invoice status), AD-19, UX-DR81, UX-DR92 (payouts table and states), UX-DR93, UX-DR103

**Acceptance Criteria:**

**Given** the portal Payouts view and the home metric row
**When** they open
**Then** the metric row adds "Payout this month" (the latest statement's net, or "₹0") and the payouts table lists each month with accrued, clawbacks, gross, TDS, net, status and GST-invoice status, plus a current-month row "Accruing" with this month's accruals so far

**Given** status labels
**When** they render
**Then** they read "Pending — below ₹500 minimum, rolls over", "KYC needed before first payout" (linking to KYC), "Upload GST invoice", "Ready — paying by the 10th", "Paid <date> · Ref <bank reference>" and "Recovered from next payout" for a negative carry, always as text with a `status-badge`, never colour alone

**Given** a GST-registered partner with a `held_invoice` or `ready` statement
**When** they upload a PDF (≤ 5 MB, `application/pdf`)
**Then** it is stored under a private partner prefix in R2 (never a branch prefix) via a 10-minute presigned PUT, linked to the statement, the hold is released, and finance staff can open it from the admin app; any other type or size is refused with the reason

**Given** a partner who is not GST-registered
**When** Payouts renders
**Then** no invoice column or upload control is shown

**Given** a statement already `paid`
**When** the partner tries to replace its invoice
**Then** the upload is refused with "This payout is already paid. Contact support to change its invoice."

### Story 11.20: Staff mark payouts paid after a manual bank transfer

As an Arena 360 finance staff member,
I want a list of payouts to make, the bank details to pay, and a way to record each transfer,
So that every partner is paid by the 10th and the books show it.

**Covers:** FR191 (payout by the 10th), FR194 (payout marking after manual transfer, audited), AD-19, AD-21, UX-DR96 (payout statement message)

**Acceptance Criteria:**

**Given** a staff member with the `finance` role
**When** they open Payouts
**Then** they see `ready` statements for the month with partner, gross, TDS, net, KYC status and invoice link, held statements in a separate list with their reason, and a CSV export of the ready list without account numbers

**Given** a ready statement
**When** staff press "Show bank details" with a reason
**Then** the decrypted account number, IFSC and legal name are shown for that view and the reveal is audited

**Given** a completed transfer
**When** staff press "Mark paid" and enter the bank reference (UTR) and transfer date
**Then** the statement becomes `paid` with `paid_at` and the reference, a `payout` ledger line of −net is written, the action is audited, and the partner gets one payout-statement message (WhatsApp, with email copy) showing month, accrued, TDS, paid and invoice status

**Given** a statement that is not `ready`
**When** anyone tries to mark it paid
**Then** the API returns `PAYOUT_NOT_READY` with the hold reason

**Given** a statement marked paid by mistake
**When** a `superadmin` reverses it with a reason
**Then** it returns to `ready`, a compensating ledger line is written, and both actions remain in the audit

**Given** the 10th of the month in Asia/Kolkata
**When** any statement for the previous month is still `ready`
**Then** Payouts shows "<n> payouts are late" and the ops channel is alerted once

### Story 11.21: Staff curate game records in the admin app

As an Arena 360 catalogue editor,
I want to add and correct games, their executables and age ratings in a browser,
So that every café's kiosks match games to proper names without anyone editing files.

**Covers:** FR193 (curate the global game catalogue), FR194 (audited), AD-17, AD-21

**Acceptance Criteria:**

**Given** a staff member with the `catalogue` role
**When** they open Catalogue
**Then** they see the Story 6.1 `catalog_games` list with name, launcher, category, age rating, published, media status (cover yes/no, video screened/unscreened/none), searchable by name or executable file name, paged 50 at a time

**Given** a game
**When** they create or edit it
**Then** they can set name, launcher, category and age rating and add or remove executable signatures (file name, product name, publisher, optional SHA-256), with `base_rev` checked so a concurrent edit returns `STALE` and shows the other editor's change

**Given** a new game with the same name and launcher as an existing one
**When** it is saved
**Then** it is refused `DUPLICATE` with a link to the existing record

**Given** Publish or Unpublish
**When** it is confirmed
**Then** `catalog_version` increments so Hubs pick it up through the Story 6.1 manifest on their next pull, and an unpublished or deleted game is delivered as a tombstone

**Given** any catalogue change
**When** it is saved
**Then** it is written to `admin_audit` with the before and after field values, and the Story 6.1 `arena-cloud catalog import` continues to work against the same tables and audit

### Story 11.22: Every catalogue video is flash-analysed before anyone can approve it

As an Arena 360 catalogue editor,
I want every game video checked automatically for dangerous flashing,
So that no café's screens ever play a video that could trigger a seizure.

**Covers:** FR193 (media flash-screened before publishing), AD-17, AD-21, NFR26

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `catalog_media_screening` (media_id, analyser version, result `queued|pass|fail|error`, failing timestamps, analysed_at, error text), and every existing video `catalog_media` row with `flash_screened = false` is queued

**Given** a video `catalog_media` row with `flash_screened = false` and no screening result (including videos added by the Story 6.1 `catalog import`)
**When** the jobs role picks it up
**Then** it decodes the file from R2 and runs a photosensitivity analysis (general flash and red flash: no more than 3 flashes in any 1-second period and no saturated-red transitions, per ITU-R BT.1702 thresholds), off the request path, and records pass or fail with the failing timestamps

**Given** a failing video
**When** the result is recorded
**Then** the media is marked "Failed flash check at <timestamps>" and can never become `flash_screened`, whatever path tries to set it

**Given** a passing video
**When** the result is recorded
**Then** `flash_screened` stays false and the media waits for a person's approval; passing the analysis alone never publishes it

**Given** a file that cannot be decoded
**When** analysis runs
**Then** it is retried once, then recorded `error` with the reason, and treated as not screened

**Given** any result
**When** it is recorded
**Then** an `admin_audit` row with `actor{kind: system}` records the media id, analyser version and result, and the Story 6.1 manifest still excludes every video whose `flash_screened` is false

### Story 11.23: Staff upload game media and a second person approves screened videos

As an Arena 360 catalogue editor,
I want to upload covers and videos in the admin app and have a colleague sign off each video,
So that game art reaches cafés quickly and no video publishes on one person's say-so.

**Covers:** FR193 (curate media; flash-screened before publishing), FR194 (audited), AD-17, AD-36 (shared catalogue prefix), AD-21

**Acceptance Criteria:**

**Given** a game in the admin app
**When** an editor uploads a cover (JPEG/PNG/WebP, ≤ 2 MB, at least 600 × 900) or a video (MP4 H.264, no audio track, ≤ 30 s, ≤ 20 MB)
**Then** the browser hashes it, uploads it by presigned PUT to `catalog/<sha256>.<ext>` (skipped if the hash exists), a `catalog_media` row is created (video with `flash_screened = false`, entering the Story 11.22 queue), and the upload is audited; files outside the limits or with an audio track are refused with the reason

**Given** a game's media panel
**When** it renders
**Then** each video shows its state as text: "Analysing", "Failed flash check at <timestamps>", "Couldn't analyse — upload again", "Waiting for approval" or "Screened · approved by <name>", and each cover shows a preview

**Given** a video "Waiting for approval"
**When** a staff member with the `catalogue` role who did not upload it watches it and presses "Approve"
**Then** `flash_screened = true` is set, `catalog_version` increments so Hubs receive it on their next manifest pull, and the approver is written to `admin_audit`

**Given** the uploader of a video
**When** they press "Approve" on it
**Then** it is refused `SAME_PERSON` with "Someone else must approve this video"

**Given** a video that failed or errored
**When** anyone tries to approve it
**Then** it is refused `NOT_SCREENABLE`, and the editor's only option is to upload a new file

**Given** a cover or video replaced or removed
**When** it is saved
**Then** the game points to the new media, `catalog_version` increments, the old object stays in R2 (content-addressed, possibly shared), and the change is audited

### Story 11.24: Staff approve or reject 7-day refunds in the admin app

As an Arena 360 finance staff member,
I want refund requests in a queue with the window and amounts shown,
So that owners get eligible refunds quickly and nothing outside the policy is paid.

**Covers:** FR194 (7-day refunds, audited), FR180, AD-20, AD-21

**Acceptance Criteria:**

**Given** a staff member with the `finance` role
**When** they open Billing → Refund requests
**Then** they see `requested` refund requests oldest first with tenant, branch, plan, amount, `first_paid_at`, requested at and time left in the 7-day window, from the Epic 10 `refund_requests` table

**Given** a request
**When** staff press Approve with a reason
**Then** the admin API calls the same refund service as `arena-cloud ops refund approve` (Epic 10), the request moves to `issued`, and a Razorpay error leaves it `requested` with the error shown inline

**Given** a request
**When** staff press Reject with a reason
**Then** the same service as `ops refund reject` runs and the owner sees the reason, as defined in Epic 10

**Given** a request the service refuses (`REFUND_NOT_ALLOWED`)
**When** staff act on it
**Then** the error is shown with the policy reason and nothing changes

**Given** any approve or reject
**When** it completes
**Then** exactly one `admin_audit` row records staff, reason, amounts and request id, whether the action came from the admin app or the CLI

### Story 11.25: Staff grant and revoke comp licences in the admin app

As an Arena 360 staff member,
I want to give a branch a free paid tier for a set time, and take it back,
So that pilots, partners' demo cafés and support goodwill don't need a fake payment.

**Covers:** FR194 (audited admin actions), AD-10, AD-20 (entitlements, no gate commands), AD-21, AD-29

**Acceptance Criteria:**

**Given** a staff member with the `finance` or `superadmin` role
**When** they open a branch in Billing and choose "Grant comp"
**Then** they pick Basic or Premium, an end date no more than 12 months ahead and a reason, and the admin API calls the same service as `arena-cloud ops grant-comp` (Story 2.7), so the entitlement source becomes `comp` and the next heartbeat delivers the licence

**Given** a branch with an active paid subscription
**When** comp is granted
**Then** it is refused `COMP_CONFLICTS_WITH_SUBSCRIPTION` with the subscription shown

**Given** an active comp
**When** staff choose "End comp" with a reason
**Then** the entitlement returns to `free` and the next heartbeat delivers the Free licence; running sessions and open orders are untouched (the Epic 10 lapse rules apply)

**Given** a comp reaching its end date
**When** the cloud clock passes it
**Then** it ends the same way, and Billing lists comps ending in the next 14 days

**Given** comp revenue
**When** the partner ledger runs
**Then** comps generate no invoice and therefore no commission

### Story 11.26: Support sees a tenant and branch summary, and searches audit only with a reason

As an Arena 360 support agent,
I want one page with a branch's plan, versions and connection health, and audit search when I need it,
So that I can help an owner quickly without ever reaching into their Hub or player data.

**Covers:** FR195 (audit search with a recorded reason), FR194 (audited), AD-21, AD-37, NFR13

**Acceptance Criteria:**

**Given** a staff member with the `support` role
**When** they search Support by tenant name, owner phone (matched by hash, never listed), branch code or tenant id
**Then** they get matching tenants and branches, and the search itself is audited

**Given** a branch support view
**When** it opens
**Then** it shows only control-plane facts: tenant and branch names, tier and entitlement source, subscription state, licence claims and expiry, last heartbeat time, Hub and schema versions, device count against cap, pending and failed commands by kind, last batch and summary upload times, and open support cases; no player, sales, staff or session data is shown

**Given** the view
**When** support opens a case
**Then** a `support_cases` row (id, tenant_id, branch_id nullable, opened_by, opened_at, category, closed_at) is created and the case id is shown for reference in later actions

**Given** audit search for a branch month
**When** support enters a reason and a query from the fixed catalogue (actor, module, intent, entity, time range)
**Then** the admin app calls the Story 8.35 audit API with the reason, shows "Preparing audit…" while it builds, and returns rows with players by id only; a missing reason blocks the request

**Given** a support agent
**When** they try any action that would write to a branch
**Then** no such action exists in the support view; owner-side changes are made by the owner

### Story 11.27: The Hub builds a diagnostics bundle with no personal data and sends it only when staff press Send

As a café manager,
I want to send Arena 360 support a technical bundle from my Hub with one button,
So that they can fix my problem without seeing my players' details or logging into my PC.

**Covers:** FR195 (diagnostics bundle < 5 MB, no player PII, sent only on request), AD-21, AD-24, NFR13, UX-DR103

**Acceptance Criteria:**

**Given** a manager in Hub Settings → System → Diagnostics
**When** they press "Create diagnostics bundle"
**Then** the Hub builds a zip with Hub `tracing` logs (most recent first), branch settings without secrets, the device list (name, class, version, last seen, status), licence claims, `PRAGMA integrity_check` result, schema and app versions, upload and command backlog counts, and a `manifest.json` listing every section and what was trimmed

**Given** the bundle
**When** it is built
**Then** it never contains device tokens, licence signatures, the branch KEK or player keys, PIN hashes or verifiers, passwords, player names, phones or emails, and a redaction pass removes anything matching phone, email or PAN patterns from log lines

**Given** the bundle would exceed 5 MB compressed
**When** it is built
**Then** the oldest log lines are dropped until it is under 4.5 MB, and the manifest records what was trimmed

**Given** a built bundle
**When** it is shown to the manager
**Then** they see its size and a list of what it contains, and two buttons, "Send to Arena 360 support" and "Save to file"; nothing leaves the Hub until one is pressed

**Given** "Send to Arena 360 support" with the WAN up
**When** it is pressed
**Then** the bundle is uploaded by presigned PUT to `tenants/<t>/branches/<b>/diagnostics/<ulid>.zip` (30-day lifecycle), a `diagnostics.sent` intent is audited with the manager, and the Hub shows a reference "DIAG-<6 chars>" to quote to support

**Given** the WAN is down
**When** the manager presses Send
**Then** it says "No internet — save the file and share it with support another way" and offers "Save to file" (folder or USB); nothing is queued to send later

**Given** a seeded branch with known player names, phones and emails
**When** the CI privacy test builds a bundle
**Then** none of those values appear anywhere in it

### Story 11.28: Kiosk logs are collected over the LAN into the diagnostics bundle

As an Arena 360 support agent,
I want the bundle to include recent logs from each gaming PC,
So that I can see kiosk-side problems without asking the café to copy files from every PC.

**Covers:** FR195 (diagnostics bundle < 5 MB, no player PII), AD-31 (kiosk service), AD-42 (paired-device class), AD-25, NFR13

**Acceptance Criteria:**

**Given** a diagnostics bundle being built at the Hub
**When** kiosks are online
**Then** the Hub sends each paired kiosk a `diag.logs_request{since: now − 24 h, max_bytes: 200 KB}` frame over its authenticated paired-device connection, and only the paired-device class may receive or answer it

**Given** a kiosk receiving the request
**When** its service answers
**Then** it returns the newest 24 h of its ring-buffer log, at most 200 KB compressed, after the same phone, email and PAN redaction pass as the Hub, read from disk by the service with no UI shown and no effect on a running session

**Given** the answers
**When** the bundle is assembled
**Then** each kiosk's log is added under `kiosks/<device name>.log`, kiosk logs together are capped at 2 MB, and the whole bundle still stays under 4.5 MB by the Story 11.27 trimming rule

**Given** a kiosk that is offline, does not answer within 10 s, or runs a release without `diag.logs_request` (`UNSUPPORTED`)
**When** the bundle is assembled
**Then** the manifest lists it as "not collected — offline", "not collected — timed out" or "not collected — update needed", and the bundle is still built without waiting further

**Given** a kiosk with seeded log lines containing a player's phone and email
**When** the CI privacy test builds a bundle
**Then** neither value appears in it

### Story 11.29: Support requests and downloads diagnostics bundles from the admin app

As an Arena 360 support agent,
I want to ask a branch for a diagnostics bundle and open what they send,
So that I can investigate without asking the owner to dig through folders.

**Covers:** FR195 (sent only on request), AD-5 (command mailbox), AD-21, AD-36, NFR13

**Acceptance Criteria:**

**Given** an open support case for a branch
**When** support presses "Request diagnostics" with a reason
**Then** a `diagnostics.request` command (case id, requested by, reason) is queued through the command mailbox, the action is audited, and the case shows "Requested — waiting for the branch"

**Given** the Hub applies `diagnostics.request`
**When** staff are signed in
**Then** the Hub shows a persistent banner "Arena 360 support asked for a diagnostics bundle (case <id>). Create and send / Decline" for managers; the bundle is never built or sent without a manager pressing Send, and Decline is reported back as the command result

**Given** a bundle sent from the Hub (on request or on its own)
**When** support opens Support → Diagnostics for the branch
**Then** they see each bundle with reference, size, sent at and sent by role, and can download it only after entering a reason; each download is a 5-minute presigned GET and is audited

**Given** a bundle older than 30 days
**When** it is listed
**Then** it shows "Expired" and cannot be downloaded

**Given** a Hub on a release that doesn't know `diagnostics.request`
**When** the command is fetched
**Then** it is rejected `UNSUPPORTED` and the case shows "Branch needs an update to send diagnostics on request — ask the manager to use Settings → System → Diagnostics"

### Story 11.30: Launch metrics are computed nightly and shown in the admin app

As the Arena 360 team,
I want the launch success metrics computed every night from data we already hold,
So that we know whether onboarding, conversion, churn, shift accuracy, support load and ad fill are on target.

**Covers:** NFR27, AD-9, AD-35, NFR13, NFR25

**Acceptance Criteria:**

**Given** the Hub heartbeat
**When** the first session at a branch is settled with a payment
**Then** the heartbeat carries `first_settled_session_at` (a timestamp only, sent until the cloud acknowledges it) and the cloud stores it on the branch once

**Given** the jobs role at 02:00 Asia/Kolkata
**When** the metrics job runs
**Then** it writes `launch_metrics` (date, metric, value, numerator, denominator) for: share of branches with first settled session < 60 min after owner sign-up (target 80%); free-to-paid within 90 days of sign-up (20%); paid churn this month (< 3%); median per-shift cash variance after a branch's first 30 days, from paid branches' rollups (< ₹50); share of branches with a support case this month (< 2%); Free ad fill rate from `ad_impressions` (40%); plus the Story 11.8 funnel counts (site visits → Get started → sign-up → OTP verified → live)

**Given** a staff member with any admin role
**When** they open Metrics
**Then** each metric shows its latest value against its target with ok or warning text, a 12-week trend table and its numerator and denominator, and missing inputs show "Not enough data yet" rather than zero

**Given** the metrics
**When** they are computed
**Then** they use only ids, timestamps, counts and amounts already in `gz_control` or rollups; no phone, email, name or player id enters `launch_metrics`

**Given** a rerun for a past date
**When** it executes
**Then** it overwrites that date's rows only and the result matches the first run for unchanged inputs

### Story 11.31: Partner money and admin access proven end to end

As the Arena 360 team,
I want the partner money flow and the access boundaries tested before every release,
So that partners are paid correctly and nobody reaches data they shouldn't.

**Covers:** FR188–FR195, AD-18, AD-19, AD-21, NFR10, NFR11, NFR13

**Acceptance Criteria:**

**Given** the Razorpay fake and a controllable cloud clock
**When** the partner suite runs
**Then** it walks: apply → approve (one WhatsApp) → KYC verify → owner signs up through the referral link → first payment (attribution locks) → monthly accruals → a month under ₹500 that rolls over → a month over ₹500 with 2% TDS → GST-registered hold → invoice upload → mark paid → a refund after payout that claws back into the next statement, asserting every ledger line and statement total to the paisa

**Given** the same statement month
**When** the payout job and webhook replays run twice
**Then** no ledger line or statement is duplicated

**Given** the access suite
**When** it runs
**Then** a partner session gets 403 on every owner, tenant, branch and admin route; an owner session gets 403 on every partner and admin route; an admin route without a valid Access JWT gets 401; a valid Access identity without a staff row gets 403; and every admin route in the router appears in the audit-action map

**Given** the privacy checks
**When** they run
**Then** partner API responses contain only allow-listed fields, KYC values appear nowhere outside the ciphertext column, the diagnostics bundle from a seeded branch contains no seeded PII, and `product_events` holds no seeded sign-up values
