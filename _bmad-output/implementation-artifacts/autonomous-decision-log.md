# Autonomous run: decision log

Started 2026-10-23, while the user was away. The instruction was: "continue developing stories in loop, take recommended decision wherever needed and keep log."

Each entry gives the story, the decision point, the choice taken (the recommended option) and why. Review these when you're back; anything here can be reversed.

## Story 1.5: shared base components meet the accessibility floor

- **Spec approval:** self-approved, as instructed. Spec: `spec-1-5-shared-base-components-meet-the-accessibility-floor.md`.
- **Gallery location:** a separate dev-only app, `apps/gallery`, rather than a hidden route inside the Hub. Why: the Hub bundle stays clean, and Playwright/axe can run against a static preview.
- **StatusBadge themes:** dark and HC only. Why: the 1.4 decision keeps Hub-only tokens out of light.
- **TabControl URL sync:** the component reports the selected tab through `onchange`; the page updates the URL. Why: it keeps the component router-agnostic.

## Scope update (user, 2026-10-23)

- "Complete all of epics, one commit after stories." Scope is now every epic in `sprint-status.yaml` (1–14), in order, with one local commit per story. Still no pushes, deploys or remote operations.

## Story 1.6: Hub service starts as the single writer of its branch database

- **Spec approval:** self-approved. Spec: `spec-1-6-hub-service-starts-as-the-single-writer-of-its-branch-databa.md`.
- **Healthz port:** loopback `127.0.0.1:7409`, overridable with `--health-port`. Why: `/healthz` is loopback-only, so it doesn't break the "no inbound ports beyond 7400/7401" rule, and 7409 stays next to the Arena range.
- **Follower takeover:** none automatic. A follower stays a follower even after the lease expires. Why: promotion is manual and epoch-fenced (AD-11), and it arrives in a later story.
- **Data dir:** `%PROGRAMDATA%\Arena360\Hub` on Windows and the XDG data dir elsewhere, overridable with `--data-dir` or `ARENA_HUB_DATA_DIR`.
- **Writer job API:** a closure over the transaction, as a stand-in until Story 1.7 replaces it with `apply()`.
- **Crash test:** a re-invoked child process is killed with `Child::kill` right after it commits, so the test works the same on Linux and Windows.
- **Log ring:** daily JSONL files; files older than 14 days are deleted, then the oldest until the total is ≤ 50 MB.
- **Review process:** to save context over 417 stories, I check each diff by running the verification commands and reading the `--stat`, instead of reading every line of large diffs. The three independent reviewers still read the full diff.
- **1.5 review outcome:** 14 fixes and 10 rejections. Notable fix: disabled Buttons use `aria-disabled` and stay focusable, so the reason can be reached from the keyboard. AlertBadge keeps an empty live region mounted at count 0, so going from 0 to 1 is still announced.

## Story 1.7: apply() and the gap-free change log

- **Spec approval:** self-approved. Spec: `spec-1-7-every-change-goes-through-apply-into-a-gap-free-change-log-s.md`.
- **Permission step before Story 1.11:** `branch.bootstrap` requires `actor.kind == system`; any other actor gets `FORBIDDEN`. Story 1.11 replaces this with the permission catalogue, so the Permission enum stays empty for now.
- **New error codes:** `VALIDATION_FAILED` (with `field`) and `ALREADY_BOOTSTRAPPED`.
- **Dev seed values:** "Dev Branch" / DEV01 / INR / Asia/Kolkata / business day starts 06:00 (360) / financial year starts in April. These are India defaults that match the product's target market.
- **Default settings seeded:** `idle_lock_minutes=5`, `hub.locale=en-IN`, `receipt.footer=""`. Later stories add more.
- **Currency validation:** a small const ISO 4217 table (INR, USD, EUR, GBP, AED, SGD and similar), to be widened later.
- **Dev seed lease:** `dev seed` takes the lease one-shot, the same way as the Hub. If a live primary holds the lease, it fails with `NOT_PRIMARY`.
- **business_day/fy stamping:** deferred to Story 1.12 (locale and timezone). The stamp step sets rev and timestamps only.
- **1.6 implementation deviations, accepted:**
  - An OS file lock on `branch.db.lock` gives one writer per data dir. Two processes on one dir share a `hub_instance_id`, so the lease row alone can't tell them apart.
  - Taking another instance's expired lease bumps the counter, `(counter+1, own_id)`, so a paused old primary sees a higher epoch and demotes.
  - A fresh lease held by another instance means follower, even if its epoch is lower.
  - New error code `INTERNAL` for storage and I/O failures.
