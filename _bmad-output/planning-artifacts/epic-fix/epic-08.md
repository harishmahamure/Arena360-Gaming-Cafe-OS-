## Epic 8: Owner sees and controls branches remotely

Owners get timely dashboards, reports, alerts, live view, audit search and exactly-once commands, and player and owner notifications go out from the cloud.

> Notes: this epic builds on the Epic 1 `apply()`, change log, typed actors and audit; the Epic 2 cloud (`arena-cloud` roles, `gz_control`, owner sign-in with TOTP, backup codes and SMS fallback in Stories 2.4–2.6), heartbeat (Story 2.11), licence and entitlement API (Story 2.12), `branch_fence` row (Story 2.11) and the **minimal setup-command mailbox** of Story 2.15. Epic 8 extends that mailbox. It adds the owner command kinds, `branch: all` fan-out, step-up MFA, the sweeper, epoch fencing of fetch and results, and the owner-side command UI. **No continuous cloud mirror:** the cloud never holds a queryable copy of branch rows. Owner figures come only from the Hub-computed `state/latest.json` (every `summary_interval_s`) and `rollups/<date>.json` (daily at close), and audit comes only from the on-demand builder. **Cadence:** `upload_interval_s` = `summary_interval_s`: Basic 900 s, Premium 300 s, floor 30 s. Free uploads no batches, summaries or rollups (Story 4.14), and its Owner app shows only Subscription, pairing and support. **Placeholders until Epics 10 and 11:** Account → Subscription shows the current tier read-only from the licence with "Plan changes are available soon", Support shows the Arena 360 support email address, and every Owner app `tier-lock` shows its "Available on …" line with no "Upgrade" link; Epic 10 replaces Subscription and switches the Upgrade link on, and Epic 11 adds the help centre. **Command catalogue grows story by story:** Story 8.18 adds the generic command path and the first owner kinds; every later story that adds a kind adds its payload schema, flags and Hub handler together, and a kind with no Hub handler is never accepted by the cloud. **Admin audit:** support-path reads write to the `admin_audit` table created in Story 6.1; the admin app UI on top of these endpoints is Epic 11. **Paid tiers before Epic 10:** Basic, Premium and the Tunnel add-on come from the Story 2.8 operator-issued comp entitlement (tier + `tunnel` flag), so every story here can be tested end to end before billing exists. **Snapshots:** this epic produces the daily snapshot because the audit builder needs one. Tier retention, the upgrade-first snapshot, the yearly GST archive, the player-key backup and the Hub header backup status are Epic 9 (FR174, FR175). **Outbox:** Story 5.19 already writes `notify_outbox` rows for WhatsApp receipts. Epic 8 adds the drain, the cloud send path and the other message kinds. **Rejection copy:** the Owner app shows only two rejection reasons: "Device/Hub revoked", and "Couldn't apply at branch" + Retry for every other `AppError`. The exact code stays in the command detail for support. **Rollup schema v1** (Story 8.5) is signed off as drafted; later fields arrive by a schema bump (the cloud accepts N and N−1, Story 8.7). **Summary `catalogue`** (Story 8.4) is confirmed: configuration only, with a per-entity `rev` for `base_rev`, and no transactions or player data. Each story lists what it covers on its **Covers** line.

### Story 8.1: Presign issues batch and summary upload URLs under the branch's own R2 prefix

As a café owner on a paid tier,
I want my branch's data to go only to my branch's storage area, through short-lived upload links,
So that nobody else's Hub can write over my data and nothing in the cloud holds a standing key to it.

**Covers:** FR167 (upload path), AD-4 (presign), AD-9, AD-22, AD-23, AD-36 (prefix layout), NFR14, NFR20 (per-tenant PUT cost guard), NFR23, NFR25; Additional Requirements "Build spikes" (aws-sdk-s3 against R2)

**Acceptance Criteria:**

**Given** the `arena-cloud` control role with R2 credentials from SOPS-managed secrets (MinIO in dev)
**When** the build spike test runs
**Then** aws-sdk-s3 issues a presigned PUT against R2 with `request_checksum_calculation = WhenRequired` and region `auto`, and a PUT to that URL succeeds; the test runs in CI against MinIO and on staging against R2

**Given** a paid Hub with a valid device token
**When** it calls `POST /v1/presign {epoch, batch: {first, last}, summary: true}`
**Then** the cloud returns `{batch_url, summary_url}` in one response, for keys `tenants/<t>/branches/<b>/changes/<epoch>/<first:012>-<last:012>.zst` and `tenants/<t>/branches/<b>/state/latest.json`, each valid for 10 minutes
**And** `tenant_id` and `branch_id` come only from the device token, never from the request body

**Given** a presign request with `kind: rollup` (with `date`) or `kind: snapshot` (with `date`)
**When** the cloud handles it
**Then** it returns a single URL for `rollups/<yyyy-mm-dd>.json` or `snapshots/<yyyy-mm-dd>.db.zst` under the same prefix; any other kind returns `UNSUPPORTED`

**Given** a requested key that would fall outside the caller's branch prefix (path traversal, another branch id)
**When** presign is called
**Then** it is refused `FORBIDDEN` and an audit event is recorded with the device id

**Given** an epoch below `branch_fence`
**When** presign is called
**Then** summary, rollup and snapshot URLs are refused `FENCED`; batch URLs are still issued so the old epoch's rows can be stored for reconciliation

**Given** a Free-tier Hub
**When** it calls presign
**Then** it is refused `FEATURE_NOT_ENTITLED` and nothing is issued

**Given** a tenant whose presign issuance exceeds its per-tenant limit in `rate_limits` (default 4× its tier cadence per branch per hour)
**When** another presign arrives
**Then** it is refused `RATE_LIMITED` with a retry-after, and the per-tenant R2 PUT count metric records the refusal for the cost guard

**Given** the control role's metrics
**When** presigns are issued
**Then** R2 PUTs per tenant are exported to Grafana Cloud with tenant and branch labels and no other identifiers

### Story 8.2: The Hub uploads gap-free change-log batches at its tier cadence

As a café owner on a paid tier,
I want every change at my branch shipped to the cloud in order, without gaps, while the café keeps running,
So that my reports and audit can always be rebuilt from what the Hub recorded.

**Covers:** FR167 (batches), AD-4 (batch format, cadence, fork declaration), AD-10 (cadence from licence), AD-36, NFR1, NFR9, NFR20

**Acceptance Criteria:**

**Given** a paid Hub whose licence carries `upload_interval_s` (Basic 900, Premium 300)
**When** the uploader task runs
**Then** it wakes every `upload_interval_s` (never more often than 30 s) or as soon as 500 rows are pending, whichever is first, and never runs inside or blocks an `apply()` transaction

**Given** pending `changes` rows above `upload_cursor.last_seq_uploaded`
**When** a cycle runs
**Then** it reads up to 5,000 rows in `seq` order, builds a msgpack header `{epoch, fork?, first, last, sha256, schema}` plus full-row maps keyed by column name, compresses it with zstd, presigns (Story 8.1), PUTs it to `batch_url`, and only then advances `last_seq_uploaded` to `last` in a new `upload_cursor` table

**Given** more than 5,000 pending rows (for example after an 8-hour outage)
**When** the uploader catches up
**Then** it ships consecutive batches back to back until the backlog is empty, with no gap or overlap between `first` and `last` of successive keys

**Given** the first batch after the Hub's epoch changed (driven in this story's tests by a fixture that sets the `lease` fork fields; the real epoch change by compare-and-set arrives in Epic 9)
**When** it is built
**Then** the header declares `fork = {parent_epoch, parent_seq}` and `first = parent_seq + 1`

**Given** a PUT that fails or times out
**When** the cycle ends
**Then** the cursor is not advanced, the same key is retried next cycle with jittered backoff, and a repeated PUT of the same key is harmless

**Given** the WAN is down
**When** cycles fail
**Then** rows accumulate locally, the Hub keeps trading, `chip-offline` is the only staff-visible change, and Hub `GET /healthz` reports last upload time and pending row count

**Given** the licence tier is Free, or changes to Free
**When** the uploader would run
**Then** it does nothing and makes no presign call (Story 4.14 behaviour is unchanged)

**Given** a batch read back from R2 in an integration test
**When** its sha256 and row count are checked
**Then** they match the header, and replaying it with `arena-replay` into an empty database reproduces those rows

### Story 8.3: The heartbeat detects upload gaps and the Hub resends them

As a café owner on a paid tier,
I want the cloud to notice when a batch went missing and ask my Hub for it again,
So that a lost upload never leaves a hole in my history.

**Covers:** FR167 (gap detection and resend), AD-4 (heartbeat gap check, no manifest), NFR1, NFR25 (heartbeat gaps metric)

**Acceptance Criteria:**

**Given** a paid Hub's heartbeat
**When** it is sent
**Then** the body adds `last_seq_uploaded`, `last_upload_at` (time of the last successful batch PUT), `epoch`, `last_summary_at` and `last_rollup_date` to the Story 2.11 fields

**Given** the cloud's cached view per branch (`hubs.last_seq_seen`, `last_epoch_seen`)
**When** a heartbeat's `last_seq_uploaded` and epoch agree with it
**Then** no R2 listing is made

**Given** a heartbeat that disagrees with the cached view
**When** the cloud handles it
**Then** it lists the branch's `changes/<epoch>/` prefix, finds the first missing seq range after the fork point, and adds `resend{from, to}` to the response; with no gap it updates the cached view and adds nothing

**Given** a heartbeat response carrying `resend{from, to}`
**When** the uploader next runs
**Then** it re-ships that range first as batches with keys for exactly those seqs, then continues from its cursor

**Given** Hub `changes` rows in the resend range that were already pruned
**When** the resend is attempted
**Then** the Hub reports `resend_unavailable{from, to}` in the next heartbeat, the cloud records it on the branch, and the gap is counted in the "heartbeat gaps" metric instead of being requested again

