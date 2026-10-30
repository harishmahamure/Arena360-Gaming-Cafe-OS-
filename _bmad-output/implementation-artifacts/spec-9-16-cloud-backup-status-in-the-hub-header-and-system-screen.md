---
title: 'Story 9.16: Cloud backup status in the Hub header and System screen'
type: 'feature'
created: '2026-10-31'
status: 'review'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-15-backups-are-kept-for-the-plan-s-retention-and-gst-records-fo.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Counter staff and managers on a paid tier cannot tell when the branch last backed up, so a stopped backup goes unnoticed.

**Approach:** `GET /ui/backup/status` answers from the Hub's own logs (`upload_cursor`, `snapshot_log`, `backup_state`, summary and GST archive logs). The header shows a muted link "Backed up 4 min ago · 3 pending" (a warning "Backup behind" badge after 2x the upload interval), System -> Backup shows the detail, and a Free Hub shows no header status and only local exports with a tier-lock.

## Boundaries & Constraints

**Always:**
- Any signed-in member may read the status (it is on the header). It never carries player data; errors are short codes mapped to plain words in the UI.
- "Behind" is `now - last_upload_at > 2 x upload_interval_s`; a paid Hub with no upload yet is "First backup pending", never behind. Free is never behind.
- The tooltip and accessible name add the last summary time and the batches pending. The change to and from "behind" is announced once in the polite live region.
- The header status is dropped below 840px; System -> Backup keeps it.
- The last upload failure is kept in `backup_state.upload_error` (migration 0089), written by the upload task only when it changes and cleared by a cycle that ends well or by a lapse to Free. The newest of that, the key-backup error and the first-snapshot error is shown.

**Never:**
- No change to upload, snapshot, key backup or retention behaviour; no owner-app UI (Story 9.17).

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Paid, healthy | upload 4 min ago, 3 batches waiting | header "Backed up 4 min ago · 3 pending", title "Last upload 4 min ago · last summary 3 min ago · 3 batches pending" |
| Behind | interval 300 s, upload 11 min ago | warning badge "Backup behind", announced once; normal after the next upload |
| Upgrade | paid, no upload yet | header "First backup pending"; System "First backup pending" |
| Free | `tier` Free or unknown | `cloud: false`; no header status; System shows local exports and "Available on Basic" |
| Phone | width < 840px | no header status; System -> Backup has it |
| Failing | cloud unreachable | System shows "Uploads: The cloud couldn't be reached..." |

</intent-contract>

## Code Map

- `crates/arena-proto/src/backup_status.rs` (new) -- `BackupStatus`, `BackupError`, `BackupErrorSource`, `behind`.
- `crates/arena-store/migrations/0089_upload_error.sql`, `src/backup_state.rs`, `src/snapshot.rs` -- `upload_error`, `latest_snapshot_log`.
- `crates/arena-core/src/{snapshot.rs,writer.rs}` -- `SnapshotReader::backup_status`, `record_upload_error`.
- `crates/arena-tasks/src/upload.rs` -- `note_outcome` after each cycle.
- `crates/arena-api/src/{ui_backup.rs,ui_auth.rs}` -- `GET /ui/backup/status`.
- `apps/hub/src/lib/{backup-status.ts,backup-status.svelte.ts,BackupChip.svelte}`, `src/routes/+layout.svelte`, `src/routes/system/backup/+page.svelte`, `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` status type; `arena-store` migration, `write_upload_error`, `latest_snapshot_log`; export reference regenerated
- [x] `arena-core` reader + writer; `arena-tasks` outcome noting; `arena-api` route; tests
- [x] Hub header chip, shared status store, System -> Backup detail, Free view; Playwright tests

**Acceptance Criteria:**
- Given a paid Hub, when the header renders, then it shows muted backup status with a tooltip and accessible name that include the last summary and batches pending, linking to System -> Backup.
- Given a last upload older than twice the interval, when the header updates, then it shows a warning badge "Backup behind" announced politely, returning to normal after the next upload.
- Given System -> Backup, when it opens, then it shows last batch upload, summary, snapshot, key backup, GST archive, pending batches, "First backup pending" after an upgrade and the last error in plain words.
- Given a Free Hub, then no header status is shown and System -> Backup shows local exports with "Available on Basic".
- Given a phone layout, then the header drops the status and System -> Backup keeps it.

## Spec Change Log

## Review Triage Log

## Design Notes

The status is read from the Hub (not the cloud) so it is honest when the WAN is down. The upload interval is the tier's default; a licence that sets its own would move only the "behind" threshold. Last summary time comes from `rollup_log` `latest`, as the heartbeat already reads it.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-api`, `cargo clippy` on them `--all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run gen:types`, `pnpm run check:strings`, `npx playwright test tests/system-backup.spec.ts` in `apps/hub`
