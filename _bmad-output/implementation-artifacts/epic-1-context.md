# Epic 1 Context: Foundation — Hub and Kiosk install, start, and staff log in

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Stand up the monorepo, CI gates, shared UI package and core runtime so that, on a dev-seeded branch, the Windows Hub and a locked-down Windows Kiosk install, start and talk over an authenticated LAN protocol. Staff sign in with roles, manager PIN elevation and idle lock, and every flow works with the WAN down. Every later epic builds on these invariants: single writer, apply(), change log, protocol envelope, tokens and accessibility. A "seeded branch" here means the dev-only `arena-hub dev seed` fixture (feature `dev-fixtures`, absent from release builds). Real pairing, mDNS and staff setup codes come in Epic 2.

## Stories

- Story 1.1: Monorepo workspace with pinned toolchains and CI gates
- Story 1.2: App skeletons for every surface build in CI
- Story 1.3: Generated protocol types and the message catalogue lint
- Story 1.4: Design tokens generated from DESIGN.md with a contrast gate
- Story 1.5: Shared base components meet the accessibility floor
- Story 1.6: Hub service starts as the single writer of its branch database
- Story 1.7: Every change goes through apply() into a gap-free change log, starting with a seeded branch
- Story 1.8: Idempotent intents, optimistic row versions and property tests on apply()
- Story 1.9: LAN protocol envelope, hello handshake and client classes
- Story 1.10: Hub app shell with left rail, header and floor placeholder on the seeded branch
- Story 1.11: Staff sign in to the Hub with roles and permissions
- Story 1.12: Branch locale settings — currency, number format and timezone
- Story 1.13: Inline manager PIN elevation
- Story 1.14: Hub idle lock with PIN unlock in place
- Story 1.15: Audit log viewer on the Hub
- Story 1.16: Kiosk privilege split — locked service and unprivileged UI
- Story 1.17: Kiosk shell replacement and Windows lockdown
- Story 1.18: Kiosk process allow/block list
- Story 1.19: Automated 30-minute kiosk escape suite on Windows 10 and 11
- Story 1.20: Kiosk admin exit with manager PIN, offline verifier and Hub remote unlock
- Story 1.21: High-contrast theme as a per-device branch setting
- Story 1.22: The branch runs with the WAN down and shows which link is offline

## Requirements & Constraints

- The branch must work fully offline. The only permitted outbound traffic is the connectivity probe. Branch surfaces (Hub, Kiosk, Kitchen) never fetch fonts, CDN assets or scripts at runtime.
- Every write is permission-checked inside `apply()` and audited with a typed actor. Nothing can be written anonymously or through a shared actor.
- Performance budgets are checked by a release benchmark on reference hardware (2015 i3 + HDD). Hub service < 50 MB RAM and Hub UI < 250 MB at idle. Kiosk service + app < 80 MB. No GPU use.
- Accessibility floor: WCAG contrast of 4.5:1 for text and 3:1 for non-text, in dark, HC and light themes. Hit targets are at least 44px. Everything is keyboard-operable. State is never shown by colour alone. Zero serious or critical axe violations.
- All user-visible text comes from one message catalogue (English at launch). Money, number and date formatting follows the branch locale.
- Supported platforms: Windows 10 1803+ and 11, 64-bit, with at least 4 GB RAM (kiosk). Installers refuse anything else and name the requirement.

## Technical Decisions

**Crate layout and layer rules (CI-enforced):**
- `arena-proto` has no workspace deps. It holds types, `AppError{code,message,field?}`, error codes, the permission catalogue and class scopes.
- `arena-store` holds all SQL (rusqlite).
- `arena-core` holds domain and `apply()`: pure, with an injected `Clock`. It depends on store and proto only, never on api, tasks or cloud.
- `arena-api` (WS/HTTP edges) depends on core.
- `arena-tasks` (lease and other background tasks) depends on core and store.
- The other crates are `arena-replay`, `arena-cloud`, `arena-hub` (bin), `arena-kiosk-core` (shared Rust Hub client), `arena-kiosk-svc` and `arena-pi`.
- UI depends only on types generated from `arena-proto` via ts-rs into the shared UI package's `generated/` folder. Hand-written protocol types are forbidden, and CI fails on stale generated files.

