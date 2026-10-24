# Arena 360

Monorepo for the Arena 360 branch Hub, Kiosk, cloud and web surfaces.

## Bootstrap

Install the pinned toolchains (Node from `.nvmrc`; Rust is selected automatically by
`rust-toolchain.toml` via rustup), then run:

```sh
corepack enable && pnpm install && cargo build --workspace && pnpm check
```

`pnpm check` runs `cargo fmt --check`, `cargo clippy` (warnings denied), the prettier check, the
layer check, the pin check, the generated-types, protocol-type and string checks, the design-token
gates (freshness, contrast, unknown tokens, raw hex), and the check scripts' own tests. Run `cargo test --workspace` for the
Rust tests.

## Layout

| Path        | Contents                                                  |
| ----------- | --------------------------------------------------------- |
| `crates/`   | Rust workspace crates (`crates/<name>`)                   |
| `apps/`     | Svelte / Tauri / Astro apps (`apps/<name>`, see below)    |
| `packages/` | Shared JS packages: `packages/ui` is `@arena/ui`          |
| `scripts/`  | Repo checks (`check-*.mjs`), `gen-types.mjs`, branch rule |

Rust crates: `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-tasks`,
`arena-replay`, `arena-kiosk-core` (libraries), `arena-cloud` (library and the `arena-cloud`
binary, see [Running arena-cloud](#running-arena-cloud)) and `arena-hub`, `arena-kiosk-svc`,
`arena-pi` (binaries), plus the test-only `arena-e2e` driver (see
[The branch with the WAN down](#the-branch-with-the-wan-down)).

| Path      | Contents                                                                              |
| --------- | ------------------------------------------------------------------------------------- |
| `deploy/` | `arena-cloud` image, k3s manifests, Cloudflare ranges, SOPS secrets, operator runbook |

### Apps

Every app except the Hub renders `Placeholder` from `@arena/ui` on its root page (the Hub has its
app shell, see [The Hub UI](#the-hub-ui)). `pnpm build` builds them all;
`pnpm --filter @arena/<name> build` builds one.

| App               | Package             | Stack                                          | Build output                     |
| ----------------- | ------------------- | ---------------------------------------------- | -------------------------------- |
| `apps/hub`        | `@arena/hub`        | Tauri + SvelteKit, adapter-static SPA          | `build/`, crate `arena-hub-ui`   |
| `apps/kiosk`      | `@arena/kiosk`      | Tauri + SvelteKit, adapter-static SPA          | `build/`, crate `arena-kiosk-ui` |
| `apps/kitchen`    | `@arena/kitchen`    | SvelteKit, adapter-static (prerendered)        | `build/`                         |
| `apps/owner`      | `@arena/owner`      | SvelteKit, adapter-cloudflare (Workers assets) | `.svelte-kit/cloudflare/`        |
| `apps/partner`    | `@arena/partner`    | SvelteKit, adapter-cloudflare (Workers assets) | `.svelte-kit/cloudflare/`        |
| `apps/admin`      | `@arena/admin`      | SvelteKit, adapter-cloudflare (Workers assets) | `.svelte-kit/cloudflare/`        |
| `apps/onboarding` | `@arena/onboarding` | SvelteKit, adapter-cloudflare (Workers assets) | `.svelte-kit/cloudflare/`        |
| `apps/marketing`  | `@arena/marketing`  | Astro + `@astrojs/svelte`, static              | `dist/`                          |

The Tauri shells live in `apps/<app>/src-tauri` (Cargo workspace members). Build one without
installers with `pnpm --filter @arena/hub tauri build --no-bundle`. On Linux, Tauri needs
`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
build-essential` for `cargo build --workspace`.

### Layer rules (enforced by `pnpm check:layers`)

Normal and build dependencies between workspace crates must follow this allow-list
(dev-dependencies are ignored; crates not listed are unrestricted):

| Crate              | May depend on                                |
| ------------------ | -------------------------------------------- |
| `arena-proto`      | —                                            |
| `arena-store`      | `arena-proto`                                |
| `arena-core`       | `arena-store`, `arena-proto`                 |
| `arena-api`        | `arena-core`, `arena-proto`                  |
| `arena-tasks`      | `arena-core`, `arena-store`, `arena-proto`   |
| `arena-replay`     | `arena-store`, `arena-proto`                 |
| `arena-cloud`      | `arena-proto`, `arena-replay`, `arena-store` |
| `arena-kiosk-core` | `arena-proto`                                |

## Running the Hub

```sh
cargo run -p arena-hub -- --data-dir /tmp/hub
curl 127.0.0.1:7409/healthz
```

| Flag                                 | Meaning                                                                                                                                |
| ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `--data-dir <path>`                  | Data directory (overrides `ARENA_HUB_DATA_DIR`)                                                                                        |
| `--health-port <port>`               | Loopback port for `GET /healthz` (default `7409`, not `0`)                                                                             |
| `--lan-port <port>`                  | TLS WebSocket port for paired apps (default `7400`)                                                                                    |
| `--web-port <port>`                  | Sniffed TLS / plain HTTP port (default `7401`)                                                                                         |
| `--cloud-host <host:port>`           | Cloud host the connectivity probe checks (default `api.arena360.cloud:443`, the production `arena-cloud` `/healthz` behind Cloudflare) |
| `--probe-internet <ip:port,ip:port>` | Internet endpoints the probe connects to (default `1.1.1.1:443,8.8.8.8:443`)                                                           |
| `--help`, `-h`                       | Print usage and exit 0                                                                                                                 |
| `--version`                          | Print the version and exit 0                                                                                                           |

The three ports must differ. Tests pass free ports; production uses the defaults.

- **Data dir:** `--data-dir`, else `ARENA_HUB_DATA_DIR`, else `%PROGRAMDATA%\Arena360\Hub` on
  Windows and `$XDG_DATA_HOME/arena360/hub` (falling back to `~/.local/share/arena360/hub`)
  elsewhere. The database is `<data>/branch.db`; `<data>/branch.db.lock` is the process writer
  lock, so a second process on the same dir starts as a read-only follower.
- **Health:** `GET 127.0.0.1:<port>/healthz` returns `db_open`, `role`, `lease`, `epoch`,
  `schema_version`, `self_instance_id`, `tls_fingerprint`, `connectivity` and `now`. It binds
  loopback only.
- **Connectivity probe** (`arena-tasks::connectivity`): the branch's only outbound traffic. At
  start and every 30 s it runs two checks at once, each with a 3 s timeout, on its own task (it
  never touches the writer, so no intent waits on it): a TCP connect to each `--probe-internet`
  endpoint (any success: the internet is up) and a TLS handshake (rustls with the pinned
  `webpki-roots`) to `--cloud-host` followed by a hand-written `GET /healthz` that must answer 200. No internet is `internet_offline`, the internet without the cloud is
  `cloud_unreachable`, both is `online`. `connectivity` is `{state, checked_at}` (UTC ms; it
  reads `online` with `checked_at: null` until the first probe finishes, so no offline chip
  shows before the Hub knows). A change of state is logged once at info.
- **UI snapshot:** `GET 127.0.0.1:<port>/ui/snapshot` on the same loopback server returns
  `HubSnapshot {branch: {name, branch_code} | null, devices: [{device_id, label, class, zone,
online}], locale, ui, connectivity}` (type in `arena-proto`; `connectivity` is the probe's
  last answer, from the Hub status; revoked devices are left out; `zone` is `null` until
  zones exist; `online` is true while the device has a live paired LAN session, read from the
  LAN registry, and the floor shows it as the words Online / Offline; `locale` is `null` before bootstrap, see **Branch locale** below). It
  reads through `arena-core` on its own read-only connection. CORS allows only
  `tauri://localhost`, `http(s)://tauri.localhost` and the vite origin `http://localhost:5173`;
  another `Origin`, or a `Host` that is not `127.0.0.1`/`localhost`/`[::1]`, gets 403.
- **Staff auth (same loopback server and origin guard):**

  | Endpoint                                                        | Answer                                                                                                                                                                                                                                                                                                 |
  | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
  | `POST /ui/auth/sign-in {login, password}`                       | `{token, staff: {staff_id, display_name, role_name, permissions}}`, or 401 `AUTH_FAILED` with one fixed message whatever was wrong (unknown login, wrong password, inactive staff)                                                                                                                     |
  | `POST /ui/auth/sign-out` (bearer)                               | 204; the token stops working                                                                                                                                                                                                                                                                           |
  | `GET /ui/me` (bearer)                                           | the session's staff plus `locked` and `has_pin`, or 401 `AUTH_FAILED`                                                                                                                                                                                                                                  |
  | `POST /ui/lock` (bearer)                                        | 204; the session is idle-locked: `/ui/intent`, `/ui/elevate` and `/ui/audit*` answer 423 `SESSION_LOCKED` and its tickets are dropped. See **Idle lock** below                                                                                                                                         |
  | `POST /ui/unlock {pin}` (bearer)                                | `{staff, switched}` for a locked session (else 409 `VALIDATION_FAILED` on `session`); own PIN first, another member's re-binds it; else 403 `PIN_REJECTED`                                                                                                                                             |
  | `POST /ui/elevate {items: [{intent_kind, key?}], pin}` (bearer) | `{ticket, items, expires_in_s}` when the PIN belongs to another active staff member whose role holds every permission the items need; otherwise 403 `PIN_REJECTED` with one fixed message. See **PIN elevation** below                                                                                 |
  | `POST /ui/elevate/drop {ticket}` (bearer)                       | 204; the ticket's unused approvals are given up                                                                                                                                                                                                                                                        |
  | `POST /ui/intent {intent, elevation_ticket?}` (bearer)          | `{applied}`; the intent runs as the session's `actor{staff, staff_id}` whatever actor was sent, and any client-supplied `elevated_by` is dropped. A ticket is redeemed for one matching approved item and becomes `elevated_by`; a bad one is 403 `PIN_REQUIRED`. Errors are `{code, message, field?}` |
  | `GET /ui/settings` (bearer)                                     | `[{key, value, rev}]`, so an edit can carry `base_rev`                                                                                                                                                                                                                                                 |
  | `GET /ui/audit?actor&module&from&to&cursor&limit` (bearer)      | `{items, next_cursor}`, newest first; needs `audit.view` or a read ticket in `X-Arena-Elevation`, else 403 `PERMISSION_DENIED`. See **Audit log** below                                                                                                                                                |
  | `GET /ui/audit/item?epoch&seq` (bearer)                         | `{item, after, before, changed}` for one change (same gate); secrets read `"[redacted]"`                                                                                                                                                                                                               |
  | `GET /ui/audit/facets` (bearer)                                 | `{staff: [{id, name}], modules}` for the audit filters (same gate)                                                                                                                                                                                                                                     |
  | `GET /ui/events?token=<token>`                                  | server-sent events: `settings.changed` `{locale, keys, ui}` when an intent wrote `settings`, `applied` `{intent_id}` for any other intent, `resync` when the stream fell behind. A bad token is 401; the stream closes once its session ends                                                           |

  Five consecutive failed sign-ins for a login (compared without case) pause it for 60 s: every
  attempt in that window gets 429 `AUTH_LOCKED` with `retry_after_s`, and a success resets the
  count; attempts in flight count toward the limit, so parallel guesses cannot slip past it.
  Tokens are 32 random bytes (base64url) held in memory only, so a Hub restart signs everyone
  out. A session ends 12 h after sign-in, or as soon as its staff member is deactivated or
  removed (every authenticated request re-reads the staff row and refreshes the name and
  role). A staff member holds at most 5 sessions; a sixth sign-in ends the oldest. A missing or
  unknown token is 401 `AUTH_FAILED`; signing out an unknown token is still 204.

- **PIN elevation:** a member whose role lacks an intent's permission asks a manager to type
  their PIN on the same screen. `POST /ui/elevate` takes the exact intents to approve
  (`items`, 1–10; a `settings.set` item names its setting key). A follower refuses first
  (`NOT_PRIMARY`), an unhandled kind is `UNSUPPORTED`, and a member who already holds every
  permission gets `VALIDATION_FAILED` on `items`. Every item's requirement must be a
  `Permission`; the PIN is checked with argon2id (at most two checks at once) against at most 50
  active staff with a PIN whose role allows all of them, never the session's own member. A match
  is an in-memory ticket of 128 random bits for this session, redeemable once per approved item
  within 60 s; a session holds at most 4 (a fifth replaces the oldest), and signing out drops
  them. A wrong PIN, a PIN whose role lacks the permission, the member's own PIN and a kind that
  is not a permission all get the same 403 `PIN_REJECTED`, which never names anyone. Five
  failures in a row pause elevation for that staff member, whatever the session, for 60 s (429
  `AUTH_LOCKED` with `retry_after_s`, already on the fifth); a check that could not run counts
  neither way. `POST /ui/intent` redeems one unused item that matches the intent (kind, and the
  key of a `settings.set`; otherwise `PIN_REQUIRED`) into
  `elevated_by{kind: pin, id: staff_id, mode: hub, verifier_version}`; a refusal (nothing was
  applied) gives the item back, unless the ticket has lapsed or gone meanwhile. `apply()` checks
  the permission against that member (active, holds it, not the actor, and the actor an active
  staff member) instead of the actor and fails `PIN_REQUIRED` ("enter the PIN again") if their
  `pin_verifier_version` moved on. Every `staff.upsert` with a new `pin_hash` bumps that version
  by 1. The idempotency fingerprint leaves `elevated_by` out, so a retry after re-entering a PIN
  is the same intent. Change rows record the actor and `elevated_by_*` (`elevated_by_kind` is
  `pin` or `mfa`, migration 0007). A client's own `elevated_by` is stripped at `/ui/intent` and
  on the LAN. The PIN is never logged, stored or echoed, and goes nowhere but this loopback call.
  **Read elevation:** `items: [{permission}]` (one item, on its own, naming `audit.view`,
  `report.view` or `player.view`) asks for a read ticket instead: the same candidates (active
  staff holding that permission, never the requester), the same throttle and the same
  refusals. It is bound to the session, the member and that permission, allows unlimited
  reads for 10 min (`expires_in_s: 600`), counts toward the 4 tickets a session holds, never
  redeems an intent (`PIN_REQUIRED`), and is dropped like any ticket by a lock, an unlock that
  switches the member, or sign-out. Every read re-checks the approver (active, their role
  still allows the permission, `pin_verifier_version` unchanged); if not, the ticket is
  dropped and the read is 403 `PERMISSION_DENIED`. **Grants are audited:** on a primary the
  Hub records each grant, before handing the ticket out, as the system intent
  `audit.read_granted {permission, requester_staff_id, approver_staff_id, verifier_version}`,
  which inserts a `read_grants` row (migration 0008) and so shows in the audit viewer (module
  `audit`); if it cannot be recorded the ticket is withdrawn and the error returned. A follower
  cannot write: it may still grant a read elevation and logs the grant at info instead.
- **Idle lock:** a signed-in Hub UI locks after `idle_lock_minutes` (1–30, default 5; the
  snapshot's `ui: {idle_lock_minutes}`, pushed in `settings.changed`, a failed read sending the
  default) without input. It calls `POST /ui/lock`; until a PIN unlocks the session the Hub
  refuses its intents and elevations with 423 `SESSION_LOCKED` (a ticket being issued is
  re-checked against the lock under the tickets lock), and nothing else stops (`/ui/events`,
  `/ui/settings`, `/ui/elevate/drop` and sign-out keep working, the writer keeps applying).
  `POST /ui/unlock {pin}` works only on a locked session (otherwise 409 `VALIDATION_FAILED` on
  `session`, before any PIN check, so it is never a way to switch members on a live screen). It
  checks the session's own member first, then the other active staff with a PIN (argon2id, at
  most 50 checks, sharing the two-at-once cap with elevation). The own PIN unlocks the session
  unchanged; another member's PIN unlocks it and makes them its staff member (`switched: true`,
  their staff block), so every later intent carries their actor, and tickets issued before the
  switch are dropped and refused. A member deactivated between the match and the re-bind counts
  as a failure. No match is 403 `PIN_REJECTED` with one fixed message, "that PIN can't unlock
  this screen". Five failures in a row, counted per staff member and per token, pause unlocking
  for 60 s, doubling with each further pause up to 15 min until a successful unlock (429
  `AUTH_LOCKED` with `retry_after_s`; each pause logs a warning with the staff id and count);
  a new session of the same member stays paused, and sign-out keeps working. Locking and
  unlocking push `session.locked` / `session.unlocked` (`{staff, switched}`) to the
  `/ui/events` streams of the same token. The Hub logs `ui.locked {staff_id, at}` and
  `ui.unlocked {staff_id, previous_staff_id?, at}` at info, never the PIN or its length. A
  re-bound session keeps its token and its 12 h lifetime, and a staff refresh in flight never
  reverts it.
- **Audit log:** the read-only routes over `changes` (`arena-api/src/ui_audit.rs`, read through
  `arena_core::audit` on their own read-only connection, never the writer). The session must be
  able to act (423 `SESSION_LOCKED` while idle-locked) and hold `audit.view`, or send a read
  ticket for `audit.view` in `X-Arena-Elevation` (allowed by the CORS preflight); otherwise 403
  `PERMISSION_DENIED`, checked before anything is read. `GET /ui/audit` filters by
  `actor=<kind>` or `<kind>:<id>`, `module=<domain>` (equality on the generated column
  `changes.module`: the text of `intent_kind` before its first `.`, or the whole kind when it
  has none), `from` (UTC ms, inclusive) and `to` (exclusive), all ANDed; it orders by
  `(at DESC, epoch DESC, seq DESC)` and pages by keyset: `cursor` is that triple of the last
  item, base64url, and `next_cursor` is `null` on the last page (no `OFFSET`). `limit` is 1–100
  (default 50); a malformed parameter is 422 `VALIDATION_FAILED` naming it. Items are
  `{epoch, seq, at, business_day, actor: {kind, id, name}, elevated_by: {kind, id, name, mode} |
null, module, intent_kind, intent_id, tbl, row_id, op}`: staff show their display name,
  devices their label, and `system`, `owner` and `admin` the catalogue keys `hub.audit.actor.*`;
  an unknown id shows the id, and a stored kind or mode this build does not know reads `unknown`
  (logged as a warning) instead of failing the page. `GET /ui/audit/item?epoch&seq` decodes the
  change's MessagePack image (`after`; for a delete, the row as it was deleted) and the image of
  the previous change to the same `(tbl, row_id)` in log order (`before`; `null` for a created
  row, including one re-created after a delete), lists the columns that differ (`changed`, empty
  for a create), and replaces `password_hash`, `pin_hash`, `token_hash` (and any column in
  `arena_core::audit::REDACTED`) with `"[redacted]"` in both images. Columns in
  `arena_core::audit::ID_ONLY` (`player_id`, for the player tables of later epics) show the id
  alone: a text value keeps only its leading id, anything joined to it (a name, a phone) is
  dropped, and a value with no id in front is redacted. An `epoch`/`seq` that names no change
  is 422 `VALIDATION_FAILED` on `seq` (a missing `epoch` names `epoch`). Migration 0008 adds the
  virtual generated columns `epoch_counter` and `module` and the indexes
  `changes(at, epoch, seq)`, `changes(actor_kind, actor_id, at, epoch, seq)`,
  `changes(actor_kind, at, epoch, seq)`, `changes(module, at, epoch, seq)` and
  `changes(tbl, row_id, epoch_counter, epoch, seq)`, so every filter combination pages without a
  sort and the previous image is one indexed lookup. A Rust test writes 100,000 changes (5
  actors, 3 modules) and requires the unfiltered, actor + date, actor + module + date,
  module-only and kind-only first pages under `AUDIT_PERF_BUDGET_MS` (default 250) in a debug
  build after a warm-up, a proxy for the < 1 s budget on the reference hardware (the release
  benchmark's job).
- **Reasons:** `IntentKind::reasons()` lists the reason codes a kind requires (generated for the
  UI as `generated/reasons.json`, read through `reasonsFor()` / `reasonOptions()` in
  `@arena/ui`). Only `device.revoke` has any today (`lost`, `stolen`, `replaced`, `other`);
  without one it fails `REASON_REQUIRED` (422, `field: "reason"`), and a reason a kind does not
  list fails `VALIDATION_FAILED` on `reason`. The chosen code is recorded on each change row
  (`changes.reason`).
- **Branch locale:** the `branch.*` settings are authoritative (the `branch` row's columns are
  only the bootstrap values), and `branch.bootstrap` seeds them from its payload:
  `branch.currency` (ISO 4217, from `arena_proto::currency::CURRENCIES`),
  `branch.symbol_position` (`before`), `branch.number_format` (`indian` for INR, otherwise
  `international`), `branch.tz` (IANA), `branch.day_start_min` (0–1439) and
  `branch.fy_start_month` (1–12). They are edited with `settings.set` (`settings.edit`).
  `apply()` stamps every change row with its `business_day` (the local date of
  `ts − day_start` in the branch tz) and `fy` (`FY2026-27` for an April start; `FY2027` for a
  January one). A `branch.tz` or `branch.day_start_min` change saved during business day D keeps
  the old value in `settings.prior_value` and applies from `effective_from` = D+1, so the rest
  of D is stamped under the old rules; a second change while one waits keeps the original prior
  value. Every other key applies at once. Existing rows are never restamped. The snapshot's
  `locale` is `{currency, exponent, symbol, symbol_position, number_format, tz, day_start_min,
fy_start_month, pending: {tz?, day_start_min?, effective_from} | null}`.
- **Permissions:** every intent kind has one requirement (`IntentKind::requirement()` in
  `arena-proto`): a catalogue permission (`Permission`, dotted `domain.verb`), or the implicit
  `System`, `Device` or `Phone`. `apply()` checks it first: a `staff` actor needs an active staff
  row whose role allows the permission (and does not deny it); an `owner` actor holds every
  permission; an `admin` actor holds none yet. Refusals are `PERMISSION_DENIED`.
  `branch.bootstrap` seeds the built-in roles (Owner, Manager, Counter, Kitchen, Kiosk-device),
  which cannot be changed; `role.upsert` makes custom roles from catalogue permissions only, and
  `staff.upsert` takes argon2id password and PIN hashes computed outside the transaction.
  - **Restricted actors.** A staff actor whose role is not the built-in Owner (a Manager, or a
    custom role holding `user.manage`) may only assign the built-in Counter or Kitchen roles,
    only edit staff who hold one of them, never edit the role it holds itself, and never write a
    role that allows a permission its own role does not allow.
  - **Last Owner.** A `staff.upsert` that would leave no active staff member holding the
    built-in Owner role fails `VALIDATION_FAILED` (`role_id` or `active`).
  - **Names.** Logins are 2–32 ASCII letters, digits, `.`, `_` or `-`, unique without case.
    Custom role names are unique without case and never a built-in name. Kiosk-device is never
    assigned to a person.
  - **Deny rows.** Each staff member holds exactly one role, so a `deny` row means the same as
    no row today; deny entries are kept for a later multi-role model.
- **Logs:** JSON lines in `<data>/logs/hub-YYYY-MM-DD.jsonl` (UTC), with `.N` part files when the
  ring fills during a day. Files older than 14 days are deleted and the total is kept at or under
  50 MB (oldest deleted first). Logs never contain PII. Set the level with `RUST_LOG` (default
  `info`, e.g. `RUST_LOG=arena_tasks=debug`).
- **Stopping:** Ctrl+C, or SIGTERM on Unix.

### LAN protocol

The Hub listens on `0.0.0.0:7400` and `0.0.0.0:7401`; no other inbound port is opened.

- **TLS.** At first start the writer generates a self-signed ECDSA P-256 certificate (SAN
  `arena360.local` plus the hostname) into `<data>/tls/hub.crt` and `hub.key` (the key is
  owner-only on Unix) and records its SHA-256 fingerprint (lower-case hex) in
  `hub_meta.tls_fingerprint`. Later starts reuse it. Clients pin that fingerprint; there is no
  CA and no unpinned fallback.
- **Ports.** `:7400` serves the TLS WebSocket at `/` for `paired` apps. `:7401` sniffs the first
  byte: `0x16` (TLS) is served exactly like `:7400`; otherwise it is plain HTTP, where `GET /ws`
  upgrades for the browser classes (`kds`, `phone`), `/kitchen` and `/console/<tv>` return a
  placeholder page and everything else is 404.
- **Frames.** UTF-8 JSON text `{v: 1, kind, rid, ...body}` with kinds `hello`, `hello.ok`,
  `intent.submit`, `intent.result`, `event`, `error`, `ping`/`pong`, `settings.get`/`settings.values` (types in `arena-proto`,
  generated into `@arena/ui/generated`). Unknown fields are ignored; an unknown kind or a `v`
  other than 1 gets `UNSUPPORTED`, an unparseable frame `BAD_FRAME`, and neither closes the
  session.
- **Hello.** The first frame must be `hello{credential, class, versions, capabilities}`, or the
  connection gets an error frame and is closed. `paired` (TLS only) and `kds` (`:7401/ws` only)
  present a device token `<device_id>.<secret>`, verified with argon2id against the `devices`
  row (`TOKEN_INVALID`, `TOKEN_REVOKED`); `phone` is anonymous. A paired client must share the
  Hub's major version and be at most two minors behind (`UNSUPPORTED` naming the minimum).
- **Sessions.** The actor is always `{device, device_id}` from the session. Each class may only
  submit the intents in its scope (`arena-proto` `class_scope`), else `FORBIDDEN` and a `warn`
  log line; no browser class can reach staff, money or device intents. Token checks are
  limited to 4 at a time, and an address with 5 failed hellos in 60 s is refused for 60 s. At
  most 512 connections are served at once. After hello the Hub sends `ping` every 15 s and
  closes a session that sent nothing for 45 s; the client does the same in reverse. A follower answers
  `hello` and refuses every intent with `NOT_PRIMARY`. Every committed intent is pushed to
  every `paired` session as `event{kind: "applied"}` (browser classes only get allow-listed
  tables, none yet); clients never poll. If the push fan-out falls behind, every session is
  dropped and clients reconnect. Each session has its own
  bounded queue, and a session that falls behind is dropped.
- **Paired scope.** A paired session may submit `kiosk.log{level: info|warn|error, code, detail}`
  (requirement: a live device actor), which writes one `device_events` row (migration 0009;
  `at` is the intent's `issued_at`, when the kiosk recorded it). Opening or closing a paired
  session pushes `devices.changed` on `/ui/events`, and the Hub UI reloads the snapshot. It may
  also submit `kiosk.process_blocked{exe_name, path, reason, mode?}` (a `device_events` row with
  `code=process_blocked`; both kinds share the 60/min per-device limit) and read branch settings
  with `settings.get{keys}` — `kiosk.*` keys only, anything else refuses the whole request with
  `FORBIDDEN`. An intent that writes `kiosk.*` settings is pushed to paired sessions as
  `event{kind: "settings.changed", values}`; browser classes can neither read nor receive them.
- **Revocation.** `device.revoke` closes the device's sessions with `TOKEN_REVOKED` within 1 s,
  and later hellos with that token are refused.
- **Client.** `arena-kiosk-core` has `HubClient::connect(url, pinned_fingerprint, hello)`:
  pinned TLS, `request(kind, body)` with a 5 s `HUB_SLOW` timeout per request, and
  `events()`. A certificate that does not match the pin fails `PinMismatch{expected,
presented}`, never falling back.
- **Firewall.** `scripts/windows/firewall.ps1` (run elevated) adds inbound allow rules for TCP
  7400 and 7401 (`-LanPort`/`-WebPort` to match non-default Hub ports) on the Private profile
  only, and warns when the active network is not Private; `-Remove` deletes them. The Hub installer story
  calls it; CI does not run it.

### Idempotency and housekeeping

Each intent carries a ULID `idempotency_key`; a missing or malformed one fails `BAD_FRAME`
before any transaction. The outcome of each key (success, or a deterministic rejection such as
`STALE`) is kept in `applied_intents`, and a retry gets that outcome back without writing
anything. Intents older than 7 days fail `EXPIRED`. While primary, the Hub prunes dedupe
records older than 30 days at start and every 24 h. That prune is a maintenance write outside
`apply()` and writes no change rows. The property suite (`crates/arena-core/tests/proptest_apply.rs`)
runs 64 cases by default; set `PROPTEST_CASES` for more.

### Seeding a dev branch

Every branch write goes through `apply()` and lands in the append-only `changes` log. To get a
branch to work on locally, build with the dev-only `dev-fixtures` feature (never enabled in
default or release builds, where `dev` is an unknown command) and run:

```sh
cargo run -p arena-hub --features dev-fixtures -- dev seed --data-dir /tmp/hub
```

`dev` must be the first argument (`arena-hub --data-dir /tmp/hub dev seed` is rejected). It
takes the lease once and submits `branch.bootstrap` as `actor{system, "dev-seed"}`: branch
"Dev Branch", code `DEV01`, INR, `Asia/Kolkata`, business day from 06:00, financial year from
April, the default settings and the built-in roles. Then, as `actor{owner, "dev-seed"}`, it
registers one `paired` device, "Dev Kiosk" (`device.register`), and the four staff members
below (`staff.upsert`), and prints the device id and token and the staff credentials once. Only
argon2id hashes are stored. On exit it releases the lease (it expires immediately), so a real Hub
can start on the same data dir straight away.

Dev staff (`dev-fixtures` builds only; never in a release build):

| Login     | Role    | Password           | PIN  |
| --------- | ------- | ------------------ | ---- |
| `owner`   | Owner   | `dev-owner-pass`   | 1111 |
| `manager` | Manager | `dev-manager-pass` | 2222 |
| `counter` | Counter | `dev-counter-pass` | 3333 |
| `kitchen` | Kitchen | `dev-kitchen-pass` | 4444 |

With the same feature, `arena-hub dev revoke-device <device_id> [--health-port <port>]` asks
the running Hub to revoke a device through the dev-only loopback endpoint
`POST /dev/revoke-device` on its health port (absent without the feature). The Hub applies
`device.revoke` (with the body's `reason`, `other` by default; an unknown reason is
`VALIDATION_FAILED` on `reason`) and closes that device's sessions.

| Exit code | Meaning                                                                        |
| --------- | ------------------------------------------------------------------------------ |
| 0         | The branch was bootstrapped                                                    |
| 1         | `ALREADY_BOOTSTRAPPED`, `NOT_PRIMARY` (another process holds the db), or other |
| 2         | `SCHEMA_TOO_NEW`: the database is newer than this binary                       |

| Exit code | Meaning                                                              |
| --------- | -------------------------------------------------------------------- |
| 0         | Clean shutdown, `--help` or `--version`                              |
| 1         | Runtime failure (logs, store, port bind, or the healthz server died) |
| 2         | `SCHEMA_TOO_NEW`: the database is newer than this binary             |
| 64        | Bad command-line arguments                                           |

## The Hub UI

`apps/hub` is the staff app shell: an 88px left rail (Floor view · POS · Players · Sessions ·
Inventory · Shift · Reports · Settings · System), a 72px header (branch name and clock) and the
main area. Below 840px wide the rail becomes a 64px bottom bar (Floor · POS · Players · Shift ·
More, where More opens a bottom sheet with the rest). Ctrl+1–4 open Floor, POS, Players and
Shift, except while a text field has focus. `/` redirects to `/floor`; screens not built yet show
a placeholder. The root layout reads `/ui/snapshot` and `/ui/me` before the first render; if the
Hub service does not answer, it shows an error state with Retry. With nobody signed in every
screen redirects to `/sign-in` (login and password, one generic failure message, a live
countdown while locked out); after sign-in it opens `/floor`, and the header shows the staff
member's name and role with a Sign out button. Settings has sections: **Branch** (currency with
its symbol, symbol position, number format, time zone, business day start and financial year
start month, with a live preview of 123456.78; Save submits one `settings.set` per changed field
in order, each with its `base_rev`; field errors show inline, a STALE save shows the current
value and asks to check it; a pending tz or day-start change says "Takes effect from {date}";
without `settings.edit` the fields stay editable, Save says "Needs manager approval" and opens
the `InlinePin` in place, listing the fields it approves: the PIN asks `/ui/elevate` for one
ticket covering exactly the changed fields, each `settings.set` carries it, unused approvals are
dropped, and focus returns to Save; a lapsed approval asks again without a round trip; Esc clears the PIN and
returns focus, and a `PIN_REQUIRED` answer asks for the PIN again), **Receipt** (the
"Receipt footer" control from Story 1.11) and **Screen lock** (`/settings/screen-lock`: the idle
lock in minutes with "1–30 minutes" shown inline, checked before any round trip, saved with
`settings.set` and the same manager approval for a Counter) and **Devices & display**
(high contrast per device; see [High contrast per device](#high-contrast-per-device)). **System → Audit**
(`/system` opens `/system/audit`): filters for who (staff names from `/ui/audit/facets`, plus
System, Owner and Devices), module and a from/to date pair in the branch time zone (`to`
inclusive, sent as the next day's start), with Clear filters resetting them all; a table of
time (`formatDateTime`, branch tz), actor name and kind, module, action and record, newest
first, with Load more and the next page loading as the end scrolls into view; "No changes match
these filters." when empty. Activating a row (Enter or click) opens the details in the page:
Before and After side by side, each changed field marked with the text "Changed" and an
`aria-label`, "Created" in the before column of a created row, a "Deleted" marker on a delete, secrets shown as "Hidden", and
"Approved by {manager} ({mode})" on an elevated change; Esc or Close returns focus to the row.
There are no edit, delete or export controls. A member without `audit.view` still sees the
entry: it opens the `InlinePin` in place, and only after a manager's PIN does it load, with the
read ticket held in the page's memory; a cancel fetches and shows nothing, and a lapsed ticket
clears the screen and asks again. Every signed-in view listens to `/ui/events`: a
`settings.changed` updates the branch locale (the header clock's time zone, formatted amounts
and a Settings form with no unsaved edits) and the idle lock in every open window without a
reload. **Idle lock:** `src/lib/idle.js` counts input idleness (`keydown`, `pointerdown`,
`pointermove`, `wheel` and `touchstart` on `window`, throttled to one reset a second, one
`setTimeout`); a period's length is read when it starts, so a new `idle_lock_minutes` applies
from the next period and never shortens the countdown in progress. On `visibilitychange`,
`focus` and `pageshow` it compares the wall clock with the last reset and locks at once if the
machine slept past the period; a clock that went backwards restarts the period. Tracking stops
while the shell is gated. When it runs out (signed-in
screens only, never sign-in) `LockOverlay` covers the viewport (`role="dialog"`,
`aria-modal`, an h1 "Locked", the staff member's name, an `InlinePin` and Sign out) while the
shell behind it stays mounted with `inert` and `aria-hidden`, so the route, scroll position and
unsaved fields survive. Digits, Backspace and Enter go to the PIN, Esc clears it without
unlocking, Tab reaches Sign out, and five wrong PINs show the countdown. A member without a PIN is told
to sign out. The overlay shows at once; the `/ui/lock` call is retried with a back-off while the
Hub does not answer, and an unlock waits for it. The overlay also shows over the unreachable
state. On unlock the UI reloads the snapshot and session (another member's PIN switches the
header to them and announces "Now signed in as {name}"), removes `inert` and returns focus to
what had it. A `SESSION_LOCKED` answer to any call, a `session.locked` event, a session the Hub
holds locked, or this window's own `arena.hub.locked` sessionStorage flag after a reload all
show the lock; a `session.unlocked` event lifts it in every window of the session. Money is
formatted only through `formatMoney(minor, locale)` from `@arena/ui/format` (exact integer
maths; Indian or international grouping; a leading `−` for negatives), paired with the
`.tabular-nums` class.

```sh
cargo run -p arena-hub --features dev-fixtures -- dev seed --data-dir /tmp/hub
cargo run -p arena-hub -- --data-dir /tmp/hub      # serves 127.0.0.1:7409
pnpm --filter @arena/hub dev                       # http://localhost:5173: sign in as counter / dev-counter-pass
pnpm --filter @arena/hub test:shell                # Playwright: builds, then runs on vite preview
```

- **Tauri shell (`arena-hub-ui`).** The window starts hidden. On start the shell probes
  `127.0.0.1:7409/healthz`; when nothing healthy answers it spawns `arena-hub` from next to its
  own executable (plain `std::process::Command`, no plugin, with `--data-dir`, detached so the
  service keeps running when the UI closes) and waits up to 10 s for it (stopping early if it
  exits), then tells the page to re-read the snapshot and shows the window. Failures go to
  `<data dir>/logs/hub-ui.log`. The UI also re-reads the snapshot on window focus, and while the
  Hub is unreachable it retries on its own (2, 4, 8 … up to 30 s apart). Install and uninstall
  stop a running `arena-hub.exe` first. The CSP adds
  `connect-src 'self' http://127.0.0.1:7409`.
- **Installer.** `src-tauri/tauri.installer.conf.json` is merged in for the installer build only:
  NSIS bundle, `bundle.externalBin: ["binaries/arena-hub"]` and the NSIS hooks in
  `src-tauri/windows/hooks.nsh`. It is not in `tauri.conf.json` because tauri-build would then
  need the binary for every workspace build and copy it over `target/<profile>/arena-hub`. The
  hook aborts in `NSIS_HOOK_PREINSTALL` on Windows builds older than 17134 with "Arena 360 Hub
  needs Windows 10 version 1803 or later." (a copy of the catalogue key
  `hub.installer.windows_too_old`; `scripts/installer-hooks.test.mjs` checks they match). To
  build it on Windows:

  ```sh
  cargo build --locked --release -p arena-hub
  mkdir apps/hub/src-tauri/binaries   # gitignored
  cp target/release/arena-hub.exe apps/hub/src-tauri/binaries/arena-hub-x86_64-pc-windows-msvc.exe
  pnpm --filter @arena/hub tauri build --config src-tauri/tauri.installer.conf.json --bundles nsis
  ```

- **Release benchmark.** `.github/workflows/release-bench.yml` (manual, and on `v*` tags; not
  part of `ci-ok`) builds the installer and a `dev-fixtures` `arena-hub`, then on the
  `[self-hosted, windows, reference-i3]` runner installs the Hub silently, seeds a branch and runs
  `scripts/bench/hub-idle.ps1`. After 5 minutes idle it requires `arena-hub` under 50 MB working
  set, the Hub UI plus its WebView2 processes under 250 MB, and 0 % GPU engine use by every Hub
  process.

`@arena/ui` has `InlinePin` (PINs of `minLength`–`maxLength` digits, 4–6 by default: that many
cells over a visually hidden numeric input, plus a keypad; digits, Backspace, Enter and Esc from
the keyboard, and the last cell submits on its own; Esc and Cancel wait while a PIN is checked; the PIN is cleared the moment it is
submitted; a live region announces "2 of 4", never the digits; a lockout shows a countdown)
and `ReasonPicker` (radio options from catalogue keys, then Confirm; a refusal shows inline),
both in the gallery with a stubbed `device.revoke` flow that asks for the reason, then the PIN.

## Running the Kiosk

The Kiosk is two processes with a privilege split:

- **`arena-kiosk-svc`** (LocalSystem Windows service) holds the device token, the pinned-TLS
  Hub connection and the offline intent queue. It is the only part that talks to the Hub.
- **`arena-kiosk-ui`** (Tauri, `apps/kiosk`) runs as the standard `player` account. It talks
  only to the service, over an ACL'd named pipe; its webview has no Hub socket, no token and no
  admin rights. The window is the player's shell (see [Kiosk lockdown](#kiosk-lockdown)).

`arena-kiosk-core` holds the platform-neutral parts both use: the Hub client, the reconnect
back-off, the IPC messages, the watchdog state machine and the queue ordering rules.

| Command                                                                               | What it does                                                                      |
| ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `arena-kiosk-svc --console [--data-dir <path>] [--app <path>]`                        | Runs in the foreground on any OS (dev; `--app` is relaunched)                     |
| `arena-kiosk-svc --console --dev-fake-procs [...]`                                    | `dev-fixtures` builds only: also enforce against a fake process table (see below) |
| `arena-kiosk-svc service install` / `uninstall` / `run`                               | Windows: register (auto-start, LocalSystem), remove, SCM entry                    |
| `arena-kiosk-svc lockdown apply\|restore [--player-user <name>] [--data-dir <path>]`  | Windows, elevated: lock the player account down / put it back                     |
| `arena-kiosk-svc dev provision --hub <host:7400> --fingerprint <hex> --token <token>` | `dev-fixtures` builds only: write the config and the token                        |

- **Data dir:** `--data-dir`, else `%PROGRAMDATA%\Arena360\Kiosk` on Windows, else
  `ARENA_KIOSK_DATA_DIR`, else `~/.arena360/kiosk`. It holds `config.json`
  (`{hub_addr, hub_fingerprint, device_id, player_user}`, default `player_user` `player`; an
  IPv6 `hub_addr` must be bracketed; a file carrying a token is refused), `token.bin`,
  `entropy.bin` and `lockdown-backup.json` (Windows), `queue.db`, `logs/` (tracing JSON, the Hub's 14 d / 50 MB ring with a
  `kiosk-` prefix) and, off Windows, the socket `ipc/kiosk.sock` (0600, in a 0700 directory).
  Files are written through fresh `create_new` temp files and a rename.
- **Data dir ACL (Windows).** At every service start and in `service install` the directory
  gets the protected DACL `D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)` (SYSTEM and Administrators
  only, nothing inherited from `ProgramData`), read back and verified; the service does not
  start when it cannot be set. A `config.json`, `token.bin` or `entropy.bin` not owned by
  SYSTEM or Administrators is refused.
- **Token at rest.** Windows: DPAPI with machine scope (`CryptProtectData` +
  `CRYPTPROTECT_LOCAL_MACHINE`) plus optional entropy from `entropy.bin` (32 bytes from
  `BCryptGenRandom`, generated once inside the protected folder), so only SYSTEM and
  Administrators can decrypt it; the token is never written unencrypted. Elsewhere (dev and
  tests only) a 0600 file, with a `warn` line saying it is not encrypted.
- **Hub link.** `hello{class: paired, versions: {app: <crate version>}}` over pinned TLS. A
  certificate that does not match the pin is refused with an `error` line naming both
  fingerprints; there is no unpinned fallback. An unreachable Hub is retried with full-jitter
  exponential back-off between 2 and 30 s, reset by a successful hello, and the app shows
  "Reconnecting…". `TOKEN_REVOKED` / `TOKEN_INVALID` (at hello or pushed later) stop fast
  reconnects: the app is told `token_rejected` and the service tries once every 30 s until a
  re-provision lets it in (the config and token are re-read on every try).
- **Offline queue.** Kiosk events are `kiosk.log` intents whose ULID ids and `issued_at` are set
  when the event happens, stored in `queue.db` (at most 10 000; past that the oldest are dropped
  with a `warn`). On every connect (and whenever an event is queued while connected) the queue
  is replayed oldest first, one at a time: `Applied` or a deterministic refusal deletes the
  entry. A transient error (`HUB_SLOW` — including the Hub's 60-a-minute per-device `kiosk.log`
  limit —, `NOT_PRIMARY`, `UNSUPPORTED`, `BAD_FRAME`, a closed link…) or a `VALIDATION_FAILED`
  on `issued_at` (this kiosk's clock is ahead; logged once per connection with the skew the Hub
  measured) keeps the entry and stops the replay until the next connect, or until a 30 s timer
  set when it stopped. Entries older than 7 days are dropped with a `warn` line. The Hub's
  dedupe makes a replay after a crash apply once. An app `log` request is checked against the
  Hub's `kiosk.log` rules first (`VALIDATION_FAILED` in the reply).
- **IPC.** JSON lines (`arena_kiosk_core::ipc::IpcMsg`): `ping{n}`/`pong{n}`,
  `hub_status{state: connected|reconnecting|token_rejected}`, `app_state{json}`,
  `restore_state{json}`, `request{rid, kind, body}` / `reply{rid, body, error?}` (kinds: `log`,
  `hub_status`). Windows: `\\.\pipe\arena-kiosk-v1`, created first-instance-only with remote
  clients rejected and the DACL `D:P(A;;GA;;;SY)(A;;0x12018b;;;<player SID>)`: SYSTEM, and the
  player (a `SidTypeUser` account) with `FILE_GENERIC_READ | FILE_WRITE_DATA |
FILE_WRITE_ATTRIBUTES` only, so it cannot create pipe instances; no Everyone, Users,
  Authenticated Users, Interactive, Anonymous or Administrators. The service accepts a client
  only when its process image (`GetNamedPipeClientProcessId`) is the installed
  `arena-kiosk-ui.exe`; the app accepts the pipe only when its server runs as SYSTEM
  (`GetNamedPipeServerProcessId`, else the pipe owner). Every 30 s the service re-reads
  `player_user` and rebinds when it changed or did not resolve (until then the pipe is
  SYSTEM-only). Elsewhere: the socket. One app connection at a time; the newest wins. The Tauri
  shell forwards `hub_status` to the webview as the event `hub-status` (`KioskHubStatus`) and
  the first `restore_state` of the process as `restore-state`, and sends the webview's latest
  route up as `app_state` on every (re)connect. The event and command names are in
  `apps/kiosk/src-tauri/ipc-names.json`, read by both the shell's tests and the webview.
- **Watchdog.** The service pings the app every 2 s. The shell answers only while the webview's
  `heartbeat` command (every 1 s) is at most 3 s old, so a hung webview counts as hung. After 3
  missed pongs (or no app connected for 3 ticks after a 10 s grace) the service relaunches the
  app, and restarts back off (2, 4, 8 … up to 60 s more each time until a pong). It launches
  only into a console session whose user is `player_user` (`WTSGetActiveConsoleSessionId` +
  `WTSQueryUserToken` + `CreateProcessAsUserW`), suspended inside a kill-on-close job object;
  a restart terminates that job (WebView2's children included) and waits for it to exit. While
  there is no player session, or the pipe is not open to a resolved player, nothing counts as a
  miss and one `kiosk.log{warn, no_session}` is queued; each restart queues
  `kiosk.log{warn, app_restarted}`. The desktop is never exposed during the restart because the
  player's shell is the kiosk itself (Story 1.17's shell replacement, no Explorer behind it).
- **Service host.** `service install` protects the data folder, then creates the service (an
  existing one is reconfigured; one still marked for deletion is retried for up to 10 s), sets
  restart-on-failure and starts it; a service it created is deleted again if a later step
  fails. `service uninstall` treats only "not installed" as done; a service that does not stop
  within 15 s is an error and is not deleted.
- **Dev loop** (any OS):

  ```sh
  cargo run -p arena-hub --features dev-fixtures -- dev seed     # prints the device token
  cargo run -p arena-hub                                          # healthz shows tls_fingerprint
  export ARENA_KIOSK_DATA_DIR=/tmp/kiosk                          # the service and the app
  cargo run -p arena-kiosk-svc --features dev-fixtures -- dev provision \
    --hub 127.0.0.1:7400 --fingerprint <tls_fingerprint> --token <device token>
  cargo run -p arena-kiosk-svc -- --console
  pnpm --filter @arena/kiosk tauri dev                           # connects to ipc/kiosk.sock
  ```

  The app finds the socket through `ARENA_KIOSK_DATA_DIR` (else `~/.arena360/kiosk`): when the
  service runs with `--data-dir <path>`, set `ARENA_KIOSK_DATA_DIR=<path>` for the app too.

- **Installer.** `apps/kiosk/src-tauri/tauri.installer.conf.json` (installer build only): a
  per-machine NSIS bundle with the WebView2 bootstrapper embedded
  (`webviewInstallMode: embedBootstrapper`), `arena-kiosk-svc` as an `externalBin`, and
  `src-tauri/windows/hooks.nsh`. The hooks refuse Windows builds older than 17134, a 32-bit OS
  and less than 4 GB of installed RAM (`GetPhysicallyInstalledSystemMemory`), each with the
  text of its catalogue key (`kiosk.installer.*`; `scripts/kiosk-installer.test.mjs` checks
  them). Before an install or uninstall they run `arena-kiosk-svc service uninstall`; after
  install, `service install` then `lockdown apply` (the account comes from the config, not a
  hard-coded flag). A failed service install or lockdown **aborts** the install with a non-zero
  exit, even in silent mode, so a half-secured kiosk is never left behind. Before uninstall the
  hooks run `lockdown restore` first (while the exe is still present); a failed restore falls
  back to `lockdown reset-shell` so the player's Shell is not left pointing at a removed exe. An
  in-place update (Tauri passes `/UPDATE` to the old uninstaller) **skips** the restore, so the
  player is never handed a desktop between uninstall and reinstall. Each failure is reported
  with its `kiosk.installer.*` catalogue text. CI builds it in the `installer (kiosk, windows)`
  job (part of `ci-ok`); locally on Windows:

  ```sh
  cargo build --locked --release -p arena-kiosk-svc
  mkdir apps/kiosk/src-tauri/binaries   # gitignored
  cp target/release/arena-kiosk-svc.exe apps/kiosk/src-tauri/binaries/arena-kiosk-svc-x86_64-pc-windows-msvc.exe
  pnpm --filter @arena/kiosk tauri build --config src-tauri/tauri.installer.conf.json --bundles nsis
  ```

- **Release benchmark.** The `kiosk-idle` job of `release-bench.yml` installs the Kiosk on the
  reference i3 and runs `scripts/bench/kiosk-idle.ps1`. The machine's console must be signed in
  as the player account and the runner must be an administrator (the data folder is
  SYSTEM/Administrators only). The script lets the service's watchdog launch the app into the
  player's session and waits for the service's `app connected` log line; after 5 minutes idle
  with no player (game) session, the service, the app and its WebView2 processes together stay
  under 80 MB working set, with 0 % GPU engine use.
- **Tests.** `cargo test -p arena-kiosk-svc` drives the service against an in-process Hub LAN
  server (connect and online, pin mismatch, Hub down and back, offline queue order and
  exactly-once, a replay after a crash, a follower Hub keeping the queue until a primary applies
  it, revocation and re-provision, the watchdog with a mock launcher, newest connection wins).
  On Windows CI the pipe tests check the DACL read back from a real pipe, that a second
  instance is denied, the client image check and sequential connects. `pnpm --filter
@arena/kiosk test:ui` (Playwright, the Tauri shell stubbed in the page) covers the status
  line, the heartbeat and state restore.

## Kiosk lockdown

Only the `player` account is locked down; administrators, other accounts and HKLM policy are
never touched. The plan is pure (`arena_kiosk_core::lockdown`: `plan_apply`, `capture_backup`,
`plan_restore`) and is run by `arena-kiosk-svc lockdown apply|restore`, which the installer
calls elevated.

**What is set,** in the player's own hive (keys relative to `HKEY_USERS\<player SID>`):

| Key                                                           | Value                    | Data                                         |
| ------------------------------------------------------------- | ------------------------ | -------------------------------------------- |
| `Software\Microsoft\Windows NT\CurrentVersion\Winlogon`       | `Shell` (REG_SZ)         | `"<install dir>\arena-kiosk-ui.exe"`         |
| `Software\Microsoft\Windows\CurrentVersion\Policies\System`   | `DisableTaskMgr`         | `1`                                          |
|                                                               | `DisableLockWorkstation` | `1`                                          |
|                                                               | `DisableChangePassword`  | `1`                                          |
|                                                               | `DisableRegistryTools`   | `1` (no regedit/reg.exe for the player)      |
| `Software\Microsoft\Windows\CurrentVersion\Policies\Explorer` | `NoRun`                  | `1`                                          |
|                                                               | `NoClose`                | `1`                                          |
|                                                               | `NoLogoff`               | `1`                                          |
|                                                               | `NoDriveTypeAutoRun`     | `0xFF` (autorun off on every drive type)     |
|                                                               | `NoAutorun`              | `1`                                          |
|                                                               | `NoDrives`               | `0x03FFFFFB` (every letter but C: hidden)    |
|                                                               | `NoViewOnDrive`          | `0x03FFFFFB` (every letter but C: refused)   |
| `Control Panel\Accessibility\StickyKeys`                      | `Flags` (REG_SZ)         | `"506"` (the Shift×5 hotkey and feature off) |
| `Control Panel\Accessibility\Keyboard Response`               | `Flags` (REG_SZ)         | `"122"` (the hold-Shift hotkey off)          |
| `Control Panel\Accessibility\ToggleKeys`                      | `Flags` (REG_SZ)         | `"58"` (the NumLock×5 hotkey off)            |

Policy values are REG_DWORD; the accessibility `Flags` are REG_SZ. The accessibility hotkeys
otherwise open a dialog that escapes the kiosk. With the kiosk as the shell, Explorer never
starts for the player, so there is no desktop, taskbar or Start menu behind the app, even while
the watchdog restarts it.

**Protected keys.** After the values are set, the DACL of `…\Winlogon` and the two
`Policies\…` keys is tightened to SYSTEM + Administrators full control and the player read-only
(`D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;<player SID>)`, inheritance broken), so the player
cannot re-enable anything by editing its own hive; combined with `DisableRegistryTools` it has
no registry path to the lockdown. Each key's previous security descriptor (and whether the key
existed at all) is recorded in the backup for restore.

- **`lockdown apply`** (the account comes from the provisioned `config.json`'s `player_user`,
  or `--player-user <name>`; `player` by default): validates the name (1–20 chars, the SAM
  limit, no trailing dot), refuses the shell unless it exists and is named `arena-kiosk-ui.exe`,
  makes sure a local standard account exists (created with `NetUserAdd` and a random
  32-character password that is never stored or shown; auto-logon is not set up yet) with a
  profile (`CreateProfile`), and refuses an administrator **or** a member of Administrators,
  Power Users, Backup Operators, Remote Desktop Users or Hyper-V Administrators (matched by the
  group SID, so it works in any UI language). It then opens the player's hive: `HKEY_USERS\<SID>`
  when the player is signed in, else its `NTUSER.DAT` loaded under
  `HKEY_USERS\ArenaPlayerTemp-<pid>` (`RegLoadKeyW`, with `SeRestorePrivilege` and
  `SeBackupPrivilege`) and unloaded afterwards. A machine-wide mutex (`Global\ArenaKioskLockdown`)
  serializes the hive work across a service start and an installer run. The shell path is the
  configured `app_path`, else `arena-kiosk-ui.exe` next to the service. The account name is
  qualified with the computer name for the SID lookup.
- **Backup.** Before anything is written, the previous value of every touched name (or
  "absent"), whether each key already existed, and each protected key's SDDL go into
  `%PROGRAMDATA%\Arena360\Kiosk\lockdown-backup.json` (the protected data folder; a backup not
  owned by SYSTEM or Administrators, or of another format version, is refused), with the account
  name and SID. A value of another type is kept byte for byte with its type code. Applying again
  keeps the first backup, so the originals are never replaced by the lockdown's own values; a
  backup for another account or SID is refused.
- **`lockdown restore`**: writes back exactly what the backup holds, deletes the names that were
  absent, puts each protected key's DACL back, and deletes any key the lockdown itself created
  that it left empty, then removes the backup (a failed restore keeps the backup for a retry).
  The account is kept. No backup: nothing to do; the account deleted since: the backup is
  dropped.
- **`lockdown reset-shell`**: the uninstaller's last resort when a restore fails — it points the
  player's `Winlogon\Shell` back at `explorer.exe` (and hands the Winlogon key's DACL back)
  so the PC is still usable, leaving the backup for a later proper restore.
- **Off Windows** every `lockdown` subcommand prints `lockdown is only available on Windows` and
  exits 2.
- **Takes effect at next sign-in.** The shell and policy keys apply when the player next signs
  in; apply/restore log a warning when the player is already signed in. Signing the player in is
  manual for now (auto-logon is a later story): at the counter, sign in once as `player` (the
  account has a random password nobody holds, so a Manager sets one, or configures auto-logon,
  until that story lands), after which Winlogon launches the kiosk as its shell. Sign the player
  out (or reboot) for an apply or restore to take hold.

**In the app** (`arena-kiosk-ui`, the player's session):

- **Launch ownership.** Winlogon launches the kiosk as the player's shell (no Explorer); the
  service's watchdog relaunches it only when it hangs or crashes. A machine-wide mutex
  (`Global\ArenaKioskUi`) makes a second instance exit at once, so Winlogon and the watchdog can
  never leave two running; a restart kills the previous one (its kill-on-close job, plus
  `taskkill` by image name).
- The window is undecorated, not resizable, `alwaysOnTop`, `skipTaskbar` and starts hidden. At
  start and on every `WM_DISPLAYCHANGE`/`WM_DPICHANGED` it is moved over the union of all
  monitor rectangles (`EnumDisplayMonitors`; the union is `arena_kiosk_core::monitors::union`),
  re-asserts topmost and is shown; when another window steals the foreground (`WM_ACTIVATEAPP`)
  it re-asserts topmost and focus. When the union cannot be computed it falls back to the primary
  monitor's rect, and when no monitor is reported at all to real fullscreen. A close request is
  always refused (`CloseRequested` → `prevent_close`). The locked placeholder is positioned over
  the primary monitor, not stretched across every screen.
- A `WH_KEYBOARD_LL` hook on its own thread (with its message loop) runs every key through
  `arena_kiosk_core::keys::KeyFilter`, resynced from `GetAsyncKeyState` on every event so a
  missed key-up or a pre-held modifier cannot stick: it swallows both Windows keys and any chord
  with them, Alt+F4, Alt+Tab, Alt+Esc, Ctrl+Esc and Alt+Space, and always passes Ctrl+Alt+Shift+A
  (reserved for the admin exit, Story 1.20). Ctrl+Alt+Del cannot be hooked; the policy keys take
  Task Manager, lock, log off and change password off its screen. The install retries with
  backoff and the hook is re-installed every 60 s (a silently removed hook comes back); while it
  is not active the app reports it over IPC and the service queues
  `kiosk.log{error, lockdown_degraded}`. Even without a window the hook is still started.
- **WebView2** is hardened (`with_webview`): no default context menus, no browser accelerator
  keys (Ctrl+P, Ctrl+R, Ctrl+F, F5, F12, Ctrl+Shift+I…), no devtools and no status bar. The
  frontend repeats the block as defense in depth (`contextmenu`, the same key combos and
  `window.print` are cancelled; kiosk pages carry no file picker), so a reload, print, find or
  devtools cannot escape the kiosk.
- With no session the app shows the locked placeholder ("This PC is ready. Log in at the
  counter or with your account.", `player.locked.*`).
- Debug builds only: `ARENA_KIOSK_DEV_UNLOCKED=1` skips the single-instance guard, the hook, the
  topmost placement and the close refusal, so a developer's desktop stays usable; it is ignored
  in release builds (a release-gated test asserts this).

The automated 30-minute escape suite (Story 1.19) verifies all of this on real Windows 10 and
11; CI here type-checks the Windows code and tests the pure plan (values, backup, created-key
cleanup, protected-key SDDL capture), the key filter and its resync, the monitor union and the
executor on an in-memory registry.

## Kiosk process allow/block list

The branch settings `kiosk.process_allow` and `kiosk.process_block` (JSON arrays of
`{path_glob, signer?}`, at most 500 rules each, edited with `settings.set` under
`settings.edit`) decide which programs may run in the player's session, and
`kiosk.process_mode` (`enforce` | `report`) decides whether a disallowed program is killed or
only reported. Every branch is seeded with the defaults in `arena-proto`
(`DEFAULT_PROCESS_ALLOW` / `DEFAULT_PROCESS_BLOCK`): the kiosk app and service (`{install}\…`,
signer `Arena 360`); the Windows session essentials under `{system}\System32` (signer
`Microsoft Windows`, including `userinit.exe`, which Winlogon runs at every sign-in, and
`backgroundTaskHost.exe`) and `{system}\SystemApps\*\TextInputHost.exe`; and WebView2's
`msedgewebview2.exe` (signer `Microsoft Corporation`) in either Program Files, the per-user
`C:\Users\*\AppData\Local\Microsoft\EdgeWebView\Application\*\` and a fixed-version runtime
anywhere under `{install}\**\`. The block-list names `cmd.exe`, `powershell.exe`, `pwsh.exe`,
`regedit.exe`, `taskmgr.exe`, `mmc.exe`, `explorer.exe`, `control.exe`, `msconfig.exe`,
`wscript.exe`, `cscript.exe` and `mshta.exe` at any path.

- **Mode and rollout.** A new branch (`branch.bootstrap`) starts in `enforce`. A branch that
  existed before these settings gets them from the `branch.seed_defaults` backfill in `report`:
  nothing is killed and no toast is shown, but every program that enforce mode would kill is
  sent to the Hub as `kiosk.process_blocked{…, mode: "report"}` (stored with the detail
  `<exe> (<reason>, report only): <path>`), once per running process. Check the Hub audit log
  for those rows, add allow rules for the café's games and launchers, and then switch the
  branch to enforcement with `settings.set{key: "kiosk.process_mode", value: "enforce"}`
  (`base_rev` the setting's current `rev`, permission `settings.edit`); kiosks apply it live.
  A kiosk that has never received a mode from its Hub (or whose cached copy is damaged) also
  only reports.
- **Rules.** `path_glob` is matched case-insensitively against the normalised full path: `*` is
  any run within one folder, `**` any run across folders, `?` one character. Two placeholders
  may start a rule (and nowhere else): `{install}` (the kiosk's install directory) and
  `{system}` (`%SystemRoot%`, so the defaults work when Windows is not in `C:\Windows`).
  `signer` is the Authenticode subject CN a valid signature must carry.
- **Validation** (`settings.set` refuses with `VALIDATION_FAILED` on `value`, naming the rule;
  the kiosk re-checks pushed values and keeps its previous list on a refusal): well-formed JSON,
  at most 500 rules; allow rules must be full paths (`X:\…`, `{install}\…`, `{system}\…`); the
  allow-list may not be empty; block rules may be a bare file name or a full path but not a
  catch-all (only wildcards, optionally with `.exe`: `*`, `*.exe`, `C:\**`); a signer is 1–256
  characters with no leading or trailing space; an allow rule without a signer may not sit under
  a user-writable root (`X:\Users\`, `X:\ProgramData\`, `X:\Temp\`, `{system}\Temp\`, or a
  wildcard first folder). `kiosk.process_mode` is `enforce` or `report`.
- **Decision** (`arena_kiosk_core::procpolicy::decide`, pure): a block match kills; otherwise
  an allow rule must match the path and, when it names a signer, a valid Authenticode signature
  by exactly that CN. Nothing is ever allowed by file name alone, so a renamed `cmd.exe` is
  killed as not allowed.
- **Delivery.** The kiosk service asks `settings.get` for the three keys after every hello
  (giving up after 5 s), applies `settings.changed` pushes live, and caches them in `queue.db`
  (`kv`) with their revisions, so they apply offline and across restarts, and an older revision
  never replaces a newer one (the defaults apply until the Hub has ever answered).
- **Enforcement** (`arena-kiosk-svc` `procs`, Windows only). A WMI `Win32_ProcessStartTrace`
  subscription reports each new process (WMI only; there is no ETW path). The service resolves
  its session, token user and image path, verifies the signature when the decision depends on it
  (`WinVerifyTrust`, embedded or through the system catalogs, cached by path, size and mtime and
  verified once per file even under a burst) and calls `TerminateProcess` on a disallowed one.
  The WMI path aims at 200 ms from creation to kill (the elapsed time is a `debug` log line);
  this has not been measured yet and is confirmed on real Windows 10 and 11 by Story 1.19. When
  the WMI watch is down, a sweep every 500 ms is the fallback, so a kill can then take up to
  about 500 ms. Every running process is also swept at start, right after the watch
  (re)subscribes, within 2 s of a settings change and every 10 s. Creation events are buffered
  up to 1024 (beyond that a sweep is requested instead) and at most 8 checks run at once.
- **Safety rules.** Only processes in the player's console session whose token user is the
  configured player account are ever considered (never SYSTEM, LocalService, NetworkService,
  DWM/UMFD or any other user, so `csrss`, `winlogon` and `dwm` are out of reach); the configured
  kiosk app (its path re-read from `config.json` at every sweep) and the service image are never
  killed, and neither is a WebView2 process (`msedgewebview2.exe`) descending from the kiosk app,
  so the UI's runtime survives wherever it lives (every other program the app, as the player's
  shell, starts is decided by the lists as usual); critical images such as `svchost.exe` and `csrss.exe` under
  `System32` are never killed; a process whose details cannot be read is left alone (a `warn`
  line once per pid and reason); the kill re-checks the process on the handle it terminates.
- **Player and Hub.** After a kill the app shows "That app isn't allowed on this PC."
  (`player.app.blocked`) as a polite (`aria-live`), non-focus-stealing toast that stays until
  its Dismiss button is pressed; repeats collapse into it. The toast is only sent for a kill
  that followed a creation event of a process with a visible top-level window, or one started
  within the last 5 s by a process of the player's session; sweeps and background processes
  never toast (the session-0 service cannot see the player's windows, so on Windows the 5 s rule
  decides). Each kill (or report-mode find) is queued as `kiosk.process_blocked`, delivered
  through the offline queue exactly once and shown in the Hub audit log.
- **Caps.** The same path and reason within 5 s is reported once; at most 30
  `kiosk.process_blocked` a minute leave a kiosk, and the rest are counted in one
  `kiosk.log{warn, process_blocked_summary, "N more"}` a minute (the Hub's 60-a-minute limit is
  shared by `kiosk.log` and `kiosk.process_blocked`). The stored detail is capped at 500
  characters like a `kiosk.log` detail; a longer path is shortened from the left (`…`). A
  failed kill is warned about once per pid and path and reported once as
  `kiosk.log{error, kill_failed}`.

There is no staff GUI for the lists yet; edit them with `settings.set`. The fake process table
(`procs::fake`) drives the service tests; the Windows code is type-checked in CI and exercised by
the Story 1.19 escape suite on real Windows.

## Kiosk admin exit

A manager can drop a locked-down kiosk to the desktop for 15 minutes (`ADMIN_EXIT_MS`, shared by
`arena-proto` and `arena-kiosk-core`), three ways. Every exit is a `device_admin_exits` row
(migration 0010) shown in the audit viewer with the kiosk, the approving manager, the mode, the
start and the re-lock time, and a "Flagged for review" marker when flagged.

- **At the PC, Hub up (`hub`).** Ctrl+Alt+Shift+A (passed by the 1.17 keyboard hook; nothing on
  screen mentions it) opens an `InlinePin` titled "Admin exit". The PIN goes over IPC
  `admin_exit{pin}` to the service only. It is never stored or logged, and the service clears its
  copy after the check on a best-effort basis (IPC and JSON buffers may still hold transient
  copies until they are freed). The service sends
  `kiosk.verify_admin_pin{pin_proof}` over the pinned-TLS LAN link; the Hub checks it with
  argon2id against at most 50 active staff whose role allows `device.remote` and answers
  `kiosk.admin_pin_ok{staff_id, verifier_version}` or `PIN_REJECTED` (five wrong in a row pause
  that device for 60 s, `AUTH_LOCKED`). **Trade-off:** `pin_proof` is the PIN itself. It never
  leaves the pinned TLS link between this kiosk and its Hub and is never stored or logged, the
  same way the Hub UI's loopback `/ui/elevate` carries it; "the PIN never leaves the screen" holds
  in spirit, not to the letter.
- **At the PC, Hub down or slow (`cached_verifier`).** The Hub pushes the manager verifier
  (`event{kind:"verifier", verifier}`: the staff ids, argon2id PIN hashes and PIN versions of the
  same staff, never a PIN) to paired sessions after every hello and whenever it changes; browser
  classes never get it. The service keeps the highest version in `verifier.bin` (DPAPI, machine
  scope, on Windows) and checks the PIN against it off the async threads. The version is
  `max(pin_verifier_version) + hub_meta.verifier_counter`, bumped on any change to that list, and
  `verifier_history(version, valid_from)` records when each took effect.
- **Remotely (`remote`).** The floor's "Remote unlock" on a paired device submits
  `device.admin_unlock{device_id}` (`device.remote`; a Counter gets the manager PIN in place).
  An offline device is refused at once. The Hub pushes `event{kind:"admin_unlock", exit_id}` to
  that device; the kiosk grants it and answers `kiosk.admin_unlock_ack{exit_id}`, which reaches
  the Hub UI as `event: device.admin_unlock_ack` on `/ui/events`. Without it within 5 s the floor
  says "Failed — retry".

The kiosk records each exit as `device.admin_exit{exit_id, mode}` (device actor, `elevated_by`
`{pin, staff_id, hub|cached_verifier, verifier_version}`; the only intent that accepts a
`cached_verifier` elevation and the only LAN intent whose `elevated_by` is kept), queued with its
ULID key and `issued_at` minted at the moment of the exit and replayed through the offline queue.
`apply()` requires the elevating member to hold `device.remote`; when the verifier version used is
older than the one in effect at `issued_at` the exit is still applied, `flagged = 1`,
`flag_reason = stale_verifier` (a manager removed since then is accepted only this way).

During an exit the service starts Explorer in the player's session (its own job object),
suspends process enforcement, and the app drops topmost and the keyboard lockdown and shrinks to a
corner badge "Admin exit: 14:59 left". At the end Explorer's job is ended, any `explorer.exe`
left in the player's session is closed, enforcement resumes with a sweep, the app covers every
screen again and `device.admin_relock{exit_id}` is queued. A service restart re-locks a recorded
exit at once. Five wrong PINs at the kiosk pause the prompt for 60 s with a countdown; the count
lives in the service, so restarting the app does not reset it. A failed attempt is logged with
the device and count (never the PIN) and queued as `kiosk.log{warn, admin_exit_failed}`.

## High contrast per device

High contrast (pure black and white, accent `#FFE14D`, 2px white outlines on interactive
surfaces, a 4px yellow focus ring, no shadows or translucency, instant motion: the generated
`-hc` tokens plus the `[data-theme="hc"]` rules in `@arena/ui/base.css`) is a branch setting per
device, set by a manager. There is no player-facing control, no light theme on Hub, Kiosk or
Kitchen (`scripts/no-theme-toggle.test.mjs` fails on a `theme-toggle`, a `data-theme="light"` or
a `ThemeToggle` import in their sources), and the OS high-contrast setting is not read. Art and
video (none yet) must be hidden in HC by the epics that add them.

- **Settings.** `display.hc.hub` is this Hub's UI; `display.hc.<device_id>` is one per paired
  device. Values are `"true"` or `"false"` (`SettingRule::Bool`, matched by the `display.hc.`
  prefix plus `hub` or a ULID). Bootstrap creates `display.hc.hub`, `device.register` creates the
  device's row (`false`) in the same change, and `branch.seed_defaults` backfills any missing row
  for the Hub and every non-revoked paired device. They are edited with `settings.set`
  (`settings.edit`, `base_rev` required); a Counter's change needs a manager's PIN in place.
- **Hub UI.** Settings → **Devices & display** (`/settings/devices-display`) lists "This Hub" and
  every non-revoked paired device by label, each with a 44px switch that saves its own row. The
  snapshot's `ui.hc` (from `display.hc.hub`) sets `data-theme` on `<html>` before the shell
  renders, and `settings.changed` on `/ui/events` switches it live.
- **Kiosk.** A paired session may read its own `display.hc.<device_id>` with `settings.get`
  (another device's key or the Hub's is `FORBIDDEN`), and a change is pushed only to that device
  in `settings.changed`. The service caches it in `queue.db` `kv` (same rev guard as the process
  lists) and sends IPC `theme{hc}` to the app on every connection and every change (`theme` is
  also a request kind). The Tauri window is configured hidden and not auto-created: at start the
  shell waits up to 1 s for the service's `theme`, else uses `<app local data>/theme.json` (saved
  on every change), builds the window with an initialization script that sets `data-theme` before
  any page script runs, and only then shows it. Later changes arrive as the `theme` event and
  apply without a restart.

## The branch with the WAN down

Story 1.22. The branch works with no internet; the connectivity probe (see
[Running the Hub](#running-the-hub)) is the only outbound connection anything makes.

- **Hub header.** While the probe says a link is down, the main header shows a `chip-offline`
  before the clock (the `chip-offline` tokens, inverted in HC): a ⚠ glyph and "Internet
  offline" or "Cloud unreachable", so colour is never the only cue. The state comes from the
  snapshot's `connectivity` and live from `connectivity.changed` on `/ui/events`; the existing
  polite live region (`hub-announce`) announces each change once ("Internet and cloud back
  online" when it clears). The chip disappears on the first probe that finds both up (within
  30 s). A newly opened stream gets the current state first, and the layout keeps whichever
  answer has the newer `checked_at`, so a slow snapshot never puts an old state back. The chip
  shows only while someone is signed in (the push stream is live). Nothing else in the UI
  changes.
- **Kiosk dev hooks** (`dev-fixtures` builds only, `arena-kiosk-svc/src/dev.rs`): with
  `--console --dev-fake-procs` the service enforces the process lists against an in-memory
  process table with a logged-on `player` (and, without `--app`, a launcher whose admin-exit
  desktop is a no-op). On the app channel, `dev.spawn_process {path}` starts a fake player
  process, `dev.hub_link {online}` holds the Hub link down (or releases it) as if the Hub were
  unreachable, `dev.relock` ends the running admin exit and `dev.status` answers
  `{hub_state, verifier_version, admin_exit_until, queued}`. Release builds answer every
  `dev.*` request `UNSUPPORTED`.
- **e2e driver** (`crates/arena-e2e`, `publish = false`): `arena-e2e run` drives a running
  Hub over its loopback HTTP API and a console kiosk over its app channel through every Epic 1
  flow: staff sign-in, a branch settings save, PIN elevation (a Counter's save refused, then
  approved with the Manager's PIN), idle lock and unlock, the audit list, the kiosk connected
  and online on the floor, the HC toggle reaching the kiosk, a process block reported to the
  Hub, and an admin exit through the Hub and one with the cached verifier while the link is
  held down. It keeps a ledger of the intents it caused. `arena-e2e check` proves `seq`
  gap-free per epoch, that no intent inserted the same row twice, and the applied intents per
  kind equal to the ledger (none lost or doubled), and writes the normalised change table (ids, epochs, timestamps and hashes masked;
  sorted, since kiosk events and UI intents interleave differently; the watchdog's own
  `app_restarted` / `no_session` logs left out). `arena-e2e compare` diffs two tables.
- **`scripts/wan-down/run.sh`** (Linux, passwordless sudo, `strace`, `curl`; CI job
  `wan-down`, part of `ci-ok`) builds the release binaries with the dev fixtures and the Hub UI,
  then runs a pass with the network up and one inside a network namespace with only `lo`
  (`ip netns`). Each pass seeds a fresh Hub, starts it and the console kiosk, runs the driver
  and the Hub UI smoke (`apps/hub/playwright.wan-down.config.ts`: sign-in, floor, settings
  against the real Hub, failing on any request that leaves loopback). In the namespace the
  Hub, the kiosk, the driver and the Hub UI smoke (vite preview and the browser) run under
  `strace -f -e trace=connect`, and the Hub must report
  `internet_offline`. It fails unless every flow passed, both normalised tables are equal, the
  ledger and `seq` checks pass, the Hub logged no errors, and
  `scripts/wan-down/check-trace.mjs` finds no `connect()` outside loopback and local socket
  families except the probe's endpoints, and those only in the Hub's own trace (the internet
  endpoints, plus whatever the cloud host
  resolves to on the runner; inside the namespace name lookups go only to `files` and a
  loopback resolver, so the cloud lookup normally fails there); every other trace must stay on
  loopback, except that the UI's may make Chromium's built-in IPv6 reachability check (a UDP
  connect to `[2001:4860:4860::8888]:443` that fails with ENETUNREACH in the namespace, so
  nothing is sent; no launch flag disables it), `CHROMIUM_IPV6_PROBE` in `check-trace.mjs`. Windows-only enforcement (the real lockdown and process killing) is out of this
  Linux job's reach; the Story 1.19 escape suite covers it.
- **`scripts/wan-down/soak.sh --hours <h>`** runs the Hub and the kiosk in the namespace and
  loops the driver for `h` hours, then restores the network while they run (a veth pair and
  NAT into the namespace) and asserts that the probe sees the internet again, that `seq` is
  gap-free and the ledger matches, and that the Hub logged no errors. CI runs it for 0.05 h
  (3 minutes) at the end of `run.sh`; `release-bench.yml`'s manual `soak` input runs 8 hours on
  a self-hosted Linux runner.

## Running arena-cloud

`arena-cloud` is the cloud control plane: one binary, run once per role. Story 2.1 ships
`--role control` (`--role all` runs every role this build has, which today is only `control`),
which connects to `gz_control` (Turso, Mumbai, `aws-ap-south-1`) over libsql,
applies the embedded migrations and serves `GET /healthz`. It is deployed to k3s behind
Cloudflare at `api.staging.arena360.cloud` (staging) and `api.arena360.cloud` (production); see
[`deploy/README.md`](deploy/README.md).

```sh
ARENA_CONTROL_DB_URL=libsql://gz-control-<org>.aws-ap-south-1.turso.io \
ARENA_CONTROL_DB_TOKEN=<token> \
cargo run -p arena-cloud -- --role control --listen 127.0.0.1:8080

# Locally against sqld (no token needed on loopback http):
sqld --http-listen-addr 127.0.0.1:8081 --db-path /tmp/gz_control.sqld &
ARENA_CONTROL_DB_URL=http://127.0.0.1:8081 cargo run -p arena-cloud -- --role control
```

| Flag / variable            | Meaning                                                                                    |
| -------------------------- | ------------------------------------------------------------------------------------------ |
| `--role control\|all`      | Required. Any other role exits 64 with "role not available yet"                            |
| `--role jobs --task <t>`   | Run one task and exit (see **Nightly backup**); `--task` without `--role jobs` exits 64    |
| `--dev`                    | Local dev mode (see **Local dev mode** below): no env, loopback only                       |
| `--data-dir <dir>`         | With `--dev` only: where `gz_control.db` lives (default `./.arena-cloud-dev`)              |
| `--listen <addr:port>`     | Listen address (default `0.0.0.0:8080`)                                                    |
| `--help`, `--version`      | Print usage or the version and exit 0                                                      |
| `ARENA_CONTROL_DB_URL`     | `libsql://` or `https://` (plain `http://` only for `127.0.0.1`/`localhost`); else exit 64 |
| `ARENA_CONTROL_DB_TOKEN`   | Turso auth token; may be empty only for loopback `http://`. Never logged                   |
| `ARENA_OTP_PEPPER`         | Sign-up code pepper, 32+ characters; missing or short exits 64. Never logged               |
| `ARENA_WEB_ORIGINS`        | Comma-separated browser origins allowed with credentials (exact match); missing exits 64   |
| `ARENA_SMS_PROVIDER`       | `gupshup` (`fake` only with `--dev`); else exits 64. Needs `ARENA_GUPSHUP_*` below         |
| `ARENA_SMS_GLOBAL_PER_MIN` | SMS sends per rolling minute across all phones (default 60; `--dev` 10000)                 |

- **Start-up.** Logs are JSON lines on stdout. A `gz_control` newer than the binary
  (`SCHEMA_TOO_NEW`) logs both versions and exits 2 without writing. An unreachable database is
  retried with backoff for up to 60 s, then the process exits 1.
- **Migrations.** `crates/arena-store/control-migrations/NNNN_*.sql`, embedded, numbered from 1,
  forward-only, separate from the Hub's. Each is applied with its `schema_migrations` row in one
  transaction; a concurrent runner (rolling update) that loses the race re-reads and carries on.
  `0001_baseline` creates no tables; `0002_owner_signup` adds `owners`, `otp_challenges`,
  `owner_sessions` and `rate_limits`; `0003_owner_mfa` adds `owner_mfa` and
  `owner_backup_codes`; `0004_owner_signin` adds `signin_challenges`, `owner_devices` and the
  owner's `sms_fallback`, `failed_signins` and `locked_until`. All their SQL lives in `arena_store::control`. libsql lives only in `arena-store`'s `control` feature, so
  `cargo tree -p arena-hub` never shows it, and it is built remote-only (no C SQLite core).
- **`GET /healthz`** always answers 200
  `{"role":"control","version":"0.1.0","gz_control":{"reachable":true,"checked_at":<ms>},"migration_version":4}`.
  Reachability comes from a background `SELECT 1` every 15 s (3 s timeout); requests never hit
  Turso, and a Turso blip shows `reachable: false` without restarting the pod.
- **Tests.** The `gz_control` tests use `ARENA_TEST_SQLD_URL` when set (CI's libsql-server
  service), else spawn `sqld` from `PATH`, else skip with a reason; `ARENA_REQUIRE_SQLD=1` turns
  the skip into a failure. The S3 tests (the MinIO round trip in `crates/arena-cloud/tests/backup.rs`)
  use `ARENA_TEST_S3_URL` (e.g. `http://127.0.0.1:9000` from the dev compose file, credentials
  `ARENA_TEST_S3_ACCESS_KEY_ID`/`_SECRET_ACCESS_KEY`, default the dev ones) and make their own
  bucket; unset, they skip with a reason unless `ARENA_REQUIRE_S3=1`. CI's Linux `rust` job runs
  a MinIO service with both set.
- **Error envelope.** Every failure on `/v1` (and any path other than `/healthz`) is the flat
  `AppError` JSON body `{"code","message","field"?}` with `content-type: application/json`:
  an unknown path is 404 `NOT_FOUND`, a wrong method 405 `UNSUPPORTED`, a body that is not
  valid JSON or doesn't match the request 400 `BAD_REQUEST` (naming `field` when known; 413 when
  over 64 KiB), a validation failure 422 `VALIDATION_FAILED`, and an internal error or a handler
  panic 500 `INTERNAL` with a fixed message. The detail of an internal error is only logged; no
  response body carries a backtrace, panic text, file path or Rust type name. Handlers take
  `ApiJson<T>` and return `ApiError` (`crates/arena-cloud/src/api/`).
- **One SQLite.** libsql is built remote-only (`default-features = false`, `remote` + `tls`), so
  it brings no SQLite C core: rusqlite's bundled `libsqlite3-sys` is the only SQLite C library
  in `arena-cloud`. `scripts/check-sqlite-linkage.sh` (Linux and macOS; run by the CI `rust`
  job on Linux) builds the binary and fails unless it defines exactly one `sqlite3_open_v2`, no
  `libsql_`-prefixed C symbols, and `cargo tree -p arena-cloud` has no `libsql-ffi`/`libsql-sys`.
  `--self-test` proves the rules fire on crafted `nm` listings.
- **Nightly backup** (Story 2.3). `arena-cloud --role jobs --task control-backup` runs once and
  exits: it dumps `gz_control` (`ControlDb::dump`: a SQL text with a `-- arena-control-dump v1`
  header and a JSON manifest of the schema version and each table's row count; tables, then
  one INSERT per row read 1,000 at a time (keyset paging by `rowid`, or by primary key for
  `WITHOUT ROWID`), the `sqlite_sequence` high-water marks, then indexes, views and triggers;
  one read transaction on the local backend, page-by-page reads on Turso;
  `sqlite_%`, `libsql_%` and `_litestream%` objects skipped; `schema_migrations` included),
  age-encrypts it to the X25519 public keys in `ARENA_BACKUP_AGE_RECIPIENTS` (comma-separated;
  the job never holds a private key; the file decrypts with the age 1.3.2 CLI too) and puts
  `gz_control/nightly/<UTC yyyy-mm-ddTHHMMSSZ>.sql.age` in the backup bucket. Then it deletes all
  but the newest 30 nightly keys, never the one it just wrote (other keys are never touched).
  Exit 0 on success, 1 on any failure (including a bad environment, or 25 minutes without
  finishing). A failure is logged with a stable code (`BACKUP_CONFIG_INVALID`,
  `BACKUP_DUMP_FAILED`, `BACKUP_ENCRYPT_FAILED`, `BACKUP_UPLOAD_FAILED`,
  `BACKUP_RETENTION_FAILED`, `BACKUP_TIMEOUT`) and, when `ARENA_ALERT_WEBHOOK_URL` is set, POSTed
  as Slack-compatible `{"text": …}` (10 s timeout) naming `ARENA_ENV`, the task and the code,
  never a secret or the error detail; a failed POST is logged and the exit stays 1. After a
  success it GETs `ARENA_BACKUP_HEARTBEAT_URL` when set (10 s timeout; a failed ping is logged and
  the exit stays 0), so a dead-man's-switch monitor catches failures that never reach the binary
  (OOM kill, deadline kill, image pull, a missing Secret). Setup and the run share the 25-minute
  limit, and the dump is written straight into the encryptor.
  It runs as the `arena-cloud-control-backup` CronJob (02:30 Asia/Kolkata, see
  [`deploy/README.md`](deploy/README.md#restore-gz_control)).

  | Variable                            | Meaning                                                              |
  | ----------------------------------- | -------------------------------------------------------------------- |
  | `ARENA_BACKUP_S3_ENDPOINT`          | `https://<account>.r2.cloudflarestorage.com` (`http://` on loopback) |
  | `ARENA_BACKUP_S3_BUCKET`            | The environment's own backup bucket                                  |
  | `ARENA_BACKUP_S3_ACCESS_KEY_ID`     | Bucket-scoped R2 API token. Never logged                             |
  | `ARENA_BACKUP_S3_SECRET_ACCESS_KEY` | Its secret. Never logged                                             |
  | `ARENA_BACKUP_AGE_RECIPIENTS`       | `age1…` public keys, comma-separated                                 |
  | `ARENA_ALERT_WEBHOOK_URL`           | Optional webhook (Discord: append `/slack`). Never logged            |
  | `ARENA_BACKUP_HEARTBEAT_URL`        | Optional heartbeat, GET after each success. Never logged             |
  | `ARENA_ENV`                         | Optional environment name in the alert (default `unknown`)           |

  S3 goes through `aws-sdk-s3` (region `auto`, path-style, `request_checksum_calculation =
WhenRequired`, the smithy client on rustls + ring, so no aws-lc-rs) and static credentials (no
  `aws-config`); the alert through `reqwest` on the workspace rustls + ring with webpki-roots.

- **Restore.** `arena-cloud ops restore-control (--from-key <key> | --from-file <path|->)
(--identity <age identity file> | --plaintext) [--dev [--data-dir <dir>]]` restores a dump into
  the database in `ARENA_CONTROL_DB_URL`/`ARENA_CONTROL_DB_TOKEN` (or the `--dev` file). It
  refuses a target that already has user objects (`RESTORE_TARGET_NOT_EMPTY`) and a dump newer
  than the binary (`SCHEMA_TOO_NEW`) before writing anything; a wrong identity fails before the
  target is opened. With foreign keys off on the target, it applies the schema, the rows (in
  transactions of 500 statements), the `sqlite_sequence` marks, checks every reference with
  `PRAGMA foreign_key_check` (`RESTORE_FOREIGN_KEY_VIOLATION` names the table), creates the
  indexes, views and triggers, and recounts every table against the manifest
  (`RESTORE_COUNT_MISMATCH` names the table). It prints the verified counts and exits 0, or exits
  1 with `CODE: reason`; a missing `ARENA_CONTROL_DB_URL` (or, for `--from-key`, an
  `ARENA_BACKUP_S3_*` variable) is `RESTORE_CONFIG_INVALID`, exit 1. Bad flags exit 64. A failure part-way leaves a partly restored database: use a fresh one.
  `--from-key` reads the backup bucket (`ARENA_BACKUP_S3_*`); `--from-file -` reads stdin, e.g.
  `age -d -i key.txt x.sql.age | arena-cloud ops restore-control --from-file - --plaintext`. The
  restored database is already at the dump's migration version. No deploy ever runs it
  (`pnpm check:deploy` checks the workflow).
- **Owner sign-up** (Story 2.4). The onboarding app (`apps/onboarding`, served at
  `start.arena360.cloud`, staging `start.staging.arena360.cloud`) signs a café owner up with a
  phone one-time code:
  - `POST /v1/signup/start {name, phone, email, cafe_name, city}` validates the form (name, café
    and city 1–80 characters after trim; phone E.164, `+` and 8–15 digits, `+91` needs 10 digits
    starting 6–9; email `local@domain.tld` within 254 characters; else 422 `VALIDATION_FAILED`
    with `field`), stores a challenge and sends `"<code> is your Arena 360 code. It expires in 5
minutes."`, answering 202 `{challenge_id, expires_in_s: 300, resend_after_s: 30}`. The answer
    is the same whether or not the phone has an account; no `owners` row exists before a correct
    code.
  - `POST /v1/signup/resend {challenge_id}` puts a new code on the same challenge (expiry and
    attempts reset) 30 s after its previous send at the earliest (`RATE_LIMITED`, 429, with
    `Retry-After`; of two concurrent resends one wins, the other is `RATE_LIMITED`). A used
    challenge, or one voided by 5 wrong codes, is `OTP_EXPIRED`: start again.
  - `POST /v1/signup/verify {challenge_id, code}`: a wrong code is 400 `OTP_INVALID`; the 5th
    wrong code, an expired (5 min), void or used challenge is 400 `OTP_EXPIRED`. A right code
    consumes the challenge, creates the owner (`mfa_state = pending`) if the phone has none and,
    in the same transaction, a session: 200 `{next: "account_setup"}` plus the cookies. An owner
    already `enrolled` gets `{next: "sign_in"}` and no session (the code alone never bypasses
    TOTP; sign-in is Story 2.6). A repeated verify of the used challenge from the browser that
    it signed in (its access cookie is that owner's) answers `account_setup` again.
  - `GET /v1/session` answers `{owner: {name, mfa_state, has_password, backup_codes_acked}}`
    (the last two since Story 2.5) or 401 `AUTH_FAILED`.
    `POST /v1/session/refresh` rotates the refresh token (new pair, the old one marked
    `rotated_at`, its access cookie no longer valid). A rotated token presented again within
    10 s (two tabs refreshing at once) is 401 with nothing revoked; later it revokes the whole
    session family (401).
  - Cookies: `arena_access` (32 random bytes, 15 min, `Path=/v1`) and `arena_refresh` (30 days,
    `Path=/v1/session/refresh`), both `HttpOnly; Secure; SameSite=Lax`, host-only. Tokens are
    stored only as SHA-256; codes only as HMAC-SHA256(`ARENA_OTP_PEPPER`, `"otp\0"` ‖ challenge
    id ‖ code), compared in constant time. Codes and tokens come from the OS CSPRNG. The form
    kept on a challenge is cleared once it is used or voided; challenges expired over 24 h and
    sessions whose refresh expired over a day ago are deleted when new ones are written.
  - Limits: at most 5 sends per phone per rolling hour (minute buckets in `rate_limits`, pruned
    after 2 h) and `ARENA_SMS_GLOBAL_PER_MIN` sends per rolling minute across all phones
    (second buckets, key `sms:global`), each send reserved atomically before the SMS goes out.
    Beyond either, 429 `RATE_LIMITED` with `Retry-After` until the oldest counted bucket leaves
    its window, and nothing is sent. Per-IP limits are Cloudflare rules
    ([`deploy/README.md`](deploy/README.md#6a-owner-sign-up-sms-pepper-web-origins-and-per-ip-limits)). A failed SMS is 503 `INTERNAL` and is not
    counted (a timeout, where delivery is unknown, stays counted); a failed first send deletes
    the new challenge, a failed resend keeps the previous code.
  - CORS: only origins in `ARENA_WEB_ORIGINS` get `Access-Control-Allow-Origin` and
    `-Allow-Credentials: true` (and `Retry-After` exposed); a preflight or a state-changing
    request from any other `Origin` is 403 `FORBIDDEN`. Every `/v1` response has
    `Vary: Origin`.
  - Messaging goes through `crates/arena-cloud/src/messaging/` (`Sms::send`): `fake` records
    messages in memory, `gupshup` posts the Gupshup Enterprise SMS API form (`method=SendMessage`,
    `mask` = sender, `dltTemplateId`). Tests never call Gupshup (a local stub checks the request);
    a live send is an operator check.
  - Logs name a phone only as `phone_tag` (its last 2 digits and the first 8 hex characters of
    HMAC-SHA256(pepper, `"tag\0"` ‖ phone)), never the name, email, code or a token
    (`crates/arena-cloud/tests/signup.rs` asserts it on captured logs).

  | Variable                        | Meaning                                             |
  | ------------------------------- | --------------------------------------------------- |
  | `ARENA_GUPSHUP_ENDPOINT`        | `https://enterprise.smsgupshup.com/GatewayAPI/rest` |
  | `ARENA_GUPSHUP_USERID`          | The Enterprise account user id                      |
  | `ARENA_GUPSHUP_PASSWORD`        | Its password. Never logged                          |
  | `ARENA_GUPSHUP_SENDER`          | The DLT-registered sender id (`mask`)               |
  | `ARENA_GUPSHUP_DLT_TEMPLATE_ID` | The DLT template id of the sign-up message          |

- **Account setup** (Story 2.5). After sign-up, `/account-setup` in the onboarding app makes the
  owner set a password, enrol an authenticator app and save 10 backup codes, in that order; the
  step comes from `GET /v1/session`, which now answers
  `{owner: {name, mfa_state, has_password, backup_codes_acked}}`. Every route needs the session.
  - `POST /v1/account/password {password}` → 204. 10–128 Unicode scalar values (shorter: 422
    `PASSWORD_TOO_SHORT`, longer: 422 `VALIDATION_FAILED`), and its lowercase form not on the
    bundled list of the 10,000 most common passwords (422 `PASSWORD_COMMON`), all with
    `field: "password"`. Stored only as argon2id (m = 19 MiB, t = 2, p = 1, PHC string). It is
    set once: again is 409 `ALREADY_SET` (changing it is a later story). The list is
    `crates/arena-cloud/data/common-passwords.txt`, the first 10,000 entries of
    `@zxcvbn-ts/language-common` (MIT; provenance, integrity and licence in its header, version
    in the pin table).
  - `POST /v1/account/mfa/totp/start` → `{secret, otpauth_uri, qr_svg}` (`no-store`). It needs
    a password (403 `PASSWORD_REQUIRED`) and `mfa_state = pending` (409 `ALREADY_SET`). Each
    call mints a new 20-byte secret (RFC 4648 base32, no padding) and replaces the unconfirmed
    one, so leaving and coming back gets a new QR code. The URI is
    `otpauth://totp/Arena%20360:%2B<phone digits>?secret=…&issuer=Arena%20360&algorithm=SHA1&digits=6&period=30`;
    the QR code is an SVG drawn by the `qrcode` crate.
  - `POST /v1/account/mfa/totp/confirm {code}`: RFC 6238 (HMAC-SHA1 on `ring`, 6 digits, 30 s,
    one step either side). A valid code enrols the owner and answers
    `{backup_codes: ["XXXXX-XXXXX", …10]}`, this once; anything else is 400 `TOTP_INVALID`
    (`field: "code"`) and the owner stays `pending`.
  - `POST /v1/account/mfa/backup-codes/ack` → 204 (enrolled only, else 403 `FORBIDDEN`).
    `POST /v1/account/mfa/backup-codes/regenerate {code}` (enrolled only) deletes every backup
    code and issues 10 new ones in one transaction. A code for a step at or before the last
    accepted one is refused (`TOTP_INVALID`): no replay.
  - Backup codes are 10 Crockford base32 characters, stored only as argon2id hashes; typed codes
    are normalised (uppercase, hyphens and spaces dropped, I/L → 1, O → 0).
  - The TOTP secret is stored only as AES-256-GCM ciphertext under `ARENA_MFA_KEY` (base64 of
    32 bytes; `--dev` uses a fixed dev key), with the owner id as AAD and a random nonce per
    start. A secret that doesn't open under the key is 500 `INTERNAL`, logged as
    `mfa secret did not decrypt` ([`deploy/README.md`](deploy/README.md#6b-owner-mfa-key)).
  - `api::gate::RequireSetupComplete` is the extractor every setup-wizard route takes: it passes
    an owner with a password, `enrolled` and codes acked, else 403 `SETUP_INCOMPLETE`. Under
    `--dev`, `GET /v1/dev/setup-gate` answers `{"setup_complete": true}` only behind it.
  - Logs carry the owner id only: never a password, secret, otpauth URI or code
    (`crates/arena-cloud/tests/mfa.rs` asserts it on captured logs).
  - Account → Security (`/account/security`) lets an enrolled owner regenerate the backup codes
    in a dialog that asks for a current authenticator code.

- **Sign-in** (Story 2.6). `/sign-in` in the onboarding app takes "Phone or email" and the
  password, then asks for a second factor on a browser that isn't trusted. Errors never say
  which part was wrong.
  - `POST /v1/signin {login, password}`. `login` is an E.164 phone (spaces, dashes, dots and
    brackets dropped; a bare 10-digit number is read as `+91`; `00` as `+`) or an email
    (case-insensitive). A malformed login or an empty password is 422 `VALIDATION_FAILED`. An
    unknown login, a wrong password or an owner without a password is 401 `AUTH_FAILED` with
    one fixed message; an unknown login still runs one argon2 verify against a fixed dummy hash.
    A correct password while the account is paused is 429 `AUTH_LOCKED` with `Retry-After`. A
    `pending` owner gets a session and `{next: "account_setup"}`. An `enrolled` owner with a
    valid `arena_trusted` cookie gets a session and `{next: "done"}`; otherwise the answer is
    `{challenge_id, methods}` (`totp`, `backup_code`, plus `sms` when the owner turned it on)
    and no session. A challenge lasts 5 minutes and takes 5 wrong codes.
  - `POST /v1/signin/sms {challenge_id}` (403 `FORBIDDEN` unless SMS fallback is on) texts a
    6-digit code, HMAC-hashed like a sign-up code, valid 5 minutes, under the sign-up per-phone
    send limit and a 30 s resend floor: 202 `{resend_after_s: 30}`.
  - `POST /v1/signin/verify {challenge_id, method, code, trust_device, device_label?}` checks a
    TOTP code (one step either side, never a step already used), an unused backup code (then
    marked used; the answer carries `backup_codes_remaining`) or the SMS code. A wrong code is
    401 `AUTH_FAILED`, the same message as a wrong password. Success consumes the challenge and
    sets the session cookies; with `trust_device` it also stores a trusted device (a 32-byte
    token, kept as SHA-256) and sets `arena_trusted` (`Path=/v1/signin`, 30 days, `HttpOnly;
Secure; SameSite=Lax`). The device label is `device_label` (up to 60 characters) or a coarse
    one from the User-Agent, such as "Chrome on Android".
  - **Lockout.** 5 consecutive failures, password or second factor in any mix, set
    `locked_until` to 15 minutes ahead and reset the counter; a successful sign-in resets it.
    A TOTP code at sign-in also goes through the per-owner `totp:{owner_id}` failure limit
    shared with confirm and regenerate; once it is reached, `/v1/signin/verify` with `totp`
    answers 429 `AUTH_LOCKED` with `Retry-After` until the window clears.
  - **Account → Security** (enrolled owners; else 403 `SETUP_INCOMPLETE`):
    `PUT /v1/account/security/sms-fallback {enabled}` → 204; `GET /v1/account/devices` →
    `[{id, label, last_seen_at, created_at, current}]`; `DELETE /v1/account/devices/{id}` → 204
    (404 `NOT_FOUND` when it isn't a live device of this owner);
    `POST /v1/account/sign-out-everywhere` → 204, revoking every session and trusted device of
    the owner and clearing this browser's cookies. `GET /v1/session` adds `sms_fallback`.
  - A session remembers the trusted device it was signed in from (`owner_sessions.device_id`),
    which is how the device list marks the `current` one; refresh rotation carries it over.
  - Logs name a login only by `phone_tag` or an email tag (`email:` and 8 hex characters of an
    HMAC with the pepper), never a password, code or token
    (`crates/arena-cloud/tests/signin.rs` asserts it on captured logs).
  - Per-IP limits for `/v1/signin*` are Cloudflare rules
    ([`deploy/README.md`](deploy/README.md#6a-owner-sign-up-sms-pepper-web-origins-and-per-ip-limits)).

- **Setup wizard** (Story 2.7). `/setup` in the onboarding app redirects to `/setup/<current>`,
  the first step not done, so setup resumes on any device. Ten steps, in order: `cafe`, `tier`,
  `install_hub`, `devices`, `rates`, `products`, `staff`, `printer_payments`, `test_session`,
  `go_live` (the three before the test session can be skipped in a later story). A later
  step that isn't done redirects to the current one; any done step opens for edit and saving it
  returns to the current step. The wizard state is fetched again on every visit to `/setup` and
  when the tab regains focus, and cleared on sign-in, sign-out and any 401. Steps 2–10 say "This step is coming soon" until their stories land.
  - Every wizard and branch route needs a finished account setup (else 403
    `SETUP_INCOMPLETE`). The tenant always comes from the session owner, never the request.
  - `GET /v1/wizard` → `{current, steps: [{key, state, can_skip}], tenant?: {id, name},
branch?: {id, name, city, country, currency, timezone, gst_registered, gstin, branch_code,
code_editable}}` (the owner's first branch; with no tenant yet, `current` is `cafe` and
    every step is `pending`).
  - `PUT /v1/wizard/cafe {cafe_name, city, country, currency, timezone, gst_registered,
gstin?}` answers the same body. The first call creates the tenant (one per owner), the
    first branch (named after the café), its `branch_code` and all ten `onboarding_progress`
    rows with `cafe` done, in one transaction; later calls update the tenant name and the
    branch and keep `cafe` done. `country` is one of `GET /v1/meta/countries`
    (`[{code, name, currency, timezone, gst}]`: IN, NP, LK, BD, AE, SG, MY, GB, US), the
    currency one of `arena_proto::currency::CURRENCIES` and the time zone one of the canonical
    tz-database names that `GET /v1/meta/timezones` lists (aliases such as `US/Eastern` are
    refused; both lists are sent with `Cache-Control: public, max-age=86400`); the country's
    currency and zone are
    only defaults. `gst_registered` is allowed only where the country has GST (India). With
    it, `gstin` is required: 15 characters (uppercased, trimmed), state code 01–38, a PAN, an
    entity character, `Z` and the GSTN mod-36 check character; without it, a `gstin` is
    refused. Every refusal is 422 `VALIDATION_FAILED` with `field`.
  - `branch_code` comes from the branch name: the initials of up to 6 words, else the first 3
    letters and digits of the name, padded with `X`. A name with no Latin letters or digits
    uses the city the same way, then `BRX`. On a clash within the tenant the last character
    becomes 2–9, then A–Z. `PUT /v1/branches/{id}/code {code}` → `{branch_code}`
    takes 3–6 of `[A-Z0-9]` (lower case is uppercased; else 422 `VALIDATION_FAILED`,
    `field: "code"`), unique in the tenant (else 409 `BRANCH_CODE_TAKEN`), while
    `code_locked_at` is null (else 409 `ALREADY_SET`; Hub pairing locks it in Story 2.10).
  - A branch id in a path that doesn't exist or belongs to another tenant is 403 `FORBIDDEN`,
    read only by the ownership check and never written (the `OwnedBranch` extractor).
  - `GET /v1/session` adds the sign-up `cafe_name` and `city`, which prefill step 1.
  - Tables (migration 0005): `tenants`, `branches` and `onboarding_progress(branch_id, step,
state, completed_at)`, which keeps completion times for the sign-up-to-go-live metric and
    no personal data.

- **Checks.** `pnpm check:secrets` fails on plaintext secrets (unencrypted `deploy/secrets/*`,
  env files, `kind: Secret` under `deploy/k8s`, age/PEM private keys, long JWTs outside test
  fixtures). `pnpm check:deploy` renders both overlays with `kubectl kustomize` and checks the
  namespace, host, args, secret refs, probes, non-root, the Cloudflare allow list and the deploy
  workflow.

### Local dev mode

No Turso, Cloudflare or token needed. MinIO stands in for R2:

```sh
docker compose -f deploy/dev/compose.yaml up -d   # MinIO on 127.0.0.1:9000, bucket arena-dev
cargo run -p arena-cloud -- --role all --dev
curl http://127.0.0.1:8080/healthz                # {"role":"all",…,"migration_version":5}
```

Sign up locally: run the cloud as above and the onboarding app with
`pnpm --filter @arena/onboarding dev` (<http://localhost:5179>; Vite proxies `/v1` to
`127.0.0.1:8080`). Submit the form, read the code from `curl http://127.0.0.1:8080/v1/dev/sms`
and enter it: you land on `/account-setup` signed in. Set a password, scan the QR code with an
authenticator app (or paste the setup key into it), enter its code and save the backup codes:
`GET /v1/session` then reports `enrolled` with `backup_codes_acked: true`, and
`GET /v1/dev/setup-gate` answers `{"setup_complete": true}`. To sign in again, open
<http://localhost:5179/sign-in> in a fresh browser profile: the password, then the current
authenticator code. Tick "Trust this device for 30 days" and the next sign-in in that profile
skips the code; Account → Security → "Sign out everywhere" makes it ask again. "Setup wizard
starts here" opens <http://localhost:5179/setup>: save step 1 (café details) and it moves on to
step 2; signing in again in another profile opens `/setup` on step 2 with step 1 done and
editable.

- `--dev` keeps `gz_control` in `<--data-dir>/gz_control.db` (default `./.arena-cloud-dev`,
  created if missing) through rusqlite, with the same embedded migrations, the same
  `SCHEMA_TOO_NEW` refusal (exit 2) and the same per-migration transaction as Turso. Delete the
  directory to start again.
- It listens on `127.0.0.1:8080` by default and refuses a non-loopback `--listen` (exit 64). One
  warning log line says dev mode is on. The `ARENA_CONTROL_DB_*` variables are ignored.
- It mounts dev-only routes that exercise each envelope failure: `POST /v1/dev/echo`
  (`{"text":"…"}` → the same), `GET /v1/dev/fail` (`INTERNAL`) and `GET /v1/dev/panic` (a
  handler panic, answered with `INTERNAL`; the server keeps serving), plus `GET /v1/dev/sms`
  (every message the fake SMS provider recorded, `[{to, text, at}]`) and `GET /v1/dev/setup-gate`
  (passes only a signed-in owner whose account setup is complete). Without `--dev` they do not
  exist (404 `NOT_FOUND`).
- Sign-up needs no env either: `--dev` uses a fixed dev pepper and MFA key, always the fake SMS
  provider, and allows the origin `http://localhost:5179` (or `ARENA_WEB_ORIGINS` when set).
- The backup task runs against the same local file and MinIO (a throwaway key from
  `age-keygen`):

  ```sh
  ARENA_BACKUP_S3_ENDPOINT=http://127.0.0.1:9000 ARENA_BACKUP_S3_BUCKET=arena-dev \
  ARENA_BACKUP_S3_ACCESS_KEY_ID=arena-dev ARENA_BACKUP_S3_SECRET_ACCESS_KEY=arena-dev-password \
  ARENA_BACKUP_AGE_RECIPIENTS=age1… \
  cargo run -p arena-cloud -- --role jobs --task control-backup --dev
  ```

- MinIO's dev-only credentials are `arena-dev` / `arena-dev-password` (in the compose file).
  `docker compose -f deploy/dev/compose.yaml down -v` removes it and its data.

## Pinned versions

This table is checked against the manifests by `pnpm check:pins`. Change the manifest and this
table together. JS framework versions live in the pnpm `catalog:`; apps reference them as
`"catalog:"`.

| Item                                | Version                | Source                                         |
| ----------------------------------- | ---------------------- | ---------------------------------------------- |
| rust                                | 1.95.0                 | `rust-toolchain.toml`                          |
| node                                | 22.23.4                | `.nvmrc`                                       |
| node engines                        | >=22.23.4 <23          | `package.json`                                 |
| pnpm                                | 12.15.0                | `package.json`                                 |
| svelte                              | 5.57.2                 | `pnpm-workspace.yaml`                          |
| @sveltejs/kit                       | 3.1.0                  | `pnpm-workspace.yaml`                          |
| @sveltejs/vite-plugin-svelte        | 7.3.1                  | `pnpm-workspace.yaml`                          |
| vite                                | 8.3.5                  | `pnpm-workspace.yaml`                          |
| typescript                          | 6.0.3                  | `pnpm-workspace.yaml`                          |
| @sveltejs/adapter-static            | 4.0.0                  | `pnpm-workspace.yaml`                          |
| @sveltejs/adapter-cloudflare        | 8.0.1                  | `pnpm-workspace.yaml`                          |
| wrangler                            | 4.152.0                | `pnpm-workspace.yaml`                          |
| svelte-check                        | 4.7.7                  | `pnpm-workspace.yaml`                          |
| astro                               | 7.4.2                  | `pnpm-workspace.yaml`                          |
| @astrojs/svelte                     | 9.1.1                  | `pnpm-workspace.yaml`                          |
| @tauri-apps/cli                     | 2.12.4                 | `pnpm-workspace.yaml`                          |
| @tauri-apps/api                     | 2.12.4                 | `pnpm-workspace.yaml`                          |
| @tauri-apps/plugin-updater          | 2.14.0                 | `pnpm-workspace.yaml`                          |
| @fontsource-variable/sora           | 5.3.0                  | `pnpm-workspace.yaml`                          |
| @playwright/test                    | 1.64.1                 | `pnpm-workspace.yaml`                          |
| @axe-core/playwright                | 4.14.0                 | `pnpm-workspace.yaml`                          |
| yaml                                | 2.9.1                  | `package.json`                                 |
| tauri                               | =2.12.5                | `Cargo.toml`                                   |
| tauri-build                         | =2.7.4                 | `Cargo.toml`                                   |
| tauri-plugin-updater                | =2.14.0                | `Cargo.toml`                                   |
| serde                               | =1.0.229               | `Cargo.toml`                                   |
| serde_json                          | =1.0.154               | `Cargo.toml`                                   |
| ts-rs                               | =12.0.1                | `Cargo.toml`                                   |
| tokio                               | =1.54.1                | `Cargo.toml`                                   |
| axum                                | =0.8.9                 | `Cargo.toml`                                   |
| rusqlite                            | =0.40.2                | `Cargo.toml`                                   |
| tracing                             | =0.1.44                | `Cargo.toml`                                   |
| tracing-subscriber                  | =0.3.23                | `Cargo.toml`                                   |
| ulid                                | =3.0.0                 | `Cargo.toml`                                   |
| rmp-serde                           | =1.3.1                 | `Cargo.toml`                                   |
| chrono-tz                           | =0.10.4                | `Cargo.toml`                                   |
| chrono                              | =0.4.45                | `Cargo.toml`                                   |
| sha2                                | =0.10.9                | `Cargo.toml`                                   |
| tokio-tungstenite                   | =0.30.0                | `Cargo.toml`                                   |
| rustls                              | =0.23.46               | `Cargo.toml`                                   |
| tokio-rustls                        | =0.26.6                | `Cargo.toml`                                   |
| webpki-roots                        | =1.0.9                 | `Cargo.toml`                                   |
| rcgen                               | =0.14.10               | `Cargo.toml`                                   |
| argon2                              | =0.6.0                 | `Cargo.toml`                                   |
| futures-util                        | =0.3.34                | `Cargo.toml`                                   |
| windows                             | =0.62.2                | `Cargo.toml`                                   |
| windows-service                     | =0.8.1                 | `Cargo.toml`                                   |
| tempfile                            | =3.27.0                | `Cargo.toml`                                   |
| tower                               | =0.5.3                 | `Cargo.toml`                                   |
| proptest                            | =1.12.0                | `Cargo.toml`                                   |
| libsql                              | =0.10.0                | `Cargo.toml`                                   |
| hyper-rustls                        | =0.25.0                | `Cargo.toml`                                   |
| hyper                               | =0.14.32               | `Cargo.toml`                                   |
| serde_path_to_error                 | =0.1.20                | `Cargo.toml`                                   |
| aws-sdk-s3                          | =1.155.0               | `Cargo.toml`                                   |
| aws-smithy-http-client              | =1.5.1                 | `Cargo.toml`                                   |
| age                                 | =0.12.1                | `Cargo.toml`                                   |
| reqwest                             | =0.13.5                | `Cargo.toml`                                   |
| hmac                                | =0.12.1                | `Cargo.toml`                                   |
| getrandom                           | =0.3.4                 | `Cargo.toml`                                   |
| ring                                | =0.17.14               | `Cargo.toml`                                   |
| qrcode                              | =0.14.1                | `Cargo.toml`                                   |
| sops                                | 3.13.3                 | `.github/workflows/deploy-cloud.yml`           |
| age                                 | 1.3.2                  | `.github/workflows/deploy-cloud.yml`           |
| gcr.io/distroless/cc-debian12       | nonroot                | `deploy/docker/arena-cloud.Dockerfile`         |
| ghcr.io/tursodatabase/libsql-server | v0.24.31               | `.github/workflows/ci.yml`                     |
| bitnamilegacy/minio                 | 2025.7.23-debian-12-r5 | `.github/workflows/ci.yml`                     |
| k3s                                 | v1.33.4+k3s1           | `deploy/README.md`                             |
| bitnamilegacy/minio                 | 2025.7.23-debian-12-r5 | `deploy/dev/compose.yaml`                      |
| bitnamilegacy/minio-client          | 2025.7.21-debian-12-r3 | `deploy/dev/compose.yaml`                      |
| @zxcvbn-ts/language-common          | 3.0.4                  | `crates/arena-cloud/data/common-passwords.txt` |

## Generated protocol types and the message catalogue

- **Type generator: ts-rs.** Protocol types live in `crates/arena-proto` and derive `TS`. Run
  `pnpm gen:types` after changing them: it runs `cargo test -p arena-proto --features export-ts`
  with `TS_RS_EXPORT_DIR` pointing at a temp folder, writes the `index.ts` barrel, formats the
  output and only then replaces `packages/ui/src/generated`. The same run writes
  `currencies.json` from `arena_proto::currency::CURRENCIES` (code, exponent, symbol), which
  `@arena/ui/format` reads. Plain `cargo test` exports nothing.
  Commit the output. Import protocol types only from `@arena/ui/generated`.
- **Messages.** All user-visible text lives in `packages/ui/src/messages/en.json` (flat dotted
  keys, `{name}` placeholders) and is read with `t(key, params?)` from `@arena/ui`. Exempt a line
  from the string lint with `// i18n-ignore`, or put `<!-- i18n-ignore -->` on the line before.
  `.astro` files are checked for unknown `t()` keys only.
- **Gates.** `pnpm check:generated` regenerates and fails when the result differs from `HEAD`
  (so commit regenerated output before running it locally),
  `pnpm check:protocol` fails on hand-written protocol types or imports from anywhere but
  `@arena/ui/generated`, and `pnpm check:strings` fails on hard-coded user-visible strings and
  unknown message keys.

## Design tokens

- **Source.** Colours, type ramps, spacing, radii and elevation come from the frontmatter of
  `_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md` (the
  `DESIGN_MD` constant in `scripts/gen-tokens.mjs`, shared by every token script). Run
  `pnpm gen:tokens` after editing it and commit `packages/ui/src/tokens/` (`tokens.css`,
  `tokens.ts`; generated, do not edit). Generation fails on an unresolved `{group.key}` reference
  or a component colour without an `-hc` twin.
- **Themes.** `--color-<name>` holds every raw colour. Component values with colour refs inside a
  longer value (`2px solid {colors.x}`) are emitted with `var(--color-x)` and an `-hc` twin. Use the semantic aliases (`--bg`, `--text`,
  ...) and component tokens (`--<component>-<prop>`): dark is the default (`:root`,
  `[data-theme="dark"]`), `[data-theme="hc"]` maps them to the `-hc` values and
  `[data-theme="light"]` to the `-light` values (falling back to dark where there is no light twin).
- **Styling entry.** Every SvelteKit app imports `@arena/ui/base.css` once from
  `src/routes/+layout.svelte` (marketing: `src/pages/index.astro`). It bundles
  the Sora variable font (`@fontsource-variable/sora`) and imports `tokens.css`; nothing loads
  remotely. TS constants and the `TokenName` union are in `@arena/ui/tokens`.
- **Gates.** `pnpm check:tokens-fresh` regenerates and fails when `packages/ui/src/tokens` differs
  from `HEAD`. `pnpm check:contrast` checks WCAG 2.x ratios for pairs derived from DESIGN.md in the
  dark, hc and light themes (4.5 for text, 3.0 for large text and non-text); fill-only and
  decorative tokens are listed with their reason in `EXCLUDED`. `pnpm check:tokens-used` fails on
  `var(--name)` that neither `tokens.css` nor the same file declares. `pnpm check:hex` fails on raw
  hex colours outside the generated tokens (exempt a line with `hex-ignore`). After a build,
  `pnpm check:no-remote [app ...]` fails on remote URLs, fonts or CDN hosts in the Hub, Kiosk and
  Kitchen output, and on a build without a bundled `sora-*.woff2` or a CSS file declaring
  `--color-bg` (CI runs it in each of those build jobs; it is not part of `pnpm check`).

`pnpm-workspace.yaml` also carries two pnpm install-policy settings:

- `minimumReleaseAgeExclude` lets `wrangler@4.152.0` and the `miniflare` beta it pins install even
  though they are younger than pnpm's minimum release age. Remove or update these entries whenever
  the `wrangler` pin changes.
- `allowBuilds` allows the install scripts of `esbuild` and `workerd` (they fetch their platform
  binaries); pnpm blocks all other dependency build scripts.

## CI and merge blocking

`.github/workflows/ci.yml` runs on every push and pull request on `ubuntu-latest` and
`windows-latest`. The `ci-ok` job depends on all other jobs and is the single required status.
Besides the Rust, checks and per-app build jobs, `a11y` runs the Playwright suites (the gallery
and the Hub shell) and `installer` builds the Hub's Windows NSIS installer with the `arena-hub`
sidecar and uploads it as the `hub-installer` artifact, and `wan-down` runs every Epic 1 flow
with no route out (see [The branch with the WAN down](#the-branch-with-the-wan-down)).

**One-time step after the GitHub remote is added:** protect the default branch so `ci-ok` (from
GitHub Actions) must pass before a merge:

```sh
scripts/protect-main.sh                   # current repo, its default branch
scripts/protect-main.sh owner/repo        # name the repo explicitly
scripts/protect-main.sh owner/repo branch # and the branch
```

This needs the GitHub CLI (`gh`) authenticated with admin rights on the repository.
