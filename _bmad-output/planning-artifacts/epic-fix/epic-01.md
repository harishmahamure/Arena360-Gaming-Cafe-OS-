## Epic 1: Foundation — Hub and Kiosk install, start, and staff log in

On a seeded branch, the Hub and a locked-down Kiosk install and start, and a staff member logs in with roles, PIN elevation and idle lock, all with the WAN down.

> Notes: a "seeded branch" in this epic is created by a **dev-only fixture** (`arena-hub dev seed`, compiled only with the `dev-fixtures` feature) that applies `branch.bootstrap`, seeds staff and seeds one kiosk device token through `apply()`. Real Hub pairing, kiosk pairing/mDNS discovery and staff setup codes are Epic 2 (FR13–FR15, FR104). In Epic 1 the manager-PIN verifier is pushed to kiosks on Hub start and kiosk connect; the push on PIN set or rotation is added with real credentials in Story 2.14. A device can be revoked through the `device.revoke` intent from Story 1.9 (dev-fixture command in Epic 1); the owner's revoke action is Epic 8. Each story lists what it covers on its **Covers** line.

### Story 1.1: Monorepo workspace with pinned toolchains and CI gates

As a developer on Arena 360,
I want one monorepo with every Rust crate and the JS workspace root in place, with pinned versions checked by CI,
So that every later story starts from the agreed structure and version or layering drift is caught automatically instead of in review.

**Covers:** AD-8 (crate layout), AD-15, AD-41, NFR17; Additional Requirements "STARTER / SCAFFOLD" (workspace and crates), "CI/CD" (pipeline and pin gate)

**Acceptance Criteria:**

**Given** a fresh clone of the repository
**When** a developer runs the documented bootstrap command with the pinned toolchains
**Then** one package manager (pinned via `packageManager`) installs the JS workspace root and `cargo build --workspace` succeeds
**And** the Rust workspace contains the crates `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-tasks`, `arena-replay`, `arena-cloud`, `arena-hub` (bin), `arena-kiosk-core`, `arena-kiosk-svc` and `arena-pi`, each compiling with a placeholder module and one passing test

**Given** the layer table in the architecture spine
**When** CI runs the crate dependency-direction check
**Then** it fails if `arena-proto` depends on any workspace crate, or if `arena-core` depends on `arena-api`/`arena-tasks`/`arena-cloud`, naming the offending edge

**Given** the version pins
**When** a developer inspects the repo
**Then** `rust-toolchain.toml` pins a stable Rust, `.nvmrc`/`engines` pin Node 22.23.4+, and manifests pin Tauri 2.12.5, Tauri CLI 2.12.4, tauri-plugin-updater 2.14.0, Svelte 5.57.x, SvelteKit 3.1.x, Vite 8, TypeScript 6, adapter-static 4, adapter-cloudflare 8, Astro 7.4.x and @astrojs/svelte 9.1.x, listed in a pin table in the repo README
**And** a CI step fails if any pinned version is changed without updating the pin table

**Given** a push to any branch
**When** the CI pipeline runs
**Then** it runs format, lint, `cargo test --workspace` and the checks above on Linux and Windows runners, and a red check blocks merge to main

### Story 1.2: App skeletons for every surface build in CI

As a developer on Arena 360,
I want a placeholder app for every Arena 360 surface, all built from the shared UI package,
So that each later story adds features to an app that already builds and ships instead of setting one up.

**Covers:** AD-15, AD-41, NFR17; Additional Requirements "STARTER / SCAFFOLD" (app skeletons)

**Acceptance Criteria:**

**Given** the workspace from Story 1.1
**When** the shared UI package is created
**Then** it is a workspace package that every Svelte app depends on, exporting one placeholder component used by each app below

**Given** the Tauri apps
**When** `pnpm build` (or the chosen package manager equivalent) runs on the Windows CI runner
**Then** a Windows Hub Tauri app and a Kiosk Tauri app build with `adapter-static` SvelteKit front ends showing a placeholder screen

**Given** the web surfaces
**When** CI builds them
**Then** SvelteKit skeletons build for Kitchen display (`adapter-static`), Owner PWA, partner portal, admin app and onboarding (`adapter-cloudflare`, Workers static assets), and an Astro 7 marketing site skeleton renders the shared placeholder Svelte component

**Given** any app skeleton fails to build
**When** CI runs
**Then** the pipeline fails naming the app, and merge to main is blocked

### Story 1.3: Generated protocol types and the message catalogue lint

As a developer on Arena 360,
I want protocol types generated from Rust into the shared UI package and every user-visible string held in one catalogue,
So that drift in types or strings between Rust and the Svelte surfaces is caught by CI.

**Covers:** AD-8 (types generated from `arena-proto`), NFR16, UX-DR103; Additional Requirements "STARTER / SCAFFOLD" (type generation, string lint), "CI/CD" (freshness gate)

**Acceptance Criteria:**

**Given** `arena-proto` defines `AppError{code, message, field?}`, an `ErrorCode` enum and an empty permission catalogue
**When** the type generator (ts-rs, chosen and recorded in the README) runs
**Then** TypeScript types are written into the shared UI package's `generated/` folder
**And** CI regenerates them and fails with a diff listing if the committed files are stale, and a lint forbids hand-written copies of protocol types

**Given** a Svelte app from Story 1.2
**When** CI runs the import check
**Then** it fails if the app imports protocol types from anywhere other than the shared UI package's `generated/` folder