**Given** the fleet
**When** gaps are found
**Then** the "heartbeat gaps" metric per branch is exported, and no `/uploads/manifest` endpoint, `upload_manifest` table or ingest cursor exists anywhere in the codebase (checked by a CI grep)

### Story 8.4: The Hub publishes a branch summary every cadence

As a café owner on a paid tier,
I want my Hub to publish a short summary of right now (open, sessions, revenue, cash, staff, what needs attention),
So that my phone can show today's picture in under two seconds without reaching into my branch.

**Covers:** FR148 (summary data), FR167 (summary), AD-35 (`summary.publish`, `rollup_log`), AD-33, NFR5 (summary < 50 ms), NFR7

**Acceptance Criteria:**

**Given** a paid Hub with `summary_interval_s` from its licence
**When** the summary engine runs each interval (in the same cycle as the uploader, sharing its presign)
**Then** it builds `state/latest.json` from in-memory floor state plus 6 queries over stamped columns and stored invoice totals, with `{schema, branch_id, as_of, business_day, open, sessions_live, revenue_so_far_minor, currency, cash_in_drawer_minor, shifts: [{shift_id, staff_name, opened_at, variance_minor?}], staff_on_shift: [names], attention: [{kind, ref, text_params}], catalogue}` and PUTs it to `summary_url`

**Given** the `catalogue` section
**When** it is built
**Then** it carries the owner-editable state with each row's `rev`: groups, rates, plans, pricing rules, coupons, products and bundles, staff (name, role, active; no credentials), roles and devices (name, group, kind, revoked), so the Owner app can edit with `base_rev`; it holds configuration only, never transactions or player data

**Given** a published summary
**When** the PUT succeeds
**Then** a system intent `summary.publish` is applied that writes only the `rollup_log` row with `date = 'latest'` (a new `rollup_log(date PK, schema, built_at, key)` table) and appears in the audit; no business row changes

**Given** the attention list
**When** it is built
**Then** it includes open shift variance over threshold, low-stock products (Story 5.8), players at 15+ days overdue (Story 3.20) and the Hub's open header alerts, each by id and display parameters, never a player name, phone or email

**Given** the summary engine
**When** it runs on the reference PC with 60 devices and a full business day of rows
**Then** computing the file takes under 50 ms (CI benchmark), and a concurrent burst of intents shows no added `apply()` latency

**Given** the WAN is down
**When** an interval passes
**Then** no summary is published or queued; the next successful cycle publishes a fresh one (summaries are not backfilled)

**Given** a Free Hub
**When** the interval would pass
**Then** no summary is built

### Story 8.5: Rollup schema v1 and the daily rollup at close

As a café owner on a paid tier,
I want my Hub to compute each business day's totals into one fixed, versioned file shape, tested against its own Z-reports,
So that my reports are the Hub's own numbers, not a cloud recalculation.

**Covers:** FR167 (rollups), FR150 (data source), AD-35, AD-7, AD-26, NFR5 (rollup < 5 s), NFR10 (totals match Z-report); Additional Requirements "Rollup schema v1 field list" (signed off)

**Acceptance Criteria:**

**Given** `arena-proto`
**When** rollup schema v1 is defined
**Then** it is a versioned Rust type (generated to TypeScript) with `{schema: 1, branch_id, business_day, currency, revenue: {total, by_type: {play, products, plans, other}, by_method: {cash, upi, card, wallet, credit}}, refunds, discounts, voids, sessions: {count, minutes}, shifts: [{shift_id, staff_name, opened_at, closed_at, float, expected, counted, variance, reason?, approver_name?}], stock: {levels: [{product_id, name, qty, low_qty?}], waste: [{product_id, qty, value, reason}], expiring: [{product_id, lot, qty, expires_on}]}, credit: {outstanding, aged: {d0_7, d8_14, d15p}, players: [{player_id, username, outstanding, oldest_unpaid_on, consent}]}, play_hours: [{game_id, minutes}]}` in integer minor units

**Given** the rollup engine in `arena-core`
**When** it is called for a `business_day`
**Then** it runs about 20 queries over stamped columns and stored invoice totals and returns the v1 document, without writing any row or making any network call

**Given** golden tests with a fixed SQLite fixture (sessions, POS, refunds, undone sales, shifts with variance, waste, credit spends and collections)
**When** the rollup is built
**Then** the JSON equals the expected file exactly, and revenue, payments and cash totals equal the fixture's Z-reports (Story 5.28) for the same day

**Given** the reference PC and a busy day (1,000 sessions, 500 orders)
**When** the rollup runs
**Then** it completes in under 5 s; the Story 4.17 device-farm job adds the same check on a 3 GB Android device

### Story 8.6: The daily rollup publishes at close and catches up missed days

As a café owner on a paid tier,
I want my Hub to publish each business day's rollup soon after close, and fill in days it missed,
So that my reports have every day without me doing anything.

**Covers:** FR167 (rollups after close), AD-35 (`rollup.publish`, `rollup_log`), AD-4 (presign `kind: rollup`), NFR1

**Acceptance Criteria:**

**Given** the branch's closing time from opening hours (Story 2.26)
**When** closing time + 30 minutes passes, or the last open shift of the business day closes, whichever is first
**Then** the rollup engine runs its queries for that `business_day`, writes `rollups/<yyyy-mm-dd>.json` via presign `kind: rollup`, and applies `rollup.publish`, which upserts `rollup_log(date, schema, built_at, key)` and appears in the audit

**Given** a 24-hour branch with no closing time
**When** the business day rolls at `day_start_min`
**Then** the previous day's rollup runs 30 minutes after the roll

**Given** the Hub was off or offline at the trigger time
**When** it next runs with WAN available
**Then** every past business day with no `rollup_log` row (since the branch went paid) is generated and uploaded, oldest first

**Given** a late intent stamped to an already-rolled business day (for example an Undo inside its window just after close)
**When** it commits
**Then** that day's rollup is regenerated and re-uploaded, overwriting the file

### Story 8.7: Rollups regenerate after a schema bump, and the cloud accepts only the current and previous schema

As a café owner on a paid tier,
I want old days to be rebuilt in the new format after an update,
So that new report fields show for past days too, without a cloud release.

**Covers:** AD-35 (N and N−1, regeneration oldest first, max 10/min), AD-8, AD-10 (`rollup_schema` pinned per tier), NFR25 (rollup missing-day count)

**Acceptance Criteria:**

**Given** a Hub release whose current rollup schema is N and a licence whose `rollup_schema` is N
**When** the Hub starts
**Then** it finds `rollup_log` days with `schema < N` and regenerates them oldest first, at most 10 per minute, without blocking `apply()` or the normal upload cycle

**Given** a licence pinning `rollup_schema` below the Hub's current schema
**When** a rollup is built
**Then** the Hub emits the pinned schema (it keeps writers for N and N−1), so a tier's file shape changes only when the cloud raises the pin

**Given** the cloud reading a rollup file
**When** its `schema` is N or N−1
**Then** it is accepted; any other value makes the day count as missing with `ROLLUP_SCHEMA_OLD` recorded for that branch and day

**Given** a rollup day that is missing or `ROLLUP_SCHEMA_OLD`
**When** the jobs role runs its daily metrics pass
**Then** the "rollup missing-day count" metric per branch is exported

**Given** a regeneration interrupted by a restart
**When** the Hub starts again
**Then** it resumes from the oldest day still below N, and days already rebuilt are not rebuilt again

### Story 8.8: The Hub uploads a daily snapshot after close

As a café owner on a paid tier,
I want a full copy of my branch database stored each night,
So that my audit history can be rebuilt quickly from a recent starting point.

**Covers:** AD-4 (daily snapshot), AD-24 (PII ciphertext only), AD-36, NFR13

**Acceptance Criteria:**

**Given** a paid Hub after the day's rollup has published
**When** the snapshot task runs
**Then** it runs `VACUUM INTO` a temp file, compresses it with zstd, uploads it to `snapshots/<yyyy-mm-dd>.db.zst` via presign `kind: snapshot`, deletes the temp file, and records the date and last included seq in a new `snapshot_log` table

**Given** the snapshot file
**When** it is inspected in a test
**Then** player PII columns hold only ciphertext, and the player key table is not in it

**Given** the WAN is down at close
**When** the snapshot would run
**Then** it is retried on the next cycle with WAN, and only the latest missed day is uploaded (older missed days are skipped)

**Given** the snapshot is running
**When** staff keep trading
**Then** `apply()` latency stays within NFR3 on the reference PC

**Given** low free disk space (below twice the database size)
**When** the snapshot would run
**Then** it is skipped, a "Not enough disk space for backup" alert appears in the Hub header list, and an audit row is written

### Story 8.9: The owner read API serves the live summary and rollup ranges from files

As a café owner on a paid tier,
I want my app to get today's numbers and any date range quickly, only for my own branches,
So that the dashboard is fast and no one can see another café's figures.

**Covers:** FR149 (as_of), FR150 (ranges), AD-35 (owner read API, 30 s cache, ETag), AD-9, AD-23, AD-40, NFR7, NFR14, NFR23

**Acceptance Criteria:**

**Given** a signed-in owner and one of their paid branches
**When** the app calls `GET /v1/branches/{id}/live`
**Then** the cloud reads `state/latest.json`, returns it with `as_of`, `branch_online` (heartbeat within the last 3 minutes) and `last_heartbeat_at`, and serves repeated calls within 30 s from an in-memory cache checked against the R2 ETag

**Given** `GET /v1/branches/{id}/summary?from=&to=&group_by=day|week|month`
**When** it is called
**Then** the cloud loads the `rollups/` files for that range (per branch, month cache with ETag), sums them in memory, and returns totals plus per-period rows, with `missing_days` listing any day without an accepted rollup (`ROLLUP_SCHEMA_OLD` or absent)

