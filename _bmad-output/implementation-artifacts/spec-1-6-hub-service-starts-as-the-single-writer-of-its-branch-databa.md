---
title: 'Story 1.6: Hub service starts as the single writer of its branch database'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'ae0340b003ff827504d73bcfd22380e99aeb6245'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub has no database, no lease and no runtime yet. Without a crash-safe store, a fenced single writer and a health endpoint, two writers could corrupt branch data, or a crash could lose committed writes.

**Approach:** `arena-store` opens SQLite (WAL, `synchronous=FULL`) and applies embedded numbered migrations recorded in `schema_version`, refusing a newer schema. `arena-core` owns the writer actor and the shared Hub status. `arena-tasks` runs the lease task: a provisional epoch when offline, renewal every 3 s with a 15 s expiry, and demotion on supersession. `arena-api` serves the loopback `/healthz`. `arena-hub` wires it together with JSON tracing logs in a size- and age-capped ring.

## Boundaries & Constraints

**Always:**
- **Pins:** exact `=` pins in `[workspace.dependencies]` with README rows:
  - tokio 1.54.x (rt-multi-thread, macros, time, sync, signal)
  - axum 0.8.x
  - rusqlite 0.40.x with `bundled`
  - tracing, tracing-subscriber (`json`, `env-filter`) and ulid at current versions
  - dev-only: tempfile
- **Data and log location:** the data dir comes from `--data-dir` or `ARENA_HUB_DATA_DIR`. The default is `%PROGRAMDATA%\Arena360\Hub` on Windows and `$XDG_DATA_HOME/arena360/hub` (fallback `~/.local/share/...`) elsewhere. The database is `<data>/branch.db` and logs go to `<data>/logs/`.
- **`arena-store`:**
  - `Store::open(path)` sets `journal_mode=WAL` and `synchronous=FULL`, and verifies both by reading them back.
  - Migrations are embedded, numbered SQL files (`migrations/0001_init.sql`, ...) in a `const` list, applied in one transaction. Each one inserts `schema_version(version, applied_at)`.
  - If the database's maximum version is greater than the binary's latest, `open` returns `AppError{code: SCHEMA_TOO_NEW}` with both versions in the message, without writing anything (check before any `BEGIN`).
  - Migration 0001 creates:
    - `schema_version`
    - `hub_meta(key TEXT PRIMARY KEY, value TEXT)`, which holds `hub_instance_id`
    - `lease(id INTEGER PRIMARY KEY CHECK(id=1), counter INTEGER NOT NULL, hub_instance_id TEXT NOT NULL, provisional INTEGER NOT NULL CHECK(provisional IN (0,1)), renewed_at INTEGER NOT NULL, expires_at INTEGER NOT NULL)`
  - All SQL lives in this crate.
- **`hub_instance_id`:** a ULID minted once at first start and persisted in `hub_meta`; later starts reuse it.
- **`arena-core`:**
  - `Clock` trait (`now_ms()`), with a `SystemClock` and a test `ManualClock`.
  - `HubStatus` (db_open, role `primary|follower`, lease `{counter, hub_instance_id, provisional, expires_at}` or `None`, schema_version), shared via `tokio::sync::watch`.
  - The `Writer` actor owns the only write connection and runs submitted jobs, each in one `BEGIN IMMEDIATE` transaction. Its `WriterHandle::submit` returns `NOT_PRIMARY` once the writer is closed. The job API is a closure over the transaction; Story 1.7 replaces it with `apply()`.
- **Epoch order:** `(counter, hub_instance_id)`, compared lexicographically.
- **Lease task (`arena-tasks`):** it renews every 3 s and sets a 15 s expiry, both as constants. On start:
  - **Empty table:** mint a provisional `(1, own_id)`, take the lease and become primary. No cloud is contacted in this story.
  - **Row held by self, or expired:** take it or renew it (keep the counter) and become primary.
  - **Row held by another instance, with a higher epoch, not expired:** become a follower. Open no writer and never write the lease.
  - **On each tick as primary,** re-read the row inside the renewing transaction. If another epoch is higher, demote: close the writer, set the role to follower and stop renewing, all within that tick.
  - **A follower never takes over automatically.** Promotion is manual and comes in a later story.
- **`/healthz` (`arena-api`, axum):** binds `127.0.0.1` only, on port `7409` (overridable with `--health-port`). `GET /healthz` returns JSON: `{db_open, role, lease: {counter, hub_instance_id, provisional, expires_at}|null, epoch: "counter:hub_instance_id"|null, schema_version}`.
- **Logs:**
  - `tracing-subscriber` JSON lines go to `<data>/logs/hub-YYYY-MM-DD.jsonl` through a custom writer that rolls daily.
  - On rollover and at start, files older than 14 days are deleted, and then the oldest files are deleted until the total is ≤ 50 MB.
  - Log fields are ids, codes and counts only (no names, phones or emails). A comment states the no-PII rule.