**Given** the message catalogue in the shared UI package (English only, keyed strings, seeded with the fixed Voice lines from EXPERIENCE.md)
**When** the string lint runs on a Svelte or TS file containing a user-visible hard-coded string literal
**Then** the lint fails release builds and names the file and line
**And** a catalogue key referenced in code but missing from the catalogue also fails the lint

### Story 1.4: Design tokens generated from DESIGN.md with a contrast gate

As a developer building any Arena 360 surface,
I want every colour, type, spacing, radius and elevation token generated from DESIGN.md into the shared UI package, with contrast checked in CI,
So that all surfaces look the same, the HC and light themes are complete, and no inaccessible colour pair can ship.

**Covers:** NFR15, NFR26 (Sora bundled), UX-DR1, UX-DR2, UX-DR3, UX-DR4, UX-DR5, UX-DR7, UX-DR8, UX-DR9, UX-DR10, UX-DR11, UX-DR12, UX-DR13, UX-DR14, UX-DR114

**Acceptance Criteria:**

**Given** the colour, typography, spacing, radius and elevation tokens in DESIGN.md frontmatter
**When** the token generator runs
**Then** the shared UI package exposes CSS custom properties and TS constants for the Cinema Electric dark set, Hub tones, floor-tile state tokens, chip/HUD toast tokens, six group swatches, the light "Cool paper" set, and an `-hc` twin for every component token, plus the Kiosk, Hub and Web type ramps, the 4-based spacing scale with `touch-min` 44px, and the radius scale
**And** editing DESIGN.md without regenerating fails CI as stale tokens

**Given** the generated tokens
**When** the token-contrast check runs in CI
**Then** every load-bearing pair listed in DESIGN.md meets 4.5:1 for text and 3:1 for non-text/large text in dark, HC and light themes, and the build fails naming any pair below its floor

**Given** the shared UI package
**When** any surface renders text
**Then** Sora is loaded from bundled font files with tabular numerals available (`font-variant-numeric: tabular-nums` utility), and a CI check fails if any branch-surface build (Hub, Kiosk, Kitchen) references a remote font or CDN URL

**Given** a token is renamed or removed in the generated package
**When** a Svelte app still references the old token
**Then** the build fails naming the missing token, and no surface may declare raw hex colours (a lint fails on hex literals outside the generated token files)

### Story 1.5: Shared base components meet the accessibility floor

As a staff member or player using any Arena 360 surface,
I want every shared control to be keyboard-operable, clearly focused, large enough to hit and never colour- or sound-only,
So that I can use the product at the counter without a mouse and with low vision.

**Covers:** FR202, NFR15, NFR16, UX-DR16, UX-DR17, UX-DR18, UX-DR19, UX-DR22, UX-DR23, UX-DR25, UX-DR108, UX-DR109, UX-DR110, UX-DR113

**Acceptance Criteria:**

**Given** the shared UI package
**When** a developer opens the dev-only component gallery route
**Then** it shows `focus-ring`, `text-field` (incl. password variant), `button-primary`, `button-secondary`, `button-warning`, `status-badge`, `alert-badge`, `card` and `tab-control` in dark, HC and light themes, each in rest, hover, pressed, focused, disabled and error states where applicable

**Given** any focusable shared component
**When** it receives keyboard focus
**Then** the `focus-ring` shows as 2px magenta inner + 2px white outer at 2px offset (HC: 4px yellow; light: 2px `#AD0E63`) and is never suppressed by `outline: none`
**And** a Playwright test tabs through the gallery and asserts a visible focus indicator on every stop

**Given** a disabled button
**When** it renders
**Then** it shows at 40% opacity with no hover, a not-allowed cursor, and its reason as inline text at full ink (the reason comes from a catalogue key, never empty)

**Given** a `text-field` with a validation error
**When** the form is submitted or the field blurs
**Then** the field shows a 2px danger border, a ⚠ glyph and the message below, focus moves to the first invalid field, and the error is announced through a live region
**And** the label is programmatically linked, paste is allowed, and the password variant has a show/hide control

**Given** every interactive shared component
**When** the automated target-size check runs
**Then** each hit area is at least 44 × 44 px in every theme, and the check fails the build otherwise

**Given** `status-badge` and `alert-badge`
**When** they render a state
**Then** the state is carried by text (and glyph where specified), never by colour alone, and `alert-badge` count changes are announced in a polite live region

**Given** the HC theme is active
**When** components transition between states
**Then** transitions cut to instant, no shadows or translucency render, and every interactive surface shows a 2px white outline (hover 3px)

**Given** an axe-core scan of the component gallery in all three themes
**When** CI runs it
**Then** there are zero serious or critical violations, and a pseudo-locale run with strings lengthened by 30% shows no clipped or overlapping text in any component

### Story 1.6: Hub service starts as the single writer of its branch database

As a café operator,
I want the Hub service to open its local database safely and know it is the only writer for the branch,
So that branch data can never be corrupted by two writers or lost on a crash.

**Covers:** AD-1, AD-8, AD-38 (offline provisional epoch), NFR9, NFR25 (Hub logs), NFR17

**Acceptance Criteria:**

