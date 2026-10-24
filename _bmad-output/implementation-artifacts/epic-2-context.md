# Epic 2 Context: Owner onboarding and pairing

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Take a new café owner from sign-up to a live branch without support. They sign up with phone OTP and enrol TOTP. A resumable 10-step web wizard creates the tenant and first branch and issues a signed licence. A typed code pairs the Hub. Kiosks find the Hub on the LAN and pair with a code, and devices are grouped and priced. The owner invites staff with one-time setup codes, runs a 2-minute test session that locks a kiosk at zero, and marks the branch live. This epic stands up the `arena-cloud` control plane (India-hosted, behind Cloudflare) and replaces Epic 1's dev fixtures on the real path: `branch.bootstrap` arrives at first pairing, staff credentials come from setup codes, and kiosk tokens and the Hub address come from mDNS plus code pairing. `dev-fixtures` stays for automated tests only. Success means 80% of branches reach a first paid session within 60 minutes of sign-up without support.

## Stories

- Story 2.1: Arena 360 cloud runs in India behind Cloudflare
- Story 2.2: Cloud error envelope, local dev mode and single SQLite linkage
- Story 2.3: Control data is backed up nightly and the cloud is observable
- Story 2.4: Owner signs up with phone OTP
- Story 2.5: Owner sets a password and enrols TOTP with backup codes
- Story 2.6: Owner signs in on a new device with TOTP, backup code or SMS fallback
- Story 2.7: Resumable setup wizard and step 1 — café and first branch
- Story 2.8: Step 2 — Free tier issues a signed licence
- Story 2.9: Step 3 — single-use 8-character Hub pairing code
- Story 2.10: Hub installer pairs with the code and retries in place
- Story 2.11: Hub checks in by heartbeat and the wizard advances by itself
- Story 2.12: Hub verifies its licence against trusted time and enforces its limits
- Story 2.13: branch.bootstrap applies the onboarding profile at first pairing, with branch code lock
- Story 2.14: Owner and staff set their own PIN and password at the Hub with a one-time setup code
- Story 2.15: Owner changes reach the Hub through the command mailbox within one poll
- Story 2.16: Step 7 — invite staff by phone with a role and a one-time setup code
- Story 2.17: Kiosk finds the Hub on the LAN or by typed IP
- Story 2.18: Kiosk pairs with a 6-digit code from the Hub (or a manager PIN) and shows as Unassigned
- Story 2.19: Device cap and pairing rate limits at the Hub
- Story 2.20: Device groups with six swatches, held at the Hub
- Story 2.21: Register consoles, VR rigs and simulators manually
- Story 2.22: Step 4 — owner groups devices in the web wizard as they arrive
- Story 2.23: Floor zones and grid layout per branch
- Story 2.24: Device status and maintenance
- Story 2.25: Hourly postpaid rate per device group
- Story 2.26: Branch operating settings — opening hours, fulfilment, kiosk volume, backup location, ad consent
- Story 2.27: Printer detection and test print at the Hub
- Story 2.28: Wizard steps 5, 6 and 8 follow the Hub's setup progress
- Story 2.29: Step 9 — 2-minute test session locks a kiosk at zero
- Story 2.30: Owner manages branches — create, rename, timezone, new Hub pairing code
- Story 2.31: Step 10 — go-live checklist marks the branch live

## Requirements & Constraints

**Scope**
- Only the Free path of tier selection ships here; paid payment is Epic 10, behind a billing-enabled flag that stays off until then.
- A Windows Hub is refused on Free and the Android Hub ships in Epic 4. To let internal and beta cafés pair a Windows Hub end to end now, an operator-issued **comp entitlement** (audited CLI) grants Basic or Premium limits.

**Data residency and edge**
- Control-plane data stays in India.
- All cloud traffic is HTTPS through Cloudflare. The origin accepts only Cloudflare IP ranges.

**Owner identity**
- Sign-up needs phone OTP: 6 digits, 5-minute expiry, hashed. The owner row exists only after the code is verified.
- Then the owner sets a password (at least 10 characters, checked against a common-password list, paste allowed) and enrols TOTP (RFC 6238) with 10 single-use backup codes, stored only as Argon2id hashes.
- Signing in on a new device needs TOTP or a backup code. An SMS fallback is opt-in and off by default.
- Trusted-device cookies last 30 days. Refresh tokens rotate, and reuse of a rotated token revokes the whole session family.
- No account enumeration: the code is always sent, and errors never say which factor failed.

