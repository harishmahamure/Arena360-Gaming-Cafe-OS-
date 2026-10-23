## Epic 2: Owner onboarding and pairing

An owner signs up with OTP/TOTP, completes the wizard, pairs the Hub and kiosks, invites staff and runs a 2-minute test session.

> Draft notes: this epic replaces Epic 1's dev fixtures on the real path — `branch.bootstrap` now arrives at first Hub pairing (Story 2.12), staff credentials come from one-time setup codes (Story 2.13), and kiosk tokens and the Hub address come from mDNS discovery and code pairing (Stories 2.16–2.17). The `dev-fixtures` feature stays for automated tests only. FR6 is delivered for the Free path only; the paid-tier payment branch is Epic 10. Because a Windows Hub is refused on Free (FR14) and the Android Hub ships in Epic 4, Story 2.7 adds an operator-issued **comp entitlement** so internal and beta cafés can pair a Windows Hub end to end before Epic 10. The command mailbox built here (Story 2.14) carries only the **setup command kinds** (`branch.bootstrap`, `group.upsert`, `device.assign`, `device.register`, `staff.upsert`, `settings.set` for branch name/tz, `setup.test_session`, `branch.go_live`), which run on every tier; the full owner command UI, fan-out, step-up MFA, sweeper and fencing are Epic 8. In wizard step 4 the owner creates groups and drags devices into them in the web wizard, delivered to the Hub as setup commands; devices arriving are shown from the heartbeat. Steps 5 and 8 (rates, printer) are done at the Hub and the web wizard mirrors their progress from the heartbeat. Each story lists what it covers on its **Covers** line.

### Story 2.1: Arena 360 cloud runs in India behind Cloudflare

As a café owner signing up,
I want the Arena 360 service I talk to hosted in India, reachable only over HTTPS and returning plain, consistent errors,
So that my account data stays in India and every onboarding screen behaves predictably.

**Covers:** NFR14, AD-9, AD-22, AD-40; Additional Requirements "Infrastructure and deployment" (Hostinger K8s, Turso Mumbai, environments), "Build spikes" (single SQLite linkage)

**Acceptance Criteria:**

**Given** the `arena-cloud` binary started with `--role control`
**When** it is deployed by CI to the Kubernetes cluster on the Hostinger VPS in Mumbai
**Then** it runs as a Deployment in a `staging` namespace on every merge to main and in `production` on a release tag, served at `api.staging.arena360.app` and `api.arena360.app` behind Cloudflare (TLS terminated at the edge, origin reachable only from Cloudflare IP ranges)
**And** `GET /healthz` returns role, version, `gz_control` reachability and migration version

**Given** the `gz_control` database
**When** the control role starts
**Then** it connects to Turso in the Mumbai region via libsql (local SQLite file in dev), applies numbered forward-only embedded migrations recorded in a migrations table, and refuses to start with `SCHEMA_TOO_NEW` if the database is newer than the binary
**And** this story creates no business tables; later stories add only the tables they need
**And** `arena-cloud` links one SQLite library shared by rusqlite and libsql (build spike), checked by a CI test that fails on duplicate symbol linkage

**Given** cloud secrets (Turso token, signing keys added later)
**When** CI deploys
**Then** they are decrypted from SOPS 3.13.3 + age 1.3.2 files in the repo into Kubernetes secrets, and a CI check fails if any plaintext secret or VPS env file is committed

**Given** any `/v1` request that fails
**When** the API responds
**Then** the body is the `AppError{code, message, field?}` envelope with a stable code from `arena-proto`, never a stack trace, and unknown paths return `NOT_FOUND`

**Given** the dev environment
**When** a developer runs the documented `arena-cloud --role all --dev` command
**Then** the cloud runs locally with SQLite for `gz_control` and MinIO for R2, with no Turso or Cloudflare account required

### Story 2.2: Control data is backed up nightly and the cloud is observable

As a café owner,
I want Arena 360's account and licence data backed up every night and the service watched for errors,
So that a cloud failure never loses my account and problems are found before they reach me.

**Covers:** NFR21, NFR25, NFR13 (no PII in telemetry); Spine §Operational Envelope "DR", "Observability"

**Acceptance Criteria:**

**Given** the production `gz_control`
**When** the `jobs` role CronJob runs nightly
**Then** it writes an encrypted dump to an off-provider bucket and keeps the latest 30 nightly copies, deleting older ones

**Given** a nightly dump
**When** the documented restore procedure is run into staging
**Then** the control role starts on the restored database with every table and row count matching the dump, and the drill is run once before GA to prove the 4 h RTO

**Given** a nightly backup fails
**When** the job exits non-zero
**Then** an alert fires to the on-call channel and the next run is not skipped

**Given** any `/v1` request to `arena-cloud`
**When** it is handled
**Then** an OpenTelemetry trace with role, route, status and tenant/branch ids (when known) is exported to Grafana Cloud, and unhandled errors go to Sentry

**Given** traces, logs and Sentry events
**When** they are recorded
**Then** request bodies, phone numbers, emails, OTPs, passwords, tokens and codes are scrubbed, verified by a test that sends a sign-up request and asserts none of its field values appear in exported telemetry

### Story 2.3: Owner signs up with phone OTP

As a café owner,
I want to sign up with my name, phone, email, café name and city and confirm my phone with a one-time code,
So that my account exists only once I've proved the phone is mine.

**Covers:** FR1, AD-10 (SMS OTP at sign-up), AD-23, AD-27/AD-34 (messaging adapter, OTP only), NFR11, NFR13, NFR15, NFR23, UX-DR5, UX-DR6, UX-DR18, UX-DR19, UX-DR20, UX-DR91, UX-DR113

**Acceptance Criteria:**

**Given** the onboarding web app (SvelteKit, `adapter-cloudflare`, Workers static assets) at its "Get started" route
**When** a visitor opens it
**Then** it shows text fields for name, phone (with country code, default +91), email, café name and city, one primary "Send code" button, and a footer `theme-toggle` (System · Light · Dark, System default, remembered per device), in light and dark themes

**Given** a submission with a missing or invalid field (phone not E.164-valid, email malformed)
**When** "Send code" is pressed
**Then** validation runs on submit and blur, focus moves to the first invalid field, and the error is announced; nothing is sent

**Given** a valid form
**When** "Send code" is pressed
**Then** the cloud creates an `otp_challenges` row (code hashed, 6 digits, 5-minute expiry) and sends the code by SMS through the messaging adapter (Gupshup at launch; a recorded fake in dev and CI), and the page shows a 6-cell code field with "Resend code" available after 30 s
**And** no `owners` row exists yet