**Given** a machine with no Hub database
**When** `arena-hub` starts
**Then** it creates the SQLite file in WAL mode with `synchronous=FULL` (fsync on commit), applies the numbered embedded migrations in one transaction, and records each in `schema_version`
**And** it mints and persists a `hub_instance_id` once at first start

**Given** a database whose `schema_version` is newer than the binary knows
**When** `arena-hub` starts
**Then** it refuses to start with `SCHEMA_TOO_NEW` and a log line naming both versions, leaving the file untouched

**Given** no cloud is reachable at first start
**When** the lease task starts with an empty `lease` table
**Then** the Hub mints a provisional epoch `(1, hub_instance_id)` marked `provisional = 1`, takes the lease, and renews it every 3 s with a 15 s expiry

**Given** a `lease` row held by a different instance with a higher epoch that has not expired
**When** a second `arena-hub` process starts on the same database
**Then** it starts in follower role, opens no writer channel, and reports `role: follower` on `/healthz`

**Given** a running primary
**When** its lease is superseded by a higher epoch
**Then** it closes its writer channel within one renewal tick, so any later write attempt fails `NOT_PRIMARY`

**Given** a running Hub
**When** `GET /healthz` is called on the loopback interface
**Then** it returns db open, role, lease state, current epoch and schema version as JSON

**Given** the Hub is running
**When** it logs
**Then** logs are `tracing` JSON lines in a 50 MB on-disk ring buffer retained 14 days, carrying no PII

**Given** the Hub process is killed with `SIGKILL`/`TerminateProcess` immediately after a committed write
**When** it restarts
**Then** the committed write is present and the database passes `PRAGMA integrity_check`

### Story 1.7: Every change goes through apply() into a gap-free change log, starting with a seeded branch

As a café owner,
I want every change at my branch recorded once, in order, with who made it,
So that the branch has a trustworthy audit trail and can later be replicated and replayed exactly.

**Covers:** AD-2, AD-3 (`branch.bootstrap`), AD-7, NFR11 (audit on every write), NFR25 (typed actor), FR110 (recording half)

**Acceptance Criteria:**

**Given** the Hub is primary
**When** any intent is submitted
**Then** a single Writer actor processes it in one `BEGIN IMMEDIATE` transaction in the order permission → epoch check → TTL/dedupe → `base_rev` → validate → mutate → stamp → append `changes` → commit → emit, and no I/O other than SQLite happens inside the transaction
**And** a failure at any step rolls back and returns exactly one typed `AppError`
**And** in this story the TTL/dedupe step is a pass-through stub with its place fixed in the order; Story 1.8 fills it

**Given** a committed intent
**When** the `changes` table is inspected
**Then** each written row has one change row with `seq` (gap-free per epoch), `epoch`, `tbl`, `row_id`, `op`, the full row after write as a MessagePack map, `intent_id`, `intent_kind`, typed `actor_kind`/`actor_id` and `at`, and the table rejects UPDATE and DELETE (append-only)

**Given** the dev fixture `arena-hub dev seed` in a build with the `dev-fixtures` feature
**When** it runs against an empty database
**Then** it submits a `branch.bootstrap` intent as `actor{kind: system}` that creates the `branch` row (name, `branch_code`, ISO 4217 currency, tz, `day_start_min`, FY start) and the `settings` table defaults, visible as change rows
**And** running it a second time is rejected as already bootstrapped, and a release build has no `dev seed` command

**Given** an intent on a Hub in follower role or with a superseded epoch
**When** it reaches `apply()`
**Then** it fails `NOT_PRIMARY` and writes nothing

**Given** the change log of the seeded branch
**When** a test replays `changes` into an empty database
**Then** the resulting rows equal the source database's branch rows, and `seq` has no gaps for the epoch

### Story 1.8: Idempotent intents, optimistic row versions and property tests on apply()

As a café owner,
I want retried or late actions to apply exactly once and never overwrite a newer change,
So that flaky connections and double taps can never double-charge or silently undo someone's edit.

**Covers:** AD-14, AD-39, AD-2, NFR11, NFR25

**Acceptance Criteria:**

**Given** an intent whose `idempotency_key` has already been applied
**When** it is submitted again
**Then** it is not re-applied, the original outcome is returned, and no new change rows appear; dedupe records in `applied_intents` are retained 30 days

**Given** an intent older than its TTL (≤ 7 days)
**When** it is submitted
**Then** it is rejected `EXPIRED`

**Given** an edit intent carrying a `base_rev` that differs from the row's current `rev`
**When** it is applied
**Then** it is rejected `STALE` and nothing changes; a successful edit increments `rev` by exactly 1

**Given** an intent with no `idempotency_key` or a malformed one
**When** it is submitted
**Then** it is rejected with `BAD_FRAME` before any transaction opens

**Given** dedupe records older than 30 days
**When** the daily housekeeping runs
**Then** they are pruned, and no record younger than 30 days is removed

**Given** the `proptest` suite generating random sequences of the intents defined so far against an in-memory Hub
**When** it runs in CI on every push
**Then** after every step `changes.seq` is gap-free, duplicate idempotency keys apply once, stale `base_rev` is rejected, and replaying `changes` into an empty database reproduces the same rows

### Story 1.9: LAN protocol envelope, hello handshake and client classes

As a café operator,
I want every device on the café LAN to talk to the Hub through one authenticated protocol,
So that guests on the shared Wi-Fi cannot reach staff, money or device actions.

