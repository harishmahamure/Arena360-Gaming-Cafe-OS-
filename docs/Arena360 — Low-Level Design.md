# Arena360 — Low-Level Design

Oct 22, 2026 · @Harish

## Scope and conventions

This LLD specifies each component to the level an engineer implements from: module boundaries, full schema, the intent catalogue with validation and error codes, the algorithms inside the Hub, the Kiosk's lockdown mechanics, cloud internals and the test plan. It builds on "Gamezone SaaS — Low-Level Architecture" (infrastructure tables, change-log batch format, Hub↔cloud endpoints, security credentials), which stays authoritative for those; this document does not repeat them.

**Conventions used throughout**

| Convention | Rule |
| --- | --- |
| Ids | ULID, 26 chars, minted by the writer (Hub for branch rows, cloud for control rows); `branch_id` on every branch row |
| Money | `INTEGER` paise (or minor units of the branch currency); never float; display formatting at the edge |
| Time | `INTEGER` unix milliseconds UTC; branch timezone applied only for display, opening hours and dynamic-pricing windows |
| Booleans | `INTEGER` 0/1 |
| Enums | `TEXT` with a `CHECK (col IN (...))`; the Rust enum is the source of truth and the migration regenerates the CHECK |
| Soft delete | `deleted_at INTEGER NULL` on catalogue tables (plans, products, staff); ledgers are never deleted |
| Versioning | `updated_at` and `rev INTEGER` on mutable rows; the change log carries the full row after write |
| Errors | Typed `AppError { code, message, field? }`; codes are stable strings listed in section 10; HTTP 4xx/5xx only wrap them |
| Naming | `snake_case` tables and columns; intents are `noun.verb` (`session.start`); permissions match intents one-to-one |
| Transactions | One `apply()` = one `BEGIN IMMEDIATE` … `COMMIT`; no nested transactions; no I/O inside a transaction except SQLite |
| Config | Branch settings in the `settings` table; tenant settings arrive as commands; process flags only for ports and paths |
| Logging | `tracing` spans per intent with `intent_id`, `actor`, `device_id`; no PII in logs beyond player id |

## Hub module map

The Hub is one Rust workspace with four layers; the only path from a socket to the database runs through `apply()`, and the background tasks either feed it (poller, follower) or read what it wrote (uploader, lease).

&#91;embedded content: Hub module map · api, core, store, tasks\]

**Crates**

