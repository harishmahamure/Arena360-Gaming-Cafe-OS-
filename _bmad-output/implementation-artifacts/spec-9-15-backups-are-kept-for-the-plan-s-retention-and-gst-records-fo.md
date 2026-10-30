---
title: 'Story 9.15: Backups are kept for the plan''s retention and GST records for 8 years'
type: 'feature'
created: '2026-10-31'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-14-the-hub-emits-a-yearly-gst-archive-kept-for-8-years.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Cloud backups grow without bound and the Hub's `changes` log is never trimmed; nothing enforces "1 or 3 years" for backups or "8 years" for GST archives.

**Approach:** A daily `--role jobs --task retention-sweep` lists each branch's `changes/`, `snapshots/`, `rollups/` and `gst-archive/` objects in the data bucket and deletes what is past its limit (plan retention with a 30-day grace after a downgrade, 8 years from FY end for archives, always keeping the snapshot a retained batch range restores from). Hub daily housekeeping prunes `changes` rows uploaded more than 90 days ago.

## Boundaries & Constraints

**Always:**
- Sweep touches only `changes/`, `snapshots/`, `rollups/` (older than the retention) and `gst-archive/` (older than 8 years from FY end). Never `keys/`, `state/`, `audit-cache/` or anything else. A key it cannot parse as dated is kept.
- Retention days come from the licence tiers (Basic 365, Premium 1,095). Age of `changes/` objects = bucket last-modified; `snapshots/` and `rollups/` = the date in the key (end of that day UTC).
- `gz_control` keeps `branch_retention` (branch_id, paid_days, prev_days, changed_at). Each run observes the branch's current paid tier: a change to a different paid retention stores the old as `prev_days` and stamps `changed_at`. Effective retention = `prev_days` while it is longer than `paid_days` and under 30 days old, else `paid_days`. A lapsed (Free) branch keeps `paid_days`. A branch that never had a paid tier is skipped for the three prefixes.
- The newest snapshot dated before the day of the oldest retained `changes/` object is never deleted; with no retained batch the newest snapshot is kept. So every retained batch has a base.
- A GST archive is deleted only once now is past 8 years after the latest possible end of its FY (`FYyyyy-yy` -> 31 Dec of yyyy+1, `FYyyyy` -> 31 Dec of yyyy; the label does not say the start month, so the bound can only be late, never early), on every tier and after a lapse.
- One branch failing (list or delete) is logged with its code and the rest still run; the run then fails (alerted). Logs carry branch id, prefix and counts, never object contents.
- Hub: while primary, housekeeping deletes `changes` rows with `at` older than 90 days that the upload cursor has passed (`(epoch_counter, epoch) < cursor` or same epoch and `seq <= last_seq_uploaded`), in batches, through the Writer. It never deletes the highest-`seq` row of an epoch (so `next_change_seq` and the snapshot position survive), never a row with no cursor (Free), and keeps rows of `orders`, `order_lines`, `payments`, `refunds`, `invoice_sequences` and `gst_archive_log` until they are 8 years old.

**Never:**
- No deletion of GST rows in the Hub tables, no change to upload, snapshot or archive paths, no UI.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Basic, old objects | rollup 400 days old, 100 days old | 400-day one deleted | none |
| Premium to Basic | moved 10 days ago, object 500 days old | kept (Premium 1,095 applies) ; at 31 days deleted | none |
| Lapse to Free | last paid Basic, object 400 days old | deleted at 365 | none |
| Base snapshot | snapshots 400/380 days old, oldest retained batch 300 days old | 380-day snapshot kept, 400-day deleted | none |
| GST archive | now 2027-01-15: FY2017-18 and FY2018 / FY2018-19 | FY2017-18 and FY2018 deleted (8 years after 31 Dec 2018 and 31 Dec 2018 have passed); FY2018-19 kept | none |
| Other keys | `keys/player-keys.bin`, `state/latest.json` | untouched | none |
| Delete fails | store error | branch counted failed, others run | job fails, alerted |
| Hub prune | uploaded rows 91 days old, unuploaded 91 days old, money rows 91 days old | uploaded non-money deleted; others kept | none |
| Free Hub | no upload cursor | nothing pruned | none |

</intent-contract>

## Code Map