- **1.6 review outcome:** 21 fixes and 6 rejections. Key fixes: a primary now demotes when its own lease expires; the writer lock is taken before any migration or write; a job panic is reported as INTERNAL instead of NOT_PRIMARY; SIGTERM is handled; a single day's log now counts toward the 50 MB cap.

## Story 1.8: idempotent intents, row versions and property tests

- **Spec approval:** self-approved. Spec: `spec-1-8-idempotent-intents-optimistic-row-versions-and-property-test.md`.
- **First edit intent:** `settings.set` (key/value with `base_rev`), so the `STALE`/`rev` rules have a real edit to act on. Permission stub until 1.11: the system, owner and staff actors are allowed.
- **Which outcomes are recorded for dedupe:** successes, plus deterministic rejections (VALIDATION_FAILED, STALE, ALREADY_BOOTSTRAPPED, FORBIDDEN), so a retry returns the same answer. Transient errors (NOT_PRIMARY, EXPIRED, INTERNAL) are never recorded.
- **Idempotency key format:** must be a valid ULID; anything else is `BAD_FRAME`, before any transaction.
- **Future-skew guard:** an `issued_at` more than 5 minutes in the future is rejected with `VALIDATION_FAILED`.
- **Dedupe pruning:** a maintenance write outside `apply()`, producing no change rows. It runs at start and then every 24 h, only while primary.

## Story 1.9: LAN protocol envelope, hello handshake and client classes

- **Spec approval:** self-approved. Spec: `spec-1-9-lan-protocol-envelope-hello-handshake-and-client-classes.md`.
- **Frame encoding:** JSON text frames for every class. Browser classes (kitchen, phone) can read them directly, and the TS types come from `arena-proto`. MessagePack stays inside the change log only.
- **Release window:** a release is `major.minor`. The Hub accepts the same major with `client.minor >= hub.minor - 2`.
- **Revoking while the Hub runs:** `dev revoke-device` calls a dev-only loopback endpoint (`POST /dev/revoke-device`, feature-gated), because the running Hub holds the writer lock.
- **Class credentials in Epic 1:** `paired` and `kds` use device tokens from the `devices` table; `phone` is anonymous for now (its per-console token comes in a later epic). `paired` is TLS-only; `kds` and `phone` use plain `:7401/ws`.
- **Firewall:** `scripts/windows/firewall.ps1` (Private profile only, ports 7400 and 7401) for a future installer story to call. There is no installer yet.
- **Permission catalogue:** it starts with `device.remote` and `device.pair`. Story 1.11 adds the rest.
- **1.7 review outcome:** 18 fixes, 3 rejections and 1 item deferred to 1.8 (the multi-epoch replay test). Key fixes: unverified `elevated_by` is rejected until 1.13; the epoch guard also checks lease expiry; time zones are validated with chrono-tz (=0.10.4); foreign keys are enforced; `dev seed` releases the lease when it exits.

## Story 1.10: Hub app shell

- **Spec approval:** self-approved. Spec: `spec-1-10-hub-app-shell-with-left-rail-header-and-floor-placeholder-on.md`.
- **How the UI gets data:** a loopback-only `GET 127.0.0.1:7409/ui/snapshot` on the existing health server. CORS allows only the Tauri and dev origins. Why: the Hub UI is local, so it doesn't need a LAN device token, and the single writer stays in `arena-hub`.
- **How the service starts:** the Tauri app spawns `arena-hub` as a bundled sidecar when `/healthz` doesn't answer, and leaves it running when the UI closes. Registering the Hub as a Windows service comes later.
- **Installer:** the CI NSIS bundle, with an install hook that blocks Windows builds older than 17134 (10 1803). NSIS can't read the JS catalogue, so its message is duplicated there with a comment pointing to the key.
- **Benchmark:** `release-bench.yml` runs on a self-hosted `[windows, reference-i3]` runner and can't run until that runner exists. It isn't part of `ci-ok`.
- **Shell tests:** Playwright against `vite preview` with a stubbed snapshot (`page.route`), at desktop and phone widths.
- **1.8 implementation choices, accepted:** dedupe runs before the TTL check, so retrying a recorded key returns its first outcome even after 7 days. A future-skew rejection is not recorded. Keys are compared upper-cased. Setting values are capped at 4096 characters.

