---
title: 'Story 1.7: Every change goes through apply() into a gap-free change log, starting with a seeded branch'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'b68a57b3d2a383d0007a7e0d68bbf93a13711923'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-6-hub-service-starts-as-the-single-writer-of-its-branch-databa.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub can open its database and hold the lease, but there is no single mutation path. Nothing records who changed what in order, and there is no branch to work on yet.

**Approach:** Define the intent envelope and typed actor in `arena-proto`. Implement `apply()` in `arena-core` as the fixed pipeline, run by the Writer actor in one `BEGIN IMMEDIATE` transaction. Add an append-only `changes` table with gap-free per-epoch `seq`, the `branch.bootstrap` intent and a dev-only `arena-hub dev seed`. Add a replayer in `arena-replay` that rebuilds rows from `changes`.

## Boundaries & Constraints

**Always:**
- **`arena-proto` (all types exported with ts-rs; regenerate with `pnpm gen:types`):**
  - `Actor { kind: ActorKind (staff|owner|admin|system|device), id: String }`
  - `ElevatedBy { kind, id, mode: hub|cached_verifier, verifier_version }`, an optional field on intents, unused here.
  - `Intent { intent_id: Ulid string, kind: IntentKind, idempotency_key: String, issued_at: i64 ms, base_rev: Option<i64>, actor: Actor, elevated_by: Option<ElevatedBy>, payload: IntentPayload }`
  - `IntentKind` serializes as `domain.verb`, starting with `branch.bootstrap`.
  - `IntentPayload` is tagged by kind. `BranchBootstrap { name, branch_code, currency (ISO 4217 alpha-3), tz (IANA), day_start_min (0–1439), fy_start_month (1–12) }`.
  - New `ErrorCode`s: `VALIDATION_FAILED` and `ALREADY_BOOTSTRAPPED`.
- **Migration 0002 (`arena-store`, all SQL here):**
  - `branch(branch_id TEXT PK, name, branch_code, currency, tz, day_start_min, fy_start_month, rev INTEGER NOT NULL, created_at, updated_at)`
  - `settings(branch_id, key, value TEXT, rev, updated_at, PRIMARY KEY(branch_id, key))`
  - `changes(epoch TEXT NOT NULL, seq INTEGER NOT NULL, tbl, row_id, op CHECK(op IN ('insert','update','delete')), row BLOB NOT NULL, intent_id, intent_kind, actor_kind CHECK(...5 kinds), actor_id NOT NULL, elevated_by_kind/_id/_mode/_verifier_version NULL, at INTEGER NOT NULL, PRIMARY KEY(epoch, seq))`
  - `BEFORE UPDATE` and `BEFORE DELETE` triggers on `changes` that `RAISE(ABORT, 'changes is append-only')`.
  - Every branch row carries `branch_id`. Enums are TEXT with CHECK constraints.
- **`apply()` (`arena-core`):**
  - Signature: `apply(tx: &mut StoreTx, ctx: &ApplyCtx{clock, epoch, role}, intent) -> Result<Applied, AppError>`.
  - Steps run in exactly this order, each a named function:
    - **permission:** in this story, `branch.bootstrap` requires `actor.kind == system` (else `FORBIDDEN`). Story 1.11 replaces it with the catalogue.
    - **epoch:** `NOT_PRIMARY` if the role isn't primary or the ctx epoch no longer matches the lease row read inside the transaction.
    - **ttl_dedupe:** a pass-through stub, documented as Story 1.8's.
    - **base_rev:** for edit intents a mismatch gives `STALE`. Bootstrap has none.
    - **validate:** `VALIDATION_FAILED` with `field` set. Checks: currency is a known ISO 4217 code from a small const table (at least INR, USD, EUR, GBP, AED, SGD); tz is non-empty and contains `/`; ranges as above.
    - **mutate:** bootstrap fails `ALREADY_BOOTSTRAPPED` if a `branch` row exists. Otherwise it inserts the branch (a writer-minted ULID) and the default settings.
    - **stamp:** `rev=1` on insert and +1 on update; `created_at`/`updated_at` from the clock.
    - **append_changes:** one row per written row, with `seq = max(seq for epoch) + 1`, and the full row after the write as a MessagePack map (`rmp_serde::to_vec_named`).
    - **commit,** then **emit** on a `tokio::sync::broadcast` of `Applied { intent_id, changes: Vec<ChangeMeta> }`.
  - No I/O other than SQLite inside the transaction. Any error rolls back and returns exactly one `AppError`.