- `crates/arena-cloud/src/retention.rs` (new) -- pure `plan_branch` + `sweep_once`/`run_job`.
- `crates/arena-cloud/src/{store.rs,presign.rs}` -- list with last-modified (`list_modified`), memory store timestamps.
- `crates/arena-cloud/src/{config.rs,backup.rs,lib.rs}` -- `Task::RetentionSweep`, codes, dispatch.
- `crates/arena-store/control-migrations/0031_branch_retention.sql`, `src/control.rs`, `src/control/retention.rs` -- state table and queries.
- `crates/arena-store/src/upload.rs` -- `prune_uploaded_changes`; `crates/arena-core/src/writer.rs` -- `prune_changes`.
- `crates/arena-tasks/src/housekeeping.rs` -- daily changes prune.

## Tasks & Acceptance

**Execution:**
- [x] `arena-store` control migration + `branch_retention` queries; tests
- [x] `arena-cloud` store listing with last-modified; `retention.rs` planner and job; config/dispatch; tests
- [x] `arena-store`/`arena-core`/`arena-tasks` changes prune; tests

**Acceptance Criteria:**
- Given the daily sweep, when it runs for a branch, then `changes/`, `snapshots/`, `rollups/` objects older than the effective retention are deleted and `gst-archive/`, `keys/`, `state/` are untouched.
- Given `gst-archive/<fy>.zst`, when 8 years have passed since its FY end, then only then is it deleted, on any tier or lapse.
- Given a Premium to Basic move or a lapse, when the sweep runs, then the previous retention applies for 30 days (a lapse keeps the last paid retention).
- Given Hub housekeeping, when it runs, then uploaded `changes` rows older than 90 days go, unuploaded ones and money/tax rows younger than 8 years stay, and Free prunes nothing.
- Given a sweep that would delete the base snapshot of a retained batch range, when it plans, then that snapshot is kept.

## Spec Change Log

## Review Triage Log

## Design Notes

Planning is a pure function of (objects with age, retention, now) so every I/O row is a unit test. Object age for `changes/` needs the bucket's last-modified, which the key-only `list` lacks; `list_modified` adds it for S3 and the memory store.

## Verification

**Commands:**
- `cargo test -p arena-store -p arena-core -p arena-cloud -p arena-tasks`, `cargo clippy` on them `--all-targets -- -D warnings`, `cargo fmt --check`

## Auto Run Result

Status: done

Implemented per spec. Judgement calls and risks:
- `changes` is append-only by trigger, so migration 0088 (Hub) replaces `changes_no_delete` with a version that allows a delete only while the one-row `changes_prune_permit` is present and the row is old enough, passed by the upload cursor, not the newest of its epoch, and (for money/tax tables) 8 years old. The prune opens and removes the permit inside its own transaction; a plain `DELETE` is still refused.
- The retention grace needs the tier history the cloud does not keep, so gz_control gets `branch_retention` (migration 0031), written by the sweep when it first sees a branch or sees its paid retention change. A downgrade that happened before the first sweep is not graced.
- `changes/` object age is the bucket's last-modified time (new `list_modified` on `S3Store`, `MemoryStore`, `Presigner`); snapshots and rollups use the date in the key. The base snapshot is chosen by date (newest ending before the oldest retained batch was written), not by reading its position.
- A GST archive's FY end is bounded by the label alone (the start month is not in it), so it can be kept up to about 11 months past the exact 8 years, never shorter.
- Deferred: the Hub's local audit and reports lose `changes`-based detail older than 90 days once uploaded (cloud audit cache serves it); the sweep has no deploy schedule here (run `--role jobs --task retention-sweep` daily like the other jobs).

## Verification Results

- `cargo fmt --check`, `cargo clippy` (proto, store, core, cloud, tasks, api, hub, replay; `--all-targets -D warnings`): clean.
- `cargo test` on the same crates: all new tests pass. Failing, none in touched code and the same as at the last story: `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings`, `arena-tasks` `heartbeat::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::a_printer_that_stops_reading_times_out`, `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start_...`. `arena-api` `ui_events::a_component_change_also_reports_the_bundles_that_hold_it` failed once in the full run and passes alone.
- Export reference (`docs/export-format/reference.md`) regenerated for the new table.

## Code Review (2026-10-31)

Reviewed the implementation commit against the spec: retention planner, grace/lapse state machine, base-snapshot rule, GST 8-year bound, prune trigger/permit, Writer path and housekeeping. No blocking findings. fmt and clippy (`-D warnings`) clean. Remaining test failures (`a_hung_run_times_out_with_its_own_code`, `unreachable_at_start_*`, heartbeat trusted-time, LAN printer timeout) are timing/network tests outside the touched code and were failing before this story.