**Given** `branch=all` on either endpoint
**When** it is called
**Then** the response has one entry per permitted branch plus a total, and branches in different currencies are never summed together (one total per currency)

**Given** a branch id not in the session's permitted branches
**When** any owner read endpoint is called
**Then** it returns `NOT_FOUND`, and the R2 key is always built from the session's tenant and branch, never from request input

**Given** a Free branch
**When** `live` or `summary` is called
**Then** it returns `FEATURE_NOT_ENTITLED`

**Given** a tenant above its per-tenant Owner API limit
**When** more calls arrive
**Then** they return `RATE_LIMITED` with a retry-after

**Given** a branch with no summary file yet
**When** `live` is called
**Then** it returns `{as_of: null}` and the app shows "Waiting for the first update from this branch."

### Story 8.10: The Owner app shell — navigation, themes, install and Free limits

As a café owner,
I want one app on my phone and laptop with the same five places to go, in light or dark,
So that I always know where things are, and on Free I see only what my plan includes.

**Covers:** AD-15, AD-40, NFR15, NFR16, UX-DR5, UX-DR6, UX-DR11, UX-DR13, UX-DR17, UX-DR20, UX-DR24, UX-DR82, UX-DR90, UX-DR98 (Owner PWA no network), UX-DR113, UX-DR114, UX-DR115 (key-owner-today), UX-DR116

**Acceptance Criteria:**

**Given** the Owner PWA (SvelteKit, `adapter-cloudflare`, Workers static assets) at `app.arena360.cloud`
**When** a signed-in owner opens it on a phone (< 768 px)
**Then** `owner-nav` is a 64 px bottom tab bar Today · Reports · Alerts · Manage · Account (icon + `hub-micro`, `border-strong` top edge), Today is home, and the layout is one column with a 16 px gutter

**Given** a desktop width (≥ 1024 px)
**When** the app opens
**Then** `owner-nav` is a 256 px left sidebar (icon + `hub-label`, active item filled) with content up to about 1200 px and a 40 px gutter, with full feature parity with the phone

**Given** the theme
**When** the app opens
**Then** it follows `prefers-color-scheme` live, Account → Theme offers `theme-toggle` System · Light · Dark remembered per device, light uses the "Cool paper" tokens, and both themes pass the token contrast check and an axe scan with zero serious/critical violations

**Given** Account
**When** it opens
**Then** it holds MFA (the Story 2.5–2.6 security screens), Subscription, Branches (Story 2.30), Support and Theme

**Given** Account → Subscription before Epic 10
**When** it opens
**Then** it is a read-only placeholder showing each branch's current tier and flags from its licence with the line "Plan changes are available soon" and no purchase control; Epic 10 replaces this screen

**Given** Account → Support before Epic 11
**When** it opens
**Then** it shows the Arena 360 support email address as a `mailto:` link and "Help centre — available soon"; it calls no API

**Given** any Owner app `tier-lock`
**When** it renders before Epic 10
**Then** it shows its "Available on …" line with no "Upgrade" link

**Given** a supported browser
**When** the owner installs the app
**Then** a web manifest and service worker make it installable to the home screen, and the service worker caches the app shell for offline open

**Given** any screen that loads data
**When** a response arrives
**Then** the payload is stored in IndexedDB with its `as_of`, the next open renders the cached payload at once with its age, and logout clears the cache

**Given** no network
**When** the owner opens the app
**Then** it shows cached screens with their data age and a "You're offline" `status-badge`, never a blank or broken page

**Given** a screen reader
**When** any route opens
**Then** it has `header`, `nav` and `main` landmarks, one `h1`, labelled controls, the active nav item exposed as current, and all strings come from the message catalogue

### Story 8.11: The Owner app offers a high-contrast theme

As a café owner with low vision, or reading my phone in bright light,
I want a high-contrast theme in the Owner app,
So that I can read my figures, alerts and controls clearly on any device.

**Covers:** FR202 (HC per device — Owner app), UX-DR4, UX-DR13, UX-DR17, NFR15

**Acceptance Criteria:**

**Given** Account → Theme (Story 8.10)
**When** it opens
**Then** `theme-toggle` offers System · Light · Dark · High contrast, the choice is remembered per device (not synced to other devices) and kept across logout, and High contrast overrides `prefers-color-scheme`

**Given** High contrast chosen
**When** any Owner app screen renders
**Then** it uses the shared HC token set (the `-hc` tokens used by Hub, Kiosk and Kitchen in Story 1.21): solid surfaces with no shadows or translucency, a 2px outline on every interactive surface and the yellow focus ring

**Given** High contrast
**When** status badges, variance flags, data-age warnings, command-row states and tier-locks render
**Then** each carries its text label or glyph, so no state is shown by colour alone

**Given** theme System and the browser reports `forced-colors: active`
**When** the app renders
**Then** it respects the system colours and every control stays visible and focusable

**Given** CI
**When** the HC token contrast check and an axe scan run on Today, Reports, Alerts, Manage and Account in High contrast at phone and desktop widths
**Then** the contrast check passes and axe reports zero serious or critical violations

### Story 8.12: On Free, the Owner app shows only Subscription, pairing and support

As a café owner on Free,
I want the Owner app to show only what my plan includes, and say plainly what upgrading unlocks,
So that I'm never shown empty dashboards or broken screens.

**Covers:** FR152, AD-29 (owner-side gates in the cloud API), UX-DR24, UX-DR90 (Free shows only Subscription, pairing, support)

**Acceptance Criteria:**

**Given** a tenant whose branches are all Free
**When** the app opens
**Then** only Account → Subscription (the Story 8.10 placeholder), pairing (Branches) and Support (the Story 8.10 placeholder) are usable; Today, Reports, Alerts and Manage show `tier-lock` "Available on Basic" (no "Upgrade" link until Epic 10), and no owner read API is called

**Given** a mixed tenant (one Free branch, one paid)
**When** Today, Reports or Manage open
**Then** the Free branch appears with `tier-lock` in place of its figures and controls, and the paid branch works normally

**Given** a Free branch
**When** any owner read or command endpoint other than Subscription, pairing and support is called for it
**Then** the cloud returns `FEATURE_NOT_ENTITLED`, so the gate holds even if the UI is bypassed

**Given** a Free branch that upgrades
**When** its next heartbeat carries the paid licence
**Then** the locked areas unlock on the next app open without signing out

### Story 8.13: Today shows every branch with its figures, cash variance and data age

As a café owner,
I want to open my phone and see each branch's state, money and staff right now, with how old each number is,
So that I know at a glance whether everything is fine, even when a branch has gone offline.

**Covers:** FR148, FR149, NFR7, UX-DR81, UX-DR90 (Today), UX-DR98, UX-DR100 (Needs attention empty), UX-DR103, UX-DR115 (key-owner-today), UX-DR116

**Acceptance Criteria:**

**Given** Today for a tenant with paid branches
**When** it opens
**Then** each branch is a `card` (56 px apart) with `metric-figure`s: open/closed, sessions live, revenue so far, cash in drawer with the signed per-shift variance beside it (counted − expected, "+ over" / "− short"), staff on shift and last sync, each with a `hub-micro` data age ("as of 2 min ago"); on desktop the figures sit in a two-column grid

**Given** a shift variance beyond the branch threshold
**When** the card renders
**Then** the variance shows a warning edge and the label "Over threshold" (text, not colour alone)

**Given** the app is visible
**When** the tier cadence passes (Basic 15 min, Premium 5 min)
**Then** Today re-fetches `live` for each branch without a full reload and without moving focus, and pull-to-refresh (phone) or a Refresh button fetches at once

**Given** a figure's data age beyond 30 minutes during the branch's opening hours
**When** it renders
**Then** the age turns to warning ink

**Given** a branch whose heartbeat is older than 3 minutes
**When** Today renders
**Then** its card drops figures to muted ink, shows the "Branch offline" `status-badge`, keeps the last values with a growing data age, and is never shown as an error

**Given** "Needs attention"
**When** Today renders
**Then** it lists the `attention` items of every branch's summary, newest first, each naming the branch, and shows "All quiet." when there are none

**Given** a 3-year-old mid-range phone on a throttled 4G profile in a Lighthouse CI run
**When** Today opens with two branches and a warm service worker
**Then** first render of figures is under 2 s

**Given** the "data age within cadence" target
**When** Today opens
**Then** the app reports each open's data age to the cloud (branch id, age in seconds, tier; no other data), and the "summary age per branch" metric and a 99th-percentile dashboard exist in Grafana

### Story 8.14: The Owner app stays in setup mode until a branch goes live, then opens its dashboard

As a café owner setting up a branch,
I want the Owner app to keep guiding me through setup until my branch is live and then switch to the dashboard,
So that I never see empty figures for a branch that isn't open yet, and I land on my numbers the moment it is.

**Covers:** FR11 (go live unlocks the Owner app dashboard), UX-DR84, UX-DR90 (Today), UX-DR100

**Acceptance Criteria:**

**Given** `GET /v1/branches`
**When** the Owner app calls it
**Then** each branch carries `live_at` (null until the Story 2.31 "Go live") and its setup progress (steps done of 10, next missing step) from the setup status

**Given** a tenant whose branches all have `live_at` null
**When** Today opens
**Then** it shows setup mode: one card per branch, "Setting up <branch> — <n> of 10 steps done", naming the next missing step, with a primary "Continue setup" that opens that step of the resumable wizard; no `live` or `summary` call is made for these branches, and Reports, Alerts and Manage show "Available after <branch> goes live" with a "Continue setup" link

**Given** a tenant with one live branch and one still in setup
**When** Today opens
**Then** the live branch shows its normal card (Story 8.13 behaviour) and the other shows its setup card in the same list; Reports, Alerts and Manage list only live branches