- **Writer:** the Writer from 1.6 gains `submit_intent(intent) -> Result<Applied, AppError>`, which runs `apply()` inside its `BEGIN IMMEDIATE` job. The closure API stays private to tests.
- **Default settings:** a const list in `arena-core`, with at least `idle_lock_minutes=5`, `hub.locale=en-IN` and `receipt.footer=""`. Every value is TEXT.
- **Dev seed:**
  - `arena-hub dev seed [--data-dir]` exists only with Cargo feature `dev-fixtures`, via `#[cfg(feature = "dev-fixtures")]` on the subcommand and its module.
  - It opens the store, takes the lease as in 1.6 (one-shot) and submits `branch.bootstrap` as `actor{system, "dev-seed"}` with fixed values: name "Dev Branch", code "DEV01", INR, Asia/Kolkata, `day_start_min` 360, `fy_start_month` 4.
  - It prints the outcome. It exits 0 on success and non-zero on `ALREADY_BOOTSTRAPPED` or `NOT_PRIMARY`.
  - A test asserts that a default-feature build doesn't recognise `dev`.
- **`arena-replay`:** `replay(changes_iter, dest: &mut StoreTx)` applies each change (insert or update means upsert of the msgpack row into `tbl`; delete removes it) in `(epoch, seq)` order onto a freshly migrated empty database. The table name is checked against an allow-list (`branch`, `settings`).
- **CI:** the rust job adds `cargo test --locked -p arena-hub --features dev-fixtures`.
- **Pins:** `rmp-serde` 1.3.x, exact, with a README row.

