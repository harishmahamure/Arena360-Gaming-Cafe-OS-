---
title: 'Story 9.11: Upgrading from Free uploads a full snapshot before the first batch'
type: 'feature'
created: '2026-10-31'
status: 'in-review'
baseline_revision: 'b8317ac3184156eb99946f405f2043ffdda3f800'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-8-the-hub-uploads-a-daily-snapshot-after-close.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-2-player-keys-are-backed-up-and-the-branch-key-is-escrowed.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A Free branch uploads no change log, so when its licence turns paid the cloud holds none of its history. Uploading every old row as batches would be slow and still leave the cloud without a snapshot to rebuild from, and heartbeat gap detection would ask for all of it.

**Approach:** A paid primary Hub whose `upload_cursor` does not exist yet first stores a full snapshot of today (`snapshots/<today>.db.zst`, `snapshot_log`) once the player key backup is stored, then sets the cursor to the snapshot's last change `seq` in the same transaction, so batching starts one row after it. The heartbeat reports that `seq` as `base_seq`, and the cloud's gap check starts after it. A lapse back to Free forgets the cursor, so the next upgrade takes a fresh snapshot.

## Boundaries & Constraints

**Always:**
- Gate: paid tier, primary, token present (the uploader's existing gate). Before the resend pass and any batch, a cycle with no `upload_cursor` runs the first-snapshot step; with a cursor it costs one read.
- Order: the key backup first. While `backup_state.keys_dirty()` the step does nothing but record `first_snapshot_error = KEY_BACKUP_PENDING` and retry in a minute (`keys/player-keys.bin` is stored by the existing Story 9.2 task).
- The snapshot is the Story 8.8 file (`VACUUM INTO` on its own connection, key table dropped, zstd, scratch files deleted), the same disk rule (twice the database, `backup_disk` alert and audit row), PUT through presign `kind: snapshot` with the Hub's business day as `date`.
- After the PUT answered 2xx, one Writer transaction (primary and epoch checked) writes the `snapshot_log` row, sets `upload_cursor` to the copy's last change `(epoch, seq)` (the lease's epoch and 0 when the copy holds no row), stores `backup_state.base_epoch/base_seq` and clears the pending mark and error.
- A failed step leaves no `snapshot_log` row and no cursor, so no batch goes up and the next cycle takes the snapshot again under the same key. `backup_state.first_snapshot_pending = 1` with `first_snapshot_error` (a code, never player data) and `first_snapshot_error_at` say so.
- Heartbeat: `HeartbeatRequest.base_seq` is the base when `base_epoch` is the lease's epoch. The cloud's first missing `seq` is `max(fork_seq, base_seq) + 1`.
- Lapse: a cycle that sees tier `Free` (not unknown) forgets the cursor, the base and the pending mark once; stored objects stay in the cloud and `snapshot_log` keeps its rows.
- Migration `0086`; export reference meanings for the new columns; regenerated TS types.