**Given** the owner presses "Go live" and `live_at` is set
**When** the app next opens, is refreshed, or polls branches (every 60 s while Today is visible)
**Then** Today switches that branch from its setup card to its dashboard card without sign-out, showing "Waiting for the first update from this branch." until its first summary arrives (Story 8.9), and Reports, Alerts and Manage include it

**Given** a Free branch that goes live
**When** Today switches
**Then** it follows the Free limits of the Story 8.12 behaviour ("On Free, the Owner app shows only Subscription, pairing and support")

**Given** the go-live checklist's "Standby PC" item
**When** this story is tested on a paid comp branch with no standby
**Then** Go live is not blocked by it and the switch to the dashboard happens; standby stays advisory until Epic 9 makes its state real

### Story 8.15: Reports — revenue and cash reconciliation

As a café owner,
I want revenue by day, week, month or custom range, per branch, type and payment method, and every shift's cash count,
So that I can see what each branch earns and spot cash problems shift by shift.

**Covers:** FR150 (revenue, cash reconciliation), NFR7, NFR10, UX-DR25, UX-DR81, UX-DR90 (Reports)

**Acceptance Criteria:**

**Given** Reports → Revenue
**When** it opens
**Then** it shows a range picker (Today · This week · This month · Custom), a branch picker (one or All branches) and totals by type (play, products, plans, other) and by payment method (cash, UPI, card, wallet, credit), with refunds as negative lines and discounts listed, all from `summary?range=`

**Given** a range that includes today
**When** it renders
**Then** today's row comes from the live summary and is labelled "So far today" with its data age; past days come from rollups

**Given** days in the range without an accepted rollup
**When** it renders
**Then** a muted line says "No data yet for 2 days in this range" with the dates, and totals are labelled as partial

**Given** Reports → Cash reconciliation
**When** it opens
**Then** it lists shifts in the range with staff, opened/closed time, float, expected, counted and signed variance, reason and approver; variances over the branch threshold are flagged with a warning edge and "Over threshold"

**Given** a multi-currency tenant
**When** All branches is chosen
**Then** totals are shown per currency, never mixed

### Story 8.16: Reports — stock

As a café owner,
I want stock levels, low stock, waste and expiring items per branch and in total,
So that I can reorder and see where stock is lost without visiting.

**Covers:** FR150 (stock), UX-DR81, UX-DR90 (Reports)

**Acceptance Criteria:**

**Given** Reports → Stock
**When** it opens for one branch
**Then** it shows current levels from the latest rollup (with its date) and the live summary's low-stock attention items, a Low stock list (qty ≤ threshold), Waste in the range by product and reason with value, and Expiring lots in the next 7 days

**Given** All branches
**When** chosen
**Then** levels and waste are listed per branch and as a total per product matched by product name and category, with branches that lack the product shown as "—"

**Given** a branch with stock tracking unused (no tracked products)
**When** the report opens
**Then** it shows "No tracked products at this branch."

### Story 8.17: Owner reports export to CSV and PDF and match the Hub's Z-reports exactly

As a café owner,
I want to download any report and trust that it equals the branch's own Z-reports,
So that I can hand it to my accountant without reconciling two sets of numbers.

**Covers:** FR150 (exact match, CSV/PDF), NFR10 (sum of rollups = Z-report over 30 synthetic days), UX-DR90 (Exports)

**Acceptance Criteria:**

**Given** Revenue, Cash reconciliation or Stock with a range and branch chosen
**When** the owner chooses Export → CSV
**Then** the cloud streams a CSV built from the same rollups (one row per day per branch, amounts in major units with the currency code), with a header row naming the range and branches

**Given** Export → PDF
**When** chosen
**Then** a print-styled view of the report opens and the browser's print-to-PDF produces it with branch, range, generated time and data age in the header

**Given** Reports → Exports
**When** it opens
**Then** it lists the exports available for the current range in one place

**Given** a CI job that generates 30 synthetic business days at a Hub (sessions, POS, refunds, undos, shifts) and uploads its rollups to MinIO
**When** the owner read API sums the 30 days
**Then** revenue by type, payments by method, refunds and cash expected/counted/variance equal the sum of the Hub's 30 days of Z-reports to the minor unit; any difference fails the build

### Story 8.18: The cloud accepts owner command kinds, checks them, and fans "all branches" out

As a café owner,
I want only valid changes I'm entitled to send to be accepted, for one branch or all of them at once,
So that I can't send something a branch can't apply.

**Covers:** FR154, FR155 (only valid targets), FR156 (one key per change), AD-5 (catalogue, `group_id` fan-out, `schema_version`), AD-8 (`BRANCH_UPDATE_REQUIRED`), AD-14, AD-23, AD-29, NFR23

**Acceptance Criteria:**

**Given** the command catalogue in `arena-proto`
**When** this story ships
**Then** it adds the generic owner-command path and the first owner kinds `rate.upsert`, `plan.upsert`, `rule.upsert`, `coupon.upsert`, `product.upsert` (products and bundles) and `backup.now`, each with a payload schema, `schema_version`, required tier/flag and a `sensitive` flag (all false), and reserves the names `hub.revoke` and `backup.restore` as sensitive with no handler
**And** the Story 2.15 setup kinds keep working unchanged
**And** the cloud refuses any kind not in the catalogue, or reserved without a handler, with `UNSUPPORTED`; later stories add the staff, role, device, credit-reminder and alert-threshold kinds together with their Hub handlers

**Given** `POST /v1/commands {branch_id | "all", idempotency_key, kind, payload, base_rev?}` from a signed-in owner
**When** the cloud handles it
**Then** it validates the payload, checks the branch belongs to the session's tenant and is entitled for the kind (`FEATURE_NOT_ENTITLED` otherwise), and inserts a `commands` row with the relay-assigned `id`, `issued_by` and `status = pending`

**Given** `branch: "all"`
**When** it is accepted
**Then** one row per permitted, entitled branch is inserted, each with `group_id` = the user's idempotency key and a per-branch key derived from it; the response lists every created row; branches not entitled are listed as skipped with their reason

**Given** the same `(branch_id, idempotency_key)` posted twice
**When** the second arrives
**Then** the existing row is returned and nothing new is inserted

**Given** a branch whose last heartbeat reported a supported command `schema_version` range below the kind's version
**When** the command is posted
**Then** it is refused `BRANCH_UPDATE_REQUIRED` for that branch only

**Given** a tenant above its per-tenant command-post limit
**When** more posts arrive
**Then** they are refused `RATE_LIMITED`

**Given** `GET /v1/commands?status=&branch=&group=`
**When** the owner app calls it
**Then** it returns the tenant's commands with state, error code, time queued and `done_at`, newest first, paged

### Story 8.19: The Hub applies owner commands exactly once, and the owner's change wins

As a café owner,
I want my change to land once at the branch and take precedence over an edit made there a moment before,
So that the branch ends up exactly as I set it, and staff can see what changed.

**Covers:** FR156, FR157 (≤ 1 min on floor and attract), AD-5 (owner wins), AD-39 (`base_rev`, overwritten change listed), AD-2, AD-30, NFR3, UX-DR102 (command conflict toast), UX-DR103

**Acceptance Criteria:**

**Given** a fetched owner command of a kind that exists so far (`rate.upsert`, `plan.upsert`, `rule.upsert`, `coupon.upsert`, `product.upsert`, `backup.now`)
**When** the poller applies it through `apply(command.apply{c})`
**Then** the inner intent runs with `actor{kind: owner, id}`, the key, `applied_seq` and epoch are recorded in `applied_commands` in the same transaction, and the result is posted with epoch and `applied_seq`

**Given** a command whose `base_rev` is lower than the row's current `rev` because staff edited it at the Hub
**When** it applies
**Then** it applies anyway, the `updated_by_owner` event names the overwritten change (its intent id, actor and time), the audit keeps both changes, and the Hub shows a toast naming the entity: "Price updated by owner", "Plan updated by owner", "Pricing rule updated by owner", "Coupon updated by owner", "Product updated by owner"; the toast table is keyed by kind so later kinds add their own line

**Given** a command whose inner intent fails validation (for example a plan for a deleted group)
**When** it is applied
**Then** the Hub posts `rejected` with the `AppError` code, nothing changes, and the command is not retried by the Hub

**Given** an applied `rate.upsert`, `rule.upsert` or `plan.upsert`
**When** it commits
**Then** `catalog.changed` / `settings.changed` reach floor tiles, the Hub header `chip-rule` and kiosk attract screens, and an integration test shows the new rate on both within 1 minute of apply

**Given** `backup.now`
**When** it applies
**Then** the uploader and summary engine run a cycle at once (Stories 8.2, 8.4), and the command is reported applied when that cycle's uploads succeed or `rejected` with the transport error

**Given** a command whose kind has no Hub handler in this binary (a newer kind sent to an older Hub)
**When** it is fetched
**Then** the Hub posts `rejected` with `UNSUPPORTED` and changes nothing

**Given** the `proptest` generator
**When** this story ships
**Then** it includes owner commands interleaved with staff edits of the same rows, and the invariant holds that the final row equals the last applied owner command where one followed the staff edit

### Story 8.20: Commands are swept, expire after 7 days, and are fenced by epoch

As a café owner,
I want a command that got stuck to be retried, one that waited a week to expire visibly, and a replaced Hub never to apply my changes,
So that every change ends in a clear state and only the real main Hub applies it.

**Covers:** FR155 (Expired after 7 days), FR156, AD-5 (sweeper, `FENCED` re-queue, fence move), AD-11, AD-38, NFR25 (command backlog)

**Acceptance Criteria:**

**Given** the jobs role sweeper, running every minute
**When** it finds `fetched` commands older than 10 minutes with no result
**Then** it returns them to `pending` so the next fetch delivers them again; the Hub's `applied_commands` dedupe makes a second delivery report `applied` without re-applying