## Story 1.11: staff sign-in, roles and permissions

- **Spec approval:** self-approved. Spec: `spec-1-11-staff-sign-in-to-the-hub-with-roles-and-permissions.md`.
- **Permission catalogue:** taken from the LLD intent→permission table plus `report.view`, `audit.view`, `player.view` and `ops.restore` (about 34, close to "~40"). Implicit requirements are modelled as `Requirement::{System, Device, Phone}`, so every intent has exactly one requirement.
- **`device.revoke` permission:** `device.remote`, following the epic's acceptance criteria rather than the LLD's `device.manage`, because the epic is the more recent source.
- **Owner/admin actors:** an `owner` actor holds every permission (owner commands arrive later). An `admin` actor holds none until the admin epic.
- **Sessions:** in-memory bearer tokens on the loopback server; signing in again is required after a Hub restart. The lockout is also in-memory, per login.
- **Refusal demo:** the Settings placeholder gets one "Receipt footer" control, so the inline `PERMISSION_DENIED` refusal can be demonstrated and tested.
- **Dev credentials:** `owner`/`manager`/`counter`/`kitchen`, password `dev-<login>-pass`, PINs 1111–4444. These exist only in `dev-fixtures` builds.
- **1.8 review outcome:** 14 fixes, 3 rejections, 1 deferred (replicating dedupe records to the standby). Key fixes: a request fingerprint and a new `IDEMPOTENCY_CONFLICT` code; outcomes are recorded from an allow-list and permission denials are never recorded; per-key setting rules (`idle_lock_minutes` 1–30); housekeeping runs on promotion and deletes in batches; `sha2 =0.10.9` pinned.

## Story 1.12: branch locale settings

- **Spec approval:** self-approved. Spec: `spec-1-12-branch-locale-settings-currency-number-format-and-timezone.md`.
- **Source of truth:** locale lives in `settings` keys `branch.*`. The `branch` row's currency/tz/day_start columns are kept only as bootstrap values. Why: the acceptance criteria say changes go through `settings.set` with a key and value.
- **Deferred tz/day-start:** `prior_value` + `effective_from` columns on `settings`. A second change while one is pending replaces the new value but keeps the original prior value.
- **business_day/fy:** stamped on every change row (new `business_day`/`fy` columns on `changes`), so the rule can be observed now, before sessions and orders exist. The FY label format is `FY2026-27`.
- **Currency table:** a single source in `arena-proto`, exported to `packages/ui/src/generated/currencies.json` by `gen:types`. The formatter and the Rust validation share it.
- **Live updates:** a loopback SSE stream, `/ui/events?token=…` (EventSource can't send headers). This is server push, not polling.
- **Defaults:** symbol before; Indian number format when the currency is INR, otherwise international. Negatives use U+2212.
- **1.9 implementation choices, accepted:** a device token is `<device_id>.<secret>`, so each hello checks one argon2 hash. Event and error frames nest their bodies. Token checks read through a read-only connection, off the async threads. A follower with no certificate yet serves a temporary one.
- **1.9 review outcome:** 19 fixes and 4 rejections. Security fixes: no missed kills during the handshake; `applied` events go only to paired sessions; argon2 checks are capped, throttled per IP and timing-matched; 512-connection cap, 15 s ping and 45 s idle close. Deviations: an oversized frame closes the session (tungstenite can't continue after it); a revoked device actor gets FORBIDDEN, because PERMISSION_DENIED arrives in 1.11.