**Never:**
- No change to `apply()`, no new intent, no UI (the Hub header and System screen are Story 9.16), no cloud-side manifest, no retention change, no restore from the snapshot (Stories 9.12, 9.13).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Upgrade | paid, no cursor, keys backed up | snapshot PUT, then cursor = its last seq, base kept; later rows go as batches from `seq + 1` | none |
| Keys not backed up | `keys_dirty` | nothing presigned, `KEY_BACKUP_PENDING`, retry in 60 s | none |
| Snapshot PUT fails | 503 or unreachable | no row, no cursor, no batch, pending + last error | backoff, same key next time |
| Low disk | free < 2 x database | no presign, `backup_disk` alert once, `BACKUP_DISK` error, no batch | rechecked each cycle |
| Already uploading | cursor exists | no snapshot | none |
| Lapse | tier `Free` after paid | cursor, base, pending cleared; nothing uploaded | none |
| Re-upgrade | paid again, no cursor | fresh snapshot of today, history since the lapse inside it | none |
| Heartbeat | base_seq 1200, batches from 1201 | no `resend` for 1..1200; a hole after 1200 is still asked for | none |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0086_first_snapshot.sql`, `src/backup_state.rs` -- columns; `write_first_snapshot_pending`, `complete_first_snapshot`, `stop_uploads`; `upload.rs` `EpochProgress.base_seq`.
- `crates/arena-core/src/{snapshot.rs,rollup_plan.rs,writer.rs,status.rs}` -- `SnapshotReader::upload_start`, `business_day`, Writer `record_first_snapshot_pending`, `record_first_snapshot`, `stop_uploads`, `UploadStatus.base_seq`.
- `crates/arena-proto/src/pairing.rs`, `crates/arena-tasks/src/heartbeat.rs`, `crates/arena-cloud/src/api/heartbeat.rs` -- `base_seq` on the wire and in `find_gap`.
- `crates/arena-tasks/src/upload.rs` -- `first_snapshot_pass`, `stop_uploads`; test switches `without_summary` / `without_snapshot` also turn the step off.

## Tasks & Acceptance

**Execution:**
- [x] `arena-store` -- migration 0086, bookkeeping functions, `base_seq` in the epoch progress, export reference.
- [x] `arena-core` -- upload-start read, business day, Writer methods, status field.
- [x] `arena-proto` / `arena-cloud` / heartbeat -- `base_seq` and the gap base.
- [x] `arena-tasks` -- first snapshot before any batch, lapse handling.
- [x] Tests for every matrix row (store unit, uploader, cloud gaps).

**Acceptance Criteria:**
- [x] Given a Free branch whose heartbeat delivers a paid licence, when the uploader runs, then a full snapshot and the key backup are stored before any batch, and batching starts from the snapshot's `seq + 1`.
- [x] Given the snapshot upload fails, then no batch is uploaded until it succeeds and `backup_state` records "first snapshot pending" with the last error.
- [x] Given heartbeat gap detection after the upgrade, then the snapshot `seq` (`base_seq`) is the starting point and no `resend` is asked for history before it.
- [x] Given a lapse back to Free, then uploads stop at once, stored objects stay, and re-upgrading starts again with a fresh full snapshot.

## Spec Change Log

## Review Triage Log

Self-review (2026-10-31): fmt, clippy `-D warnings` on the touched crates, cargo tests for proto/store/core/cloud/tasks/api/hub, `check:protocol/layers/strings`, `format:check` green. `check:generated` reports only the uncommitted regenerated `HeartbeatRequest.ts`.

## Design Notes

The cursor is the "has uploaded" marker, so setting it in the snapshot's own transaction makes the snapshot and the start of batching one step: a crash between the PUT and the record repeats the PUT under the same key and nothing else. The key backup is a precondition rather than a second upload in this task, because the Story 9.2 task already owns sealing and storing `keys/player-keys.bin`; a Hub whose key backup keeps failing therefore stays on "first snapshot pending" and its `keys_error` says why.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api`, `cargo test -p arena-hub --features dev-fixtures --test pairing`, `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-cloud -p arena-api -p arena-hub --all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run check:generated` (after commit), `check:protocol`, `check:layers`, `check:strings`, `format:check`

## Auto Run Result

Status: in-review

Implemented per spec. Failures not caused by this story (they fail the same way without it): `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start...`, `arena-tasks` `heartbeat::trusted_time...` and `printer::lan::a_printer_that_stops_reading_times_out`; `arena-e2e` does not compile on Windows; clippy errors in `arena-kiosk-svc` tests.

Judgement calls and risks:
- The snapshot's `date` is the Hub's business day of the upgrade, so the next daily snapshot (taken for a rolled-up day later than that date) is for the day after, one day later than it would otherwise be.
- The first batch after the upgrade does not carry a fork header even in a forked epoch: the snapshot already holds the state at the fork, and the cursor is in that epoch.
- Until the snapshot is stored the cycle returns early, so the branch summary and rollups also wait for it.