**Given** `pending` commands older than 7 days
**When** the sweeper runs
**Then** they become `expired` with `COMMAND_EXPIRED`, and a later fetch never returns them

**Given** a fetch `GET /v1/commands?after=&epoch=` with an epoch below `branch_fence`
**When** the command role handles it
**Then** it returns `FENCED` with the current fence and no commands, and the Hub self-locks per Story 2.11 behaviour

**Given** a result post whose epoch is below `branch_fence`
**When** it arrives
**Then** it is rejected `FENCED` and the command is re-queued to `pending`

**Given** `branch_fence` moves to a new epoch with `fork_seq`
**When** the control role commits the move
**Then** commands applied at the old epoch with `applied_seq > fork_seq` return to `pending`

**Given** re-queued commands whose `id` is below the Hub's cursor
**When** the next fetch runs
**Then** the relay returns every `pending` row for the branch in `id` order, regardless of the cursor, so re-queued rows are never skipped

**Given** a test that moves the fence with a fixture CAS during a burst of 100 commands
**When** the new-epoch Hub fetches
**Then** every command ends applied exactly once at the new epoch, and none is lost or doubled

**Given** the command role metrics
**When** commands wait
**Then** "command backlog" (pending + fetched per branch, oldest age) is exported

### Story 8.21: Preview the target branches and follow each command row to Applied

As a café owner,
I want to see which branches a change will reach and whether they're online before I send it, then watch each one apply,
So that I know exactly where my change has landed.

**Covers:** FR155, FR156, UX-DR23 (owner command states), UX-DR83, UX-DR101 (apply errors), UX-DR103 (command rejected copy), UX-DR113 (polite live region), UX-DR115 (key-owner-price-change), UX-DR116

**Acceptance Criteria:**

**Given** the preview and command-row components
**When** this story is tested
**Then** they are exercised with a fixture `rate.upsert` command built in the test (no Manage screen exists yet); the Manage → Pricing story is their first real consumer

**Given** any Manage edit about to be sent
**When** the owner chooses Send
**Then** a preview lists each target branch (one, or "All branches" expanded) with an online/offline `status-badge` from its heartbeat, marks branches that can't take the change as not offered with the reason inline (not entitled, needs a Hub update), and offers Send and Cancel

**Given** a sent change
**When** the cloud accepts it
**Then** Manage shows one `command-row` per target branch: what changed, scope (branch or "All branches"), `status-badge` and time queued, grouped under Today / Earlier

**Given** a row's server state
**When** it renders
**Then** `pending` and `fetched` show "Waiting for branch · 2 min" (info), `applied` shows "Applied · 14:32" (ok), `rejected` shows "Device/Hub revoked" when the code is `TOKEN_REVOKED` and otherwise "Couldn't apply at branch" with Retry (danger), and `expired` shows "Expired" with Resend (muted)

**Given** Retry on a rejected row or Resend on an expired one
**When** pressed
**Then** a new command with a new idempotency key and the current `base_rev` is sent for that branch only, and the old row stays in history

**Given** the command list is open
**When** a row changes state
**Then** it updates within 10 s (polling while visible) and the change is announced once in a polite live region

**Given** an offline branch's card on Today
**When** it has pending commands
**Then** they are listed on the card as "Waiting for branch"

### Story 8.22: Commands queue on the owner's device while it is offline

As a café owner on a bad connection,
I want changes I make with no signal to wait on my phone and send themselves when I'm back online,
So that I don't lose a change or send it twice.

**Covers:** FR155 (Queued on this device), AD-14 (client ULID key, TTL ≤ 7 days, creation order), UX-DR83, UX-DR98, UX-DR103 (queued offline)

**Acceptance Criteria:**

**Given** no network
**When** the owner sends a change
**Then** the command (with a client ULID `idempotency_key`, `base_rev` and `created_at`) is stored in IndexedDB and its rows show "Queued on this device" (muted), and the preview shows target branch statuses as last known with their age

**Given** queued commands
**When** the network returns (or the app is reopened online)
**Then** they are posted in creation order, each row moves to "Waiting for branch" once accepted, and a retry after a lost response returns the same server row (no duplicate)

**Given** a queued command older than 7 days
**When** the app next runs
**Then** it is not sent and shows "Expired" with Resend

**Given** a command refused by the cloud on send (for example `FEATURE_NOT_ENTITLED` or `BRANCH_UPDATE_REQUIRED`)
**When** the queue drains
**Then** that row shows "Couldn't apply at branch" with Retry and the rest of the queue continues

**Given** logout
**When** it completes
**Then** queued commands are kept only if the owner chooses "Keep queued changes"; otherwise they are deleted with the cache

### Story 8.23: Manage → Pricing — plans and rates, pricing rules and coupons

As a café owner,
I want to change rates, plans, pricing rules and coupons for one branch or all of them from my phone,
So that I can run a price change without going to the café.

**Covers:** FR154 (prices, plans, rules), FR157, UX-DR25 (`tab-control`), UX-DR76 (rule rows), UX-DR83, UX-DR90 (Manage → Pricing), UX-DR115 (key-owner-price-change), UX-DR116

**Acceptance Criteria:**

**Given** Manage → Pricing
**When** it opens
**Then** a `tab-control` offers Plans & rates · Pricing rules · Coupons (selected tab in the URL), filled from each branch's summary `catalogue` with its data age

**Given** Plans & rates
**When** the owner edits a group's hourly rate or a plan (name, minutes, price, validity, groups)
**Then** the form validates like the Hub (same field rules and error copy), and Send opens the Story 8.21 preview with `rate.upsert` or `plan.upsert` and the row's `base_rev`; no MFA prompt is shown

**Given** Pricing rules
**When** the list renders
**Then** each rule row shows name, schedule (weekdays and hours, or dates), branches and status (active now, scheduled, ended), and create/edit sends `rule.upsert`

**Given** Coupons
**When** the owner creates or edits one
**Then** `coupon.upsert` is sent with the same fields as the Hub (Story 5.21)

**Given** "All branches" for an edit of an item that exists by name in some branches only
**When** the preview opens
**Then** branches where the item doesn't exist are offered "Create here" or excluded, and the owner chooses before sending

**Given** an applied rate change at an online branch
**When** Journey 6 is run end to end in staging
**Then** the row reads Applied and the new rate shows on the floor view and kiosk attract within 1 minute of apply, with command delivery under 2 minutes

### Story 8.24: Manage → Products and bundles

As a café owner,
I want to add or change products and bundles for my branches remotely,
So that the menu and prices are right everywhere without calling the counter.

**Covers:** FR154 (products/bundles), UX-DR83, UX-DR90 (Manage → Products)

**Acceptance Criteria:**

**Given** Manage → Products
**When** it opens
**Then** it lists products and bundles per branch from the summary `catalogue` with category, price, tax rate, active flag and kitchen flag, searchable by name or barcode

**Given** a product edit (name, category, price, tax rate, HSN, barcode, active, kitchen flag, low-stock threshold)
**When** sent
**Then** `product.upsert` goes through the preview with `base_rev`, with no MFA prompt, and stock quantities are never editable from the Owner app

**Given** a bundle edit
**When** the owner sets components and the bundle's own price
**Then** components must exist at each target branch, otherwise that branch is not offered with the reason inline

**Given** a product image
**When** the owner attaches one
**Then** it is resized to at most 512 px and 200 KB in the browser before it is sent in the payload

### Story 8.25: Step-up MFA for sensitive commands

As a café owner,
I want a fresh authenticator check only when I change staff, roles or revoke a device,
So that a stolen session can't lock out my staff, and I'm not nagged for everyday price changes.

**Covers:** FR3, AD-10 (step-up), AD-30 (`elevated_by.kind = mfa`), NFR11, NFR15 (WCAG 3.3.8), UX-DR83 (MFA only for sensitive kinds), UX-DR112

**Acceptance Criteria:**

**Given** a command whose kind is flagged `sensitive` (tested in this story with a fixture sensitive kind registered only in the test catalogue; the staff, role and device stories add the real ones)
**When** it is posted without a step-up proof younger than 5 minutes
**Then** the cloud refuses it `MFA_REQUIRED`, and the app asks for a current TOTP code (or a backup code, or an SMS code when the owner enabled fallback) in place, then re-sends the same command with the same key

**Given** a correct code
**When** the step-up succeeds
**Then** the session holds a step-up proof for 5 minutes, and the command row records `elevated_by{kind: mfa, id: owner_id}`, which the Hub copies into the change row when it applies

**Given** a non-sensitive kind (prices, plans, rules, coupons, products, credit reminder, backup now, alert thresholds)
**When** sent
**Then** no MFA prompt appears, even with no recent step-up

**Given** a sensitive command queued on the device while offline (Story 8.22)
**When** the queue drains
**Then** it is held as "Queued on this device" until the owner passes step-up MFA in the app; it is never sent without it

**Given** 5 wrong step-up codes in a row
**When** the next is entered
**Then** step-up pauses for 15 minutes with the time shown, and the sign-in session itself is not ended

**Given** the step-up dialog
**When** used with a keyboard and screen reader
**Then** focus moves into it, paste is allowed, the error never says which factor failed, and Esc cancels without sending

### Story 8.26: Manage → Staff users and roles by command

As a café owner,
I want to add, change and deactivate staff and edit roles for one branch or all,
So that I can manage my team remotely without ever knowing their PIN or password.

**Covers:** FR154 (staff users/roles, one branch or all), FR3, AD-10 (credentials never travel), AD-30, UX-DR90 (Staff users & roles), UX-DR102 (setup code display), UX-DR103

**Acceptance Criteria:**

**Given** the command catalogue
**When** this story ships
**Then** it adds `staff.upsert`, `staff.deactivate` and `role.upsert`, each flagged `sensitive`, with their payload schemas and Hub handlers, and the Hub owner-wins toast table gains "Staff member updated by owner" and "Role updated by owner"