**Lockouts and rate limits**
- OTP send: 5 per phone per hour.
- Sign-in: 5 failures pause it for 15 minutes.
- Cloud pairing: 5 attempts per code, 20 per IP per hour.
- Kiosk pairing at the Hub: 5 per code, 20 per LAN IP per hour.
- Setup code and pairing PIN: 5 failures pause entry for 60 s.
- Per-IP limits are enforced at Cloudflare; per-phone, per-tenant and per-code limits in `gz_control.rate_limits`.

**Secrets and codes**
- Every code (OTP, pairing, setup) is stored only as a hash and encodes nothing.
- Device and Hub tokens are 256-bit, returned once and stored server-side as Argon2id hashes. Clients keep them in DPAPI (Android Keystore on Android).
- Staff credentials never travel in commands; only `setup_code_hash` and `setup_expires_at` do. A PIN, password or code never appears in logs, change rows or telemetry.

**Tenancy**
- `tenant_id` and the permitted branch ids come only from the authenticated session.
- A request that names a tenant or branch the session doesn't own returns `FORBIDDEN` and touches nothing.

**Offline**
- The branch keeps trading through WAN loss.
- Heartbeat failures retry with jittered backoff and never block an intent.
- A bad licence falls back to Free limits; a Windows Hub never locks.
- Web wizard views of Hub state show the last reported state with its age and a "Hub offline" badge, never a broken page.

**Telemetry**
- OpenTelemetry traces go to Grafana Cloud and errors to Sentry.
- Scrub request bodies, phones, emails, OTPs, passwords, tokens and codes. A test asserts that sign-up field values never appear in exported telemetry.
- `gz_control` is backed up nightly, encrypted and off-provider, with 30 copies kept and a 4 h RTO restore drill before GA.

**Accessibility**
- Web surfaces have landmarks, one `h1`, labelled fields and targets of at least 44 px, with zero serious or critical axe violations in light and dark themes.

## Technical Decisions

**Cloud**

`arena-cloud` is one binary run with `--role control|command|jobs|all`.
- Each role is a Kubernetes workload on the Hostinger VPS in Mumbai: `staging` deploys on merge to main, `production` on a release tag.
  - `api.staging.arena360.cloud` and `api.arena360.cloud`.
- `gz_control` (the only cloud database) lives on Turso Mumbai via libsql 0.10, with numbered, forward-only embedded migrations and `SCHEMA_TOO_NEW` on a newer database. Each story adds only the tables it needs.
- rusqlite and libsql must link one shared SQLite library; a CI test catches duplicate symbols.
- Dev mode is `arena-cloud --role all --dev`: a local SQLite file for `gz_control`, MinIO for R2, and a recorded fake SMS adapter. No cloud accounts are needed.
- Secrets live in SOPS 3.13.3 + age 1.3.2 files in the repo and become Kubernetes secrets at deploy. CI fails on committed plaintext secrets or env files.

**Cloud API**
- Paths are versioned under `/v1`, with additive changes only within a version.
- Errors use the `AppError{code,message,field?}` envelope with stable `arena-proto` codes and never a stack trace. An unknown path returns `NOT_FOUND`.
- Request and response DTOs live in `arena-proto` and are generated to TypeScript.
- Browser auth is an HttpOnly, Secure, SameSite=Lax session cookie with a rotating refresh token.
- Hub auth is a bearer device token.

**Adapters**
- Messaging goes through an adapter: Gupshup at launch, a recorded fake in dev and CI.
- The printer goes through an adapter (ESC/POS over USB or LAN :9100) with a recorded fake.

**Licence**
- An EdDSA JWT (ed25519-dalek 3.0 + jsonwebtoken 11.1) that expires 7 days after issue.
- Claims are `{tenant_id, branch_id, tier, flags, features, upload_interval_s, summary_interval_s, rollup_schema, retention_days, limits, iat, exp}`.
- Tier defaults live in one table in `arena-proto`, shared by cloud and Hub.
  - Free: devices 20, users 3, branches 1, no uploads.
  - Basic: devices 60. Premium: devices 200. Each add-on: +20 devices.