**Given** the correct code within 5 minutes
**When** it is entered
**Then** the `owners` row is created (name, phone, email, café name, city, `mfa_state = pending`), the challenge is consumed, a browser session cookie (HttpOnly, Secure, SameSite=Lax, rotating refresh) is issued, and the owner lands on password + TOTP setup (Story 2.4)

**Given** a wrong code
**When** it is entered
**Then** a plain inline error shows and the cells clear; after 5 wrong attempts the challenge is void and a new code must be requested

**Given** a phone number that already belongs to an owner
**When** sign-up is submitted
**Then** the code is still sent (no account enumeration), and a correct code leads to sign-in for that existing account instead of creating a duplicate

**Given** repeated sends
**When** a phone requests more than 5 codes in an hour, or an IP exceeds the Cloudflare per-IP limit on the sign-up endpoints
**Then** further sends are refused with `RATE_LIMITED` and the page says when to try again; per-phone counts live in `gz_control.rate_limits`

**Given** the sign-up flow
**When** an axe scan runs in both themes
**Then** it has landmarks, one `h1`, labelled fields, all targets ≥ 44 px and zero serious/critical violations, and logs contain no phone number or email in clear

### Story 2.4: Owner sets a password and enrols TOTP with backup codes

As a café owner,
I want to set a password and link an authenticator app, and get backup codes,
So that my account can't be taken over with just my phone number.

**Covers:** FR2, AD-10, NFR11, NFR15 (WCAG 3.3.8), UX-DR18, UX-DR112, UX-DR113

**Acceptance Criteria:**

**Given** a newly verified owner with `mfa_state = pending`
**When** they reach account setup
**Then** they must set a password (at least 10 characters, rejected if it appears in a bundled common-password list, show/hide and paste allowed) before any wizard step opens

**Given** a password is set
**When** the TOTP step opens
**Then** the cloud generates a TOTP secret (RFC 6238, SHA-1, 6 digits, 30 s), stores it encrypted in `owner_mfa`, and shows it as a QR for the authenticator app plus the secret as copyable text for owners who cannot scan

**Given** the owner enters a valid current TOTP code (±1 step tolerance)
**When** they confirm
**Then** `mfa_state` becomes `enrolled`, 10 single-use backup codes are generated, shown once with Copy and Download (.txt), and stored only as Argon2id hashes in `owner_backup_codes`
**And** the owner must tick "I've saved my backup codes" before continuing to the wizard

**Given** an invalid TOTP code at enrolment
**When** it is submitted
**Then** a plain inline error shows and enrolment stays pending; leaving and returning resumes at the TOTP step with a new secret

**Given** an enrolled owner in Account → Security
**When** they choose "Regenerate backup codes" and confirm with a current TOTP code
**Then** all previous backup codes stop working and 10 new ones are shown once

**Given** an enrolled owner
**When** they turn on "SMS code as a fallback"
**Then** the setting is stored and SMS becomes an offered alternative at sign-in (Story 2.5); it is off by default

### Story 2.5: Owner signs in on a new device with TOTP, backup code or SMS fallback

As a café owner,
I want signing in on a new phone or laptop to ask for my authenticator code, with backups if I don't have it,
So that only I can open my account, and I'm never locked out by a lost phone.

**Covers:** FR2, AD-10, AD-23, AD-40, NFR11, NFR23, UX-DR18, UX-DR112

**Acceptance Criteria:**

**Given** an enrolled owner on a browser without a trusted-device cookie
**When** they sign in with phone (or email) and password
**Then** a TOTP challenge is required before any session is issued
**And** a correct code issues the session cookie and offers "Trust this device for 30 days", recorded in `owner_devices` with a device label and last-seen time

**Given** a browser with a valid trusted-device cookie
**When** the owner signs in with the correct password
**Then** no TOTP prompt is shown

**Given** the TOTP challenge
**When** the owner chooses "Use a backup code" and enters an unused one
**Then** sign-in succeeds, that code is consumed, and the remaining count is shown with a prompt to regenerate when 3 or fewer remain

**Given** SMS fallback is enabled for the owner
**When** they choose "Text me a code" at the challenge
**Then** an SMS OTP (6 digits, 5 minutes) is sent through the messaging adapter and accepted in place of TOTP; if fallback is off, the option is not shown

**Given** wrong password or wrong second factor
**When** submitted
**Then** the error never says which part failed, and after 5 consecutive failures the account's sign-in pauses for 15 minutes with the time shown; Cloudflare per-IP limits apply to the sign-in endpoints

**Given** a signed-in owner
**When** the access cookie expires
**Then** the refresh token rotates silently; reuse of an already-rotated refresh token revokes the whole session family and forces sign-in

**Given** Account → Security
**When** the owner views trusted devices
**Then** each device shows label and last seen, and "Sign out everywhere" revokes all sessions and trusted-device cookies

### Story 2.6: Resumable setup wizard and step 1 — café and first branch

As a café owner,
I want a 10-step setup that remembers where I stopped, starting with my café's details,
So that I can set up my branch over several sittings without redoing anything.

**Covers:** FR4, FR5, FR177 (create first branch, set timezone), AD-3 (`branch_code` assigned by control plane), AD-7, AD-9, UX-DR84, UX-DR91, UX-DR113, UX-DR116 (phone dots)

**Acceptance Criteria:**

**Given** an owner with `mfa_state = enrolled` and no tenant
**When** they open the wizard
**Then** `wizard-progress` shows "Step 1 of 10" and ten ≥ 44 px rows — Café details, Subscription tier, Install Hub, Devices, Rate plans, Products, Staff, Printer & payments, Test session, Go live — with "Can skip" tags on Products, Staff and Printer & payments; the current row has `aria-current="step"`; below 840 px it shows 10 dots plus "Step 1 of 10 · Café details"

**Given** step 1
**When** it opens
**Then** café name and city are prefilled from sign-up, and the owner sets country, currency (ISO 4217, defaulted from country), timezone (IANA, defaulted from country), and GST registered yes/no with GSTIN required and format-checked only when yes

**Given** valid step 1 values
**When** the owner continues
**Then** the cloud creates the `tenants` row and the first `branches` row (name, tz, currency, GST flag, GSTIN) in one transaction, with `tenant_id` taken from the session only, and assigns a `branch_code` (3–6 uppercase letters/digits derived from the branch name, unique per tenant) shown as editable until the Hub pairs
**And** step progress is stored in `onboarding_progress` (branch, step, state, completed_at)

**Given** an owner who leaves mid-wizard
**When** they return on any device
**Then** the wizard opens on the first incomplete step; done steps show an ok check and link back for edits; upcoming steps beyond the first incomplete one are not openable (direct URL redirects to the current step)

**Given** a done step re-opened for edit
**When** the owner saves a change
**Then** the change is stored, completion is kept, and the owner returns to the current step

**Given** a step completes by itself (later stories)
**When** the wizard advances
**Then** focus moves to the new step's heading and the change is announced once in a polite live region