**Given** Manage → Staff users & roles
**When** it opens
**Then** it lists staff per branch from the summary `catalogue` (name, phone, role, active) and the tenant's roles with their permissions from the fixed catalogue

**Given** a new staff member (name, phone, role) for one branch or All branches
**When** the owner sends it and passes step-up MFA
**Then** the cloud generates one 8-character setup code per target branch, sends `staff.upsert` carrying only `setup_code_hash` and `setup_expires_at = now + 72 h` (as in Story 2.16), and shows once "Setup code for Neha: K7Q4 9MZD. Give it to her; it works once." with Copy, one per branch

**Given** a role change or `staff.deactivate`
**When** sent with step-up MFA
**Then** the Hub applies it, a deactivated person's Hub sign-in ends at once, and the Owner cannot deactivate the last Owner (rejected, shown as "Couldn't apply at branch")

**Given** a custom role edit
**When** sent
**Then** `role.upsert` carries permissions only from the `arena-proto` catalogue; unknown permissions are refused by the cloud before sending

**Given** a Free branch at its 3-user limit
**When** the owner adds a user there
**Then** that branch is not offered in the preview and shows `tier-lock` "Available on Basic"

### Story 8.27: Manage → Devices and groups, and revoking a device

As a café owner,
I want to see my branches' devices and groups and revoke a lost or replaced device from my phone,
So that a stolen PC or kiosk can never connect again.

**Covers:** FR154 (revoke device), FR3, AD-10 (revocation within one heartbeat, immediately on LAN), AD-29, NFR11, UX-DR7, UX-DR83, UX-DR90 (Devices & groups)

**Acceptance Criteria:**

**Given** the command catalogue
**When** this story ships
**Then** it adds the owner command kinds `group.upsert` and `device.assign` (non-sensitive) and `device.revoke` (sensitive), with their payload schemas; their Hub handlers apply the Hub's existing intents of the same names (`device.revoke` is the intent built in Story 1.9; groups and assignment are the Epic 2 intents) with `actor{kind: owner}` and are not redefined here

**Given** Manage → Devices & groups
**When** it opens
**Then** it lists each branch's groups (name and swatch dot) and devices (name, kind, group, revoked) from the summary `catalogue`, and group edits send `group.upsert` / `device.assign` (non-sensitive)

**Given** "Revoke device" on a kiosk or Pi
**When** the owner confirms and passes step-up MFA
**Then** the `device.revoke` command is sent; at the Hub it applies the existing `device.revoke` intent, which marks the token revoked, closes the device's socket at once with `TOKEN_REVOKED`, frees the device slot, and is audited with `elevated_by{kind: mfa}`

**Given** a command for a branch whose Hub token has been revoked in the cloud
**When** it is pending
**Then** the cloud marks it `rejected` with `TOKEN_REVOKED`, shown as "Device/Hub revoked"

**Given** a revoke for a device that is already revoked
**When** applied
**Then** it reports `applied` and changes nothing

> Notes: revoking the Hub itself (`hub.revoke`) and releasing the branch key for a restore (`backup.restore`) are reserved in the catalogue with their sensitive flags but delivered with Hub replacement in Epic 9 ("Owner replaces a failed Hub and gets a new pairing code" for `hub.revoke`, "Owner releases the branch key to the replacement Hub" for `backup.restore`, both citing FR3), which covers FR3's "backup restore" sensitive action.

### Story 8.28: The cloud sends player and owner messages through a swappable messaging adapter, exactly once

As a café owner,
I want messages to my players sent from Arena 360's own WhatsApp account, once, with SMS if WhatsApp fails,
So that I need no provider account or keys, and no player gets the same message twice.

**Covers:** FR187 (cloud send, exactly once, versioned templates, no Free sends), AD-27 (send ledger, recipient hash only), AD-34 (adapter, recorded fake, manual mode), NFR13, NFR23, UX-DR96

**Acceptance Criteria:**

**Given** `POST /v1/notify {outbox_id, kind, purpose, consent_policy_version, recipient, params}` from a paid Hub's device token
**When** the cloud handles it
**Then** it inserts a `send_ledger` row (new table in `gz_control`: id, tenant_id, branch_id, `outbox_id` unique, kind, purpose, `recipient_hash` (salted SHA-256), template version, channel whatsapp|sms, state queued|sent|failed, attempts, provider message id, created_at, sent_at), picks the current approved template version for `kind`, and sends through the messaging adapter

**Given** the same `outbox_id` posted again
**When** the cloud handles it
**Then** no second send happens and the original result is returned

**Given** the adapter selected by config (Gupshup at launch)
**When** a WhatsApp send fails with a non-retryable delivery error
**Then** the same message is sent by SMS, and the ledger records both attempts and the final channel

**Given** a Free branch
**When** it posts to `/notify`
**Then** it is refused `FEATURE_NOT_ENTITLED` and nothing is sent

**Given** templates for credit reminder, low balance, session ending, console 5-minute alert and receipt
**When** they are stored (new table `message_templates`: kind, version, WhatsApp template id, SMS text, approved_at)
**Then** each has a version, the approved WhatsApp template id and an SMS text, and changing a template creates a new version without changing the ledger rows of earlier sends

**Given** dev and CI
**When** the adapter is the recorded fake
**Then** sends are captured for assertions and no provider is called; a "manual" mode logs the rendered message for operators instead of sending

**Given** the cloud logs and traces
**When** a send happens
**Then** they carry the outbox id and branch id only, never the phone number or message text

### Story 8.29: The Hub drains its notification outbox after the undo window

As a player,
I want my receipt and other messages sent only once the action can no longer be undone, and only if I still agree to messages,
So that I never get a message for something that was taken back.

**Covers:** FR187 (never for an undone action, consent re-check, queued while offline), FR72 (WhatsApp receipt sent), AD-27, AD-28, AD-13 (notification sent is irreversible), AD-24, NFR13

**Acceptance Criteria:**

**Given** `notify_outbox` rows in state `queued` whose `not_before` has passed
**When** the drain task runs (every 10 s on a paid Hub)
**Then** for each row it re-checks the player's current consent through a per-row eligibility check that later rules can extend (the erasure rule is added with erasure in Epic 9), skips (state `skipped`) rows whose forward intent was reverted, decrypts the recipient, and posts to `/v1/notify` keyed by the outbox row ULID

**Given** a cloud reply
**When** it arrives
**Then** the Hub applies `notify.sent` (with channel) or `notify.failed` (with code), and "notification sent" is registered with the irreversible-effect guard so the forward action can no longer be undone

**Given** a receipt row (Story 5.19) whose order has no invoice number yet
**When** the drain looks at it
**Then** it waits until the number exists

**Given** the WAN is down
**When** rows are due
**Then** they stay `queued` and are sent in `not_before` order once the cloud is reachable; rows older than 7 days are marked `expired` instead of sent

**Given** a player whose consent was withdrawn after the row was queued
**When** the drain reaches it
**Then** it is marked `skipped` and nothing is posted

**Given** a lost reply after the cloud sent the message
**When** the drain retries
**Then** the send ledger returns the original result and no second message is sent

### Story 8.30: Low-balance, session-ending and console 5-minute messages

As a player who agreed to service messages,
I want a WhatsApp when my wallet runs low, when my session is about to end, and 5 minutes before my console time is up,
So that I can top up or extend before I'm cut off.

**Covers:** FR187 (low balance, session ending, console 5-min alert), FR141 (optional 5-min WhatsApp/SMS with consent), FR114 (per-message on/off), AD-27, UX-DR96

**Acceptance Criteria:**

**Given** a paid branch with "Low balance" on in Settings → Notifications (Story 5.15) and a profiled player with current consent
**When** the Story 6.20 low-balance crossing fires
**Then** a `notify.requested` (kind low_balance, amount) is written once per crossing

**Given** "Session ending" on and a PC session with an end time for a consenting profiled player
**When** remaining time reaches 5 minutes
**Then** one `notify.requested` (kind session_ending, minutes, device name) is written for that session; it is not repeated after an Extend

**Given** a console session for a consenting profiled player
**When** remaining time reaches 5 minutes
**Then** one `notify.requested` (kind console_5min, TV name) is written for that session

**Given** a guest, a player without consent, a Free branch or the message turned off
**When** the trigger fires
**Then** no outbox row is written

**Given** the session is ended or extended within the undo window of the triggering moment
**When** the drain runs
**Then** a session_ending or console_5min row for a session no longer near its end is skipped

### Story 8.31: Credit reminders go out on cycle days, and Free shows an overdue list

As a café owner,
I want players who owe credit reminded automatically on the days I set, only if they agreed,
So that credit gets collected without my staff chasing anyone.

**Covers:** FR52, FR49 (cycle days used), FR187 (credit reminder), AD-27, UX-DR88 (Players → Overdue), UX-DR96

**Acceptance Criteria:**

**Given** a paid branch with "Credit reminder" on and a consenting player with outstanding credit and a reminder cycle (default 3, 7, 14 days)
**When** the daily credit check runs at 11:00 branch time
**Then** for each cycle day equal to the number of days since the player's oldest unpaid spend (FIFO, Story 3.20), one `notify.requested` (kind credit_reminder, outstanding amount, branch name) is written

**Given** a player not on a cycle day today
**When** the check runs
**Then** nothing is written for them

**Given** a player without consent, a guest balance, or "Credit reminder" off
**When** the check runs
**Then** no reminder is written, and the player stays on the Hub's overdue list

**Given** the Hub was off at 11:00
**When** it starts later the same business day
**Then** the check runs once for that day; missed earlier days are not sent

**Given** the Hub Players screen
**When** this story ships
**Then** it adds a Players → Overdue view (new, read-only): players with outstanding credit sorted by oldest unpaid date, with outstanding amount, aging bucket (0–7, 8–14, 15+ days, from Story 3.20), consent state and, on paid branches, the date of the last reminder sent; tapping a row opens the player profile

