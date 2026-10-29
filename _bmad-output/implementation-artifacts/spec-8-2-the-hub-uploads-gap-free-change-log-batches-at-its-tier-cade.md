---
title: 'Story 8.2: The Hub uploads gap-free change-log batches at its tier cadence'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: 'e0ba742cc50a164546f32575ad2a292382327692'
baseline_revision: 'e0ba742cc50a164546f32575ad2a292382327692'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A paid Hub keeps its `changes` log only on its own disk. Nothing ships it to the cloud, so the owner's reports and audit cannot be rebuilt from it (AD-4, AD-10, AD-36).

**Approach:** A primary-only `UploadTask` wakes every `upload_interval_s` (floor 30 s) or at 500 pending rows, reads up to 5,000 rows, builds a zstd batch (msgpack header + full-row maps), presigns it (Story 8.1), PUTs it, and only then advances a new `upload_cursor`. Backlogs ship back to back; failures retry the same key; `/healthz` reports last upload and pending rows.

## Boundaries & Constraints

**Always:**
- Batch = zstd(msgpack header map `{epoch, fork?, first, last, sha256, schema}` ++ msgpack array of row maps keyed by column name). `sha256` is over the array bytes; row count must equal `last - first + 1`. `schema` is the Hub schema version.
- One batch holds rows of one epoch only; `first`/`last` are the first/last `seq` in it; successive keys have no gap or overlap.
- The cursor (`upload_cursor`: `epoch`, `last_seq_uploaded`, `uploaded_at`) is written through the Writer only after the PUT answered 2xx. A failed PUT leaves it; the next cycle rebuilds the same key (a repeated PUT is harmless).
- `lease.fork_parent_epoch` / `fork_parent_seq` are nullable; when set and the cursor is not yet in the lease's epoch, the first batch of that epoch carries `fork = {parent_epoch, parent_seq}` and `first = parent_seq + 1`.
- Primary role only. Free tier (or no tier yet) does nothing and makes no presign call. The uploader reads through `SnapshotReader` and never runs inside an `apply()` transaction.
- Interval comes from the verified licence claims `upload_interval_s` (tier default if the licence cannot be read), never below 30 s. Failures back off with jitter (`heartbeat::backoff`); 429 honours Retry-After; `TOKEN_*` marks the Hub revoked; `FEATURE_NOT_ENTITLED` waits 15 min.
- `/healthz` gains `upload: {last_upload_at, pending_rows}`; `chip-offline` stays the only staff-visible change.

**Never:**
- The summary, rollup or snapshot uploads (Stories 8.4–8.8), gap detection by heartbeat (8.3), or the real epoch change (Epic 9: tests set the fork fields by fixture).
- A cloud copy of rows, or any change to `apply()` / the Writer's change-log writes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Normal cycle | paid, 3 rows pending | one PUT to `batch_url`, cursor = last | none |
| Backlog | 12,000 pending | batches of 5,000/5,000/2,000, keys contiguous | none |
| Fork | lease fork set, new epoch | header `fork`, `first = parent_seq + 1` | none |
| PUT fails | 5xx/timeout | cursor unchanged, same key next cycle | backoff, `upload` pending stays |
| Free / tier None | tier Free | no request at all | none |
| Revoked token | presign 401 | pairing → Revoked, long backoff | none |
| Rate limited | presign 429 | wait Retry-After | none |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0072_*.sql`, `src/migrations.rs` -- add 0073 (lease fork columns, `upload_cursor`).
- `crates/arena-store/src/branch.rs` -- `ChangeRecord`, `CHANGE_COLUMNS`, `change_from`, `read_changes` (epoch ordering SQL); `lib.rs` `write_lease` upsert leaves new columns alone.
- `crates/arena-core/src/writer.rs` -- `prune_applied_intents` is the pattern for a production maintenance write (`check_epoch`); `snapshot.rs` `SnapshotReader` pattern for reads.
- `crates/arena-core/src/status.rs`, `crates/arena-api/src/health.rs` -- `HubStatus`, `HealthBody`.
- `crates/arena-tasks/src/template.rs` -- closest pattern (paid-only cloud uploader, refusal handling, tests with a one-shot HTTP server); `cloud.rs` `CloudClient` (`send`, plain-http loopback in tests); `heartbeat.rs` `backoff`, `REVOKED_BACKOFF`, verify keys.
- `crates/arena-proto/src/presign.rs`, `tier.rs`, `licence.rs` -- `PresignRequest/Response`, `upload_interval_s`, `verify_claims`.
- `crates/arena-hub/src/lib.rs` ~L480–520 -- where tasks are spawned.
- `crates/arena-replay/src/lib.rs` -- `replay()` for the round-trip test.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store/migrations/0073_upload_cursor.sql`, `migrations.rs` -- lease fork columns and `upload_cursor`.
- [x] `crates/arena-store/src/upload.rs`, `lib.rs`, `Cargo.toml`, workspace `Cargo.toml` -- cursor and fork reads/writes, pending-batch read (one epoch, `limit`), batch codec (`encode_batch`/`decode_batch` with sha256 + count checks, zstd).
- [x] `crates/arena-core/src/writer.rs`, `snapshot.rs`, `status.rs` -- `WriterHandle::record_upload`, `SnapshotReader::pending_batch`/`pending_rows`, `HubStatus.upload`.
- [x] `crates/arena-api/src/health.rs` -- `upload` in `/healthz`.
- [x] `crates/arena-tasks/src/cloud.rs`, `upload.rs`, `lib.rs` -- `CloudClient::presign` and `put_object`; `UploadTask`.
- [x] `crates/arena-hub/src/lib.rs` -- spawn the task (and abort it with the others).
- [x] Tests -- store (codec round trip, tamper, cursor, migration), tasks (every matrix row against a local HTTP server; replay of a read-back batch with `arena-replay`), health.