**Covers:** AD-6, AD-8 (frame `v`, two-release window), AD-25, AD-42, NFR11, NFR17

**Acceptance Criteria:**

**Given** the Hub is running
**When** it opens its LAN listeners
**Then** it serves WebSocket on `:7400` over TLS with a Hub certificate generated at first start (fingerprint stored) and HTTP(S) on `:7401`, and the Windows installer opens those ports only on the Private network profile; the Hub opens no other inbound port

**Given** a connection to `:7400`
**When** any frame is exchanged
**Then** every frame is `{v, kind, rid}` and replies correlate on `rid`; an unknown `kind` returns `UNSUPPORTED`, unknown fields are ignored, and a malformed frame returns `BAD_FRAME` without closing other sessions

**Given** a new connection
**When** the first frame is not `hello{credential, class, versions, capabilities}`
**Then** the Hub rejects it and closes the connection

**Given** a dev-fixture-seeded kiosk device token (stored as an argon2id hash in the `devices` table created by this story)
**When** a client sends `hello` with that token and `class: paired`
**Then** the session is accepted, the device's reported version is recorded, and the Hub pushes subsequent state as events (no client polling)
**And** a wrong or unknown token returns `TOKEN_INVALID` and closes the connection

**Given** a paired device whose `hello.versions` is older than the previous two releases
**When** it connects
**Then** the Hub rejects it with `UNSUPPORTED` naming the minimum version; versions within the window are accepted

**Given** a connected paired device
**When** a `device.revoke` intent (added by this story, permission `device.remote`; in Epic 1 submitted by the dev-fixture command `arena-hub dev revoke-device`) applies for it
**Then** the token is marked revoked in `devices`, its socket is closed immediately with `TOKEN_REVOKED`, and reconnects with that token are refused

**Given** the class scope table in `arena-proto` (paired, kds, phone)
**When** a `kds` or `phone` class client calls an intent outside its scope
**Then** the Hub returns `FORBIDDEN` and logs the attempt; a CI test asserts no browser-class scope includes any staff, money or device intent

**Given** the shared Rust client in `arena-kiosk-core`
**When** a request gets no reply within 5 s
**Then** it surfaces a `HUB_SLOW` status to its caller without blocking other requests

### Story 1.10: Hub app shell with left rail, header and floor placeholder on the seeded branch

As counter staff,
I want the Hub app to open straight into a familiar frame with navigation, the branch header and the floor area,
So that I can find every area of the Hub by mouse, touch or keyboard from the first day.

**Covers:** NFR2, NFR17, UX-DR58, UX-DR59, UX-DR60 (shell), UX-DR87 (shell), UX-DR104, UX-DR105 (navigation keys), UX-DR113

**Acceptance Criteria:**

**Given** the Windows Hub installer built by CI
**When** it is run on Windows 10 or 11
**Then** it installs the Tauri Hub app, which starts the `arena-hub` service and opens the Hub UI; on an unsupported Windows version the installer stops with a message naming the minimum version

**Given** a seeded branch
**When** the Hub UI opens on a window ≥ 840px wide
**Then** it shows the 88px `hub-rail` with labelled items Floor view · POS · Players · Sessions · Inventory · Shift · Reports · Settings · System, the 72px `hub-header` with branch name and clock, and Floor view selected as home
**And** the floor area renders from the local database immediately with no skeleton, showing an empty-state zone message from the catalogue and an "Unassigned" section listing the dev-seeded kiosk by name

**Given** a rail item whose screen is not built yet
**When** staff select it
**Then** a placeholder view with a single `h1` from the catalogue opens; no rail item is hidden or broken

**Given** the Hub window is narrower than 840px
**When** the layout reflows
**Then** the rail is replaced by the 64px `hub-bottom-bar` (Floor · POS · Players · Shift · More), More opens a bottom sheet listing Sessions, Inventory, Reports, Settings, System, and closing it returns to the current screen; all targets stay ≥ 44px

**Given** keyboard focus is not in a text field
**When** staff press Ctrl+1, Ctrl+2, Ctrl+3 or Ctrl+4
**Then** Floor, POS, Players or Shift opens respectively
**And** the same keys typed while a text field has focus do nothing

**Given** the Hub UI
**When** an axe scan runs on the shell
**Then** it has `header`, `nav` and `main` landmarks, one `h1` per view and zero serious/critical violations

**Given** the per-release benchmark job on the reference hardware runner (2015 i3 + HDD)
**When** the Hub has been idle for 5 minutes on the seeded branch
**Then** the Rust Hub service uses < 50 MB RAM and the Hub UI (WebView2) < 250 MB with no GPU use, and the job fails the release otherwise

### Story 1.11: Staff sign in to the Hub with roles and permissions

As a café owner,
I want each staff member to sign in as themselves and only do what their role allows,
So that every action is attributable and counter staff cannot change things reserved for managers.

**Covers:** FR103, AD-10 (staff password + PIN, Argon2id), AD-30, NFR11 (RBAC inside `apply()`), NFR25, UX-DR18, UX-DR60 (signed-in staff)

**Acceptance Criteria:**

**Given** `arena-proto`
**When** the permission catalogue is generated
**Then** it holds the fixed ~40 permissions grouped by module, each intent kind maps to exactly one permission, and a CI test fails if any intent kind has zero or more than one permission