| Crate | Modules | Depends on | Notes |
| --- | --- | --- | --- |
| `arena-core` | `intent`, `apply`, `domain::{sessions, pricing, players, wallet, pos, inventory, kitchen, shifts, credit, rbac, devices, bookings}`, `error` | `arena-store` | Pure logic; no sockets, no clocks except an injected `Clock` trait |
| `arena-store` | `db` (pool, WAL, pragmas), `migrations`, `changes`, `lease`, `settings`, `entitlement_cache` | rusqlite | Owns every SQL statement; domain modules call typed repository functions |
| `arena-api` | `ws` (frames, sessions per device), `http` (Axum routes, UI static), `kds` | `arena-core` | Maps frames to intents and intents to frames |
| `arena-tasks` | `lease`, `uploader`, `poller`, `follower`, `replicator` (primary side), `discovery`, `tunnel`, `heartbeat`, `printer`, `notify_outbox` | core, store, reqwest | Each task is a Tokio task with a shutdown signal |
| `arena-hub` (bin) | `main`, config, role selection (primary or follower) | all | Also the Android JNI entry |
| `arena-proto` | Shared frame and batch types (`serde`), error codes | — | Used by cloud, Kiosk (C# generated from it) and Pi |

**Threading model.** One Tokio runtime. A single `Writer` actor owns the write connection and processes intents from an `mpsc` channel in order, so `apply()` is serialised without locks; readers use a pool of 4 connections. WebSocket sessions are tasks that send intents to the writer and await a reply with the same `rid`. Timers for sessions live in the `sessions` engine as a `BinaryHeap<(ends_at, session_id)>` rebuilt from the DB on start.

**Role switch.** The binary starts as `follower` if a fresh lease with a higher epoch exists, else `primary`. Promotion swaps the `follower` task for `lease` + `replicator` and opens the writer channel; demotion does the reverse and closes it, so a demoted process physically cannot write.

## Hub domain schema

Forty-one tables in one SQLite file. Infrastructure tables (`changes`, `lease`, `upload_cursor`, `devices`, `entitlement_cache`, `applied_commands`) are defined in the earlier architecture document; the domain tables below are the complete set for the PRD. Every table carries `branch_id TEXT NOT NULL` and timestamps; they are omitted here for brevity except where they matter.

**Devices, groups, pricing**

```sql
CREATE TABLE device_groups (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('pc','console','vr','other')),
  colour TEXT NOT NULL, sort INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER);

CREATE TABLE devices_meta (             -- extends infra `devices` for gaming devices
  device_id TEXT PRIMARY KEY REFERENCES devices(id),
  group_id TEXT NOT NULL REFERENCES device_groups(id),
  label TEXT NOT NULL,                  -- 'PC 07', 'PS5 1'
  zone TEXT, floor_x INTEGER, floor_y INTEGER,
  state TEXT NOT NULL DEFAULT 'free' CHECK (state IN ('free','session','reserved','maintenance','offline')),
  last_seen_at INTEGER, kiosk_version TEXT);

CREATE TABLE rates (                    -- postpaid hourly rate per group
  id TEXT PRIMARY KEY, group_id TEXT NOT NULL REFERENCES device_groups(id),
  price_per_hour_paise INTEGER NOT NULL, min_bill_minutes INTEGER NOT NULL DEFAULT 30,
  rounding_minutes INTEGER NOT NULL DEFAULT 15, active INTEGER NOT NULL DEFAULT 1, deleted_at INTEGER);

CREATE TABLE plans (
  id TEXT PRIMARY KEY, name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('pack','block','pass','membership')),
  price_paise INTEGER NOT NULL, gst_rate_bp INTEGER NOT NULL DEFAULT 1800,
  minutes INTEGER,                      -- pack: total minutes; block: fixed length
  validity_days INTEGER,                -- pack/pass: expiry after purchase
  window_start_min INTEGER, window_end_min INTEGER,  -- pass/block: minutes from midnight local
  active INTEGER NOT NULL DEFAULT 1, deleted_at INTEGER);

CREATE TABLE plan_groups (plan_id TEXT NOT NULL REFERENCES plans(id), group_id TEXT NOT NULL REFERENCES device_groups(id), PRIMARY KEY (plan_id, group_id));

CREATE TABLE pricing_rules (            -- dynamic pricing
  id TEXT PRIMARY KEY, name TEXT NOT NULL,
  scope TEXT NOT NULL CHECK (scope IN ('group','plan')), target_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('weekly','date')),
  days_mask INTEGER,                    -- weekly: bit 0 = Monday
  start_min INTEGER, end_min INTEGER,   -- local minutes from midnight; end < start wraps midnight
  date_from TEXT, date_to TEXT,         -- date: YYYY-MM-DD inclusive, local
  adjust_kind TEXT NOT NULL CHECK (adjust_kind IN ('override_paise','percent_bp')), adjust_value INTEGER NOT NULL,
  priority INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1);

CREATE TABLE coupons (
  id TEXT PRIMARY KEY, code TEXT NOT NULL UNIQUE, kind TEXT NOT NULL CHECK (kind IN ('percent_bp','amount_paise')),
  value INTEGER NOT NULL, valid_from INTEGER, valid_to INTEGER, max_uses INTEGER, used INTEGER NOT NULL DEFAULT 0,
  min_bill_paise INTEGER, active INTEGER NOT NULL DEFAULT 1);
CREATE TABLE coupon_targets (coupon_id TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('group','plan','product')), target_id TEXT NOT NULL, PRIMARY KEY (coupon_id, kind, target_id));
```

**Players, wallet, credit, packs**

```sql
CREATE TABLE players (
  id TEXT PRIMARY KEY, phone TEXT, name TEXT NOT NULL, pin_hash TEXT, email TEXT, dob TEXT,
  notes TEXT, tags TEXT, consent_whatsapp INTEGER NOT NULL DEFAULT 0, is_guest INTEGER NOT NULL DEFAULT 0,
  credit_limit_paise INTEGER NOT NULL DEFAULT 0, credit_paused INTEGER NOT NULL DEFAULT 0,
  reminder_days TEXT NOT NULL DEFAULT '3,7,14', created_at INTEGER NOT NULL, deleted_at INTEGER);
CREATE UNIQUE INDEX players_phone ON players(phone) WHERE phone IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE wallets (player_id TEXT PRIMARY KEY REFERENCES players(id), balance_paise INTEGER NOT NULL DEFAULT 0 CHECK (balance_paise >= 0), updated_at INTEGER NOT NULL);

CREATE TABLE wallet_txns (             -- append-only; sum(delta) where kind='wallet' == wallets.balance
  id TEXT PRIMARY KEY, player_id TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('wallet','credit')),
  delta_paise INTEGER NOT NULL, reason TEXT NOT NULL,
  ref_kind TEXT, ref_id TEXT, staff_id TEXT, method TEXT, at INTEGER NOT NULL);
CREATE INDEX wallet_txns_player ON wallet_txns(player_id, at);

CREATE TABLE credit_balances (player_id TEXT PRIMARY KEY, outstanding_paise INTEGER NOT NULL DEFAULT 0 CHECK (outstanding_paise >= 0), oldest_unpaid_at INTEGER, last_reminder_at INTEGER);

CREATE TABLE player_packs (
  id TEXT PRIMARY KEY, player_id TEXT NOT NULL, plan_id TEXT NOT NULL,
  minutes_left INTEGER NOT NULL CHECK (minutes_left >= 0), expires_at INTEGER, bought_at INTEGER NOT NULL, order_id TEXT);
```

**Sessions and bookings**

```sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY, player_id TEXT, device_id TEXT NOT NULL, group_id TEXT NOT NULL,
  mode TEXT NOT NULL CHECK (mode IN ('postpaid','pack','wallet','pass','block')),
  plan_id TEXT, pack_id TEXT, rate_id TEXT,
  state TEXT NOT NULL CHECK (state IN ('active','paused','expired','ended','orphaned')),
  started_at INTEGER NOT NULL, ends_at INTEGER, paused_at INTEGER, paused_total_ms INTEGER NOT NULL DEFAULT 0,
  ended_at INTEGER, end_reason TEXT, group_bill_id TEXT, epoch INTEGER NOT NULL);
CREATE UNIQUE INDEX sessions_one_active ON sessions(device_id) WHERE state IN ('active','paused');

CREATE TABLE session_segments (         -- rate changes mid-session (transfer, pricing rule flip)
  id TEXT PRIMARY KEY, session_id TEXT NOT NULL, from_at INTEGER NOT NULL, to_at INTEGER,
  device_id TEXT NOT NULL, rate_paise_per_hour INTEGER NOT NULL, rule_id TEXT);

CREATE TABLE group_bills (id TEXT PRIMARY KEY, payer_player_id TEXT, state TEXT NOT NULL CHECK (state IN ('open','settled')), created_at INTEGER NOT NULL);

CREATE TABLE bookings (
  id TEXT PRIMARY KEY, player_id TEXT, device_id TEXT, group_id TEXT NOT NULL,
  start_at INTEGER NOT NULL, end_at INTEGER NOT NULL, deposit_paise INTEGER NOT NULL DEFAULT 0,
  state TEXT NOT NULL CHECK (state IN ('held','confirmed','seated','no_show','cancelled')), order_id TEXT);
```

**POS, inventory, kitchen**

```sql
CREATE TABLE products (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT, price_paise INTEGER NOT NULL,
  gst_rate_bp INTEGER NOT NULL DEFAULT 500, hsn TEXT, barcode TEXT, image_blob TEXT,
  track_stock INTEGER NOT NULL DEFAULT 1, is_kitchen INTEGER NOT NULL DEFAULT 0, station TEXT,
  low_stock_qty INTEGER, active INTEGER NOT NULL DEFAULT 1, deleted_at INTEGER);

CREATE TABLE bundle_components (bundle_id TEXT NOT NULL REFERENCES products(id), component_id TEXT NOT NULL REFERENCES products(id), qty INTEGER NOT NULL, PRIMARY KEY (bundle_id, component_id));

CREATE TABLE stock (product_id TEXT PRIMARY KEY, qty INTEGER NOT NULL DEFAULT 0 CHECK (qty >= 0), avg_cost_paise INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL);

CREATE TABLE stock_moves (              -- append-only
  id TEXT PRIMARY KEY, product_id TEXT NOT NULL, delta INTEGER NOT NULL,
  reason TEXT NOT NULL CHECK (reason IN ('sale','return','stock_in','waste','expired','count_adjust','transfer_out','transfer_in')),
  ref_id TEXT, cost_paise INTEGER, expiry TEXT, supplier TEXT, note TEXT, staff_id TEXT, at INTEGER NOT NULL);

CREATE TABLE orders (
  id TEXT PRIMARY KEY, player_id TEXT, session_id TEXT, group_bill_id TEXT, device_id TEXT,
  state TEXT NOT NULL CHECK (state IN ('open','placed','settled','voided')),
  subtotal_paise INTEGER NOT NULL DEFAULT 0, discount_paise INTEGER NOT NULL DEFAULT 0, gst_paise INTEGER NOT NULL DEFAULT 0, total_paise INTEGER NOT NULL DEFAULT 0,
  coupon_id TEXT, invoice_no TEXT, placed_at INTEGER, settled_at INTEGER, staff_id TEXT);
CREATE TABLE order_lines (id TEXT PRIMARY KEY, order_id TEXT NOT NULL, product_id TEXT, plan_id TEXT, session_id TEXT, qty INTEGER NOT NULL, unit_paise INTEGER NOT NULL, gst_rate_bp INTEGER NOT NULL, line_paise INTEGER NOT NULL, kot_state TEXT CHECK (kot_state IN ('new','preparing','ready','served','cancelled')), station TEXT);

CREATE TABLE payments (id TEXT PRIMARY KEY, order_id TEXT NOT NULL, method TEXT NOT NULL CHECK (method IN ('cash','upi','card','wallet','credit')), amount_paise INTEGER NOT NULL, reference TEXT, confirmed INTEGER NOT NULL DEFAULT 1, staff_id TEXT, at INTEGER NOT NULL);

CREATE TABLE kot_tickets (id TEXT PRIMARY KEY, order_id TEXT NOT NULL, station TEXT, printed_at INTEGER, state TEXT NOT NULL CHECK (state IN ('new','preparing','ready','served','cancelled')), updated_at INTEGER NOT NULL);
```

**Shifts, cash, expenses, invoices**

```sql
CREATE TABLE shifts (
  id TEXT PRIMARY KEY, staff_id TEXT NOT NULL, opened_at INTEGER NOT NULL, closed_at INTEGER,
  float_paise INTEGER NOT NULL, counted_paise INTEGER, expected_paise INTEGER, variance_paise INTEGER,
  variance_reason TEXT, approved_by TEXT, state TEXT NOT NULL CHECK (state IN ('open','closed')));
CREATE TABLE cash_events (id TEXT PRIMARY KEY, shift_id TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('sale','refund','deposit','expense','float_in','drawer_open')), amount_paise INTEGER NOT NULL, ref_id TEXT, category TEXT, note TEXT, receipt_blob TEXT, staff_id TEXT, at INTEGER NOT NULL);

CREATE TABLE invoice_sequences (fy TEXT PRIMARY KEY, next INTEGER NOT NULL);   -- 'FY26-27'
CREATE TABLE refunds (id TEXT PRIMARY KEY, order_id TEXT NOT NULL, amount_paise INTEGER NOT NULL, method TEXT NOT NULL, reason TEXT NOT NULL, approved_by TEXT, credit_note_no TEXT, at INTEGER NOT NULL);
```

**Staff, RBAC, settings, games, outbox**

```sql
CREATE TABLE staff (id TEXT PRIMARY KEY, name TEXT NOT NULL, login TEXT NOT NULL UNIQUE, pw_hash TEXT NOT NULL, pin_hash TEXT NOT NULL, role_id TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 1, deleted_at INTEGER);
CREATE TABLE roles (id TEXT PRIMARY KEY, name TEXT NOT NULL, builtin INTEGER NOT NULL DEFAULT 0);
CREATE TABLE role_perms (role_id TEXT NOT NULL, perm TEXT NOT NULL, PRIMARY KEY (role_id, perm));
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL);

CREATE TABLE games (id TEXT PRIMARY KEY, name TEXT NOT NULL, launcher TEXT NOT NULL CHECK (launcher IN ('steam','epic','riot','standalone')), exe_path TEXT, launch_arg TEXT, art_blob TEXT, category TEXT, age_rating TEXT, active INTEGER NOT NULL DEFAULT 1);
CREATE TABLE game_launches (id TEXT PRIMARY KEY, game_id TEXT NOT NULL, session_id TEXT, device_id TEXT NOT NULL, at INTEGER NOT NULL, duration_ms INTEGER);

CREATE TABLE notify_outbox (id TEXT PRIMARY KEY, channel TEXT NOT NULL CHECK (channel IN ('whatsapp','sms')), to_phone TEXT NOT NULL, template TEXT NOT NULL, params TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('queued','uploaded','sent','failed')), created_at INTEGER NOT NULL);
CREATE TABLE ad_impressions (id TEXT PRIMARY KEY, slot TEXT NOT NULL, creative_id TEXT, device_id TEXT, at INTEGER NOT NULL);
```

**Indexes that matter**: `sessions(state, ends_at)` for the timer heap rebuild; `orders(session_id)`; `order_lines(order_id)`; `stock_moves(product_id, at)`; `cash_events(shift_id)`; `pricing_rules(scope, target_id, active)`; `player_packs(player_id, expires_at)`; `games(name COLLATE NOCASE)`.

**Invariants enforced by `apply`, not only by CHECK**: one active or paused session per device; `wallets.balance_paise >= 0`; `credit_balances.outstanding_paise <= players.credit_limit_paise` unless an override is recorded; `stock.qty >= 0` for tracked products; order totals equal the sum of lines plus GST minus discount; payments on a settled order equal its total.

## Hub intent catalogue

An intent is a typed request; `apply()` is the only function that turns one into rows. The pipeline for every intent is identical: check the actor's permission → check the lease epoch → `BEGIN IMMEDIATE` → load and validate → mutate → append `changes` rows with one `intent_id` → `COMMIT` → emit events to connected sockets. A failure at any step rolls back and returns one `AppError`.

```rust
pub enum Actor { Device { device_id }, Staff { staff_id, device_id }, Command { command_id, issued_by }, System }
pub struct Intent { pub kind: IntentKind, pub actor: Actor, pub rid: Option<String>, pub pin_staff_id: Option<String> }
pub fn apply(ctx: &mut Ctx, intent: Intent) -> Result<Outcome, AppError>
```

`pin_staff_id` is the manager whose PIN was entered for an elevated action; permission is checked against that staff member, and both actors are written to the change log.

| Intent | Permission | Validation (fails with) | Writes | Events |
| --- | --- | --- | --- | --- |
| `player.create` | `player.manage` or Device (kiosk sign-up) | phone unique (`PLAYER_PHONE_EXISTS`), PIN 4–6 digits | players, wallets | — |
| `player.login` | Device | phone + PIN match (`AUTH_FAILED`), lockout after 3 (`AUTH_LOCKED`) | — (read) | — |
| `wallet.topup` | `wallet.topup` | amount > 0, method valid, cash needs open shift (`SHIFT_NOT_OPEN`) | wallet\_txns, wallets, cash\_events, orders+payments (receipt) | `wallet.changed` |
| `credit.set_limit` | `credit.manage` | limit ≥ outstanding (`CREDIT_BELOW_OUTSTANDING`) | players | — |
| `credit.collect` | `credit.collect` | amount ≤ outstanding | wallet\_txns(kind credit), credit\_balances, payments, cash\_events | `credit.changed` |
| `session.start` | `session.start` or Device (prepaid self-start) | device free (`PC_BUSY`), not maintenance (`PC_UNAVAILABLE`), plan allowed for group (`PLAN_NOT_FOR_GROUP`), funds: pack minutes > 0 / wallet ≥ min bill / credit headroom (`INSUFFICIENT_BALANCE`, `CREDIT_LIMIT`), pass within window (`PASS_OUTSIDE_WINDOW`), booking conflict (`DEVICE_RESERVED`) | sessions, session\_segments, wallet\_txns or player\_packs, devices\_meta.state | `session.started`, `pc.power on` |
| `session.pause` / `session.resume` | `session.pause` | state active/paused | sessions | `session.paused` |
| `session.extend` | `session.extend` or Device | funds as above, not past closing (`PAST_CLOSING`) | sessions.ends\_at, wallet\_txns/packs | `session.extended` |
| `session.transfer` | `session.transfer` | target free, group change allowed | session\_segments (close + open), sessions.device\_id, two devices\_meta | `session.moved` to both kiosks |
| `session.end` | `session.end` or Device (player done) or System (timer) | state active/paused; reason required for staff force (`REASON_REQUIRED`); manager PIN for force (`PIN_REQUIRED`) | sessions, orders (postpaid bill line), devices\_meta.state | `session.ended`, `pc.power standby` after grace |
| `session.settle` | `pos.sell` | bill computed; payments sum = total (`PAYMENT_SHORT`); credit path checks limit | payments, cash\_events, orders.state, invoice\_sequences | `receipt.ready` |
| `booking.create` / `booking.cancel` | `booking.manage` | no overlap on device/group capacity (`BOOKING_CONFLICT`) | bookings, orders (deposit) | — |
| `order.create` | `pos.sell` or Device | each product active; stock ≥ qty for tracked (`OUT_OF_STOCK`) with override permission `stock.override`; bundle components checked | orders, order\_lines, kot\_tickets (kitchen items) | `kot.new`, `order.placed` |
| `order.settle` | `pos.sell` | as session.settle; coupon valid (`COUPON_INVALID`) | payments, stock\_moves (if decrement-at-sale), orders | `receipt.ready` |
| `order.void` | `pos.void` | manager PIN; order not settled | orders.state, stock returned | — |
| `refund.create` | `refund.issue` | amount ≤ paid; reason; manager PIN; over owner threshold needs command approval (`APPROVAL_REQUIRED`) | refunds, payments(negative), cash\_events, invoice\_sequences (credit note) | — |
| `kot.advance` | `kitchen.use` | state order new→preparing→ready→served | kot\_tickets, order\_lines.kot\_state; stock\_moves at served | `kot.changed` |
| `stock.in` / `stock.waste` / `stock.adjust` | `stock.adjust` | qty > 0; reason; expiry date valid | stock, stock\_moves | `stock.low` when under threshold |
| `product.upsert` / `plan.upsert` / `rate.upsert` / `rule.upsert` / `coupon.upsert` / `group.upsert` | `catalog.edit` | referential checks; rule window valid | catalogue tables | `catalog.changed` to all kiosks |
| `discount.manual` | `discount.manual` | manager PIN; reason | orders.discount\_paise | — |
| `shift.open` | `shift.open` | no open shift for staff (`SHIFT_ALREADY_OPEN`) | shifts, cash\_events(float\_in) | — |
| `shift.close` | `shift.close` | counted entered; variance over threshold needs `shift.override_variance` PIN + reason | shifts (expected, variance), cash\_events | `shift.closed` (owner alert if over) |
| `cash.deposit` / `expense.create` | `cash.manage` | open shift; amount ≤ expected drawer (`DRAWER_SHORT`) | cash\_events | — |
| `device.register` / `device.assign` / `device.maintenance` / `device.revoke` | `device.manage` | code valid (`PAIR_CODE_INVALID`); group exists | devices, devices\_meta | `epoch.changed` not needed; `device.state` |
| `device.remote` | `device.remote` | action in set | — | `pc.power`, `pc.lock`, `pc.message` to that kiosk |
| `staff.upsert` / `role.upsert` | `user.manage` | login unique; builtin roles immutable | staff, roles, role\_perms | — |
| `settings.set` | `settings.edit` | key in catalogue; value type | settings | `settings.changed` |
| `game.upsert` / `game.launch_report` | `catalog.edit` / Device | exe path on allow-list | games, game\_launches | `catalog.changed` |
| `notify.enqueue` | System | consent present | notify\_outbox | — |
| `backup.export` | `backup.export` | path writable | — (file) | — |
| `lease.promote` | `ops.promote` | current lease stale (`LEASE_FRESH`) | lease | `epoch.changed` |
| `command.apply` wrapper | Command actor | command id not in applied\_commands (`ALREADY_APPLIED`); inner intent validated as above | applied\_commands + inner writes | inner events |

**Permission catalogue** (41): the union of the Permission column plus `report.view`, `audit.view`, `player.view`, `ops.restore`. Built-in roles: Owner (all), Manager (all except `ops.*`, `user.manage` limited to Counter and Kitchen), Counter (`session.*` except force end, `pos.sell`, `wallet.topup`, `credit.collect`, `shift.open`, `shift.close`, `stock.in`, `player.manage`), Kitchen (`kitchen.use`), Device (implicit: `player.login`, `session.start` prepaid, `session.extend`, `session.end` own, `order.create`, `game.launch_report`).

**Validation order inside a session start** (the most complex intent): device state → booking window → plan allowed for group → pricing rule resolution (section 5) → funds → write. Funds are checked last so the error message can name the exact rate the player would have paid.

## Hub engines

Six pieces of logic carry most of the correctness risk; each is specified as an algorithm with its edge cases.

**Rate resolution** (`pricing::resolve(group_id, plan_id?, at_local)`)

1. Base = active `rates` row for the group (postpaid) or the plan's price (prepaid). No base → `NO_RATE`.
2. Candidate rules = active `pricing_rules` whose scope matches (group or plan) and whose window contains `at_local`: weekly rules test `days_mask` and `[start_min, end_min)` with midnight wrap (`end < start` means the window spans midnight and the day tested is the day the window started); date rules test `date_from ≤ date ≤ date_to`.
3. Pick the single highest `priority`; ties broken by `date` over `weekly`, then latest `updated_at`. Only one rule applies; rules never stack.
4. Apply: `override_paise` replaces base; `percent_bp` multiplies (negative = discount). Result rounded to the nearest rupee for display, kept in paise for billing.
5. Return `{rate_paise, rule_id?, next_change_at}`. `next_change_at` is the earliest boundary of any candidate or future rule in the next 24 h; the session engine schedules a segment flip at that instant so a happy hour that starts mid-session changes the rate from that minute.

**Postpaid bill** (`sessions::bill(session)`): for each `session_segment`, minutes = ceil((to − from − paused\_in\_segment) / 60 000); segment cost = minutes × rate / 60. Sum, then apply `min_bill_minutes` on the first segment only and round the total minutes up to `rounding_minutes` on the last segment. Pausing creates no segment; it subtracts time. Transfers close a segment and open one at the new device's rate.

**Session timer engine**

- State: `BinaryHeap<Reverse<(ends_at, session_id)>>` plus a map of warning marks already sent per session. Rebuilt from `sessions WHERE state IN ('active','paused')` on start and after promotion.
- Tick every 1 s: pop entries with `ends_at ≤ now`; for each, `apply(session.end{reason:'expired', actor:System})`. Warnings are emitted (not persisted) at `ends_at − 10 min`, `− 5 min`, `− 60 s` to the owning kiosk; postpaid sessions have no `ends_at` but get a `closing_at` from opening hours and the same warnings.
- Pause removes the entry; resume re-inserts with `ends_at += paused_duration`.
- Grace: after `session.end`, the kiosk receives `session.expire{grace_s}`; the Hub sends `pc.power standby` to the Pi only after grace elapses and only if configured.
- Clock skew: kiosks display the Hub's remaining seconds from `session.tick`, never their own clock.

**Lease** (`tasks::lease`)

```
loop every 3 s:
  row = SELECT * FROM lease WHERE id = 1
  if row.epoch > self.epoch: demote(); break
  UPDATE lease SET holder = self.id, expires_at = now + 15 s WHERE epoch = self.epoch
  if rows_affected == 0: demote(); break
```

Every `apply()` re-reads `lease.epoch` inside its transaction and aborts with `NOT_PRIMARY` if it differs from `self.epoch`. Promotion = `UPDATE lease SET epoch = epoch + 1, holder = self.id WHERE expires_at < now` (fails if fresh), then start the writer.

**Uploader** (`tasks::uploader`, paid tiers)

```
every interval or when pending_rows >= 500:
  rows = SELECT * FROM changes WHERE seq > cursor ORDER BY seq LIMIT 5000
  if empty: continue
  batch = zstd(msgpack(header, rows)); key = tenants/{t}/branches/{b}/changes/{epoch:08}/{first:012}-{last:012}.zst
  url = POST /uploads/presign {key}        -- retry with backoff; stop if offline
  PUT url batch                             -- idempotent on key
  POST /uploads/manifest {key, sha256, first, last, epoch}
  if reply == gap{missing}: re-enqueue missing range first
  UPDATE upload_cursor SET last_seq_uploaded = last
```

The interval floor is 30 s regardless of entitlement. Snapshots: on the 1st of each month at 04:00 local, `VACUUM INTO` a temp file, zstd, upload to `snapshots/`.

**Poller** (`tasks::poller`)

```
every 60 s, or immediately on a poke frame from the tunnel:
  cmds = GET /commands?after={last_id}
  for c in cmds (ordered by id):
    if c.id in applied_commands: report applied (duplicate); continue
    result = apply(command.apply{c})      -- records c.id inside the same tx
    POST /commands/{c.id}/result {status, error?}
```

A command whose inner intent fails validation reports `rejected` with the `AppError` code; a transport failure reporting the result is retried on the next poll (the cloud treats a duplicate result as idempotent).

**Replication** (`tasks::replicator` on the primary, `tasks::follower` on the standby)

- Follower connects to `ws://primary:7400/v1/replicate?from_seq=N&token=...`; the primary streams closed batches first, then live rows as they commit (a broadcast channel from the writer).
- Follower applies rows in a write transaction per batch with the same replayer the ingester uses; its own `changes` table receives the rows verbatim (same `seq`, same `epoch`), so after promotion it continues the sequence.
- Lag = `primary.last_seq − follower.last_seq`, shown in both UIs; promotion warns if lag > 0 and shows the number of rows that would be lost.
- Off-site standby (premium): identical, with the WebSocket replaced by a WebRTC data channel; the control plane relays SDP/ICE; the follower reconnects with exponential backoff and resumes from its cursor.

## Kiosk internals

The Kiosk is a Tauri 2 application: a Rust core that owns the lockdown, the Hub connection and launcher control, and a WebView2 front end (Svelte, same components as the Hub UI) for everything the player sees. Windows 10 and later only; Windows 7 is not supported. A separate Windows service keeps the lockdown enforceable even if the app is killed.

**Processes**

| Process | Account | Role |
| --- | --- | --- |
| `arena-kiosk-svc` | LocalSystem, Windows service (Rust, `windows-service` crate) | Holds the device token, maintains the Hub WebSocket, enforces process allow/deny lists, restarts the app if it dies, applies updates, performs launcher logout, runs the booster; exposes a local named-pipe API to the app |
| `arena-kiosk` (Tauri) | `player` (standard user, registered as the Windows shell for that SID) | Attract screen, login, plan picker, game grid, orders in the main window; the HUD as a second always-on-top Tauri window; talks only to the service over the pipe |

The app and the service share the `arena-proto` and `arena-kiosk-core` crates, so frame handling, cleaners and the booster are one implementation; the Tauri app only adds windows and IPC commands.

**Lockdown mechanics** (Windows 10, 11)

- Shell replacement for the `player` account via the per-user `Winlogon\Shell` value; Explorer never starts for that account; policy keys written by the installer: `DisableTaskMgr`, `NoRun`, `NoClose`, `DisableLockWorkstation`, `DisableChangePassword`, removable drives hidden, USB autorun off.
- Low-level keyboard hook (`SetWindowsHookExW`, in the app process, `windows` crate) swallows `Win` combinations and `Alt+F4` on kiosk windows; `Ctrl+Alt+Del` options are reduced by policy.
- The service subscribes to process creation (ETW or WMI `Win32_ProcessStartTrace`) and terminates any process not on the allow-list within 200 ms; allowed launchers and games are matched by signed path.
- Both kiosk windows are `WS_EX_TOPMOST`; the main window covers the desktop when no session is active; the HUD window is layered, click-through when collapsed, and never takes focus from a game.
- Admin exit: manager PIN in the app → service verifies with the Hub → starts Explorer for 15 minutes with a countdown badge, then re-locks.
- Watchdog: service pings the app every 2 s over the pipe; three misses restart it with the last state.

**Hub connection** (service): `tokio-tungstenite` WebSocket; mDNS via `mdns-sd`, fallback to a stored Hub IP; reconnect with jittered backoff 2–30 s; on `epoch.changed` re-resolve immediately. Every frame carries `rid`; the service times out at 5 s and the app shows "Hub slow" without blocking.

**HUD**: a 44px pill at the top centre in its own Tauri window, expandable to a 320px panel. Games in exclusive fullscreen hide every overlay, so the service also raises warnings as Windows toasts plus an audio cue and, for games flagged in the library as supporting it, briefly forces borderless. Remaining time comes only from `session.tick` frames.

**Launcher logout** (service, at every session end): kill launcher processes; remove Steam `loginusers.vdf` entries and `ssfn*` files, Epic `GameUserSettings.ini` credentials, Riot `Riot Client\Data` session, Discord `Local Storage`, and the Chrome/Edge `player` profile; restart launchers logged out. Each cleaner verifies its result; failures report `device.cleanup_failed` to the Hub and the tile shows a badge until a manager clears it.

**Library scan** (first run and daily): parse Steam `libraryfolders.vdf` and `appmanifest_*.acf`, Epic `Manifests/*.item`, Riot install registry and configured folders; propose `games` rows to the Hub; art is fetched once by the Hub and served to kiosks over LAN.

**Booster** (on launch): close processes on the branch's close-on-launch list, set the game to `ABOVE_NORMAL_PRIORITY_CLASS`, switch to the High Performance power plan, trim standby memory; reverse at session end. Nothing the booster does is required for correctness.

**Orders**: the app renders the menu from `catalog.get`; `order.create` goes through the service; `kot.changed` events show as toasts.

**Updates**: the service checks the Hub (`GET /kiosk/update`) every 6 hours; the Hub serves a signed installer it cached from the cloud (Tauri's updater format with an ed25519 signature plus Authenticode on the binary). Install only when no session is active; keep the previous build for rollback; report version in `hello`.

**WebView2**: Windows 10 1803+ and 11 ship the Evergreen runtime; the installer bundles the Evergreen bootstrapper for machines without it and pins no fixed version. The front end uses no features newer than the runtime floor.

**Diagnostics**: a local ring-buffer log; "send diagnostics" from the Hub pulls the last 5 MB over the LAN socket.

**Minimum spec**: Windows 10 1803 or later, 64-bit, 4 GB RAM; older Windows 7 cafe PCs are not supported and the downloads page says so.

## Pi agent and Android Hub

**Pi agent** (`arena-pi`, Rust, aarch64, Raspberry Pi OS Lite)

| Concern | Design |
| --- | --- |
| Image | Prebuilt image with the agent as a systemd service, read-only root with an overlay, Wi-Fi and Ethernet; first boot shows a pairing screen with its TV number and a 6-digit code field |
| Display | Agent draws directly to the framebuffer (`/dev/fb0`) with a small immediate-mode renderer: QR idle screen, lock screen with countdown, warning screen; fonts from the design system; 48px minimum type |
| CEC | `libcec` via `cec-client` bindings: `on`, `standby`, `set_active_source` to switch the TV between the Pi's HDMI and the console's; capability probed at pairing and reported in `hello` |
| Smart plug | Optional Tuya or Tapo local API module for consoles whose TVs lack CEC; the Hub decides which to use per device |
| Hub link | Same LAN protocol as the Kiosk: `hello` with device token and `kind: pi`; receives `pc.power`, `pc.lock`, `pc.message`, `session.tick`; sends `pc.state`, `controller.check` results |
| Session on console | Start by QR (phone opens `http://hub:7401/q/{device}` on LAN or the Tunnel hostname; login; plan; Hub tells the Pi `pc.power on` + `input console`) or by staff from the Hub. Time-up: `input pi` + lock screen; after grace, `standby` or plug off per config |
| Warnings | At 5 and 1 minutes the Pi switches input to itself for 4 s to show the warning, then back; configurable off for competitive play, in which case the warning goes to the player's phone |
| Offline | Loses the Hub → shows "Hub unreachable" and leaves the console untouched; reconnects with backoff |
| Updates | Agent self-updates from the Hub-served binary with signature check; image updates are manual |

**Android Hub** (Free tier; Kotlin shell around the shared Rust `arena-core` and `arena-store` via JNI)

| Concern | Design |
| --- | --- |
| Process | A foreground service hosts the Rust runtime (Tokio on a dedicated thread), the WebSocket server on `:7400` and the HTTP UI on `:7401`; the Kotlin activity is a WebView onto `localhost:7401`, so the UI is the same Svelte app |
| Storage | SQLite file in app-private storage; export to `Downloads/` or a USB OTG drive for backups; 20 GB free required (installer check) |
| Battery and sleep | `WAKE_LOCK` and battery-optimisation exemption requested at setup; a persistent notification shows branch status; the device is expected to stay on power |
| Network | mDNS via `NsdManager`; LAN only; no tunnel or standby on Android (Windows-only features) |
| Ads | AdMob SDK in the Kotlin shell only: a banner anchored under the Hub header and a rewarded-video entry point the cafe can enable; the Rust core is told `ad.shown{slot, creative}` and logs `ad_impressions`; rewarded completion calls `apply(wallet.reward{minutes})` with a daily cap per player; no ad code in the Rust crates |
| Printing | ESC/POS over Bluetooth SPP or LAN 9100 from the Rust `printer` task; USB not supported on Android |
| Limits | 20 devices, 3 users per the Free entitlement; the same binary enforces it from the cached entitlement |
| Upgrade path | Paying unlocks the Windows Hub; the Android DB exports as a snapshot that the Windows Hub imports on pairing, so no data is lost on upgrade |

## Cloud internals

One Axum binary, `arena-cloud`, started with `--role control|ingest|command|all`. Modules: `tenants`, `auth`, `pairing`, `entitlement`, `billing`, `referrals`, `uploads`, `ingest`, `commands`, `notify`, `signal`, `owner_api`, `metrics`.

**Pairing** (`POST /pair`): look up `pairing_codes` by code with `used_at IS NULL AND expires_at > now` under a transaction; mark used; mint a 32-byte token; store argon2id hash in `hubs` (or `devices` for remote devices); return the token once with branch id, tier and the first entitlement JWT. Rate limit: 5 attempts per code, 20 per IP per hour, via a `rate_limits` table with `expires_at` (no Redis).

**Entitlement** (`POST /heartbeat`): verify hub token; update `last_seen_at`, `epoch`, version, last seq; materialise flags from `subscriptions` + add-ons + tier defaults; sign `{tenant_id, branch_id, tier, flags, upload_interval_s, retention_days, limits, iat, exp = now + 7 d}` with the EdDSA key from the environment; return with `pending_commands` count and `poke` hint. Key rotation: two keys accepted for verification, one for signing, rotated with a Hub release.

**Billing** (`billing`): Razorpay Subscriptions API; one plan per (tier, currency band); mandates created at checkout; webhooks `subscription.activated|charged|halted|cancelled` verified with the webhook secret and applied idempotently by event id (`billing_events` table). A halted subscription moves the tenant to `grace` for 7 days (entitlement still issued), then `lapsed` (free flags). Currency band fixed at sign-up from billing country; price stored on the subscription row.

**Referrals** (`referrals`): tenant sign-up accepts `ref=<code>`; `partners(code, owner_contact, margin_bp = 1000)`; monthly job computes payouts = 10% of charged subscription amounts for tenants with that code while active; `partner_payouts` rows exported to CSV for finance; partner dashboard reads them.

**Uploads and ingester**

```
POST /uploads/presign  -> verify token; check key prefix == tenants/{t}/branches/{b}/; sign PUT for 10 min
POST /uploads/manifest -> insert upload_manifest; if first_seq != cursor.last_seq + 1 for that epoch: return gap{from, to}

ingest loop (per worker, branches sharded by hash(branch_id) % N):
  for each manifest row with ingested_at IS NULL ORDER BY branch_id, epoch, first_seq:
    if epoch < cursor.epoch: mark quarantined; raise owner alert; continue
    if epoch > cursor.epoch and first_seq != 1 and cursor.last_seq > 0: wait (ordering)   -- new epoch continues seq
    fetch batch from R2; verify sha256; decompress; decode
    open branch DB (create + migrate on first use)
    BEGIN; apply rows via replayer (upsert by (tbl,row_id), delete on op D); insert changes rows; COMMIT
    update ingest_cursor; mark manifest ingested
```

The replayer is the same crate the standby uses (`arena-replay`), compiled for libSQL instead of rusqlite via a feature flag. Ingest lag per branch is exposed as a metric and on the owner app.

**Commands** (`commands`): `POST /commands` from the owner API validates the payload against the intent schema (shared `arena-proto`), expands `branch: all` into one row per branch with a `group_id`, and writes rows in each branch's mirror DB. `GET /commands?after=` for Hubs returns pending rows and marks them fetched; `POST /commands/{id}/result` is idempotent. A sweeper marks `fetched` older than 10 minutes back to `pending` and `pending` older than 7 days `expired`. If the Hub has a live tunnel registered, the command API sends a `poke` frame through it.

**Notifications** (`notify`): the ingester watches `notify_outbox` rows arriving in batches; for each `queued` row it calls Gupshup (WhatsApp template; on failure SMS) and writes `sent` or `failed` back as a command to the Hub so the branch sees delivery state. Owner OTP and owner alerts call Gupshup directly. Templates are versioned in config; a template change is a cloud deploy, not a Hub release.

**Signaling** (`signal`, premium): `POST /signal/{pair_id}` stores SDP offers, answers and ICE candidates in a short-lived table; each Hub long-polls `GET /signal/{pair_id}?since=`; `POST /turn/credentials` returns Cloudflare TURN credentials valid 1 hour. Nothing media-related runs on the VPS.

**Owner API** (`owner_api`): session cookies with TOTP on new devices; every handler takes `tenant_id` from the session, never from the request; dashboards query the branch mirror DB with a 30 s in-memory cache keyed by (branch, query, range); exports stream CSV; audit reads the mirrored `changes` table with filters.

**Metrics**: per-branch ingest lag, command backlog, heartbeat age; per-tenant R2 bytes and request counts (cost guard); global webhook failure rate. Alerts page the on-call when many branches miss heartbeats at once.

## Owner app

Flutter, one codebase for Android, iOS and web, structured as feature modules over a thin API client. The app is a reader of mirrors and a writer of commands; it holds no business logic beyond input validation that the cloud repeats.

| Layer | Contents |
| --- | --- |
| `api` | `dio` client with the session cookie, tenant-scoped base URL, typed DTOs generated from `arena-proto`, retry with backoff on 5xx, offline queue for command submissions |
| `state` | `riverpod` providers per screen; a `BranchSummaryProvider` polls every 60 s while the screen is visible; `CommandsProvider` reconciles local pending commands with server state |
| `features` | `home`, `branch`, `revenue`, `cash`, `stock`, `credit`, `players`, `devices`, `staff`, `audit`, `commands`, `billing`, `settings`, `live` (premium) |
| `cache` | SQLite via `drift`: last successful payload per screen with its fetched-at, so the app opens instantly and shows age; cleared on logout |
| `auth` | Login, TOTP enrolment and challenge, `local_auth` biometric unlock, secure storage for the session |
| `notifications` | Push via FCM/APNs for alerts; tapping deep-links to the branch or command |
| `live` | Premium: opens a WebSocket to the branch's Tunnel hostname with a short-lived token from `GET /live/{branch}`; renders the floor view from `session.tick` and device events using the same frame types as the Kiosk |

**Command states in the UI** (driving the "waiting for branch" experience)

| Server state | Shown as | Colour | Action |
| --- | --- | --- | --- |
| local only (no network) | Queued on this phone | `info` | Sends when online |
| `pending` | Waiting for branch · 2 min | `info` | Cancel (if still pending) |
| `fetched` | Applying… | `info` | — |
| `applied` | Applied · 14:32 | `accent` | — |
| `rejected` | Rejected: plan not found | `danger` | Edit and resend |
| `expired` | Expired, branch was offline 7 days | `warn` | Resend |

**Data age**: every number carries `as_of`; the UI shows "2 min ago" in `ink-3` and turns it `warn` past 30 minutes during opening hours, which is also the owner-alert threshold.

**Web build**: same code, deployed to Cloudflare Pages under `app.arena360.app`; MFA and cookies identical; printing of reports uses the browser.

**Sizes**: Android app under 25 MB; first dashboard render under 2 s on a 3-year-old phone over 4G.

## Error codes, events and observability

Error codes are stable strings shared across Hub, Kiosk, cloud and Owner app from `arena-proto`; user-facing text is looked up by code per locale, never sent over the wire.

**Error codes** (grouped; the full enum lives in `arena-proto::ErrorCode`)

| Group | Codes |
| --- | --- |
| Auth | `AUTH_FAILED`, `AUTH_LOCKED`, `TOKEN_INVALID`, `TOKEN_REVOKED`, `PIN_REQUIRED`, `PERMISSION_DENIED`, `MFA_REQUIRED` |
| Lease | `NOT_PRIMARY`, `LEASE_FRESH`, `EPOCH_STALE` |
| Players and money | `PLAYER_PHONE_EXISTS`, `PLAYER_NOT_FOUND`, `INSUFFICIENT_BALANCE`, `CREDIT_LIMIT`, `CREDIT_PAUSED`, `CREDIT_BELOW_OUTSTANDING`, `PAYMENT_SHORT`, `PAYMENT_OVER`, `COUPON_INVALID`, `DRAWER_SHORT` |
| Sessions | `PC_BUSY`, `PC_UNAVAILABLE`, `DEVICE_RESERVED`, `PLAN_NOT_FOR_GROUP`, `PASS_OUTSIDE_WINDOW`, `PAST_CLOSING`, `SESSION_NOT_ACTIVE`, `REASON_REQUIRED`, `NO_RATE`, `BOOKING_CONFLICT` |
| POS and stock | `PRODUCT_INACTIVE`, `OUT_OF_STOCK`, `BUNDLE_SHORT`, `ORDER_SETTLED`, `APPROVAL_REQUIRED`, `KOT_BAD_TRANSITION` |
| Shifts | `SHIFT_NOT_OPEN`, `SHIFT_ALREADY_OPEN`, `VARIANCE_OVER_THRESHOLD` |
| Devices | `PAIR_CODE_INVALID`, `PAIR_CODE_EXPIRED`, `DEVICE_LIMIT`, `USER_LIMIT`, `FEATURE_NOT_ENTITLED` |
| Commands and sync | `ALREADY_APPLIED`, `COMMAND_EXPIRED`, `UPLOAD_GAP`, `BATCH_CHECKSUM`, `EPOCH_QUARANTINED` |
| Transport | `UNSUPPORTED`, `BAD_FRAME`, `RATE_LIMITED`, `INTERNAL` |

**Events** (server-pushed frames on the LAN protocol; also the vocabulary for the owner live view)

`session.started`, `session.tick`, `session.paused`, `session.extended`, `session.moved`, `session.expire`, `session.ended`, `wallet.changed`, `credit.changed`, `order.placed`, `kot.new`, `kot.changed`, `receipt.ready`, `stock.low`, `catalog.changed`, `settings.changed`, `device.state`, `pc.power`, `pc.lock`, `pc.message`, `shift.closed`, `epoch.changed`, `poke`.

**Tracing and logs**

| Where | What | Retention |
| --- | --- | --- |
| Hub | `tracing` JSON lines with intent id, actor, device, duration; ring buffer 50 MB on disk; no PII beyond ids | 14 days local |
| Kiosk | Local ring buffer; crash dumps via Sentry (Win 8+) with PII scrubbed | 7 days local |
| Cloud | OpenTelemetry traces per request with tenant and branch; metrics: ingest lag, command backlog, heartbeat age, R2 ops per tenant, webhook failures | 30 days |
| Owner app | Sentry breadcrumbs, no payload capture | 30 days |

**Health endpoints**: Hub `GET /healthz` (db open, lease state, last upload, standby lag); cloud `GET /healthz` per role; the control plane marks a branch "offline" after 30 minutes without heartbeat or upload during its opening hours.

**Diagnostics bundle**: Hub zips its last 5 MB of logs, `settings`, device list, entitlement claims (not the token), SQLite integrity-check output and kiosk logs pulled over LAN; uploaded to R2 under a support prefix only when staff press "send".

## Migrations, versioning and compatibility

The fleet will always run several versions at once, so every schema and protocol change is additive first and destructive only after the fleet has caught up.

**Schema migrations** (`arena-store::migrations`, numbered SQL files embedded in the binary)

- Forward-only, applied in a transaction on startup; `schema_version` table records each; the Hub refuses to start on a database newer than it knows (`SCHEMA_TOO_NEW`) so a rollback cannot corrupt data.
- Additive rules: new columns are nullable or defaulted; new tables may be created freely; a column is dropped only two releases after nothing reads it; renames are add + backfill + drop across releases.
- The same files run on the cloud mirrors (libSQL) via the feature-flagged store crate, so a mirror is always migrated to at least the batch's version before ingest; a batch from a newer Hub than the cloud supports is held (not quarantined) until the cloud deploys.
- The change-log payload is the full row as a MessagePack map keyed by column name; the replayer ignores unknown columns and fills missing ones with defaults, which is what makes mixed versions safe.

**Protocol versioning** (`arena-proto`)

- Every frame carries `v` (protocol major); unknown message kinds return `UNSUPPORTED`; unknown fields are ignored. A major bump is reserved for a change the old side cannot ignore and has not happened in the plan.
- The Hub accepts Kiosks from the two previous releases; `hello` reports versions and the Hub flags outdated kiosks on the floor view.
- Commands carry `schema_version`; a Hub rejects a command it cannot parse with `UNSUPPORTED`, which the owner app shows as "update the branch".

**Release train**

| Artifact | Cadence | Signed | Rollout |
| --- | --- | --- | --- |
| Cloud binary | Weekly | — | Blue/green on the VPS behind Cloudflare; migrations run first |
| Hub (Windows, Android) | Every two weeks | Authenticode; Play signing | Control-plane flag 1% → 10% → 50% → 100% over 7 days; installs on restart with no active session |
| Kiosk | With the Hub or alone | Authenticode | Served by each Hub after the Hub updates; between sessions only |
| Pi agent | Monthly | ed25519 signature | Served by the Hub |
| Owner app | Every two weeks | Store signing | Store rollout; web immediate |

**Rollback**: previous Hub and Kiosk builds are kept on disk; a rollback is the previous version re-flagged, which works because schema changes are additive. Cloud rollback is the previous image; a migration that must be reversed ships with its down script and is run manually.

**Feature flags**: entitlement flags gate paid features; a separate `features` claim in the same JWT carries rollout flags (`auto_promote`, `decrement_at_sale`, `new_floor_view`) so behaviour can be switched per branch without a release.

## Test plan

Correctness lives in `apply()`, so that is where most tests go; the rest of the plan proves the system survives the real world of cafes.

| Level | What | Tooling | Gate |
| --- | --- | --- | --- |
| Unit | Rate resolution across midnight wraps, date vs weekly priority, rounding and minimum bill; bill computation over segments and pauses; coupon rules; GST and invoice numbering | `cargo test`, table-driven | Every push |
| Property | Random sequences of intents against an in-memory Hub: invariants hold after every step (no negative wallet, stock or credit; one active session per device; order totals balance; `changes` seq gap-free and replay reproduces the DB byte for byte) | `proptest` | Every push |
| Replay | Record real-shaped change logs; replay on rusqlite and libSQL; compare table checksums | Golden files | Every push |
| Contract | Frame schemas and error codes generated from `arena-proto` compile in Rust, C# and Dart; a fixture set of frames round-trips | Codegen + CI | Every push |
| Integration | Docker: cloud binary + MinIO + local libSQL + one Hub + 5 fake kiosks; run the eight journeys end to end including a command round-trip and an owner API read | `cargo test --features integration` | Nightly and before release |
| Failover | Kill the primary mid-session; promote; assert kiosks reconnect under 10 s, timers resume, old primary demotes on return, ingester quarantines a late old-epoch batch | Integration harness | Before release |
| Offline soak | 20 real kiosks on a test LAN, WAN cut for 8 h, 1,000 sessions and 500 orders; zero loss, uploads catch up within 10 min of WAN return | Test cafe | Before release and quarterly |
| Load | 60 devices, 5 intents/s sustained and 50/s burst on a 2015 i3 with HDD | `k6` against the LAN API | Before release |
| Kiosk lockdown | Scripted escape attempts (key combos, task manager, USB, Alt+Tab, process spawn) on Win 7, 10, 11 VMs; manual pen test before GA | Test matrix | Before release; pen test at GA |
| Launcher logout | Log into Steam, Epic, Riot, Discord, Chrome; end session; assert no residual login or saved credential | Test matrix | Every Kiosk release |
| Hardware | Printer list (USB, LAN, Bluetooth), cash drawer kick, scanner, CEC on 5 TV brands, two smart plugs | Test cafe bench | Monthly |
| Restore drill | Snapshot + batches → fresh Hub; checksum equals the source | Integration | Before release |
| Cloud | Webhook idempotency, pairing rate limits, entitlement signing and rotation, ingest sharding, command sweeper, PPP band assignment | `cargo test`, integration | Every push |
| Owner app | Widget tests per screen; command state transitions; offline queue; MFA flow | Flutter test | Every push |
| Android Hub | Instrumented test on a 3 GB device: 20 kiosks, foreground service survives Doze, ad slot renders without blocking `apply` | Device farm | Before release |

**Exit criteria for GA**: all gates green, the 8-hour soak passed twice, pen test findings closed, and 20 beta cafes running wave 1 for 30 days with median shift variance under ₹50.
