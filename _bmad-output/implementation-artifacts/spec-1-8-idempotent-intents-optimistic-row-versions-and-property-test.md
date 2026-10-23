---
title: 'Story 1.8: Idempotent intents, optimistic row versions and property tests on apply()'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '02024c39bff076d3db86feade8c079192547384a'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-7-every-change-goes-through-apply-into-a-gap-free-change-log-s.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `apply()` has a pass-through TTL/dedupe stub and has never exercised `base_rev` on an edit. Retried, late or stale intents could double-apply or silently overwrite newer edits.

**Approach:** Validate the idempotency key before any transaction, fill in the TTL/dedupe step with an `applied_intents` table, enforce `base_rev`/`rev` on edits through a first edit intent (`settings.set`), add daily dedupe housekeeping, and add a proptest suite over random intent sequences that checks the log invariants.

## Boundaries & Constraints

**Always:**
- **Pre-transaction frame check:** `idempotency_key` must be a valid ULID string (26 Crockford base32 characters). If it is missing, empty or malformed, `submit_intent` returns `BAD_FRAME` (with `field: "idempotency_key"`) before the Writer opens any transaction. Put the check in a pure `validate_frame(&Intent)`.
- **Migration 0003:** `applied_intents(idempotency_key TEXT PRIMARY KEY, intent_id, intent_kind, outcome BLOB NOT NULL, applied_at INTEGER NOT NULL)`, plus an index on `applied_at`. `outcome` is the MessagePack of the stored result: either the `Applied` summary or the `AppError`.
- **ttl_dedupe step** (fills the 1.7 stub, staying at its fixed position after epoch):
  - **TTL:** if `now - issued_at > 7 days` (const `INTENT_TTL_MS`), fail with `EXPIRED`. A future `issued_at` beyond a 5-minute skew (const) fails with `VALIDATION_FAILED` on `issued_at`.
  - **Dedupe:** if `applied_intents` already has the key, return the stored outcome unchanged. A stored success returns the same `Applied`; a stored error returns the same `AppError`. Nothing else is written, and no change rows appear.
  - **Recording:** after a successful mutate, record the success in the same transaction. Deterministic business rejections (`VALIDATION_FAILED`, `STALE`, `ALREADY_BOOTSTRAPPED`, `FORBIDDEN`) are recorded in a separate short transaction after the rollback, so a retry returns the same outcome. `NOT_PRIMARY`, `EXPIRED` and `INTERNAL` are never recorded.
- **New intent `settings.set`:**
  - Payload: `SettingsSet { key, value }`. `base_rev` is required, so a missing one fails `VALIDATION_FAILED` on `base_rev`.
  - Permission stub until Story 1.11: `actor.kind` must be in {system, owner, staff}; device and admin get `FORBIDDEN`.
  - Validate: the key must exist in `settings`; otherwise `VALIDATION_FAILED` on `key`.
  - base_rev step: if it differs from the row's `rev`, fail `STALE` and change nothing.
  - Mutate: update `value`, `rev += 1`, `updated_at`, and append one `update` change row.
  - Regenerate TS types.
- **Housekeeping:** `HousekeepingTask` lives in `arena-tasks`. It runs at start, every 24 h, and on each promotion to primary (via `status.changed()`). It exits when the writer closes. It prunes in batches of `PRUNE_BATCH` (500) through `WriterHandle::prune_applied_intents(cutoff, limit)`: one writer job per batch, each re-checking the epoch, until a batch comes back short. The cutoff is a strict `<`, so a record exactly 30 days old is kept. `crates/arena-hub/tests/housekeeping.rs` checks it on the real binary.
- **Property tests (`proptest`, exact pin, with a README row):** in `arena-core`, against an in-memory Hub (`Store` on a temp file or `:memory:`, plus a ManualClock).
  - They generate random sequences of: bootstrap, `settings.set` with a correct, stale or missing `base_rev`, duplicates of earlier idempotency keys, expired `issued_at`, and malformed keys.
  - After every step they assert:
    - `changes.seq` is 1..n with no gaps for the epoch;
    - a duplicate key yields the same outcome as the first time, and the change count doesn't grow;
    - a stale `base_rev` is rejected and the row is unchanged;
    - a successful edit bumps `rev` by exactly 1.
  - After the sequence, replaying `changes` with `arena-replay` into an empty database reproduces the `branch` and `settings` rows.
  - Use a modest case count (e.g. 64), configurable with `PROPTEST_CASES`. The suite runs in the normal `cargo test --workspace`.