**Given** the migration added by this story (`staff`, `roles`, `role_perms`)
**When** the branch is seeded
**Then** built-in roles Owner, Manager, Counter, Kitchen and Kiosk-device exist with the LLD permission sets and are marked `builtin`
**And** the dev fixture seeds one staff member per staff role with Argon2id-hashed password and PIN (dev only; real setup-code onboarding is Epic 2)

**Given** a `role.upsert` intent for a custom role
**When** it lists permissions each marked allow or deny
**Then** a role is created only from catalogue permissions; an unknown permission is rejected with a field error, and any attempt to modify a built-in role is rejected

**Given** the Hub sign-in screen
**When** a staff member enters a correct login and password
**Then** they are signed in, their name and role show in the `hub-header`, and Floor view opens

**Given** a wrong login or password
**When** sign-in is submitted
**Then** the error does not say which field failed, nothing about the account is revealed, and after 5 consecutive failures sign-in for that login is paused for 60 s with a visible countdown

**Given** a signed-in Counter
**When** they submit an intent requiring a permission their role does not hold
**Then** `apply()` rejects it with `PERMISSION_DENIED` before any write, and the UI shows the plain refusal inline (no blocking modal)

**Given** a signed-in staff member
**When** they choose Sign out
**Then** the Hub returns to the sign-in screen and subsequent intents from that UI are rejected until someone signs in

**Given** any intent submitted from the Hub UI
**When** it is applied
**Then** its change rows carry `actor{kind: staff, id}` of the signed-in staff member, never a shared or anonymous actor

### Story 1.12: Branch locale settings — currency, number format and timezone

As a branch manager,
I want to set the branch currency, symbol position, number format and timezone,
So that every amount and time on the Hub reads the way my café expects, and day boundaries stay correct.

**Covers:** FR111, AD-7, NFR16 (locale formatting, currency per branch), UX-DR18, UX-DR88 (Settings)

**Acceptance Criteria:**

**Given** a Manager or Owner on Settings → Branch
**When** they open the screen
**Then** it shows currency (ISO 4217 code), symbol position (before/after), number format (e.g. Indian 1,23,456.78 or international 123,456.78), timezone (IANA) and day start, each with its current value

**Given** a valid change
**When** they save
**Then** a `settings.set` intent is applied with the setting key and value, the change appears in the change log, and a `settings.changed` event updates every open Hub view without a reload

**Given** an invalid currency code or an unknown timezone
**When** they save
**Then** the field shows an inline error from the catalogue and nothing is written

**Given** a currency is set
**When** any amount is displayed on the Hub
**Then** it is formatted from integer minor units using the code's exponent, the chosen symbol position and number format, with tabular numerals, via one shared formatter in the UI package

**Given** a timezone or day-start change saved during business day D
**When** `apply()` stamps `business_day` on later rows
**Then** rows in day D still use the old tz/day start and the new values take effect from business day D+1, and the settings screen shows "takes effect from <date>"

**Given** two Hub windows editing the same setting
**When** the second saves with an outdated `base_rev`
**Then** it is rejected `STALE` and the screen shows the current value with a plain message to retry

**Given** a Counter
**When** they open Settings → Branch
**Then** the fields are read-only and the save action states why it is unavailable

### Story 1.13: Inline manager PIN elevation

As counter staff,
I want a manager to approve an action by typing their PIN right where I am,
So that I can finish the job without signing out or leaving the screen, and the audit shows who approved it.

**Covers:** FR105, AD-30, UX-DR66, UX-DR101 (role lacks permission), UX-DR104

**Acceptance Criteria:**

**Given** a signed-in Counter on Settings → Branch
**When** they activate Save on a setting their role lacks permission for
**Then** the action stays visible and an `inline-pin` (4–6 cells, `surface-2` cells with `border-strong` edge, numeric keypad + keyboard) opens in place in that panel without navigating away

**Given** the inline PIN is open
**When** a Manager's correct PIN is entered
**Then** the Hub verifies it with Argon2id, applies the intent with permission checked against the manager, and the change row records `actor` = the Counter and `elevated_by{kind: pin, id: manager, mode: hub, verifier_version}`

**Given** an incorrect PIN, or the PIN of a staff member whose role also lacks the permission
**When** it is submitted
**Then** nothing is applied, the cells clear, and a plain inline message says the PIN can't approve this action without naming anyone

**Given** an intent that requires a reason in addition to elevation
**When** the PIN is accepted
**Then** the reason picker appears in the same place and the intent is applied only once a reason is chosen

**Given** a PIN was entered
**When** the intent completes or is cancelled with Esc
**Then** the PIN is cleared from memory and the UI, is never sent to any other screen or device, and focus returns to the control that opened it

**Given** a manager's PIN has been rotated
**When** an elevation carries the old `verifier_version`
**Then** it is rejected and the user is asked to enter the PIN again

**Given** the full flow
**When** it is driven by keyboard only
**Then** opening, entering the PIN, choosing a reason and submitting all work without a mouse

### Story 1.14: Hub idle lock with PIN unlock in place

As a café owner,
I want an unattended Hub to lock itself and unlock with a PIN back to the same screen,
So that nobody can use a signed-in counter while staff step away, and staff don't lose their place.

**Covers:** FR106, AD-30 (5-min idle lock), UX-DR102 (idle lock), UX-DR66

**Acceptance Criteria:**

