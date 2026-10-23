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
`arena-replay`, `arena-cloud`, `arena-kiosk-core` (libraries) and `arena-hub`, `arena-kiosk-svc`,
`arena-pi` (binaries).

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

| Flag                   | Meaning                                                    |
| ---------------------- | ---------------------------------------------------------- |
| `--data-dir <path>`    | Data directory (overrides `ARENA_HUB_DATA_DIR`)            |
| `--health-port <port>` | Loopback port for `GET /healthz` (default `7409`, not `0`) |
| `--lan-port <port>`    | TLS WebSocket port for paired apps (default `7400`)        |
| `--web-port <port>`    | Sniffed TLS / plain HTTP port (default `7401`)             |
| `--help`, `-h`         | Print usage and exit 0                                     |
| `--version`            | Print the version and exit 0                               |

The three ports must differ. Tests pass free ports; production uses the defaults.

- **Data dir:** `--data-dir`, else `ARENA_HUB_DATA_DIR`, else `%PROGRAMDATA%\Arena360\Hub` on
  Windows and `$XDG_DATA_HOME/arena360/hub` (falling back to `~/.local/share/arena360/hub`)
  elsewhere. The database is `<data>/branch.db`; `<data>/branch.db.lock` is the process writer
  lock, so a second process on the same dir starts as a read-only follower.
- **Health:** `GET 127.0.0.1:<port>/healthz` returns `db_open`, `role`, `lease`, `epoch`,
  `schema_version`, `self_instance_id`, `tls_fingerprint` and `now`. It binds loopback only.
- **UI snapshot:** `GET 127.0.0.1:<port>/ui/snapshot` on the same loopback server returns
  `HubSnapshot {branch: {name, branch_code} | null, devices: [{device_id, label, class, zone}],
locale}` (type in `arena-proto`; revoked devices are left out; `zone` is `null` until zones
  exist; `locale` is `null` before bootstrap, see **Branch locale** below). It
  reads through `arena-core` on its own read-only connection. CORS allows only
  `tauri://localhost`, `http(s)://tauri.localhost` and the vite origin `http://localhost:5173`;
  another `Origin`, or a `Host` that is not `127.0.0.1`/`localhost`/`[::1]`, gets 403.
- **Staff auth (same loopback server and origin guard):**

  | Endpoint                                                        | Answer                                                                                                                                                                                                                                                                                                 |
  | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
  | `POST /ui/auth/sign-in {login, password}`                       | `{token, staff: {staff_id, display_name, role_name, permissions}}`, or 401 `AUTH_FAILED` with one fixed message whatever was wrong (unknown login, wrong password, inactive staff)                                                                                                                     |
  | `POST /ui/auth/sign-out` (bearer)                               | 204; the token stops working                                                                                                                                                                                                                                                                           |
  | `GET /ui/me` (bearer)                                           | the session's staff plus `locked` and `has_pin`, or 401 `AUTH_FAILED`                                                                                                                                                                                                                                  |
  | `POST /ui/lock` (bearer)                                        | 204; the session is idle-locked: `/ui/intent` and `/ui/elevate` answer 423 `SESSION_LOCKED` and its tickets are dropped. See **Idle lock** below                                                                                                                                                       |
  | `POST /ui/unlock {pin}` (bearer)                                | `{staff, switched}` for a locked session (else 409 `VALIDATION_FAILED` on `session`); own PIN first, another member's re-binds it; else 403 `PIN_REJECTED`                                                                                                                                             |
  | `POST /ui/elevate {items: [{intent_kind, key?}], pin}` (bearer) | `{ticket, items, expires_in_s}` when the PIN belongs to another active staff member whose role holds every permission the items need; otherwise 403 `PIN_REJECTED` with one fixed message. See **PIN elevation** below                                                                                 |
  | `POST /ui/elevate/drop {ticket}` (bearer)                       | 204; the ticket's unused approvals are given up                                                                                                                                                                                                                                                        |
  | `POST /ui/intent {intent, elevation_ticket?}` (bearer)          | `{applied}`; the intent runs as the session's `actor{staff, staff_id}` whatever actor was sent, and any client-supplied `elevated_by` is dropped. A ticket is redeemed for one matching approved item and becomes `elevated_by`; a bad one is 403 `PIN_REQUIRED`. Errors are `{code, message, field?}` |
  | `GET /ui/settings` (bearer)                                     | `[{key, value, rev}]`, so an edit can carry `base_rev`                                                                                                                                                                                                                                                 |
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
  `intent.submit`, `intent.result`, `event`, `error`, `ping`/`pong` (types in `arena-proto`,
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
- **Revocation.** `device.revoke` closes the device's sessions with `TOKEN_REVOKED` within 1 s,
  and later hellos with that token are refused.
- **Client.** `arena-kiosk-core` has `HubClient::connect(url, pinned_fingerprint, hello)`:
  pinned TLS, `request(kind, body)` with a 5 s `HUB_SLOW` timeout per request, and
  `events()`.
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
`settings.set` and the same manager approval for a Counter). Every signed-in view listens to `/ui/events`: a
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