**Given** a request that names a tenant or branch the session doesn't own
**When** it reaches the API
**Then** it returns `FORBIDDEN` and nothing is read or written

### Story 2.7: Step 2 — Free tier issues a signed licence

As a café owner,
I want to choose the Free tier without a card and have my branch's entitlement issued at once,
So that I can start installing straight away and the Hub knows exactly what my branch may do.

**Covers:** FR6 (Free path), AD-10 (licence JWT, claims, signing key), AD-20 (entitlement source, no gate commands), AD-29, NFR11, NFR20, UX-DR25, UX-DR93; Additional Requirements "Build spikes" (ed25519-dalek + jsonwebtoken EdDSA)

**Acceptance Criteria:**

**Given** step 2
**When** it opens
**Then** it shows a Monthly · Yearly `tab-control` and three equal cards — Free ₹0 (1 branch, ads on the Android Hub), Basic ₹200/branch/month + 18% GST (yearly ₹2,000), Premium ₹1,999 + 18% GST (yearly ₹19,990) — with values identical to the shared pricing table in the UI package

**Given** the owner picks Free
**When** they continue
**Then** no payment is asked, an `entitlements` row (branch, tier `free`, source `free`, add-ons none, issued_at) is written, and step 2 is marked done

**Given** the tenant already has a Free branch
**When** the owner picks Free for another branch
**Then** Free is disabled with the inline reason "One Free branch per account"

**Given** the owner picks Basic or Premium
**When** billing is not yet enabled (feature flag off until Epic 10)
**Then** Continue is disabled with an inline reason and no entitlement is written; nothing else in the wizard changes

**Given** an entitlement
**When** the licence module signs a licence for the branch
**Then** it is an EdDSA JWT with claims `{tenant_id, branch_id, tier, flags, features, upload_interval_s, summary_interval_s, rollup_schema, retention_days, limits, iat, exp = iat + 7 d}`, signed with the key from SOPS-managed secrets
**And** for Free: no paid flags, `upload_interval_s = summary_interval_s = 0` (no uploads), `limits = {devices: 20, users: 3, branches: 1}`; tier defaults live in one table in `arena-proto` shared by cloud and Hub

**Given** an Arena 360 operator running the audited `arena-cloud ops grant-comp --branch <id> --tier basic|premium --until <date>` command
**When** it completes
**Then** the branch's entitlement source becomes `comp` with that tier, the licence carries paid limits (Basic devices 60, Premium 200), the action is logged with operator and reason, and the wizard shows step 2 done for that branch

**Given** the signing and two-verify-key setup
**When** the licence unit tests run
**Then** a token signed with the current key verifies against the Hub's two-key verify set, a token signed by an unknown key fails, and a token with an edited claim fails

### Story 2.8: Step 3 — single-use 8-character Hub pairing code

As a café owner,
I want a short code shown large that I type into the Hub installer, with a copy button and a visible expiry,
So that pairing the Hub is one typed code and nothing else.

**Covers:** FR13 (code display, expiry, regenerate), AD-10 (single-use short-expiry code), UX-DR85, UX-DR84, UX-DR113, UX-DR116 (no QR)

**Acceptance Criteria:**

**Given** steps 1 and 2 are done
**When** step 3 opens
**Then** the cloud creates a `pairing_codes` row (code hash, branch_id, tier at issue, `expires_at = now + 15 min`, `used_at` null) and the page shows "1 · Download" (Hub for Windows 10 or 11, plus an "Installing on an Android tablet or phone instead?" link), "2 · Install", "3 · Type the code", and the `pairing-code` card

**Given** the `pairing-code` card
**When** it renders
**Then** the 8-character code is shown in `web-display` tabular figures grouped 4 + 4, from an alphabet without ambiguous characters (no 0/O, 1/I/L, U), with a secondary "Copy code" button, "Single use" and an "Expires in mm:ss" countdown; there is no QR anywhere on the page

**Given** the countdown
**When** it runs
**Then** it is announced to screen readers only at 10, 5 and 1 minutes remaining and at expiry, never every tick

**Given** "Copy code" is pressed
**When** the clipboard write succeeds
**Then** a polite status "Code copied" is announced; if it fails the code stays selectable for manual copy

**Given** the code expires
**When** the countdown reaches zero
**Then** the code is struck through with an "Expired" `status-badge`, the message "This code has expired. Generate a new code and type it into the Hub installer." shows, and "Generate a new code" becomes the one primary action

**Given** "Generate a new code" is pressed (or the page is reloaded while a code is live)
**When** a new code is issued
**Then** every earlier unused code for that branch is invalidated, so only one code is ever valid per branch

**Given** the codes stored in `gz_control`
**When** the table is inspected
**Then** only a hash of each code is stored; the code encodes nothing itself and maps to tenant, branch and tier only by lookup

### Story 2.9: Hub installer pairs with the code and retries in place

As a café owner at the counter PC,
I want the Hub installer to ask only for the code, and to let me retype it if it's wrong,
So that installing the Hub never needs a restart or anything else typed.

**Covers:** FR13 (installer retry in place), FR14 (code exchange, token, licence, Windows Hub on Free refused), AD-10, AD-22, AD-23, NFR11, NFR23, UX-DR18, UX-DR103 (installer bad-code line)

**Acceptance Criteria:**

**Given** a freshly installed Windows Hub with no pairing
**When** the Hub UI opens
**Then** it shows the always-dark "Pair this Hub" screen with one 8-character code field (case-insensitive, space ignored), a primary "Pair" button and "Quit setup"; no other Hub screen is reachable until paired

**Given** a valid, unused, unexpired code
**When** the Hub posts `POST /v1/pair {code, platform: windows|android, hub_instance_id, version}`
**Then** in one transaction the cloud marks the code used, mints a 256-bit Hub device token, stores its Argon2id hash in a new `hubs` row (branch_id, platform, instance id, paired_at), and returns the token once with tenant id, branch id, tier, `branch_code` and the first licence JWT
**And** the Hub stores the token with DPAPI (Android Keystore on Android), writes the licence to `licence_cache`, and records branch and tenant ids locally

**Given** a wrong, used or expired code
**When** "Pair" is pressed
**Then** the installer stays on the same screen, clears the field, keeps focus in it, and shows "That code didn't work. Get a new code from your browser."; the response code (`PAIR_CODE_INVALID` / `PAIR_CODE_EXPIRED`) is logged but the text does not say which

**Given** a Free branch
**When** a `platform: windows` Hub tries to pair
**Then** pairing is refused with `FEATURE_NOT_ENTITLED`, the code is not consumed, and the installer explains that Free runs on the Android Hub and links to the Android download; `platform: android` with the same code is accepted (verified by an API test)