**Given** a signed-in staff member and the default idle timeout of 5 minutes
**When** there is no keyboard, mouse or touch input for 5 minutes
**Then** the Hub UI shows a lock screen with the signed-in staff name and an `inline-pin`, and nothing behind it can be operated

**Given** a Manager changes the idle timeout in Settings (allowed range shown inline)
**When** the setting is saved
**Then** the new timeout applies to every Hub UI from the next idle period, and an out-of-range value is rejected with a field error

**Given** the lock screen
**When** the same staff member enters their correct PIN
**Then** the Hub returns to exactly the screen, scroll position and unsaved field contents shown before the lock

**Given** the lock screen
**When** a different staff member enters their own correct PIN
**Then** the Hub stays on the same screen, the header shows the new staff member, and all later intents carry the new actor

**Given** the lock screen
**When** a wrong PIN is entered 5 times in a row
**Then** the PIN entry pauses for 60 s with a visible countdown, and Sign out remains available

**Given** the Hub UI is locked
**When** LAN events arrive (e.g. device state)
**Then** the Hub service keeps processing them and the UI shows the current state after unlock; locking never stops the service

**Given** the lock and unlock events
**When** they occur
**Then** they are written to the Hub log with staff id and time (no PIN material), and unlock is done entirely on the keyboard without a mouse

### Story 1.15: Audit log viewer on the Hub

As a café owner or manager,
I want to browse every change at the branch with who made it, when, and what it was before and after,
So that I can answer "who changed this?" at the branch without the internet.

**Covers:** FR110, AD-2 (log = audit), AD-30 (both actors), NFR11, NFR25, UX-DR23, UX-DR88 (System → Audit)

**Acceptance Criteria:**

**Given** a signed-in Manager or Owner (permission `audit.view`)
**When** they open System → Audit
**Then** the newest changes are listed first with time (branch timezone), actor name and kind, module, intent and the affected record

**Given** a change row
**When** staff open it
**Then** the before and after values are shown side by side, where "before" is the row from the previous change to the same record (or "Created" if none), and only changed fields are highlighted with a text marker, not colour alone

**Given** an elevated change
**When** it is displayed
**Then** both the acting staff member and the approving manager (`elevated_by`) are shown, with the elevation mode

**Given** filters for actor, module and date range
**When** staff combine them
**Then** only matching changes are listed, filters can be cleared in one action, and no match shows the catalogue empty-state line

**Given** a branch with 100,000 change rows
**When** a filtered query runs on the reference hardware
**Then** the first page renders in under 1 s and further pages load on scroll or "Load more"

**Given** a Counter without `audit.view`
**When** they select System → Audit
**Then** the action stays visible and opens `inline-pin` for manager elevation (Story 1.13); without elevation no audit data is shown

**Given** the audit viewer
**When** it is used
**Then** it is read-only (no edit or delete controls exist), it works with no WAN, and personal data columns are shown only as ids

### Story 1.16: Kiosk privilege split — locked service and unprivileged UI

As a café operator,
I want the kiosk's enforcement to run as a protected Windows service separate from the player-facing app,
So that a player who crashes or kills the app cannot unlock the PC, and the device token stays protected.

**Covers:** AD-22 (DPAPI token), AD-31, AD-25 (kiosk client), NFR2 (Kiosk < 80 MB), NFR11, NFR17

**Acceptance Criteria:**

**Given** the Kiosk installer on Windows 10 1803+ or 11, 64-bit, with ≥ 4 GB RAM
**When** it runs
**Then** it installs `arena-kiosk-svc` as a LocalSystem Windows service and the `arena-kiosk` Tauri app, bundles the WebView2 Evergreen bootstrapper if the runtime is missing, and refuses to install on unsupported Windows versions, 32-bit or < 4 GB RAM with a message naming the requirement

**Given** the dev fixture kiosk token and a stored Hub IP (dev only; pairing and mDNS are Epic 2)
**When** the service starts
**Then** it stores the token encrypted with DPAPI, connects to the Hub over WebSocket TLS pinned to the Hub certificate fingerprint, sends `hello{class: paired}`, and the Hub shows the kiosk as online

**Given** the Hub certificate presented does not match the pinned fingerprint
**When** the service connects
**Then** it refuses the connection and logs the mismatch; it never falls back to an unpinned connection

**Given** the Hub becomes unreachable
**When** the service reconnects
**Then** it retries with jittered backoff between 2 and 30 s and the app shows the "Reconnecting…" catalogue line without blocking

**Given** the Tauri app runs as the standard `player` user
**When** it needs Hub data or enforcement actions
**Then** it talks only to the service over a named pipe whose ACL admits only the kiosk app's user, and the app has no direct Hub socket, admin rights or access to the token

**Given** the app is killed or hangs
**When** the service misses 3 consecutive 2 s pipe pings
**Then** the service restarts the app with its last state, and the kiosk screen stays covered (no desktop exposed) throughout

**Given** a kiosk idle with no session on reference hardware
**When** the per-release benchmark measures it
**Then** service + app together use < 80 MB RAM with no GPU use, and the job fails the release otherwise

**Given** kiosk-originated log events
**When** the Hub is unreachable
**Then** they are queued as intents with client idempotency keys and replayed in creation order on reconnect, applying once

### Story 1.17: Kiosk shell replacement and Windows lockdown

As a café operator,
I want the kiosk to replace the Windows shell for the player account and block escape routes,
So that a player cannot reach the desktop, Task Manager or settings on a café PC.

