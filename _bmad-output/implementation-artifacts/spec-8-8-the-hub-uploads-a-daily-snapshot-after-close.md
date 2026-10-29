---
title: 'Story 8.8: The Hub uploads a daily snapshot after close'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '0600bac'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The cloud holds the change log and the rollups but no starting point for rebuilding a branch month, so an audit rebuild would replay the log from the first row (AD-4, NFR13).

**Approach:** In the uploader cycle, after a day's rollup has published, a paid primary Hub copies its database with `VACUUM INTO` on a connection of its own, drops the player key table from the copy, compresses it with zstd, PUTs it to `snapshots/<yyyy-mm-dd>.db.zst` via presign `kind: snapshot`, deletes the scratch files, and records the date and last included change `seq` in the new `snapshot_log` table.

## Boundaries & Constraints

**Always:**
- Paid tier, primary only (the uploader's existing gate); a Free Hub, a follower and an unpaired Hub store nothing.
- Due: the latest dated `rollup_log` row is newer than the latest `snapshot_log` row (or there is none). Only that latest day is taken: days missed while offline or with the WAN down are skipped, not caught up.
- The copy is made on its own read connection (never the shared snapshot reader, never the writer), in the database's own folder, as `snapshot-<date>.db(.zst).tmp`; leftovers of an interrupted run are deleted first and the scratch files are deleted as soon as the bytes are held.
- The copy holds no `player_keys` table and no free page of it (`secure_delete`, `VACUUM`); player PII columns are `*_enc` ciphertext already and stay as they are.
- `snapshot_log(date, epoch, last_seq, bytes, key, uploaded_at)` is local bookkeeping outside the change log (like `upload_cursor`), written through the Writer only after the PUT answered 2xx and only while the Hub is primary and holds its epoch. A failed PUT leaves no row; the next cycle takes the snapshot again under the same key.
- Disk: skipped when free space on the scratch disk is under twice the database (file + WAL). A branch-level `backup_disk` alert is raised once per day per run through `alert.raise` (system actor `hub-snapshot`), which shows in the Hub header list ("Not enough disk space for backup") and writes the audit row. If the free space cannot be read the snapshot goes ahead.

**Never:**
- A change to `apply()`, a new intent kind, a cloud copy of rows, the restore or rebuild itself (9.x, 8.36), or the backup status screens (9.16).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Close | rollup of D published, no snapshot | PUT `snapshots/D.db.zst`, `snapshot_log` row | none |
| Long outage | rollups for D-2..D published | one snapshot, for D | none |
| WAN down | snapshot PUT 503 | no row, scratch files gone, retried next cycle | backoff |
| Low disk | free < 2 x database | no presign, one `backup_disk` alert + audit row, rechecked each cycle | logged |
| Alert stays open | disk still full | no second alert | `STALE` swallowed |
| Free / follower | tier Free | nothing | none |
| Trading | `apply()` during the copy | commits as usual (WAL, own connection) | none |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0076_snapshot_log.sql`; `src/snapshot.rs` -- `SnapshotLogRow`, `find_snapshot_log`, `latest_snapshot_date`, `write_snapshot_log`, `database_bytes`, `build_snapshot_file` / `SnapshotFile`.
- `crates/arena-proto/src/alert.rs` -- `AlertKind::BackupDisk`; `rollup.rs` -- `snapshot_object_key`.
- `crates/arena-core/src/snapshot.rs` -- `SnapshotReader::{snapshot_due, snapshot_scratch, snapshot_file}`; `writer.rs` -- `record_snapshot`, `raise_branch_alert`.
- `crates/arena-tasks/src/disk.rs` -- `free_bytes` (Windows `GetDiskFreeSpaceExW`, Unix `statvfs`); `src/upload.rs` -- `publish_snapshot`, `alert_disk_low`.
- `apps/hub/src/lib/alerts.svelte.ts`, `packages/ui/src/messages/en.json`, generated `AlertKind.ts` -- the alert row.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store` -- migration 0076, snapshot module (copy, strip, compress, log).
- [x] `crates/arena-proto` -- `backup_disk` alert kind, `snapshot_object_key`, generated TS.
- [x] `crates/arena-core` -- due check, scratch/size, copy through the reader, Writer `record_snapshot` and `raise_branch_alert`.
- [x] `crates/arena-tasks` -- free-space probe, `publish_snapshot` in the cycle after the rollups, disk rule and alert.
- [x] Hub UI -- alert label and tile mapping (`backup_disk`, no tile tag, no sound).
- [x] Tests -- store (copy has no keys, last seq, leftovers, log, footprint); disk probe; uploader (follows the rollup and recorded after the PUT, PII ciphertext and no key table, latest missed day only, failed PUT retried, low disk alert once with audit row, disk rule, Free, writes continue during copies).

**Acceptance Criteria:**
- Given a paid Hub after the day's rollup has published, when the snapshot task runs, then it `VACUUM INTO`s a temp file, compresses with zstd, uploads to `snapshots/<yyyy-mm-dd>.db.zst` via presign `kind: snapshot`, deletes the temp file and records the date and last included seq in `snapshot_log`.
- Given the snapshot file inspected in a test, then PII columns hold only ciphertext and the player key table is not in it.
- Given the WAN is down at close, then it is retried on the next cycle with WAN and only the latest missed day is uploaded.
- Given staff keep trading while it runs, then `apply()` keeps committing (own connection under WAL).
- Given free disk space below twice the database size, then it is skipped, a "Not enough disk space for backup" alert shows in the Hub header list and an audit row is written.

## Spec Change Log

## Review Triage Log

Code review (2026-10-30): Blind/Edge/Acceptance pass done by direct inspection; no blocking findings. fmt, clippy -D warnings, proto/store/core/tasks tests green except two pre-existing failures in untouched files (heartbeat, printer::lan); check:generated and check:strings ok.

## Design Notes

`snapshot_log` is not change-logged: a snapshot is a property of this Hub's copy of the cloud, not branch business data, and a replayed log must not claim a snapshot that this database never uploaded. The audit row for the low-disk skip is the system `alert.raise` the alert already needs. The compressed file is held in memory for the PUT (`CloudClient::put_object` takes bytes); the scratch files are deleted before the upload starts. `SNAPSHOT_DISK_FACTOR = 2` counts the WAL with the database, and the temp copy plus the compressed file stay under that margin.

## Verification

**Commands:**
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-replay -p arena-hub -p arena-api -p arena-cloud -p arena-kiosk-core`; `cargo clippy` on them with `--all-targets -D warnings`; `cargo fmt --check`; `pnpm run check:generated` (clean once `AlertKind.ts` is committed); `check:protocol`; `check:strings`; `check:layers`; `check:no-manifest`; `format:check`.

## Auto Run Result

Status: in-review (implemented and verified; the review pass is the next iteration's).

- Summary: after a day's rollup publishes, a paid primary Hub stores a zstd copy of its database (no player key table) at `snapshots/<date>.db.zst` and logs date, epoch and last seq in `snapshot_log`; low disk skips it with a `backup_disk` alert and audit row.
- Files: `crates/arena-store/{migrations/0076_snapshot_log.sql,src/{snapshot,lib,migrations}.rs}`; `crates/arena-proto/src/{alert,rollup,lib}.rs`; `crates/arena-core/src/{snapshot,writer}.rs`; `crates/arena-tasks/{Cargo.toml,src/{disk,lib,upload}.rs}`; `apps/hub/src/lib/alerts.svelte.ts`; `packages/ui/src/{messages/en.json,generated/AlertKind.ts}`.
- Verification: fmt and clippy `-D warnings` clean; all suites green except the five failures that predate the story and sit in untouched files (`arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings`; `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-cloud` `tests/backup.rs` `a_hung_run_times_out_with_its_own_code` and `tests/binary.rs` `unreachable_at_start_retries_then_exits_1_without_logging_the_token`); `check:protocol`, `check:strings`, `check:layers`, `check:no-manifest`, `format:check` ok; hub `svelte-check` reports only the two untouched `playwright.kitchen.config.ts` errors.
- Decisions: `snapshot_log` outside the change log; the copy runs on a fresh read connection so neither the shared reader nor the writer waits; the key table is dropped from the copy rather than excluded at read time; one low-disk alert per day per run.
- Residual risks: the "apply() within NFR3 on the reference PC" proof is a test of bounded write latency during copies of a 30k-row log, not a reference-hardware measurement (Story 8.42 / the reference-PC passes own that); the whole compressed file is held in memory for the PUT; the Hub header list shows the alert but acknowledging it does not re-trigger a snapshot until the next published day.