**Given** a Free branch
**When** Players → Overdue opens on the Hub
**Then** it shows the same list with a "Reminders are available on Basic" `tier-lock` line, and no reminder is ever written

### Story 8.32: Reports — credit, with a one-tap reminder

As a café owner,
I want to see who owes what at each branch, by age, and send a reminder with one tap,
So that I can follow up on overdue credit myself.

**Covers:** FR150 (credit, tap to send reminder, credit export CSV/PDF), FR154 (send credit reminder), FR52, UX-DR81, UX-DR83, UX-DR90 (Reports → Credit, Exports)

**Acceptance Criteria:**

**Given** Reports → Credit
**When** it opens
**Then** it shows outstanding per branch aged 0–7, 8–14 and 15+ days from the latest rollup (with its date), and a list of players by player id and username with outstanding and oldest unpaid date, never phone, name or email

**Given** the command catalogue
**When** this story ships
**Then** it adds `credit.remind{player_id}` (non-sensitive) with its payload schema and Hub handler

**Given** a player row with consent recorded
**When** the owner taps "Send reminder"
**Then** a `credit.remind{player_id}` command is sent (no MFA) and the row shows its `command-row` state

**Given** the Hub applies `credit.remind`
**When** the player has consent and outstanding credit
**Then** it writes a `notify.requested` (kind credit_reminder) and reports applied; otherwise it reports rejected, shown as "Couldn't apply at branch"

**Given** a player without consent
**When** the row renders
**Then** "Send reminder" is not offered and the row says "No consent for messages"

**Given** a reminder already sent to that player today (automatic or manual)
**When** the owner taps again
**Then** the app warns "Reminder already sent today" and asks to confirm before sending

**Given** Reports → Credit with a range and branch chosen
**When** the owner chooses Export → CSV or PDF
**Then** it exports exactly as the other owner reports do (CSV built from the same rollups, one row per player per branch with player id, username, outstanding, aging bucket and oldest unpaid date, never phone, name or email; PDF via the print-styled view), and Credit is listed in Reports → Exports

### Story 8.33: Owner alerts by web push and WhatsApp, and the Alerts tab

As a café owner,
I want alerts on my phone the moment something needs me, with WhatsApp as the dependable channel,
So that I don't miss a problem even when the app is closed.

**Covers:** FR153 (channels, Alerts list), AD-27 (VAPID web push, WhatsApp primary), UX-DR96 (owner alerts), UX-DR100 ("All quiet."), UX-DR113

**Acceptance Criteria:**

**Given** Alerts → Notification settings
**When** the owner turns on push on a supported browser
**Then** the app requests permission, subscribes with the cloud's VAPID public key, and stores the subscription in `push_subscriptions` (new table in `gz_control`: id, owner_user_id, endpoint unique, p256dh key, auth secret, user agent, created_at, last_success_at); on iOS not installed to the Home Screen it shows "Add Arena 360 to your Home Screen to get alerts here." and does not ask

**Given** an owner alert raised for a branch (a test fixture source in this story)
**When** the cloud handles it
**Then** it writes an `owner_alerts` row (new table: id, tenant, branch, kind, display parameters, created_at; kept 90 days), sends web push to each subscription, and sends WhatsApp to the owner's phone through the messaging adapter, each exactly once per alert id

**Given** a push subscription the push service reports gone (404/410)
**When** a send fails that way
**Then** the subscription is deleted

**Given** a tapped push notification
**When** the app opens
**Then** it deep-links to the branch card or report the alert names

**Given** the Alerts tab
**When** it opens
**Then** it lists `owner_alerts` newest first with branch, kind, plain text in the staff register and time, and shows "All quiet." when empty; a new alert is announced once in a polite live region

**Given** Alerts → Notification settings
**When** the owner turns WhatsApp alerts off
**Then** only push and the Alerts list are used (on by default)

### Story 8.34: The Hub raises owner alerts for cash variance, low stock, overdue credit and large refunds or discounts

As a café owner,
I want to be told when a shift's cash is off, stock runs low, credit goes 15+ days overdue, or a big refund or discount is given,
So that I can act the same day.

**Covers:** FR153 (Hub-sourced alerts, per-tenant thresholds), FR85 (owner alert on variance over threshold), FR91 (low stock — owner half), AD-27, AD-5 (`settings.set` thresholds), UX-DR96

**Acceptance Criteria:**

**Given** Alerts → Thresholds in the Owner app
**When** the owner sets cash variance (default: the branch threshold), large refund (default ₹500) and large discount (default ₹200 or 20%)
**Then** a non-sensitive `settings.set` command (this story adds the owner-alert-threshold keys to the catalogue with their Hub handler) is sent to all branches and each Hub stores them as `owner_alert_thresholds`

**Given** a `shift.closed` with |variance| over the threshold
**When** it commits
**Then** a `notify.requested` (purpose owner_alert, kind cash_variance, branch, staff name, amount, reason) is written with `not_before` = commit

**Given** a `stock.low` crossing (Story 5.8), a player reaching 15+ days overdue (Story 3.20), or a refund or discount over its threshold
**When** it commits
**Then** a `notify.requested` owner_alert of the matching kind is written once per crossing, with `not_before` = commit + the action's undo window

**Given** a refund or discount undone inside its window
**When** the drain runs
**Then** its owner alert is skipped

**Given** the drain posts an owner_alert row
**When** the cloud receives it
**Then** it creates the alert of Story 8.33 for the tenant's owners, deduped by outbox id, with no player personal data (player ids only)
**And** the owner-alert kind list is an `arena-proto` enum so later epics add kinds (each with its plain-text line) without changing this path

### Story 8.35: The cloud alerts the owner when a branch goes offline during opening hours

As a café owner,
I want to know when a branch has been unreachable for half an hour while it should be open,
So that I can call the branch before customers notice.

**Covers:** FR153 (branch offline: no heartbeat or no upload for 30 min in opening hours), FR149 (offline state), AD-27, NFR25; Spine §Operational Envelope "Tenant alerts"

**Acceptance Criteria:**

**Given** the heartbeat
**When** a paid Hub sends it
**Then** it carries the branch's opening hours and timezone (from Story 2.26 settings) and the cloud caches them on the branch

**Given** the jobs role, every 5 minutes
**When** a paid branch has had no heartbeat for 30 minutes and the current time is inside its opening hours
**Then** one "branch offline" owner alert (Story 8.33) is raised for that outage

**Given** a paid branch whose heartbeat is alive but whose `last_upload_at` and `last_summary_at` (Story 8.3) are both older than 30 minutes inside its opening hours
**When** the jobs role runs
**Then** one "branch not uploading" owner alert is raised for that episode ("<branch> is online but hasn't backed up for 30 min"), and the next successful upload closes the episode so a later one can alert again

**Given** a branch that was alerted offline
**When** its heartbeat returns
**Then** a "branch back online" entry is added to the Alerts list (push only, no WhatsApp) and the next outage can alert again

**Given** a branch offline outside opening hours
**When** opening time arrives and it is still offline
**Then** the alert is raised 30 minutes after opening time

**Given** many branches missing heartbeats at once (for example a cloud-side fault)
**When** more than 20% of paid branches would alert in one run
**Then** owner alerts are held and the on-call is paged instead

### Story 8.36: The audit builder rebuilds a branch month on demand

As a café owner or Arena 360 support agent,
I want a month of a branch's history rebuilt in the cloud only when I ask for it,
So that I can search what happened without the cloud keeping a standing copy of my data.

**Covers:** FR158 (build), AD-37, AD-4 (replay honouring fork points), AD-21, AD-23, AD-24, AD-36 (audit-cache 24 h), NFR13, NFR23, NFR24, NFR25 (builder queue depth)

**Acceptance Criteria:**

**Given** `POST /v1/branches/{id}/audit/build {month}` from an owner of that branch, or the support path of Story 8.37 with a recorded reason
**When** no ready cache exists for that month
**Then** a `builder_jobs(branch_id, month, state = queued)` row is created or the existing queued/building job is reused, and the response is `AUDIT_BUILDING` with progress 0

**Given** the `builder` role
**When** it picks a queued job
**Then** it downloads the nearest snapshot on or before the month start plus every batch up to the month end, replays them with `arena-replay` into a temp SQLite honouring fork points (orphaned rows kept in a separate table), uploads `audit-cache/<yyyy-mm>.db.zst` (the R2 prefix has a 24 h lifecycle rule), sets the job `ready` with `built_at` and `key`, and updates `progress` as it goes

**Given** no snapshot before the month
**When** the job runs
**Then** it replays from the branch's first batch

**Given** a missing batch range or a checksum mismatch
**When** the replay hits it
**Then** the job becomes `failed` and the API returns `AUDIT_BUILD_FAILED`; a new build request starts a new attempt

**Given** the round-trip test
**When** a fixture Hub's snapshot and batches are replayed by the builder
**Then** the checksum of the rebuilt database equals the Hub database's checksum for the same seq

**Given** a tenant above its per-tenant audit-build limit
**When** another build is requested
**Then** it returns `RATE_LIMITED`, and "builder queue depth" is exported as a metric

### Story 8.37: The builder serves audit queries from a local cache

As a café owner or support agent,
I want searches over a built month to answer quickly from a fixed set of queries,
So that audit is fast after the first build and can never run arbitrary queries over my data.

**Covers:** FR158 (search), AD-37 (fixed query catalogue, 50 GB LRU), AD-21 (support reads with a recorded reason), AD-24 (player ids only), NFR7, NFR24 (audit cache 24 h)

**Acceptance Criteria:**