**Covers:** FR116, FR117 (USB autorun), NFR12, NFR17

**Acceptance Criteria:**

**Given** the Kiosk installer
**When** it completes
**Then** a standard `player` account exists whose per-user `Winlogon\Shell` is `arena-kiosk`, so Explorer never starts for that account
**And** policy keys `DisableTaskMgr`, `NoRun`, `NoClose`, `DisableLockWorkstation`, `DisableChangePassword` are set, removable drives are hidden and USB autorun is off for that account

**Given** the `player` account is signed in
**When** the player presses Win-key combinations or Alt+F4 on kiosk windows
**Then** the low-level keyboard hook swallows them and nothing outside the kiosk opens

**Given** the player presses Ctrl+Alt+Del
**When** the secure screen appears
**Then** only the options left by policy (no Task Manager, lock, change password or sign-out to desktop) are offered, and returning lands back on the kiosk

**Given** no session is active
**When** the kiosk is running
**Then** the main window is `WS_EX_TOPMOST` and covers every monitor, showing a locked placeholder screen from the catalogue

**Given** a USB drive with an autorun payload is inserted
**When** Windows detects it
**Then** nothing runs and the drive is not visible to the player account

**Given** an administrator account on the same PC
**When** it signs in
**Then** it gets the normal Windows shell; the lockdown applies only to the `player` account

**Given** the Kiosk is uninstalled by an administrator
**When** the uninstaller completes
**Then** the `player` account's shell and policy keys are restored to Windows defaults

### Story 1.18: Kiosk process allow/block list

As a café operator,
I want the kiosk to kill any program that isn't allowed, tell the player, and log it at the counter,
So that players can't run tools or apps that break the café's rules or the lockdown.

**Covers:** FR117, AD-31, AD-14, NFR12, UX-DR101 (blocked app), UX-DR103

**Acceptance Criteria:**