**Given** repeated attempts
**When** one code receives 5 failed attempts, or one IP makes more than 20 pairing attempts in an hour
**Then** the code is invalidated or the IP is refused with `RATE_LIMITED`, enforced both by a Cloudflare per-IP rule on `/v1/pair` (committed as config in the repo) and in `arena-cloud` against `gz_control.rate_limits`

**Given** the cloud is unreachable from the counter PC
**When** "Pair" is pressed
**Then** the installer shows a plain "Can't reach Arena 360 — check the internet and try again" message and keeps the typed code

**Given** a Hub that is already paired
**When** it restarts
**Then** it skips the pairing screen and the code endpoint is never called again

### Story 2.10: Hub checks in by heartbeat and the wizard advances by itself

As a café owner,
I want the setup page to move on by itself the moment my Hub connects,
So that I know the Hub is paired without refreshing or guessing.

**Covers:** FR13 (advance on first heartbeat), AD-10 (licence refresh), AD-38 (provisional epoch confirmed on first heartbeat), AD-44 (cloud time), NFR11, UX-DR84, UX-DR113

**Acceptance Criteria:**

**Given** a paired Hub
**When** its heartbeat task runs (every 10 s until the branch is live, then every 60 s)
**Then** it posts `POST /v1/heartbeat` with bearer device token, epoch, version, schema version and `last_seq_uploaded` (0 on Free), and never blocks any intent while doing so

**Given** a heartbeat with a valid token
**When** the cloud handles it
**Then** it updates `hubs.last_seen_at`, re-signs the licence from the current entitlement, and returns `{licence, fence, cloud_time}`; the Hub replaces `licence_cache` with the new licence

**Given** the branch's first heartbeat and no `branch_fence` row
**When** the cloud handles it
**Then** it creates `branch_fence` with the Hub's provisional epoch by compare-and-set, and the Hub clears `lease.provisional`

**Given** the wizard is open on step 3
**When** the first heartbeat for that branch is recorded
**Then** step 3 is marked done and the page advances to step 4 within 5 s (server-sent events with a 5 s polling fallback), moving focus to the step 4 heading

**Given** a heartbeat with an unknown or revoked token
**When** the cloud handles it
**Then** it returns `TOKEN_INVALID` / `TOKEN_REVOKED`, and the Hub shows a staff-visible "This Hub is no longer paired" state without stopping local trading

**Given** the WAN is down
**When** heartbeats fail
**Then** the Hub retries with jittered backoff up to its normal interval, Epic 1's `chip-offline` behaviour applies, and nothing else changes

### Story 2.11: Hub verifies its licence against trusted time and enforces its limits

As a café owner,
I want the Hub to obey only a genuine, current licence and keep working offline,
So that tampering can't unlock paid features and a dropped internet never stops my café.

**Covers:** AD-10 (two verify keys, tampering ⇒ Free limits), AD-29 (entitlement checks in `apply()`), AD-44, NFR11, NFR22

**Acceptance Criteria:**

**Given** a licence in `licence_cache`
**When** the Hub loads it at start and after each heartbeat
**Then** it verifies the EdDSA signature against the two verify keys compiled into the Hub release and checks `branch_id`/`tenant_id` match the paired branch
**And** the verified claims are exposed to `apply()` through one entitlement API (`limit(name)`, `has_flag(name)`) used by every gate

**Given** a licence that fails verification or names another branch
**When** the Hub loads it
**Then** the Hub applies Free limits, keeps running (a Windows Hub does not lock), shows staff "Licence couldn't be verified" in the header alert list, and writes an audit row

**Given** each heartbeat response
**When** it carries `cloud_time`
**Then** the Hub stores `trusted_clock {cloud_ms, mono_ms, wall_delta_ms}`, persists it across restarts, and evaluates licence `exp` against `trusted_now` = last cloud time + monotonic elapsed

**Given** the wall clock differs from `trusted_now` by more than 5 minutes
**When** the check runs
**Then** staff see a "PC clock is wrong" alert and an audit row is written; business timestamps keep using the wall clock

**Given** a licence whose `exp` has passed on `trusted_now`
**When** the Hub evaluates it
**Then** a Free licence keeps working indefinitely offline; a paid (comp) licence falls back to Free limits for new pairings only and nothing running is ended

**Given** the user changes the wall clock back by a week
**When** licence expiry is evaluated
**Then** the expiry decision is unchanged because it uses `trusted_now`

### Story 2.12: branch.bootstrap applies the onboarding profile at first pairing, with branch code lock

As a café owner,
I want the Hub to set itself up with the details I entered online the moment it pairs,
So that I never type my café's name, timezone, currency or tax details twice.