## Pinned versions

This table is checked against the manifests by `pnpm check:pins`. Change the manifest and this
table together. JS framework versions live in the pnpm `catalog:`; apps reference them as
`"catalog:"`.

| Item                         | Version       | Source                |
| ---------------------------- | ------------- | --------------------- |
| rust                         | 1.95.0        | `rust-toolchain.toml` |
| node                         | 22.23.4       | `.nvmrc`              |
| node engines                 | >=22.23.4 <23 | `package.json`        |
| pnpm                         | 12.15.0       | `package.json`        |
| svelte                       | 5.57.2        | `pnpm-workspace.yaml` |
| @sveltejs/kit                | 3.1.0         | `pnpm-workspace.yaml` |
| @sveltejs/vite-plugin-svelte | 7.3.1         | `pnpm-workspace.yaml` |
| vite                         | 8.3.5         | `pnpm-workspace.yaml` |
| typescript                   | 6.0.3         | `pnpm-workspace.yaml` |
| @sveltejs/adapter-static     | 4.0.0         | `pnpm-workspace.yaml` |
| @sveltejs/adapter-cloudflare | 8.0.1         | `pnpm-workspace.yaml` |
| wrangler                     | 4.152.0       | `pnpm-workspace.yaml` |
| svelte-check                 | 4.7.7         | `pnpm-workspace.yaml` |
| astro                        | 7.4.2         | `pnpm-workspace.yaml` |
| @astrojs/svelte              | 9.1.1         | `pnpm-workspace.yaml` |
| @tauri-apps/cli              | 2.12.4        | `pnpm-workspace.yaml` |
| @tauri-apps/api              | 2.12.4        | `pnpm-workspace.yaml` |
| @tauri-apps/plugin-updater   | 2.14.0        | `pnpm-workspace.yaml` |
| @fontsource-variable/sora    | 5.3.0         | `pnpm-workspace.yaml` |
| @playwright/test             | 1.64.1        | `pnpm-workspace.yaml` |
| @axe-core/playwright         | 4.14.0        | `pnpm-workspace.yaml` |
| yaml                         | 2.9.1         | `package.json`        |
| tauri                        | =2.12.5       | `Cargo.toml`          |
| tauri-build                  | =2.7.4        | `Cargo.toml`          |
| tauri-plugin-updater         | =2.14.0       | `Cargo.toml`          |
| serde                        | =1.0.229      | `Cargo.toml`          |
| serde_json                   | =1.0.154      | `Cargo.toml`          |
| ts-rs                        | =12.0.1       | `Cargo.toml`          |
| tokio                        | =1.54.1       | `Cargo.toml`          |
| axum                         | =0.8.9        | `Cargo.toml`          |
| rusqlite                     | =0.40.2       | `Cargo.toml`          |
| tracing                      | =0.1.44       | `Cargo.toml`          |
| tracing-subscriber           | =0.3.23       | `Cargo.toml`          |
| ulid                         | =3.0.0        | `Cargo.toml`          |
| rmp-serde                    | =1.3.1        | `Cargo.toml`          |
| chrono-tz                    | =0.10.4       | `Cargo.toml`          |
| chrono                       | =0.4.45       | `Cargo.toml`          |
| sha2                         | =0.10.9       | `Cargo.toml`          |
| tokio-tungstenite            | =0.30.0       | `Cargo.toml`          |
| rustls                       | =0.23.46      | `Cargo.toml`          |
| tokio-rustls                 | =0.26.6       | `Cargo.toml`          |
| rcgen                        | =0.14.10      | `Cargo.toml`          |
| argon2                       | =0.6.0        | `Cargo.toml`          |
| futures-util                 | =0.3.34       | `Cargo.toml`          |
| tempfile                     | =3.27.0       | `Cargo.toml`          |
| tower                        | =0.5.3        | `Cargo.toml`          |
| proptest                     | =1.12.0       | `Cargo.toml`          |

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
sidecar and uploads it as the `hub-installer` artifact.

**One-time step after the GitHub remote is added:** protect the default branch so `ci-ok` (from
GitHub Actions) must pass before a merge:

```sh
scripts/protect-main.sh                   # current repo, its default branch
scripts/protect-main.sh owner/repo        # name the repo explicitly
scripts/protect-main.sh owner/repo branch # and the branch
```

This needs the GitHub CLI (`gh`) authenticated with admin rights on the repository.