**Never:**
- Real permissions or roles (Story 1.11), dedupe or TTL logic (1.8), LAN submission (1.9).
- `dev seed` in release or default builds.
- Writing to any table except through `apply()`. Migrations and the lease are the only exceptions.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Bootstrap | empty db, primary, system actor | a branch row plus settings rows; one change row each; `seq` 1..n gap-free; `emit` fires once | N/A |
| Again | already bootstrapped | `ALREADY_BOOTSTRAPPED`; no new rows or changes | rollback |
| Wrong actor | `actor.kind = staff` | `FORBIDDEN`; nothing written | rollback |
| Follower | role follower | `NOT_PRIMARY`; nothing written | N/A |
| Superseded | the lease row now holds a higher epoch | `NOT_PRIMARY`; nothing written | rollback |
| Bad currency | `"XXX1"` | `VALIDATION_FAILED`, `field="currency"` | rollback |
| Mid-pipeline failure | an injected failure after mutate | no branch row, no change rows | exactly one AppError |
| Append-only | `UPDATE changes ...` / `DELETE FROM changes` | SQLite aborts | N/A |
| Replay | the seeded database's changes replayed into an empty database | the `branch` and `settings` rows are equal | N/A |
| Release | default features | `arena-hub dev seed` is an unknown command | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-store` (1.6) -- `Store::open`, the migrations const list, the lease and meta queries. Add migration 0002 and the query functions used by `apply()`. All SQL stays here.
- `crates/arena-core` (1.6) -- the Clock, HubStatus and Writer actor (BEGIN IMMEDIATE jobs, `NOT_PRIMARY` after close). Add `apply/` modules for the steps.
- `crates/arena-hub/src/{main.rs,config.rs}` -- argument parsing. Add the `dev` subcommand behind the feature.
- `crates/arena-proto/src/error.rs` -- add the codes, then `pnpm gen:types` and commit `packages/ui/src/generated`. `pnpm check:generated` must pass.
- `crates/arena-replay` -- the placeholder; it depends on store and proto (layer table).

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/{intent.rs,actor.rs,error.rs}` + regenerated `packages/ui/src/generated/*` -- the types.
- [x] `crates/arena-store/{migrations/0002_*.sql,src/*}` -- tables, triggers and queries.
- [x] `crates/arena-core/src/apply/*`, `writer.rs`, `settings_defaults.rs` -- the pipeline and its tests, one per matrix row.
- [x] `crates/arena-replay/src/lib.rs` -- the replayer and the replay-equality test.
- [x] `crates/arena-hub/{Cargo.toml,src/dev.rs,src/main.rs}` -- the `dev-fixtures` feature and the seed command, with a test.
- [x] `Cargo.toml`, `README.md`, `.github/workflows/ci.yml` -- the pin, the docs (dev seed usage) and the CI step.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` and `cargo test --locked -p arena-hub --features dev-fixtures`, then every matrix row has a passing test.
- Given `cargo run -p arena-hub --features dev-fixtures -- dev seed --data-dir <tmp>` run twice, then the first succeeds and the second exits non-zero with `ALREADY_BOOTSTRAPPED`.
- Given the repo, when `pnpm check` runs, then it passes, including `check:generated` and `check:layers`.

## Implementation Notes

- **Commit and emit are run by the Writer.** `apply(tx: &mut StoreTx, ctx, &intent)` runs permission → append_changes on the caller's transaction, because a `&mut` transaction cannot commit itself. `WriterHandle::submit_intent` commits it with `Store::write_tx_mut` and then calls `apply::emit` on the writer thread, so events keep commit order. `WriterHandle::subscribe()` returns the broadcast receiver.
- **`Writer::spawn(store, clock, status_rx)`.** Each intent builds its `ApplyCtx` from the `HubStatus` watch when it runs: the role, plus `epoch` from the lease, which is `None` when no lease is held and then fails `NOT_PRIMARY`.
- **Closure API is test-only.** `WriterHandle::submit` is `cfg(any(test, feature = "test-jobs"))`. arena-tasks enables `test-jobs` in its dev-dependencies only. Lease renewal now uses a dedicated `WriterHandle::renew_lease`, with the same re-read-then-renew logic and the `LeaseRenewal` enum moved to core. The tasks test hook for injected failures now short-circuits before the call.
- **Stamp is a separate write.** mutate inserts rows with `rev = 0` and zero timestamps. stamp runs `rev = rev + 1` and sets `updated_at`, and `created_at` on insert. This gives `rev = 1` on insert and +1 on update through the same code.
- **Permission uses the payload's kind,** so a mislabelled envelope can't borrow another kind's permission. validate also rejects `intent.kind != payload.kind` (`field = "kind"`). An actor with an empty id is `FORBIDDEN`.
- **Validation extras:** `name` and `branch_code` must not be blank. The currency table has 20 codes, including the six required.
- **`changes.row_id`** is the key columns joined by `/` (`<branch_id>` and `<branch_id>/<key>`). The MessagePack row is a `BTreeMap<String, RowValue>` (untagged Null/Integer/Real/Text). BLOB columns are rejected until a table needs them.
- **Replay** sorts by `(counter numeric, hub_instance_id, seq)`, because epoch text order would sort `10:` before `9:`. It refuses a `seq` gap within an epoch, a malformed epoch or row, and any table outside `branch`/`settings`. It also checks column names against `pragma_table_info` (in arena-store) before building SQL.
- **`dev seed`** prints `dev seed: <CODE>: <message>` to stderr. It exits 1 on `ALREADY_BOOTSTRAPPED` or `NOT_PRIMARY` and 2 on `SCHEMA_TOO_NEW`. `--health-port` is not accepted after `dev seed`.
- **Not via apply():** `hub_meta.hub_instance_id` minting (Story 1.6) still writes directly at start-up, next to migrations and the lease.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Delete change reads the row after deleting it, so every delete intent would fail (blind, edge) | medium | `append_changes` calls `read_row` after mutate; no pre-image is kept. | patch |
| 2 | Unverified `elevated_by` written to the audit log (blind, edge) | medium | Nothing checks it before Story 1.13, and a test asserts it is recorded. | patch |
| 3 | `elevated_by_*` columns not all-or-none | low | No table CHECK. | patch |
| 4 | tz check is "contains '/'", which accepts `A/B` and rejects `UTC` (all three layers) | medium | Wrong zones will break business-day maths in 1.12. | patch (chrono-tz) |
| 5 | `intent_id` not checked as a ULID; `base_rev` allowed on bootstrap | low | Empty ids reach `changes`. | patch |
| 6 | kind/payload mismatch check is dead code | low | Only one kind exists; it becomes testable in 1.8. | reject |
| 7 | Epoch guard ignores lease expiry (blind, edge) | medium | A writer with an expired lease that hasn't been taken over keeps committing. | patch |
| 8 | Replay doesn't check that `row_id` matches the row's key, that dest is empty, or that a deleted row exists (blind, edge) | low | Cheap consistency checks. | patch |
| 9 | Duplicate replay allow-list; `created_at` hard-coded per table | low | Can drift when a table is added. | patch |
| 10 | No multi-epoch replay test | low | No update intent exists yet; Story 1.8 adds `settings.set` and the test. | defer (1.8) |
| 11 | `dev seed` leaves the lease held for 15 s | low | A real Hub started right after must wait. | patch |
| 12 | Dev-seed exit codes not pinned (blind, verify-gap) | low | Tests only check non-zero. | patch |
| 13 | CI never runs clippy with `dev-fixtures` | low | Feature code is never linted. | patch |
| 14 | Broadcast lag undocumented | low | Bounded at 1024. | patch (doc) |
| 15 | A panic after commit is reported as INTERNAL | low | `emit` runs inside `catch_unwind`. | patch |
| 16 | `PRAGMA foreign_keys` never enabled | medium | `settings.branch_id` REFERENCES is unenforced. | patch |
| 17 | Non-UTF-8 TEXT decoded lossily | low | Writes come only from Rust strings. | reject |
| 18 | `Store::changes()` orders epochs as text | low | Order `10:` < `9:`. | patch |
| 19 | Lease failure injection moved out of the writer job | low | Tests still cover the demotion behaviour. | reject |
| 20 | v1→v2 migration upgrade untested (verify-gap) | medium | Pre-verified; a 1.6 database must open on this binary. | patch |
| 21 | `name`/`branch_code` length and charset unchecked | low | Direct bounds. | patch |
| 22 | README seeding gaps (lease, first arg) | low | Direct doc fix. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check` -- expected: exit 0