**Covers:** FR14 (`branch.bootstrap`), FR5, AD-3, AD-7, AD-2, AD-14, NFR25, UX-DR102 (owner's setup code display), UX-DR103

**Acceptance Criteria:**

**Given** a successful `/v1/pair` for a branch's first Hub
**When** the response is built
**Then** it carries a `branch.bootstrap` command `{idempotency_key, profile: {name, branch_code, currency, tz, day_start_min, gst_registered, gstin, tax_mode}, owner_staff: {name, login = owner phone, role: Owner, setup_code_hash, setup_expires_at}}`; the setup code itself never travels

**Given** the Hub receives the bootstrap command
**When** it applies it through `apply()` with `actor{kind: owner, id}`
**Then** the `branch` row and settings defaults are created and the owner's staff row (role Owner, no password or PIN) is created, all as change rows under one intent; `tax_mode` is `gst_in` only for a GST-registered Indian branch, else `simple`

**Given** a branch already bootstrapped
**When** a second bootstrap arrives (retry or re-pair)
**Then** the idempotency key applies it once; a different key is rejected `ALREADY_BOOTSTRAPPED` and nothing changes

**Given** the Hub is bootstrapped
**When** the wizard advances past step 3
**Then** the owner sees once "Your Hub sign-in code: XXXX XXXX. Enter it at the Hub to choose your PIN and password." with Copy; the plaintext is held encrypted in `gz_control` only until shown or 24 h, then deleted

**Given** a release build of the Hub
**When** it starts unpaired
**Then** it has no dev seed path; the only way to a bootstrapped branch is pairing (the dev fixture remains in the `dev-fixtures` feature for tests)

**Given** a bootstrapped branch
**When** a Manager or Owner changes `branch_code` in Settings → Branch
**Then** `settings.set` accepts it (uppercase letters/digits, 3–6) while `branch.code_locked_at` is null, and rejects it with `BRANCH_CODE_LOCKED` once set
**And** a system function `lock_branch_code()` (to be called by the invoice engine) sets `code_locked_at` idempotently, covered by a unit test

### Story 2.13: Owner and staff set their own PIN and password at the Hub with a one-time setup code

As a staff member (or the owner) signing in to the Hub for the first time,
I want to enter my setup code and choose my own PIN and password,
So that nobody else ever knows my credentials.

**Covers:** FR104 (Hub half), AD-10, AD-30, NFR11, NFR25, UX-DR66, UX-DR102, UX-DR103

**Acceptance Criteria:**

**Given** the Hub sign-in screen
**When** a person chooses "First time here? Use a setup code"
**Then** the screen shows "Enter your setup code, then choose your PIN and password." with login (phone) and an 8-character code field

**Given** a staff row whose `setup_code_hash` matches and `setup_expires_at` is in the future
**When** the code is submitted
**Then** the person chooses a password (same policy as Hub staff passwords) and a 4–6 digit PIN entered twice, and `staff.set_credentials` stores Argon2id hashes, clears `setup_code_hash`, bumps `pin_verifier_version`, and signs them in

**Given** a wrong, used or expired code
**When** it is submitted
**Then** `SETUP_CODE_INVALID` is shown as one plain line that doesn't say which, and after 5 failures for that login entry pauses for 60 s with a countdown

**Given** a staff member whose credentials are already set
**When** anyone submits a setup code for them
**Then** it is rejected; codes work once

**Given** `staff.set_credentials`
**When** its change row is written
**Then** the row carries `actor{kind: staff, id: self}` and the stored hashes, never the PIN, password or code in clear, and the PIN never appears in any log

**Given** the owner completed this with their own code from Story 2.12
**When** they sign in normally later
**Then** they use their phone and password like any staff member, with the Owner role

**Given** the Hub release build
**When** it runs
**Then** no staff are seeded; the dev staff fixture exists only under `dev-fixtures`

### Story 2.14: Owner changes reach the Hub through the command mailbox within one poll

As a café owner,
I want changes I make online during setup to reach my Hub reliably and exactly once,
So that groups, device assignments, staff invites and setup actions land even if the Hub was briefly offline.

**Covers:** FR104 ("within one poll"), FR8 (delivery path), AD-5 (setup kinds only), AD-14, AD-39, AD-40, NFR11, NFR25

**Acceptance Criteria:**

**Given** the `command` role of `arena-cloud`, deployed as its own Kubernetes workload
**When** the onboarding API submits a setup command
**Then** a `commands` row is written (`branch_id`, `idempotency_key`, `kind`, `schema_version`, `payload`, `issued_by`, `status = pending`) with a relay-assigned monotonic `id`, and a duplicate `(branch_id, idempotency_key)` returns the existing row

**Given** the command catalogue in `arena-proto`
**When** a command kind is submitted
**Then** only setup kinds (`group.upsert`, `device.assign`, `device.register`, `staff.upsert`, `settings.set` for name/tz, `setup.test_session`, `branch.go_live`) are accepted, on every tier; any other kind returns `UNSUPPORTED`

**Given** a paired Hub
**When** its poller runs every 60 s, or immediately when a heartbeat response carries `pending_commands > 0`
**Then** it fetches `GET /v1/commands?after=<cursor>` with its epoch in `id` order, rows move to `fetched`, and each is applied once through `apply()` with `actor{kind: owner}`, deduped in `applied_commands`

**Given** a command applied or rejected at the Hub
**When** the Hub posts `POST /v1/commands/{id}/result` with epoch and `applied_seq` or an `AppError`
**Then** the row becomes `applied` or `rejected` with the error, and repeating the same result is a no-op

**Given** the Hub is offline when a command is queued
**When** it reconnects
**Then** it fetches and applies the backlog in `id` order, and no command is applied twice even if a result post was lost

**Given** an applied owner command that overwrote a local edit
**When** it applies
**Then** the Hub toasts "<Entity> updated by owner" and the audit keeps both changes

### Story 2.15: Step 7 — invite staff by phone with a role and a one-time setup code

As a café owner,
I want to add my manager by phone, pick their role and get a code to hand them,
So that they can sign in at the Hub with their own PIN and password without me ever knowing them.

**Covers:** FR9, FR104 (owner half), AD-10, AD-5, UX-DR84, UX-DR102, UX-DR103

**Acceptance Criteria:**

**Given** step 7 on a paired, bootstrapped branch
**When** it opens
**Then** it shows a form for name, phone and role (Manager, Counter, Kitchen; Manager preselected for the first invite), an "Invite" primary button, the list of invited staff with their status, and "Skip for now"

**Given** valid details
**When** "Invite" is pressed
**Then** the cloud generates an 8-character setup code, sends a `staff.upsert` command carrying only `setup_code_hash` and `setup_expires_at = now + 72 h`, and shows once "Setup code for <name>: XXXX XXXX. Give it to her; it works once." with Copy
**And** the staff member's Hub login is their phone number

**Given** the Hub applies `staff.upsert`
**When** the login already exists at the branch
**Then** it is rejected `LOGIN_TAKEN` and the invite row shows "Couldn't apply at branch" with the reason

**Given** an invite that has not been used
**When** the owner chooses "New code"
**Then** a fresh code and hash are issued by a new command, the old code stops working on apply, and the new code is shown once

**Given** at least one Manager invite applied at the Hub
**When** the result arrives
**Then** step 7 is marked done; "Skip for now" also leaves it marked skipped and finishable later from the Hub

**Given** the Free 3-user limit in the licence
**When** an invite would exceed it
**Then** the Hub rejects `USER_LIMIT` and the wizard shows `tier-lock` "Available on Basic" beside the invite

### Story 2.16: Kiosk finds the Hub on the LAN or by typed IP

As a café owner setting up gaming PCs,
I want each kiosk to find the Hub by itself, or let me type the Hub's address,
So that pairing works on any café network.

**Covers:** FR15 (discovery, manual IP fallback), AD-8 (mDNS service name in `arena-proto`), AD-42, NFR17, UX-DR18, UX-DR103

**Acceptance Criteria:**

**Given** a paired Hub
**When** it is running
**Then** it advertises the mDNS service named in `arena-proto` with TXT records branch name, Hub name, protocol version and Hub certificate fingerprint, and shows its LAN IP in Settings → Devices

**Given** an unpaired kiosk on first run
**When** the kiosk app starts
**Then** the locked kiosk screen shows "Looking for the Hub…" and lists discovered Hubs by branch and Hub name within 10 s

**Given** no Hub is discovered in 10 s
**When** the search ends
**Then** the screen shows a "Hub IP address" field with the hint that the IP is shown on the Hub under Settings → Devices, plus "Search again"

**Given** a typed IP
**When** the kiosk checks it
**Then** it connects to `:7401` and confirms an Arena 360 Hub answers with branch name and fingerprint; otherwise it shows "No Hub at that address" and keeps the field

**Given** the kiosk is unpaired
**When** the kiosk shows the discovery screen
**Then** the lockdown from Epic 1 stays in force and Ctrl+Alt+Shift+A admin exit still works

### Story 2.17: Kiosk pairs with a 6-digit code from the Hub (or a manager PIN) and shows as Unassigned

As a café owner setting up gaming PCs,
I want to type the 6-digit code shown on the Hub into each PC,
So that every PC joins my branch and appears on the floor within a minute.

**Covers:** FR15, AD-10, AD-22, AD-42, NFR11, UX-DR2 (unassigned), UX-DR61 (unassigned dashed), UX-DR66

**Acceptance Criteria:**

**Given** a Manager or Owner on the Hub
**When** they open Settings → Devices → "Pair a kiosk"
**Then** a 6-digit single-use code is shown with a 10-minute countdown; after a code is used, a fresh one appears at once so many PCs can be paired in a row

**Given** a kiosk that has selected a Hub
**When** the 6-digit code is entered
**Then** over TLS to `:7401` the kiosk sends `pair{code, pc_name, version}`; the Hub mints a 256-bit device token (Argon2id hash in `devices`), creates a `devices_meta` row (`kind = pc`, `label = pc_name`, `group_id` null) via `device.register`, and returns the token and Hub certificate fingerprint once
**And** the kiosk service stores the token with DPAPI and pins that fingerprint for every later connection

**Given** a just-paired kiosk
**When** it sends `hello{class: paired}`
**Then** it appears on the Hub floor's Unassigned section as a dashed entry with its PC name within 60 s of entering the code

**Given** the kiosk alternative "Use a manager PIN"
**When** a Manager's PIN is entered on the kiosk instead of the code
**Then** the Hub verifies it with Argon2id and pairs the kiosk the same way, recording `elevated_by` the manager; a wrong PIN clears the cells and 5 failures pause the option for 60 s

**Given** a wrong or expired code
**When** it is entered
**Then** the kiosk stays on the same screen, clears the field and shows a plain retry line

**Given** Epic 1's dev kiosk token and stored Hub IP
**When** a release-build kiosk is installed
**Then** neither exists; a kiosk can reach a Hub only by pairing

**Given** a paired kiosk listed on the floor
**When** a Manager uses Remote unlock from its entry (Story 1.18)
**Then** it works against the real paired kiosk

### Story 2.18: Device cap and pairing rate limits at the Hub

As a café owner,
I want the Hub to stop at my tier's device limit and to resist code guessing on the café Wi-Fi,
So that limits are clear and a stranger can't pair a device.

**Covers:** FR20, AD-23, AD-29, NFR23, UX-DR24

**Acceptance Criteria:**

**Given** the licence `limits.devices` (Free 20, Basic 60, Premium 200, +20 per add-on)
**When** a kiosk pairing or `device.register` would make the non-revoked device count exceed it
**Then** `apply()` rejects it with `DEVICE_LIMIT` in the permission step and nothing is written; the count includes kiosks, Pis and manually registered devices and excludes standby Hubs and kitchen displays

**Given** a refused pairing at the cap
**When** the Hub shows it
**Then** Settings → Devices shows `tier-lock` "Available on Basic" (or Premium) with an underlined "Upgrade" link, and the kiosk shows "This branch has reached its device limit — ask the owner."

**Given** one kiosk pairing code
**When** it receives 5 failed attempts
**Then** it is invalidated and the Hub panel shows a fresh code

**Given** one LAN IP
**When** it makes more than 20 pairing attempts (code or PIN) in an hour
**Then** further attempts from that IP get `RATE_LIMITED` until the hour rolls, and the Hub log records it

**Given** a device is revoked
**When** the count is recalculated
**Then** the slot is freed immediately for a new pairing

### Story 2.19: Device groups with six swatches, held at the Hub

As a café owner,
I want device groups with a name and colour that the Hub enforces, and that managers can edit at the Hub later,
So that every device is priced and shown by its group, whether groups were set up online or at the counter.

**Covers:** FR17, AD-2, AD-39, UX-DR7, UX-DR63, UX-DR88 (Settings → Layout)

**Acceptance Criteria:**

**Given** a `group.upsert` intent (from the Hub UI or a setup command)
**When** it applies
**Then** it stores name (unique per branch), kind (pc, console, vr, other), colour from exactly the six swatches teal, orange, lime, sand, coral, silver, and sort order, in a new `device_groups` table; any other colour is rejected with a field error

**Given** a `device.assign` intent for a device
**When** it applies
**Then** it sets the device's `group_id`; every device belongs to at most one group, and assigning removes it from Unassigned

**Given** a PC (kiosk) device and a console group, or a console device and a PC group
**When** `device.assign` is applied
**Then** it is rejected `GROUP_KIND_MISMATCH`; console groups are never shared with PC groups

**Given** a group that still contains devices
**When** someone deletes it
**Then** it is refused with an inline reason; an empty group is soft-deleted

**Given** a Manager or Owner on Settings → Layout at the Hub
**When** they create or edit a group, or drag a device (or use the keyboard "Move to group" control) into a group
**Then** the same intents apply, so groups can be changed at the Hub after the wizard

**Given** groups exist
**When** the floor placeholder renders
**Then** each assigned device shows a `group-chip` (swatch dot + group name in muted ink), and Unassigned devices show none

**Given** a stale edit (old `base_rev`) from a second Hub window
**When** it is saved
**Then** it is rejected `STALE` with the current values shown; an owner setup command with an old `base_rev` still applies and the Hub toasts "Group updated by owner"

**Given** a Counter
**When** they try to edit groups at the Hub
**Then** the action opens `inline-pin` for manager elevation

### Story 2.20: Register consoles, VR rigs and simulators manually

As a café owner,
I want to add my PS5s, VR rigs and simulators by hand with a group and TV number,
So that they appear on the floor and can be timed like PCs.

**Covers:** FR16, FR20, AD-29

**Acceptance Criteria:**

**Given** Settings → Devices → "Add device"
**When** a Manager picks Console, VR rig or Simulator and enters a label, a group of the matching kind and (for consoles) a TV number
**Then** `device.register` creates a `devices_meta` row with that kind and no device token, and it appears on the floor placeholder like a PC with its label and, for consoles, "TV n"

**Given** a `device.register` setup command from the owner (used by wizard step 4)
**When** the Hub applies it
**Then** the result is the same as registering at the Hub, with `actor{kind: owner}`

**Given** a console TV number already used at the branch
**When** it is saved
**Then** it is rejected with a field error

**Given** the device cap is reached
**When** a manual device is added
**Then** it is refused `DEVICE_LIMIT` with the same `tier-lock` as pairing

**Given** a manually registered device
**When** a Manager edits or removes it
**Then** edits go through `device.register` with `base_rev`, and removal soft-deletes it and frees its cap slot

### Story 2.21: Step 4 — owner groups devices in the web wizard as they arrive

As a café owner,
I want to see my PCs arrive in the setup page, create groups and drag each device into one there,
So that I can organise the whole floor from my browser without walking to the counter.

**Covers:** FR8 (step 4), FR17, FR16 (add consoles from the wizard), AD-5, AD-29, UX-DR7, UX-DR84, UX-DR105, UX-DR113

**Acceptance Criteria:**

**Given** a paired Hub before go-live
**When** it heartbeats
**Then** the heartbeat carries a `setup` block with devices (id, label, kind, group id, status; no player data) and groups (id, name, kind, colour, sort), stored as the latest row in a new `branch_setup_status` table

**Given** step 4 is open
**When** a kiosk pairs at the branch
**Then** it appears in the step's Unassigned list within 15 s with its PC name and status, without a page reload

**Given** step 4
**When** the owner creates a group (name, kind, one of the six swatches)
**Then** a `group.upsert` setup command is sent through the mailbox, the group shows at once marked "Waiting for branch", and the mark clears when the Hub reports it applied

**Given** an Unassigned device and a group
**When** the owner drags the device onto the group (or, by keyboard, focuses the device, chooses "Move to group" and picks one with arrow keys + Enter)
**Then** a `device.assign` setup command is sent, the device moves optimistically with "Waiting for branch", and on rejection (e.g. `GROUP_KIND_MISMATCH`) it returns to its previous place with the plain reason inline

**Given** step 4
**When** the owner chooses "Add console" (or VR rig / simulator) with label, matching group and TV number
**Then** a `device.register` setup command is sent; a `DEVICE_LIMIT` rejection shows `tier-lock` "Available on Basic"

**Given** at least one device in a group, as reported by the Hub
**When** the setup block arrives
**Then** step 4 completes by itself and focus moves to the step 5 heading

**Given** the Hub is offline
**When** the owner works in step 4
**Then** the last reported devices show with their age ("as of 3 min ago") and a "Hub offline" status badge; new group and assignment commands queue as "Waiting for branch" and apply in order when the Hub returns

**Given** groups were also edited at the Hub
**When** the next heartbeat arrives
**Then** the wizard shows the Hub's current groups and assignments

### Story 2.22: Floor zones and grid layout per branch

As a café owner,
I want to arrange device tiles into zones like "PC hall" and "Console room",
So that the floor view matches my café and staff find each seat at a glance.

**Covers:** FR19, UX-DR87 (zones), UX-DR88 (layout editor), UX-DR105

**Acceptance Criteria:**

**Given** Settings → Layout
**When** a Manager adds, renames, reorders or deletes a zone
**Then** a `floor_zones` table stores name and sort, and a zone with devices cannot be deleted until they move

**Given** the layout grid
**When** a device tile is dragged to a cell in a zone, or moved by keyboard (arrow keys move the tile, Enter drops)
**Then** `device.place` stores `zone_id`, `floor_x`, `floor_y` on `devices_meta`; two devices cannot occupy one cell

**Given** a saved layout
**When** the Hub restarts or another Hub window opens
**Then** the floor placeholder shows zones in sort order with devices in their cells, and the Unassigned section stays at the end

**Given** a phone-width Hub (< 840 px)
**When** the layout editor opens
**Then** it falls back to a list with "Zone" and "Position" fields per device, with 44 px targets

### Story 2.23: Device status and maintenance

As counter staff,
I want every device to show one clear status and to take a broken PC out of use without deleting it,
So that nobody starts a session on a device that can't be used.

**Covers:** FR18, UX-DR2, UX-DR23, UX-DR110

**Acceptance Criteria:**

**Given** `devices_meta.state`
**When** the schema is created
**Then** it allows exactly online, offline, in session, reserved and maintenance (in session and reserved are set by later features); every status shows as text plus its state token, never colour alone

**Given** a paired kiosk
**When** its socket connects or stays disconnected for 15 s
**Then** its status becomes online or offline with `last_seen_at`, pushed to every open Hub view within 2 s

**Given** a manually registered device
**When** it is not in maintenance
**Then** it shows online

**Given** a Manager sets a device to maintenance with an optional note
**When** `device.maintenance` applies
**Then** the device shows "Maintenance", stays in its group and position, and is flagged unavailable for starts; "Return to service" restores its previous status

### Story 2.24: Hourly postpaid rate per device group

As a café owner,
I want to set an hourly rate for each device group,
So that every device is priced by its group from the first session.

**Covers:** FR57, FR8 (step 5 completion source), AD-7, AD-39, UX-DR8

**Acceptance Criteria:**

**Given** Settings → Layout → a group
**When** a Manager or Owner sets an hourly rate
**Then** `rate.upsert` writes a `rates` row (group, `price_per_hour_minor` in the branch currency's minor units, active) and the amount displays with tabular numerals in the branch format

**Given** a rate change
**When** it is saved
**Then** the previous rate row is deactivated (kept for history) and the new one becomes the group's only active rate, so a later session start reads the active rate at that moment

**Given** zero, negative or non-numeric input
**When** it is saved
**Then** a field error shows and nothing is written

**Given** each group
**When** the Hub computes setup status
**Then** groups without an active rate are listed as "No rate yet"

**Given** a Counter
**When** they edit a rate
**Then** `inline-pin` opens for manager elevation and the change is audited with both actors

### Story 2.25: Branch operating settings — opening hours, fulfilment, kiosk volume, backup location, ad consent

As a café owner,
I want to set my opening hours and the few branch-wide settings the go-live checklist needs,
So that pricing, passes and alerts follow my real hours and the branch is ready to open.

**Covers:** FR115, FR11 (checklist inputs), AD-7, UX-DR88

**Acceptance Criteria:**

**Given** Settings → Branch → Hours
**When** a Manager sets open and close times per weekday (closed allowed; close past midnight allowed)
**Then** `settings.set` stores them as local minutes from midnight with wrap in `branch.opening_hours`, and invalid entries (open = close) show a field error

**Given** Settings → Branch
**When** a Manager sets order fulfilment (deliver to seat / pick up at counter) and kiosk default volume (0–100)
**Then** they are stored and pushed by `settings.changed` to connected kiosks

**Given** Settings → System → Backup location
**When** a Manager picks a local folder or removable drive
**Then** the path is stored and checked as writable, with an inline error if not

**Given** a Free branch
**When** Settings → Branch → Ads opens
**Then** it shows the ads consent view with the ad policy link and a consent tick stored with time and staff id; on paid tiers this section is absent

**Given** these settings
**When** the Hub computes setup status
**Then** it reports opening hours set yes/no, backup location set yes/no and ad consent yes/no

### Story 2.26: Printer detection and test print at the Hub

As a café owner,
I want the Hub to find my receipt printer and print a test,
So that I know receipts will work before I open.

**Covers:** FR10, AD-28, AD-34, NFR18, UX-DR95

**Acceptance Criteria:**

**Given** Settings → Printer
**When** a Manager presses "Find printer"
**Then** the Hub lists ESC/POS printers found on USB and on the LAN at port 9100 (plus a manual IP field), through the printer adapter that has a recorded fake for tests

**Given** a selected printer and paper width 58 or 80 mm
**When** "Test print" is pressed
**Then** a text-only ESC/POS Font A test page (branch name double width/height, date, "Printer test OK", width-specific ruler) prints and the result is recorded as a setup status of ok or failed with the error

**Given** no printer is found or the test fails
**When** the Manager chooses "Skip printer for now"
**Then** the setup status is skipped, and nothing else on the Hub depends on a printer

**Given** UPI and card
**When** the printer & payments screen shows
**Then** it states that UPI and card are recorded by reference at settle and asks for no payment-provider keys

### Story 2.27: Wizard steps 5, 6 and 8 follow the Hub's setup progress

As a café owner,
I want the online wizard to tick off rates and the printer as I finish them at the Hub,
So that I always know what's left without switching screens to check.

**Covers:** FR8 (step 5 completion), FR4 (skippable steps), FR10 (step completion), UX-DR84, UX-DR113

**Acceptance Criteria:**

**Given** a paired Hub before go-live
**When** it heartbeats
**Then** its `setup` block also carries groups without a rate, manager exists yes/no, printer status, test session status, and opening hours/backup/ad consent flags

**Given** step 5 (Rate plans)
**When** every group has an active rate
**Then** the step completes by itself; until then it lists groups without a rate

**Given** step 6 (Products)
**When** it opens
**Then** it explains products can be added later at the Hub and offers "Skip for now", which marks it skipped and opens step 7

**Given** step 8 (Printer & payments)
**When** the Hub reports printer ok or skipped, or the owner chooses "Skip for now"
**Then** the step completes and the next step opens

**Given** the Hub's own Settings → Setup panel
**When** staff open it before go-live
**Then** it shows the same checklist (pair kiosks, groups, rates, printer, test session) with a link to each screen; groups set in the web wizard show as done

**Given** the Hub is offline
**When** the owner views steps 5–8
**Then** the last reported state shows with its age ("as of 3 min ago") and a "Hub offline" status badge, never a broken page

### Story 2.28: Step 9 — 2-minute test session locks a kiosk at zero

As a café owner,
I want to start a 2-minute test on one PC and watch it lock exactly at zero,
So that I know the floor is really under control before I open.

**Covers:** FR11 (test session), AD-12, AD-5, NFR3, UX-DR113

**Acceptance Criteria:**

**Given** step 9 and at least one online kiosk in a group
**When** the owner picks it and presses "Start test session" (or staff press it in the Hub's Setup panel)
**Then** a `setup.test_session` command (or Hub intent) applies `device.test_session.start`, and the kiosk unlocks to a test screen showing "Test session" and a 2:00 countdown in tabular figures within 15 s of the click while the branch is online

**Given** a test session running
**When** the Hub sends the unlock
**Then** it carries `lock_after_ms = 120000`, and the kiosk service counts down on the monotonic clock so the PC locks at zero even if the Hub drops mid-test

**Given** the countdown reaches zero
**When** the kiosk locks
**Then** it returns to the locked screen and sends `device.test_session.completed`; step 9 completes by itself and the wizard says "The PC locked on cue."

**Given** the kiosk does not confirm the lock within 10 s of zero
**When** the Hub times out
**Then** the step shows "That PC didn't confirm it locked — check it and try again" with Retry

**Given** the chosen kiosk is offline or in maintenance
**When** the test is requested
**Then** it is refused with a plain reason and the device list stays open

**Given** a test session
**When** reports and bills are produced later
**Then** it creates no session, bill or money row; only the two audit intents exist

### Story 2.29: Owner manages branches — create, rename, timezone, new Hub pairing code

As a café owner,
I want one place to see my branches, rename them, set their timezone and get a pairing code,
So that I can manage branches after setup without restarting the wizard.

**Covers:** FR177, AD-3, AD-5, AD-29, UX-DR24, UX-DR90 (Account → Branches)

**Acceptance Criteria:**

**Given** the owner app route Account → Branches
**When** it opens
**Then** each branch shows name, branch code, tier, setup state (in setup / live) and Hub status (not paired / last seen time)

**Given** a branch whose Hub is not yet paired
**When** the owner renames it or changes its timezone
**Then** the change is saved in `gz_control` and will reach the Hub in `branch.bootstrap`

**Given** a paired branch
**When** the owner renames it or changes its timezone
**Then** a `settings.set` command is sent and the row shows "Waiting for branch" until applied; the timezone takes effect from the next business day at the Hub

**Given** a branch with no paired Hub
**When** the owner chooses "Get pairing code"
**Then** the same `pairing-code` card as step 3 is shown

**Given** a Free tenant with one branch
**When** the owner chooses "Add branch"
**Then** it shows `tier-lock` "Available on Basic"; with a paid or comp entitlement it creates the branch and opens its wizard at step 1

**Given** standby pairing codes
**When** the owner looks for them on this page
**Then** the action is shown as `tier-lock` on Free and is delivered with the standby feature (not in this story)

### Story 2.30: Step 10 — go-live checklist marks the branch live

As a café owner,
I want a final checklist that confirms the branch is ready and then marks it live,
So that I open with confidence and my owner app switches from setup to running mode.

**Covers:** FR11 (go live), FR4, NFR27 (onboarding telemetry), UX-DR84, UX-DR113

**Acceptance Criteria:**

**Given** step 10
**When** it opens
**Then** it lists opening hours set, backup location set, standby PC (shown "Paid tiers only" on Free and not blocking), and ad consent (Free only), each with its state from the latest setup status and a link to the Hub screen that fixes it

**Given** all required items are done, and steps 1–5 and 9 are done (6–8 done or skipped)
**When** the owner presses "Go live"
**Then** `branches.live_at` is set, a `branch.go_live` command reaches the Hub, the Hub hides its Setup panel and drops heartbeats to the normal interval, and the wizard ends on Account → Branches showing the branch as Live

**Given** a required item is missing
**When** the owner views step 10
**Then** "Go live" is disabled with the inline reason naming the first missing item

**Given** a live branch
**When** the owner opens the wizard URL again
**Then** they are taken to Account → Branches; done steps remain viewable for reference

**Given** the onboarding timeline
**When** each step completes
**Then** the cloud records step completion timestamps from sign-up to go-live in `onboarding_progress` (no PII), available for the "first paid session within 60 min" metric