- The cloud holds one signing key. The Hub compiles in two verify keys and checks the signature and that the licence names its branch and tenant.
- Gates read verified claims only through one entitlement API (`limit(name)`, `has_flag(name)`), checked in the `apply()` permission step. Device and user caps reject with `DEVICE_LIMIT` and `USER_LIMIT`.

**Trusted time**
- Every heartbeat response carries `cloud_time`.
- The Hub persists `trusted_clock{cloud_ms, mono_ms, wall_delta_ms}` and computes `trusted_now` as the last cloud time plus monotonic time elapsed since.
- Licence expiry uses `trusted_now`.
- A wall clock more than 5 minutes off `trusted_now` raises a staff alert and an audit row. Business timestamps still use the wall clock.

**Hub pairing**
- `POST /v1/pair` takes `{code, platform, hub_instance_id, version}`. In one transaction it consumes the code and creates the `hubs` row.
- It returns, once, the token, the tenant and branch ids, the tier, `branch_code`, the first licence, and the `branch.bootstrap` command. Bootstrap is not a mailbox command.
- Error codes: `PAIR_CODE_INVALID` and `PAIR_CODE_EXPIRED` (shown to the user as one undifferentiated line), `FEATURE_NOT_ENTITLED` (Windows Hub on Free; the code is not consumed), `RATE_LIMITED`.

**Heartbeat**
- `POST /v1/heartbeat` runs every 10 s before go-live and every 60 s after.
- It sends epoch, version, schema version and `last_seq_uploaded` (0 on Free). Before go-live it also sends a `setup` block (devices, groups, groups without a rate, manager present, printer status, test-session status, hours/backup/ad-consent flags) with no player data. The cloud stores the latest block in `branch_setup_status`.
- The response is `{licence, fence, cloud_time, pending_commands?}`.
- On the first heartbeat the cloud creates `branch_fence` by compare-and-set from the Hub's provisional epoch, and the Hub clears `lease.provisional`.
- `TOKEN_INVALID` or `TOKEN_REVOKED` puts the Hub in a "no longer paired" state while local trading continues.

**Command mailbox (setup kinds only)**
- Commands live in `gz_control.commands` with `branch_id`, `idempotency_key` (unique per branch), a relay-assigned monotonic `id` as the cursor, `kind`, `schema_version`, `payload`, `issued_by` and `status`.
- Status moves `pending → fetched → applied | rejected`.
- Accepted kinds are `group.upsert`, `device.assign`, `device.register`, `staff.upsert`, `settings.set` (branch name and tz), `setup.test_session` and `branch.go_live`, on every tier. Any other kind returns `UNSUPPORTED`.
- The Hub polls `GET /v1/commands?after=<cursor>` with its epoch every 60 s, or immediately on a heartbeat poke. It applies each command once through `apply()` with `actor{kind: owner}`, deduped in `applied_commands`.
- The Hub posts `POST /v1/commands/{id}/result` with `applied_seq` or an `AppError`. Repeating a result is a no-op.
- Owner wins: owner commands carry `base_rev` but always apply, and the Hub toasts "<Entity> updated by owner" while the audit keeps both changes.
- Full owner command UI, fan-out, step-up MFA, sweeper and fencing are Epic 8.

**Bootstrap and data ownership**
- `branch.bootstrap` carries the profile (name, `branch_code`, currency, tz, `day_start_min`, GST flag, GSTIN, `tax_mode`) plus the owner's staff row (login = phone, role Owner, setup code hash).
- It applies once per idempotency key; a different key fails `ALREADY_BOOTSTRAPPED`.
- After bootstrap the Hub is the sole owner of branch data, and later owner edits arrive as commands.
- `branch_code` is 3–6 uppercase letters/digits, unique per tenant, and assigned by the cloud. It stays editable until `code_locked_at` is set; then changes fail `BRANCH_CODE_LOCKED`. `lock_branch_code()` is the hook the invoice engine will call.
- The Owner's plaintext setup code is kept encrypted only until it is shown or 24 h pass.