**Never:**
- Real roles or permissions (Story 1.11).
- LAN submission (Story 1.9).
- Recording transient errors.
- Changing the 1.7 step order.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Duplicate success | resubmit an applied `settings.set` key | same `Applied`; no new change rows; the value is unchanged from the first apply | N/A |
| Duplicate rejection | resubmit a key that was rejected STALE | same `STALE` error; nothing written | N/A |
| Expired | `issued_at = now - 7d - 1ms` | `EXPIRED`; not recorded | rollback |
| Future skew | `issued_at = now + 10 min` | `VALIDATION_FAILED` `issued_at` | rollback |
| Stale | `base_rev = rev - 1` | `STALE`; the row is unchanged | rollback |
| Fresh edit | `base_rev = rev` | `rev + 1`; one update change row | N/A |
| Missing key | `idempotency_key = ""` or `"not-a-ulid"` | `BAD_FRAME`; no transaction opened (assert the writer job count is unchanged) | N/A |
| Prune | rows aged 31 d and 29 d | the 31-day row is deleted and the 29-day row kept | N/A |
| Property | random sequences | all invariants hold after every step; replay equals the source | proptest shrinks on failure |

</frozen-after-approval>

## Code Map

- `crates/arena-core/src/apply/*` (1.7) -- the pipeline with the `ttl_dedupe` stub, the base_rev step and the stamp step. Fill in the stub, and add the `settings.set` kind across the proto, validate, mutate and permission steps.
- `crates/arena-core/src/writer.rs` -- `submit_intent`. Run `validate_frame` before queuing.
- `crates/arena-store` -- add migration 0003 plus the applied_intents and settings-update queries. All SQL stays here.
- `crates/arena-replay` -- the replayer used by the property test.
- `crates/arena-tasks` -- the lease task pattern (ManualClock, tick). Follow it for housekeeping.
- `crates/arena-proto` + `pnpm gen:types` -- regenerate and commit `packages/ui/src/generated`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/intent.rs` + generated TS -- `settings.set`.
- [x] `crates/arena-store/{migrations/0003_applied_intents.sql,src/*}` -- the table and queries.
- [x] `crates/arena-core/src/apply/{frame.rs,ttl_dedupe.rs,settings_set.rs}`, `writer.rs` -- the logic, with a test for each matrix row.
- [x] `crates/arena-tasks/src/housekeeping.rs` -- the prune task and its tests.
- [x] `crates/arena-core/tests/proptest_apply.rs` -- the property suite.
- [x] `crates/arena-replay` tests -- add `multi_epoch_replay_orders_numerically_and_resets_seq` (epochs `9:Z` and `10:A` updating the same settings row; the later epoch wins; an epoch starting at seq 2 is refused). Deferred from 1.7 review #10.
- [x] `Cargo.toml`, `README.md` -- the proptest pin and its row.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked`, then every matrix row and the property suite pass on Linux and Windows.
- Given `pnpm check`, then it passes, including generated freshness.

## Implementation Notes

- **Dedupe runs before the TTL check** inside ttl_dedupe. A retry of a recorded key always gets its first outcome back, even after the intent is past its 7-day TTL. Records are kept for 30 days, longer than the TTL. An unrecorded key is TTL-checked as specified.
- **Recording is an allow-list** (`is_recordable`, review fix): `STALE`, `ALREADY_BOOTSTRAPPED`, and `VALIDATION_FAILED` only on pure-payload fields (`value`, `base_rev`, `currency`, `tz`, `name`, `branch_code`, `day_start_min`, `fy_start_month`, `intent_id`). Permission denials (`FORBIDDEN`) are never recorded, so a later grant takes effect. Validation that depends on time or on other rows (`issued_at`, `key`, `kind`) is never recorded either. So `settings.set` before bootstrap fails on `key`, and a retry after bootstrap is evaluated afresh.
- **Fingerprint** (review fix): `applied_intents.fingerprint` is the SHA-256 hex of the MessagePack encoding of payload kind, payload, `base_rev`, actor and `elevated_by`. A key hit with a different fingerprint fails `IDEMPOTENCY_CONFLICT` (`field = "idempotency_key"`). It is not recorded and writes nothing. `intent_id` and `issued_at` are not part of the fingerprint, so a client retry with a fresh intent_id still dedupes. The recorded `intent_kind` comes from the payload.
- **Per-key value rules** (`SETTING_RULES` next to `DEFAULT_SETTINGS`): `idle_lock_minutes` is an integer 1..=30, `hub.locale` matches `[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*`, and `receipt.footer` takes any text. The settings row is read once (`apply::Target`) and passed to base_rev, validate and mutate.
- **`apply::run` (crate-private) returns `Outcome::{Fresh, Replayed}`.** `apply()` keeps its 1.7 signature. On a dedupe hit the Writer commits an empty transaction and does not emit.
- **Rejection recording** happens in `WriterHandle::submit_intent`, in a second `write_tx` after the rollback. `record_rejection` re-runs the epoch guard and records nothing when this writer is no longer primary, so a follower never writes. If the record write fails, it is logged with `tracing::warn!` (intent_id and code) and the original error is returned.
- **Keys are canonicalised to upper case** before storage, so a lower-case retry dedupes. `is_ulid` also rejects a first character above `7`, because the `ulid` crate accepts 130-bit overflow.
- **`apply()` still checks the key in ttl_dedupe** as a backstop for direct callers. `validate_frame` in `submit_intent` is the gate that runs before any job, checked by `WriterHandle::jobs_submitted()`. That counter exists only under `cfg(test)` or the `test-jobs` feature, and counts only successful sends.
- **settings.set:** a stale `base_rev` fails with `STALE` and `field = "base_rev"`. If the row does not exist, base_rev passes and validate reports `key`, so `settings.set` before bootstrap fails `VALIDATION_FAILED` on `key`. Values are capped at 4096 chars (`value`).
- **Housekeeping:** `HousekeepingTask` lives in `arena-tasks` (`DEDUPE_RETENTION_MS`, `HOUSEKEEPING_INTERVAL`). It prunes through `WriterHandle::prune_applied_intents(cutoff)`, which runs the epoch guard inside its transaction. It uses a strict `<` cutoff, so a record exactly 30 days old is kept. The hub spawns it next to the lease task when primary and aborts it on shutdown.
- **Property suite:** it drives the real Writer on a temp file through a current-thread runtime, with a lease expiring at `i64::MAX` so clock jumps don't demote. Failure persistence is off, and `PROPTEST_CASES` is read explicitly (default 64). A mutation check passed: with dedupe disabled, the suite fails.
- **`check:generated`** compares against git, so it passes only once the regenerated `packages/ui/src/generated` is committed. Re-running `pnpm gen:types` produces byte-identical output.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | A reused key with a different body or kind replays the first outcome | medium | Dedupe keys on the key only, so a client bug silently "succeeds". | patch (fingerprint and `IDEMPOTENCY_CONFLICT`) |
| 2 | Dedupe runs after permission and epoch, so a retry can get NOT_PRIMARY/FORBIDDEN instead of the stored outcome (blind, edge) | low | The order is frozen by the epic (permission → epoch → TTL/dedupe). A follower retry being redirected is correct, and a changed actor becomes a fingerprint conflict. | reject |
| 3 | Recording FORBIDDEN traps a later elevated retry for 30 days | medium | Permission depends on elevation and roles. | patch (don't record permission denials) |
| 4 | VALIDATION_FAILED recorded by deny-list; a pre-bootstrap `key` error is frozen (blind, edge) | medium | State-dependent field errors get recorded. | patch (allow-list) |
| 5 | Dedupe records not in `changes`, so they're lost on failover or replica rebuild | medium (unverified impact) | There is no standby yet; replication of non-log tables comes with the standby epic. | defer |
| 6 | Failure to record a rejection is silent | low | `let _ =`. | patch |
| 7 | Housekeeping ignores promotion and lingers after demotion (blind, edge) | low | It only checks role at tick time. | patch |
| 8 | "then daily" test never checks the second prune (blind, verify-gap) | low | The writer is closed before the advance. | patch |
| 9 | No Hub-level test that housekeeping is wired (verify-gap) | low | Pre-verified gap. | patch |
| 10 | Non-ASCII frame test is 27 bytes | low | Tests the length check, not the non-ASCII case. | patch |
| 11 | Setting values not validated per key (blind, edge) | medium | `idle_lock_minutes="abc"` commits. | patch |
| 12 | Test hooks are public; jobs counted before send (blind, edge) | low | Direct fix. | patch |
| 13 | `find_setting` queried 3×; limit hard-coded in the message | low | Direct fix. | patch |
| 14 | Property test lacks actors, perturbed duplicates and persistence | low | Add the perturbed-duplicate op to cover #1. | patch (partial) |
| 15 | Prune is a single unbounded delete on the writer | low | Batch it with a LIMIT. | patch |
| 16 | A corrupt stored outcome is stuck on INTERNAL | low | Unreachable without manual edits. | reject |
| 17 | Kind validation runs after base_rev | low | Frozen order. | reject |
| 18 | Recorded `intent_kind` comes from the envelope | low | Use the payload kind. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `PROPTEST_CASES=512 cargo test -p arena-core --test proptest_apply --locked` -- expected: pass
- `pnpm check` -- expected: exit 0