**Pinned versions:**
- Toolchains: Rust stable via `rust-toolchain.toml`; Node 22.23.4+; one package manager pinned via `packageManager`.
- Tauri 2.12.5, Tauri CLI 2.12.4, tauri-plugin-updater 2.14.0.
- Svelte 5.57.x, SvelteKit 3.1.x, Vite 8, TypeScript 6, adapter-static 4, adapter-cloudflare 8, Astro 7.4.x with @astrojs/svelte 9.1.x.
- Rust crates: tokio 1.54, axum 0.8, rusqlite 0.40, tokio-tungstenite 0.30, rmp-serde 1.3, argon2 0.6, windows-service 0.8.
- The README pin table must match the manifests (CI gate). CI runs on Linux and Windows.

**App targets:**
- Tauri apps (Hub, Kiosk) and Kitchen use adapter-static.
- Owner PWA, partner portal, admin and onboarding use adapter-cloudflare (Workers static assets).
- The marketing site is Astro.

**Storage and single writer:**
- SQLite runs in WAL mode with `synchronous=FULL`.
- Migrations are embedded, numbered, forward-only and additive, recorded in `schema_version`. If the database schema is newer than the binary, startup fails `SCHEMA_TOO_NEW` and leaves the file untouched.
- `hub_instance_id` is minted once at first start.
- Lease: the epoch is `(counter, hub_instance_id)`. When offline at first start, the Hub mints a provisional `(1, id)`. The lease renews every 3 s and expires after 15 s.
- A Hub that finds a fresh higher-epoch lease starts as follower. A demoted Hub closes its writer channel within one renewal tick; any later write fails `NOT_PRIMARY`.
- `/healthz` (loopback only) reports db open, role, lease, epoch and schema version.
- Logs are `tracing` JSON in a 50 MB ring buffer kept 14 days, with no PII.

**apply() and the change log:**
- A single Writer actor runs each intent in one `BEGIN IMMEDIATE` transaction, in this order: permission → epoch → TTL/dedupe → `base_rev` → validate → mutate → stamp → append `changes` → commit → emit.
- No I/O other than SQLite happens inside the transaction. Any failure rolls back and returns exactly one `AppError`.
- `changes` is append-only (UPDATE and DELETE are rejected) and is the audit log. Each row holds:
  - `seq` (gap-free per epoch) and `epoch`
  - `tbl`, `row_id`, `op`
  - the full row after the write, as a MessagePack map
  - `intent_id`, `intent_kind`, `at`
  - `actor{kind: staff|owner|admin|system|device, id}` and `elevated_by{kind, id, mode: hub|cached_verifier, verifier_version}`
- Intents are named `domain.verb`, and each intent maps to exactly one permission.
- Each intent carries a client ULID `idempotency_key`. A missing or malformed key fails `BAD_FRAME` before any transaction opens. Dedupe records live in `applied_intents` for 30 days.
- TTL is at most 7 days, after which the intent fails `EXPIRED`. Edits carry `base_rev`; a mismatch fails `STALE`, and a successful edit bumps `rev` by exactly 1.
- Replaying `changes` into an empty database must reproduce the same rows. proptest checks this in CI.

**Data conventions:**
- ULIDs are minted by the writer, and every branch row carries `branch_id`.
- Money is stored as integer minor units, using the ISO 4217 exponent of the branch currency.
- Timestamps are UTC ms.
- `apply()` stamps `business_day` and `fy`. A timezone or day-start change takes effect from the next business day.
- Enums are stored as TEXT with a CHECK constraint; booleans are 0/1.

**LAN protocol:**
- WebSocket on `:7400` over TLS, using a Hub certificate generated at first start (its fingerprint is pinned by clients). `:7401` serves TLS for paired apps and plain HTTP by path for browser classes. No other inbound ports; the Windows firewall opens these on the Private profile only.
- Every frame is `{v, kind, rid}`.
- Errors:
  - an unknown kind returns `UNSUPPORTED`
  - unknown fields are ignored
  - a malformed frame returns `BAD_FRAME`, and other sessions stay open
- The first frame must be `hello{credential, class, versions, capabilities}`. Device tokens are stored as argon2id hashes in `devices`.
- The Hub accepts kiosks from the previous two releases.
- Revocation closes the socket with `TOKEN_REVOKED`.
- Client classes are paired, kds and phone. A request outside a class's scope fails `FORBIDDEN`. No browser class may reach staff, money or device intents.
- The Hub pushes state; clients never poll. Clients time out at 5 s and show `HUB_SLOW`.

**Identity and RBAC:**
- Staff authenticate with password + PIN, both Argon2id.
- Built-in roles are Owner, Manager, Counter, Kitchen and Kiosk-device, and cannot be edited. Custom roles may only use catalogue permissions (about 40).
- PIN elevation is verified at the Hub. A PIN never leaves the screen it was typed on, and a PIN rotation bumps `verifier_version`.