**Staff credentials**
- `staff.set_credentials` stores Argon2id hashes, clears the setup code and bumps `pin_verifier_version`.
- The change row carries `actor{kind: staff, id: self}`.
- Setting or changing a Manager or Owner PIN pushes the new versioned verifier to every connected kiosk, replacing the Story 1.20 dev verifier.
- `SETUP_CODE_INVALID` and `LOGIN_TAKEN` are the error codes. Staff setup codes are 8 characters and expire after 72 h.

**Kiosk discovery and pairing**
- The Hub advertises the mDNS service (mdns-sd 0.21, name defined in `arena-proto`) with TXT records for branch name, Hub name, protocol version and certificate fingerprint. `arena360.local` is the LAN name.
- If discovery finds nothing in 10 s, the kiosk offers a typed IP, probed on `:7401`.
- Pairing sends `pair{code, pc_name, version}` over TLS to `:7401`, using a 6-digit single-use code with a 10-minute expiry that the Hub regenerates as soon as one is used. A Manager PIN can be used instead (recorded as `elevated_by`).
- The Hub returns the token and certificate fingerprint once. The kiosk service stores the token with DPAPI and pins the fingerprint.
- Release builds have no dev token and no stored IP.

**Device schema (Hub)**
- `devices_meta`: `kind` (pc, console, vr, other; with simulator), `label`, nullable `group_id` (null means Unassigned), `tv_number` (consoles, unique among non-deleted rows), `zone_id`, `floor_x`, `floor_y`, `state`, `rev`, `deleted_at`.
- `state` takes online, offline, in session, reserved or maintenance.
- `device_groups`: name unique per branch, kind, sort order, and one of exactly six colours: teal, orange, lime, sand, coral, silver.
- `floor_zones`, and `rates` with one active `price_per_hour_minor` row per group; history is deactivated, never deleted.

**Device intents and errors**
- Intents are `device.register` (permission `device.manage`), `device.assign`, `device.place`, `device.maintenance`, `group.upsert`, `rate.upsert`, and `device.test_session.start` / `.completed`.
- `GROUP_KIND_MISMATCH`: PC and console groups never mix.
- `DEVICE_UNASSIGNED`: an Unassigned device cannot start sessions (refused in Story 3.9).
- The device cap counts kiosks, Pis and manual devices, but not standby Hubs or kitchen displays. Revoked or removed devices free their slot immediately.

**Branch settings**
- Opening hours are stored as local minutes from midnight per weekday, with wrap past midnight.
- Fulfilment mode and kiosk volume are pushed to kiosks via `settings.changed`.
- Backup location must be a writable path.
- Ad-consent tick (Free only) is stored with time and staff id.

**Test session**
- The kiosk gets an unlock carrying `lock_after_ms = 120000` and counts down on the service's monotonic clock, so it locks even if the Hub drops mid-test.
- If the kiosk doesn't confirm the lock within 10 s of zero, the Hub times out the test.
- A test session creates no session, bill or money rows; it leaves only audit intents.

**Wizard state (cloud)**
- `tenants` and `branches` are created in one transaction.
- `onboarding_progress(branch, step, state, completed_at)` holds per-step state and completion timestamps for the sign-up-to-go-live metric, with no PII.
- `branches.live_at` is set at go-live.

## UX & Interaction Patterns

**Onboarding web app**
- SvelteKit with adapter-cloudflare, in light and dark themes, with a footer `theme-toggle` (System · Light · Dark, remembered per device). Staff mood, content up to about 1200px wide.
- `wizard-progress` sits in a left column (~256px): "Step n of 10" plus ten rows of at least 44px. The current row has `aria-current="step"`. Products, Staff and Printer & payments carry a "Can skip" tag.
- Below 840px the progress list becomes 10 dots plus "Step n of 10 · <name>".
- Resuming opens the first incomplete step. Done steps link back for edits; later steps redirect to the current one.
- When a step completes by itself (heartbeat-driven, via SSE with a 5 s polling fallback), focus moves to the new step heading and the change is announced once in a polite live region.