- **Crash safety test:** an integration test spawns a child process (the test binary re-invoked with an env flag) that commits a row and prints `COMMITTED`. The parent kills it (`Child::kill`, which is SIGKILL or TerminateProcess) as soon as it reads that line, then reopens the database and asserts the row exists and `PRAGMA integrity_check` returns `ok`.
- **`arena-hub` main:** parse args, init logs, open the store (on `SCHEMA_TOO_NEW`, log both versions and exit with code 2), load or mint the instance id, start the lease task and healthz, and handle Ctrl+C gracefully.
- **Layer table:** the dependencies respect it (`pnpm check:layers` must pass): store → proto; core → store, proto; tasks → core, store, proto; api → core, proto; hub → any.

**Never:**
- Cloud lease or epoch calls (later epics), apply()/change log/intents (Stories 1.7–1.8), the LAN WebSocket (1.9).
- Automatic follower takeover.
- Any non-loopback listener in this story.
- PII in logs.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First start | no database file | file created; WAL plus FULL verified; migrations recorded; instance id minted | N/A |
| Restart | an existing database | same instance id; no migration re-applied | N/A |
| Schema too new | `schema_version` has 999 | `open` fails with SCHEMA_TOO_NEW naming 999 and the binary's version; file bytes unchanged | exit code 2 |
| Empty lease | first start | provisional `(1,id)`, `provisional=1`, primary, `expires_at = now + 15 s` | N/A |
| Renewal | 3 s pass (ManualClock / paused time) | `renewed_at` and `expires_at` advance | N/A |
| Other holder, higher, fresh | a row `(5,"Z…")` with a future expiry | follower; no writer; the lease row is not modified | N/A |
| Other holder, expired | the row's `expires_at` is in the past | takes the lease, primary | N/A |
| Superseded | a higher-epoch row is written while primary | the writer closes within one tick; `submit` → NOT_PRIMARY; role follower | N/A |
| Healthz | `GET 127.0.0.1:7409/healthz` | the JSON fields above | N/A |
| Crash | child killed right after a commit | the row is present; integrity_check `ok` | N/A |
| Log ring | files older than 14 days, or more than 50 MB in total | the oldest files are deleted until within limits | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-{store,core,tasks,api,hub}/` -- placeholders with one test each, already depending on each other per the layer table. Replace the placeholder bodies and keep a crate-level test.
- `crates/arena-proto/src/error.rs` -- `AppError` and `ErrorCode` (has `SCHEMA_TOO_NEW` and `NOT_PRIMARY`). Use them. Adding a code requires `pnpm gen:types`.
- `Cargo.toml` -- `[workspace.dependencies]` exact `=` pins, checked by `scripts/check-pins.mjs` against the README pin table. Add rows for every new pin.
- `.github/workflows/ci.yml` -- the rust job runs `cargo test --locked --workspace` on Linux and Windows. Tests must pass on both (path handling, no Unix-only signals in tests; use `Child::kill`).

## Tasks & Acceptance

**Execution:**
- [x] `Cargo.toml`, `README.md` -- pins and rows.
- [x] `crates/arena-store/{src/lib.rs,src/migrations.rs,migrations/0001_init.sql,tests/crash.rs}` -- open, pragmas, migrations, schema guard, lease and meta queries, the crash test.
- [x] `crates/arena-core/src/{clock.rs,status.rs,writer.rs}` -- Clock, HubStatus, the Writer actor (+ tests: submit after close → NOT_PRIMARY).
- [x] `crates/arena-tasks/src/lease.rs` -- the lease task with paused-time tests for every lease matrix row.
- [x] `crates/arena-api/src/health.rs` -- the axum router and a handler test (tower `oneshot`).
- [x] `crates/arena-hub/src/{main.rs,config.rs,logs.rs}` -- wiring, args and data dir, the log ring writer (+ retention tests on a temp dir).

**Acceptance Criteria:**
- Given the workspace, when `cargo test --workspace --locked` runs on Linux and Windows, then every matrix row is covered by a passing test.
- Given `cargo run -p arena-hub -- --data-dir <tmp>`, when `curl 127.0.0.1:7409/healthz` is run, then it reports `role: primary`, a provisional epoch and `schema_version: 1`. A second process on the same dir reports `role: follower` on `--health-port 7410`.
- Given the repo, when `pnpm check` runs, then it passes (layers, pins, fmt, clippy).

## Implementation Notes

- **Process writer lock.** Two processes on one data dir share one `hub_instance_id`, so the lease row alone cannot tell them apart (both would match "held by self"). `Store::try_lock_writer` takes an exclusive OS lock on `<db>.lock` (std `File::try_lock`), which the OS releases when the process dies. A process that cannot take it is a follower and never writes the lease. The lock moves into the Writer with the connection and is released when the writer closes.
- **Taking an expired row from another instance bumps the counter** (`counter + 1`, provisional). Keeping the counter could give a lower epoch than the old holder's, and a paused old primary would then not see a higher epoch and would keep renewing. A renewal of our own row keeps the counter.
- **Review fixes:** `Store::open(path, now_ms)` takes the writer lock before migrating; without it the store is a read-only follower that never migrates or mints. A renewal demotes on any row that is not exactly its own epoch, including a missing row, and on its own expiry after failed renewals. The writer survives job panics (`INTERNAL`). `/healthz` adds `self_instance_id` and `now`. `db_open` turns false only on shutdown or writer-thread death. Logs roll to `.N` part files when the ring is full.
- **A fresh row held by another instance always makes a follower,** whatever its epoch, so a start never pre-empts a live primary.
- **New error code `INTERNAL`** for storage and I/O failures (`pnpm gen:types` was run). Also added `ErrorCode::as_str`, `AppError::new` and `Display`.
- **Lease renewals go through the Writer** (one `BEGIN IMMEDIATE` that re-reads the row and either renews or reports supersession), so the writer keeps the only write connection. The start-up decision runs on the same connection before it is handed to the Writer.
- **Extra pins:** `tower =0.5.3` (dev-only, for `oneshot`). arena-api enables tokio `net`; arena-tasks enables `test-util` for its dev tests.
- **Log retention** runs at start, on rollover and whenever the ring passes 50 MB, which also rolls to a new part file. The active file is never deleted. "Older than 14 days" means an age of more than 14 days, judged from the file name's date.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Primary never demotes when its own lease expires; renewal errors only log (blind, edge) | high | `run` logs `Err(e)` and loops. Past `expires_at`, another instance can take the row while this writer stays open. | patch |
| 2 | `migrate` and instance-id minting write before the writer lock is held (blind, edge) | medium | `main::run` opens and mints before `try_lock_writer`. A follower process writes, and a newer binary could migrate under a running primary. | patch |
| 3 | A writer-job panic kills the thread and is reported as `NOT_PRIMARY`, which then logs a supersession (blind, edge) | medium | No `catch_unwind`; a channel error maps to `not_primary()`. | patch |
| 4 | Only Ctrl+C handled; health server and lease errors dropped; `/healthz` says primary after the writer closes (blind, edge) | medium | `let _ = server.await`; no SIGTERM; `close()` runs before the role update. | patch |
| 5 | `arena-store` reads the system clock for `applied_at` | low | Breaks the injected-clock rule. | patch |
| 6 | `/healthz` lacks this hub's own id and the server `now`, so an expired lease can't be spotted | low | A follower shows the holder's id only. | patch |
| 7 | `db_open` is always true | low | Never updated after the writer closes. | patch |
| 8 | Today's log file has no size limit until rollover | medium | A log storm can exceed 50 MB within one day. | patch |
| 9 | Future-dated log files are never aged out (blind, edge) | low | `today - day` is never above the limit for future dates. | patch |
| 10 | Empty `--data-dir=`, a flag taken as a value, no `--help` (blind, edge) | low | Data lands in the current dir. | patch |
| 11 | Crash test has no timeout; can't tell FULL from OFF | low | A hang blocks CI. FULL vs OFF needs a power cut, so only the timeout part is fixed. | patch (timeout) |
| 12 | SCHEMA_TOO_NEW test ignores the `-wal`/`-shm` files | low | Marginal. | reject |
| 13 | Renewal overwrites a missing, lower or equal row held by another instance (blind, edge) | medium | Supersession is detected only for a higher epoch. | patch |
| 14 | `Writer::spawn` doesn't require the lock at the type level | low | Single call site. | reject |
| 15 | No binary test for exit codes 2 and 64 (blind, verify-gap, edge) | low | The matrix row's exit code isn't pinned by any test. | patch |
| 16 | README lacks run docs (flags, env, ports, logs, exit codes) | low | Direct fix. | patch |
| 17 | Writer never times out (`HUB_SLOW`) | low | Client timeouts belong to the protocol work in Story 1.9. | reject |
| 18 | Renewal reads `now` before queuing | low | Read it inside the job. | patch |
| 19 | Counter overflow at `i64::MAX` | low | Unreachable. | reject |
| 20 | `hub_meta` NULL value blocks start | low | Unreachable through code paths. | reject |
| 21 | `ctrl_c()` error causes immediate shutdown | low | Covered by #4. | patch |
| 22 | Log retention `metadata`/`remove_file` errors abort start | low | Continue per file. | patch |
| 23 | Huge year in a log file name overflows | low | Bound the year. | patch |
| 24 | Most lease tests use ManualClock, not paused time | low | Row behaviour is covered either way. | reject |
| 25 | A job queued before `close()` is untested (verify-gap) | low | Pre-verified gap. | patch |
| 26 | `tick` NotPrimary arm untested (verify-gap) | low | Pre-verified gap. | patch |
| 27 | `--health-port 0` rejection untested (verify-gap) | low | Pre-verified gap. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked` -- expected: pass
- `pnpm check` -- expected: exit 0
- Manual: run two `arena-hub` processes on one data dir and curl both healthz ports -- expected: primary + follower.