**Kiosk security model:**
- The LocalSystem service `arena-kiosk-svc` owns enforcement, the DPAPI-protected device token, the pinned-TLS Hub connection (no unpinned fallback), and jittered reconnect between 2 and 30 s.
- The Tauri UI runs as the standard `player` user. It talks only to the service, over an ACL'd named pipe. The service restarts the app after 3 missed 2 s pings, and the screen stays covered meanwhile.
- Lockdown:
  - the `player` account's per-user `Winlogon\Shell` is the kiosk, so Explorer never starts
  - policy keys are set (DisableTaskMgr, NoRun, etc.)
  - a low-level keyboard hook swallows Win-key combinations and Alt+F4
  - the main window is topmost and covers all monitors
  - USB autorun is off and removable drives are hidden
- Process control: a process that is block-listed or not on the allow-list is killed within 200 ms. Matching uses the signed path, not the file name.
- Admin exit (Ctrl+Alt+Shift+A): a manager PIN gives 15 minutes of desktop. When offline, the PIN is checked against a cached, versioned, DPAPI-protected verifier, and the exit is flagged in the audit.
- Kiosk events are queued as idempotent intents and replayed in order.

**Tokens:**
- Tokens are generated from DESIGN.md frontmatter into CSS custom properties and TS constants. CI fails on stale tokens.
- Raw hex colours outside the generated files are forbidden (lint).
- Themes: dark "Cinema Electric" is the default, light "Cool paper" is web/Owner only, and every component token has an `-hc` twin.
- Sora is bundled, with tabular numerals.

## UX & Interaction Patterns

- **Hub shell:** an 88px `hub-rail` with Floor, POS, Players, Sessions, Inventory, Shift, Reports, Settings and System, plus a 72px `hub-header` (branch name, clock, signed-in staff, `chip-offline`). Below 840px wide the rail becomes a 64px `hub-bottom-bar` with a More sheet. Ctrl+1–4 open Floor, POS, Players and Shift, except while a text field has focus. Floor renders from the local database immediately, with no skeleton. Unbuilt rail items open a placeholder view; none are hidden.
- **Focus ring:** 2px magenta inner + 2px white outer at 2px offset. In HC it is 4px yellow; in light it is 2px `#AD0E63`. It is never suppressed.
- **Disabled controls** show at 40% opacity with the reason as inline text.
- **Field errors** show a danger border, a ⚠ glyph and the message. Focus moves to the first invalid field, and the error is announced through a live region.
- **Refusals are inline,** never in a blocking modal. A control the user lacks permission for stays visible and opens an `inline-pin` in place: 4–6 cells, numeric keypad plus keyboard. A reason picker follows when the intent needs one. Esc clears the PIN and returns focus to the control that opened it. Error messages never name anyone.
- **Lockouts:** sign-in pauses for 60 s after 5 failures, and so do PIN entry and admin exit, each with a visible countdown. Sign-in errors never say which field was wrong.
- **Idle lock:** after 5 minutes by default (Manager-configurable within a range). Unlocking restores the exact screen, scroll position and unsaved fields. A different staff member's PIN switches the actor.
- **High contrast:** pure black and white, accent `#FFE14D`, 2px white outlines on interactive surfaces, no shadows, translucency or art, and instant motion. It is set per device by a Manager, with no player-facing toggle. Hub, Kiosk and Kitchen are dark + HC only. A kiosk applies cached HC before its first paint.

## Cross-Story Dependencies

- **Build order:**
  - 1.1 comes first, then 1.2, then 1.3, 1.4 and 1.5. Generated types, tokens and the catalogue gate all UI stories.
  - On the Hub side, 1.6 comes first, then 1.7 (the dedupe step is a stub), then 1.8 (which fills it), then 1.9.
  - 1.11 (permissions, staff, roles, and the actor on every change) comes before 1.12–1.15 and 1.20–1.21.
- **Reuse:** 1.13's `inline-pin` is reused by 1.14, 1.15, 1.20 and 1.21. 1.12's `settings.set`/`settings.changed` path carries the kiosk process lists (1.18) and the per-device HC setting (1.21).
- **Kiosk order:** 1.16 (service, pipe, pinned TLS on top of 1.9) comes before 1.17, then 1.18, then 1.19 (the escape suite tests all of them), then 1.20. 1.20's remote unlock uses the 1.10 floor placeholder.
- **Final check:** 1.22 integration-tests every Epic 1 flow with the WAN blocked.
- **Epic 2:** replaces the dev fixtures with real pairing, mDNS and setup codes, and adds the verifier push on PIN rotation (Story 2.14).