**Given** the branch allow-list and block-list stored at the Hub as the settings keys `kiosk.process_allow` and `kiosk.process_block` (seeded by this story's migration with defaults: kiosk, its service, Windows essentials; block-list includes cmd, PowerShell, regedit, Task Manager) and changed through `settings.set`
**When** the kiosk service connects
**Then** it receives the lists and caches them for offline use, and any later list change is pushed by `settings.changed` and applied without restart

**Given** a process starts in the `player` session
**When** it is on the block-list, or not on the allow-list
**Then** the service terminates it within 200 ms of the process-creation event, matching allowed programs by signed path

**Given** a process was killed
**When** the kill completes
**Then** the kiosk shows "That app isn't allowed on this PC." as a non-focus-stealing toast that persists until dismissed, and the service sends a `kiosk.process_blocked` intent (device actor, executable name and path) that appears in the Hub audit log

**Given** the Hub is unreachable
**When** a blocked process is killed
**Then** enforcement uses the cached lists, the event is queued with its idempotency key, and it reaches the Hub audit exactly once after reconnect

**Given** a renamed copy of a blocked executable or an allowed name at an unsigned path
**When** it is launched
**Then** it is killed, because matching uses signed path and not file name alone

**Given** the app or service is restarted
**When** enforcement resumes
**Then** processes already running that are not allowed are terminated within 2 s of service start

### Story 1.19: Automated 30-minute kiosk escape suite on Windows 10 and 11

As a café operator,
I want every kiosk release proven against scripted escape attempts on both supported Windows versions,
So that a lockdown regression is caught before it reaches a café PC.

**Covers:** FR116, NFR12, NFR17

**Acceptance Criteria:**

**Given** Windows 10 (1803 or later) and Windows 11 VM images with the release-candidate Kiosk installed
**When** the escape suite runs in CI before release
**Then** it drives, for at least 30 minutes per OS, key combos (Win-key, Alt+Tab, Alt+F4, Ctrl+Shift+Esc, Ctrl+Alt+Del options), Task Manager, Run, Settings, USB insertion, process spawn via file dialogs, Sticky Keys and other accessibility shortcuts, and killing the kiosk app

**Given** the suite is running
**When** any attempt reaches the desktop, Explorer, a command shell, Task Manager or a non-allowed process window
**Then** the suite records the step, a screenshot and the OS, and fails the release

**Given** a passing run
**When** the suite finishes
**Then** a run report (OS build, kiosk version, attempts, duration, result) is stored as a release artefact

**Given** a new escape technique found in manual testing
**When** it is added as a scripted case
**Then** it runs on both OS images in every later run

**Given** the VM images
**When** the suite starts
**Then** each run starts from a clean snapshot, so one run's state cannot hide another's failure

### Story 1.20: Kiosk admin exit with manager PIN, offline verifier and Hub remote unlock

As a branch manager,
I want to drop a kiosk to the desktop for maintenance with my PIN at the PC, even when the Hub is down, or remotely from the Hub,
So that I can fix a PC without reinstalling, and every exit is logged.

**Covers:** FR118, AD-30 (cached verifier), AD-31, UX-DR66, UX-DR106 (admin exit combo)

**Acceptance Criteria:**

**Given** a kiosk with the Hub reachable
**When** someone presses Ctrl+Alt+Shift+A
**Then** an `inline-pin` opens in a kiosk panel (no other visible hint of the combo exists), and a correct manager PIN verified by the Hub starts Explorer for 15 minutes with a countdown badge, after which the kiosk re-locks

**Given** the Hub is reachable
**When** the Hub starts, or a kiosk connects
**Then** the Hub pushes the current versioned manager-PIN verifier (built from the dev-fixture manager PINs in Epic 1) to every connected kiosk, and the service stores it protected by DPAPI

**Given** the Hub is unreachable
**When** a manager enters a correct PIN after the combo
**Then** the service verifies it against the cached verifier, grants the same 15-minute exit, and queues an exit intent with `elevated_by.mode = cached_verifier` that is applied and flagged in the audit after reconnect

**Given** a cached verifier whose version is older than the Hub's current one
**When** the kiosk reconnects
**Then** the old verifier is replaced, and an offline exit made with a rotated-out PIN is accepted only if its verifier version was current at the time of exit, otherwise it is flagged in the audit for review

**Given** a wrong PIN, or a PIN of staff without the admin-exit permission
**When** it is entered
**Then** nothing exits, the cells clear, and the failed attempt is logged; after 5 consecutive failures the prompt pauses for 60 s

**Given** a Manager on the Hub floor placeholder (Story 1.10) selects the dev-seeded kiosk's entry
**When** they choose Remote unlock and pass the `inline-pin`
**Then** the Hub applies a `device.admin_unlock` intent and the kiosk drops to the desktop for 15 minutes within 5 s; if the kiosk does not confirm within 5 s the entry shows "Failed — retry"

**Given** any admin exit
**When** the audit log is viewed
**Then** it shows the kiosk, the approving manager, the mode (Hub, cached verifier or remote), and start and re-lock times

### Story 1.21: High-contrast theme as a per-device branch setting

As a branch manager,
I want to switch the high-contrast theme on for a specific Hub or kiosk,
So that staff or players with low vision can use that screen without changing anyone else's.

**Covers:** FR202 (HC per device), FR111 (settings path), NFR15, UX-DR4, UX-DR6, UX-DR108, UX-DR114

**Acceptance Criteria:**

**Given** a Manager or Owner on Settings → Devices & display
**When** they view the list
**Then** the Hub itself and each known kiosk appear with a high-contrast on/off control, labelled by device name

**Given** HC is switched on for a kiosk
**When** the setting is saved
**Then** a `settings.set` intent records it per device, and that kiosk switches to `-hc` tokens within 2 s without a restart, while other devices are unchanged

**Given** HC is active on a surface
**When** it renders
**Then** surfaces are pure black with white text and hairlines, the accent is `#FFE14D`, chips are inverted, every interactive surface has a 2px white outline, the focus ring is 4px yellow, art/video/translucency/shadows are removed and motion cuts to instant

**Given** a kiosk that last received HC = on
**When** it cold-starts with the Hub unreachable
**Then** it applies HC from its cached setting before the first paint

**Given** a player at a kiosk
**When** they look for a theme control
**Then** none exists on any player screen; Hub, Kiosk and Kitchen remain dark-only plus HC with no light theme or `theme-toggle`

**Given** a Counter without settings permission
**When** they change the HC control
**Then** the `inline-pin` opens in place for manager elevation, and the change is audited with both actors

**Given** the HC theme on the Hub shell and kiosk placeholder screens
**When** the contrast and axe checks run in CI
**Then** all load-bearing pairs meet 4.5:1 text and 3:1 non-text, with zero serious/critical axe violations

### Story 1.22: The branch runs with the WAN down and shows which link is offline

As counter staff,
I want the Hub and kiosks to keep working exactly the same when the internet drops, with a clear chip telling me what's down,
So that the café never stops trading because of a broken connection.

**Covers:** FR166, NFR1 (offline reliability, Epic 1 scope), NFR26 (no runtime font/CDN fetch), UX-DR76 (`chip-offline`), UX-DR98 (Hub offline state), UX-DR113 (live region)

**Acceptance Criteria:**

**Given** the Hub service is running
**When** its connectivity probe runs every 30 s
**Then** it distinguishes "internet unreachable" (no general egress) from "cloud unreachable" (egress works but the Arena 360 API does not), with neither probe blocking any intent

**Given** the internet or the cloud becomes unreachable
**When** the next probe fails
**Then** the `hub-header` shows `chip-offline` naming which one ("Internet offline" or "Cloud unreachable"), announced once through a polite live region, and nothing else in the UI changes

**Given** connectivity returns
**When** the next probe succeeds
**Then** the chip disappears within 30 s with no restart or user action

**Given** an integration test with all WAN egress blocked at the network layer from first boot
**When** it runs the Epic 1 flows — Hub install and start, dev seed, staff sign-in, branch settings, inline PIN elevation, idle lock and unlock, audit viewer, kiosk start and Hub connection, process blocking, admin exit (Hub and cached verifier), HC toggle
**Then** every flow succeeds with results identical to a WAN-up run, and the test fails if any process attempted an outbound connection other than the connectivity probe

**Given** the Hub, Kiosk and Kitchen front ends
**When** they load with the WAN down
**Then** all fonts, icons, tokens and scripts come from bundled assets, and no screen shows a broken or loading state caused by a remote resource

**Given** a WAN outage longer than 8 hours on the test LAN with the Hub and a kiosk running Epic 1 flows
**When** the WAN returns
**Then** no change rows are lost or duplicated (seq gap-free), and the Hub log shows no error caused by the outage