**Given** a `ready` job for the month
**When** `GET /v1/branches/{id}/audit?q=` is called
**Then** the builder serves it from its local copy of `audit-cache/<yyyy-mm>.db.zst` (downloading it on a miss) using only queries from the `arena-proto` query catalogue (by actor, module, intent, entity, time range; paged 50 rows), and any other query shape returns `UNSUPPORTED`

**Given** the local cache
**When** it exceeds 50 GB
**Then** least-recently-used months are evicted, and an evicted month is re-downloaded from R2 on the next query while its cache object exists

**Given** a cache whose `built_at` is older than 24 h, or whose R2 object has expired
**When** a query arrives
**Then** the API returns `AUDIT_BUILDING` and a new build job is queued

**Given** a query while the month is `queued` or `building`
**When** it is called
**Then** it returns `AUDIT_BUILDING` with the job's progress; a `failed` job returns `AUDIT_BUILD_FAILED`

**Given** query results
**When** they are returned
**Then** player PII columns are omitted (ciphertext never leaves the builder) and players appear by id only; orphaned rows are returned in a separate list

**Given** the support path: admin-scoped endpoints `POST /v1/admin/branches/{id}/audit/build` and `GET /v1/admin/branches/{id}/audit?q=`, reachable only with an Arena 360 staff credential behind Cloudflare Access
**When** a support agent calls them (from a script or curl; the admin app UI on top is Epic 11)
**Then** a non-empty `reason` is required (`VALIDATION` otherwise), and each call writes one `admin_audit` row (Story 6.1) with `actor_kind = admin`, the staff id, branch, month, query and reason before any data is returned

### Story 8.38: Audit search in Reports with "Preparing audit…"

As a café owner,
I want to search a branch's change history by who did it and what part of the app,
So that I can find out what happened to a price, a bill or a shift.

**Covers:** FR158, NFR7 (first results < 10 s), UX-DR81 (Preparing audit…), UX-DR90 (Reports → Audit), UX-DR102 (audit building/failed), UX-DR103

**Acceptance Criteria:**

**Given** Reports → Audit
**When** it opens
**Then** it offers branch, month, actor (staff, owner, system, device, player id) and module (sessions, money, POS, stock, staff, settings, commands) filters and a free-text intent search from the query catalogue

**Given** the first search of a month with no ready cache
**When** it runs
**Then** the results area shows "Preparing audit…" as muted text on a surface panel with progress (no spinner overlay), polls the API, and shows the first page of results when ready; first results arrive in under 10 s for a busy branch month in the staging benchmark

**Given** a later search of the same month within 24 h
**When** it runs
**Then** results return without the preparing state

**Given** a failed build
**When** the API returns `AUDIT_BUILD_FAILED`
**Then** the panel shows "Couldn't prepare the audit." with Retry

**Given** results
**When** they render
**Then** each row shows time, actor (staff name or "Owner" or "System"), intent, entity and changed fields, with players shown as ids only, and `elevated_by` noted ("PIN by Priya", "Owner MFA")

**Given** orphaned rows from a fork in that month
**When** results render
**Then** they are listed in a separate "Not applied (orphaned)" section

### Story 8.39: A Cloudflare Tunnel for entitled branches, run by the Windows Hub

As a café owner with the Tunnel add-on or Premium,
I want my Hub reachable for live view and remote devices without opening any port at the café,
So that remote features work through ordinary internet with nothing exposed.

**Covers:** FR151 (Tunnel prerequisite), FR21 (Tunnel prerequisite), AD-6, AD-29 (`tunnel` flag), AD-22, NFR11; Additional Requirements "Build spikes" (Cloudflare Tunnel pricing per branch)

**Acceptance Criteria:**

**Given** a branch whose licence carries the `tunnel` flag (Premium, or the Tunnel add-on on Basic)
**When** the heartbeat sees the flag for the first time
**Then** the control role creates a named Cloudflare Tunnel and an Access application for `<branch_id>.tunnel.arena360.cloud` through the Cloudflare API and returns the tunnel credential once in the heartbeat response

**Given** the credential
**When** the Windows Hub receives it
**Then** it stores it in DPAPI and runs the bundled `cloudflared` connector as a child process, outbound only, forwarding to the Hub's live-view and remote-device listeners only

**Given** the `tunnel` flag removed (downgrade or lapse after `exp`)
**When** the next heartbeat arrives
**Then** the Hub stops the connector and deletes the credential, and the cloud deletes the tunnel and Access app

**Given** an Android Hub
**When** the flag is present
**Then** no tunnel is created (no Tunnel on Android), the heartbeat response records `tunnel: unsupported_platform` for the branch (which the Live view story reads), and the Hub's Settings → System tunnel line shows "Needs the Windows Hub"

**Given** the connector crashes or the WAN drops
**When** it happens
**Then** the Hub restarts it with backoff and trading is never affected

**Given** the build spike
**When** it is run before this story starts
**Then** the per-branch cost of a tunnel plus Access app is recorded against the ₹200 add-on price

### Story 8.40: Live view — a read-only floor through the Tunnel

As a café owner on Premium or with the Tunnel add-on,
I want to watch my branch's floor second by second from anywhere,
So that I can see a busy night as it happens without anyone being able to change anything.

**Covers:** FR151, AD-43, AD-42 (live-view client class), AD-25, AD-29, UX-DR24, UX-DR61, UX-DR82 (Live view placement)

**Acceptance Criteria:**

**Given** an entitled branch
**When** the owner opens Live view (sidebar on desktop, the button on each Today branch card on phone)
**Then** the app calls `GET /v1/live/{branch}`, gets a branch-scoped token signed by the cloud valid for at most 5 minutes, and opens a WebSocket to the branch's tunnel hostname with it

**Given** the token
**When** the Hub receives the `hello`
**Then** it verifies the signature with its licence verify keys, checks `branch_id` and expiry, and admits the connection as the live-view client class, which may only subscribe

**Given** a live-view connection
**When** it sends any intent or command frame
**Then** the Hub answers `FORBIDDEN` and closes nothing else

**Given** an open live view
**When** sessions start, tick and end
**Then** the owner sees read-only floor tiles (device, state, time, player id or Guest) grouped by zone, updated from `session.tick` and `device.state` within 2 s, with a "Live" data age

**Given** the token nears expiry
**When** 4 minutes have passed
**Then** the app fetches a new token and re-authenticates without dropping the view; an expired token is refused `TOKEN_INVALID`

**Given** a branch without the `tunnel` flag
**When** the owner looks for Live view
**Then** it shows `tier-lock` "Available on Premium" (no "Upgrade" link until Epic 10), and `GET /v1/live/{branch}` returns `FEATURE_NOT_ENTITLED`

**Given** an entitled branch whose Hub is Android (`tunnel: unsupported_platform`)
**When** the owner looks for Live view
**Then** the Today branch card and the Live view entry show "Needs the Windows Hub" and `GET /v1/live/{branch}` returns `PLATFORM_UNSUPPORTED`

**Given** the Hub unreachable through the tunnel
**When** the owner opens Live view
**Then** the app shows "Branch offline" and falls back to the Today card

### Story 8.41: Remote kiosks connect through the Tunnel with their device token

As a café owner with the Tunnel entitlement,
I want a kiosk outside the café network to pair and connect securely,
So that I can run a remote seat without opening the café to the internet.

**Covers:** FR21, AD-6, AD-42 (paired-device class, device token, pinned TLS), AD-29, NFR11

**Acceptance Criteria:**

**Given** an entitled branch
**When** staff generate a kiosk pairing code and choose "Remote device"
**Then** the code screen also shows the tunnel hostname, and a kiosk outside the LAN pairs by typing it, exchanging the code for a device token as on the LAN

**Given** a remote kiosk with its device token
**When** it connects through the tunnel
**Then** it is admitted only as the paired-device class with TLS pinned to the Hub certificate, appears on the floor view with a "Remote" label, and counts toward the device cap

**Given** a connection through the tunnel without a valid device token
**When** it sends `hello`
**Then** it is refused `TOKEN_INVALID` and closed; no anonymous or browser class is accepted through the tunnel

**Given** the `tunnel` flag is removed
**When** the connector stops
**Then** remote kiosks lose connection and follow Story 6.36 Hub-unreachable behaviour; running sessions are not ended by the Hub

**Given** a revoked remote device
**When** it reconnects
**Then** it is refused `TOKEN_REVOKED`

> Notes: the Pi agent (Epic 12) uses this same remote-device path when it ships.

### Story 8.42: The owner's remote experience proven against its targets

As a café owner,
I want the dashboard speed, data age, command delivery, audit time and offline catch-up measured on every release,
So that what I'm promised is what I get.

**Covers:** NFR1 (catch-up within 10 min), NFR7, NFR20, NFR21, NFR25

**Acceptance Criteria:**

**Given** the nightly staging run with the internal test café on Basic and Premium comp entitlements
**When** it measures Today
**Then** first render is under 2 s on the reference phone profile, and data age is within cadence (Basic ≤ 15 min, Premium ≤ 5 min) on at least 99% of 1,000 scripted opens

**Given** 200 owner commands to an online branch
**When** delivery is timed from post to `applied`
**Then** p95 is under 2 minutes

**Given** an 8-hour WAN outage soak with 20 kiosks (1,000 sessions, 500 orders)
**When** the WAN returns
**Then** all batches, the current summary and any missed rollups and outbox sends complete within 10 minutes, with no gap reported by the heartbeat and no duplicate send

**Given** the cloud stopped entirely for 1 hour during the soak
**When** the branch trades
**Then** nothing at the branch changes beyond `chip-offline`, and everything catches up after the cloud returns

**Given** a month of staging traffic
**When** R2 PUTs per tenant are summed
**Then** the per-branch cost estimate is under ₹10 on average (≈ ₹6 Basic, ≈ ₹12 Premium) and is published to the metrics dashboard

**Given** a busy branch month in staging
**When** an audit is requested for the first time
**Then** first results arrive in under 10 s