**`pairing-code`**
- 8 characters in `web-display` tabular figures, grouped 4 + 4, from an alphabet without ambiguous characters (no 0/O, 1/I/L, U).
- "Copy code" is a secondary button.
- A 15-minute countdown is announced only at 10, 5 and 1 minutes and at expiry.
- No QR anywhere. On expiry the code is struck through with an "Expired" badge, and "Generate a new code" becomes the single primary action. Issuing a new code invalidates all earlier ones.

**Retry in place**
- A bad code in the Hub installer or kiosk keeps the same screen, clears the field and keeps focus there, with one plain line: "That code didn't work. Get a new code from your browser."
- The installer is always dark, and no other Hub screen is reachable until the Hub is paired.

**One-time codes**
- Copy, first person: "Setup code for <name>: XXXX XXXX. Give it to her; it works once."
- Codes are shown once with Copy.
- Hub first sign-in copy: "Enter your setup code, then choose your PIN and password."

**`tier-lock` (new shared component)**
- A lock glyph, "Available on <tier>" and an underlined "Upgrade" link, in light, dark and HC themes. It is keyboard-reachable and its meaning is carried in text.
- Until billing ships, the shared billing flag hides the Upgrade link and shows "Available soon".
- Player surfaces hide gated features entirely.

**`group-chip`**
- A read-only `surface-2` pill with an 8px swatch dot and the group name in `hub-micro` muted, in the floor tile's bottom-right corner.
- Group colour is never a tile fill or state bar. Unassigned tiles have no chip and a 2px dashed `state-unassigned` border.
- In the light theme the swatch picker rings each dot with `border-strong-light`.

**"Waiting for branch"**
- Pending owner commands show "Waiting for branch". In step 4, devices move optimistically and snap back with the plain reason inline if the Hub rejects the command.
- Every drag has a keyboard equivalent: "Move to group" with arrow keys + Enter, and the same for tiles in the layout grid.
- Below 840px the layout editor falls back to a list with Zone and Position fields.

**Status display**
- Device status is always shown as text plus its state token.
- Hub status badges ("Licence couldn't be verified", "PC clock is wrong") go in a new `hub-header` status-badge slot.

**Voice**
- Staff voice is plain, neutral and blame-free: "Waiting for branch", "Couldn't apply at branch", "The PC locked on cue."

## Cross-Story Dependencies

**Build order**
- Cloud foundation first: 2.1, then 2.2, then 2.3. Owner auth follows: 2.4, then 2.5, then 2.6.
- The wizard chain is 2.7 (tenant, branch, progress), then 2.8 (licence), 2.9 (code), 2.10 (pair), 2.11 (heartbeat, fence, self-advance), then 2.12 (verify and enforce) and 2.13 (bootstrap).
- 2.14 needs 2.13's owner setup code. 2.15 (mailbox) must land before 2.16, 2.22, 2.29, 2.30 and 2.31 can deliver their commands.

**Devices**
- 2.17 comes before 2.18, which adds `devices_meta`. 2.18 comes before 2.19 (cap, which needs 2.12's entitlement API and 2.16's `tier-lock`).
- 2.20 adds `device_groups` and the FK on `devices_meta.group_id`. 2.21 adds `tv_number`. 2.23 adds the zone and position columns, and 2.24 adds `state`.
- 2.22 needs 2.15, 2.20 and 2.21 and starts the heartbeat `setup` block. 2.28 extends that block with the data from 2.25, 2.26 and 2.27.
- 2.29 needs paired, grouped kiosks. 2.31 needs 2.26 and 2.29 plus every earlier step.

**Epic 1 reuse**
- `apply()` with typed actor and `elevated_by`, idempotency, `base_rev`/`STALE`, `settings.set`/`settings.changed`, `inline-pin` for Counter elevation (groups, rates).
- Lease and provisional epoch, `chip-offline`, `device.revoke`, the Story 1.20 kiosk verifier and Remote unlock, the floor placeholder, and the kiosk lockdown (still enforced on the discovery screen).

**Later epics**
- Story 3.9 enforces `DEVICE_UNASSIGNED`.
- Epic 4 ships the Android Hub.
- Epic 8 delivers the full owner command UI, MFA and fencing, and Story 8.14 re-targets "Open dashboard".
- Epic 9 fills in the standby PC checklist item.
- Epic 10 brings paid billing, and Story 10.5 un-hides "Upgrade".
