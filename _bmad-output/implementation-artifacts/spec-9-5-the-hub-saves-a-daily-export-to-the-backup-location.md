---
title: 'Story 9.5: The Hub saves a daily export to the backup location'
type: 'feature'
created: '2026-10-30'
status: 'review'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-4-one-tap-full-database-export-to-a-folder-or-usb-drive.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 9.4 only exports when someone taps Export, so a café that forgets has no recent copy, and nobody hears when the backup folder is gone.

**Approach:** A primary-only Hub task (`DailyExportTask`) runs the Story 9.4 export when the business day closes (closing time + 30 min, or the last shift close), as `actor{system}`, into `<branch_code>-auto-<yyyy-mm-dd-hhmm>.a360`, records it with `backup.export`, and prunes to the newest 7 auto files. A failure raises a header alert until the next success. System → Backup lists the last 7 exports with "auto" or the staff name.

## Boundaries & Constraints

**Always:**
- Auto files are recognised by name only (`<branch_code>-auto-…​.a360`); retention deletes nothing else, so files a person exported (and other branches' files) are never touched. Newest 7 by name (the stamp is branch-local time).
- `backup.export` accepts `actor.kind = system` (id `backup`) only for an auto file name, and staff only for a non-auto name. The handler stamps `by` (staff display name, `None` for auto) and keeps the 6 previous entries in `recent`, so `backup.last_export` alone holds the last 7 exports. No new setting key.
- A day is done when an auto entry in that history was made at or after its trigger instant (min of closing+30 min / roll+30 min, and the last shift close when no shift is open). Only the latest due day is exported per pass; a Hub that was off at close exports on start.
- On failure the task sets the reader's `backup_unavailable` flag and an `export_failed` flag; setup status then reports `backup_location_set = false` and `backup_export_failed = true`; the Hub header shows an alert row "Backup folder not available" until an export succeeds (success clears both). It retries every 15 min.
- Runs on a Free and a paid Hub alike; a follower Hub does nothing.

**Never:**
- No cloud upload, restore, or export-format change; no Android tree-URI write (it fails with the same alert as an unavailable folder); no manual-export file is ever pruned.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Day closes | folder set, close+30 min passed | auto file written, intent recorded `by = None`, auto files pruned to 7 | none |
| Last shift closes | shift closed, none open | export now, once | none |
| Folder gone | probe/write fails | alert badge, `backup_location_set=false` | retry in 15 min; next success clears |
| No location | `system.backup_dir` empty | nothing written, no alert | none |
| Prune | 9 auto + 3 manual files | oldest 2 auto deleted, manual kept | delete error logged |
| Already done | auto entry ≥ trigger | no second export | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/backup_export.rs`, `lib.rs` -- `ExportEntry`, `by`/`recent` on `BackupExport`, `auto_export_file_name`, `is_auto_export_name`, `AUTO_EXPORT_KEEP`, `AUTO_EXPORT_ACTOR`; `operating.rs` `SetupStatus.backup_export_failed`.
- `crates/arena-store/src/export.rs`, `lib.rs`, `rollup.rs` -- `prune_auto_exports`; shift-close facts without `paid_since`.
- `crates/arena-core/src/auto_export.rs` (new), `snapshot.rs`, `setup.rs`, `rbac.rs`, `apply/backup_export.rs` -- plan, `export_to_folder` auto variant, failure flag, system actor, history.
- `crates/arena-tasks/src/daily_export.rs` (new), `lib.rs`; `crates/arena-hub/src/lib.rs` -- the task and its spawn.
- `apps/hub/src/lib/backup.ts`, `BackupPanel.svelte`, `routes/+layout.svelte`, `packages/ui/src/messages/en.json` -- list of 7, header alert.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- entry/history fields, auto name helpers, setup flag; tests
- [x] `arena-store` -- prune, shift-close facts; tests
- [x] `arena-core` -- plan, reader variant and flags, system actor, history; tests
- [x] `arena-tasks` + `arena-hub` -- `DailyExportTask`; tests
- [x] Hub UI -- last-7 list, header alert; strings; Playwright spec

**Acceptance Criteria:**
- Given a writable location and a day that has closed, then one `…-auto-….a360` exists, `backup.last_export` shows `by = None`, and only the newest 7 auto files remain beside any manual ones.
- Given the location is unavailable when the day closes, then the header shows "Backup folder not available" and setup status reports no backup location until the next successful export.
- Given System → Backup, then it lists the last 7 exports with time, size and "auto" or the staff name.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-api` -- expected: pass
- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-api -p arena-hub --all-targets -- -D warnings` -- expected: clean
- `pnpm gen:types`, `pnpm run check:layers check:pins check:protocol check:secrets check:strings`, `pnpm --filter @arena/hub check` -- expected: clean

## Auto Run Result

Status: review

Implemented per spec. Verified: clippy `-D warnings` clean on proto/store/core/tasks/api/hub; `cargo test` passes for those crates except `heartbeat::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::a_printer_that_stops_reading_times_out`, which fail the same way on the baseline; `check:layers`, `check:strings` clean; `pnpm --filter @arena/hub check` shows only the two baseline `process` errors in `playwright.kitchen.config.ts`; the Playwright spec `backup-export.spec.ts` passes (14 tests).

Judgement calls:
- Auto files are told apart by name (`<code>-auto-<stamp>.a360`), so retention needs no extra state and cannot delete a person's file.
- `backup.last_export` now carries `by` and the six earlier entries (`recent`), so System → Backup lists the last 7 exports without a new setting key; the Hub stamps both when it applies the record.
- The failure alert is in memory (the reader's flags) and clears on the next successful export; after a Hub restart it reappears at the next attempt, not before.
- A location that is an Android folder URI is skipped quietly (the Hub cannot write it yet, Story 9.4), so no daily alert is raised for something the owner cannot fix.
- "The last shift close" brings the export forward whenever a shift closes with none open, as the rollup planner does; a day with several shifts can export more than once (pruning keeps 7).