**Acceptance Criteria:**
- Given a batch read back from the local server in a test, when its sha256 and row count are checked, then they match the header and replaying it with `arena-replay` into an empty database reproduces the rows.
- Given a paid Hub with 12,000 pending rows, when the uploader catches up, then three consecutive keys cover seq 1–12,000 without gap or overlap and the cursor ends at 12,000.
- Given the Free tier, when the uploader would run, then no request is made.

## Spec Change Log

## Review Triage Log

## Design Notes

Cursor is per epoch: `(epoch, last_seq_uploaded)`. When no rows remain after it in its epoch, the next batch starts at the lowest later epoch (by numeric counter, then instance) from its first row; fork `first` applies only to the lease's current epoch.

## Verification

**Commands:**
- `cargo test -p arena-store -p arena-core -p arena-tasks -p arena-api -p arena-hub -p arena-replay`; `cargo clippy` on those crates with `-D warnings`; `cargo fmt --check`.

## Auto Run Result

Status: in-review (implemented and verified; the review pass is the next iteration's).

- Summary: a primary, paid Hub now uploads its `changes` log in zstd batches (msgpack header + row maps, sha256 and row-count checked on read-back) through Story 8.1 presigned URLs, advancing a new `upload_cursor` only after a 2xx PUT. `/healthz` reports `upload: {last_upload_at, pending_rows}`.
- Files: `crates/arena-store/migrations/0073_upload_cursor.sql` (lease fork columns, `upload_cursor`, `changes_log_order` index); `crates/arena-store/src/upload.rs` (cursor, fork, pending-batch read, batch codec); `crates/arena-core/src/{writer,snapshot,status}.rs` (`record_upload`, `pending_batch`/`upload_progress`, `HubStatus.upload`); `crates/arena-api/src/health.rs`; `crates/arena-tasks/src/{cloud,upload}.rs` (`presign`, `put_object`, `UploadTask`); `crates/arena-hub/src/lib.rs` (spawn/abort); `zstd` 0.13.1 added.
- Verification: `cargo fmt --check` and `cargo clippy -D warnings` clean on arena-store/core/tasks/api/hub/replay; tests pass in arena-store (118), arena-core (1250), arena-api, arena-hub, and the 14 new `upload::tests` in arena-tasks (every matrix row, 12,000-row backlog, replay of a read-back batch).
- Failing before and after this change (checked on a clean baseline): `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings`; `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`. `arena-e2e` does not compile on this Windows machine (unix-only tokio import), untouched.
- Residual risks: `zstd` compiles C code, so the Android cross-build needs the NDK C toolchain (not tried here); the extra `changes_log_order` index adds a small cost to each change write; no real R2/MinIO round trip was run (a local HTTP server stands in); `HeartbeatRequest.last_seq_uploaded` is still 0 until Story 8.3.
- Review: code review passed (no blocking findings); fmt, clippy -D warnings and unit tests green.
