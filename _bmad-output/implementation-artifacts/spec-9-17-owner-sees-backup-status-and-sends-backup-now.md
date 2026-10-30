---
title: 'Story 9.17: Owner sees backup status and sends "Backup now"'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '113a944d78ed07b69e02a3d777bea48eb71772fb'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-16-cloud-backup-status-in-the-hub-header-and-system-screen.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner on a paid tier cannot see from the Owner app when a branch last backed up, and the Epic 8 `backup.now` command has no button, no rate limit and no failure line.

**Approach:** The Hub's heartbeat adds a small backup block (last snapshot, batches pending, when the last upload failed); the cloud keeps it next to the upload view and serves it on each `OwnerBranch`. Account -> Branches shows the backup line and a "Back up now" button (a `command-row` follows the command); the cloud refuses a second applied `backup.now` within 15 minutes with `RATE_LIMITED`.

## Boundaries & Constraints

**Always:**
- Only a paid branch sends the block and only a paid branch shows the line and the button; a Free branch shows `tier-lock` "Available on Basic" in place of the button and no line.
- The line shows last upload, last snapshot and pending batches, each time as its age; "Backup failed" with the time replaces the line's good state while the Hub reports a failure (cleared by the next cycle that ends well, as in Story 9.16).
- The rate limit looks at the branch's last `applied` `backup.now` (its `resolved_at`): within 15 min it is `RATE_LIMITED` "A backup ran at <time>. Try again after <time>." (times in the branch time zone, HH:MM) and nothing is queued. The same idempotency key sent again returns its first row and is never rate-limited. On an all-branches post the branch is skipped with that code.
- A rejected `backup.now` keeps the Epic 8 behaviour (rejected with the transport error); the row shows "Couldn't apply at branch" + Retry. The Hub's existing `backup.now` (an upload and summary cycle at once; key backup is part of the cycle) is unchanged.
- The heartbeat block never carries player data; the failure is only a time.

**Never:**
- No change to the Hub's upload, snapshot, key backup or retention behaviour, and no new command kind.
- No Hub-side rate limit (the cloud is the single gate).

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Healthy paid | upload 4 min ago, snapshot 3 h ago, 2 pending | "Last upload 4 min ago · Last snapshot 3 h ago · 2 batches pending" |
| Never reported | paid, Hub has not sent the block yet | "No backup information yet"; button still offered |
| Tap | no applied `backup.now` < 15 min | one `backup.now` post; row Queued/Waiting/Applied |
| Too soon | applied 10 min ago | 429 `RATE_LIMITED` with the message; shown under the button; no row |
| Same key resent | first post took | first row returned |
| Cycle fails | Hub reports transport error | row rejected, "Couldn't apply at branch" + Retry; line "Backup failed · <time>" |
| Free | tier free | `tier-lock` "Available on Basic"; no line |

</intent-contract>

## Code Map

- `crates/arena-proto/src/pairing.rs` -- `HeartbeatRequest` gains `backup: Option<HeartbeatBackup>`; `crates/arena-proto/src/wizard.rs` -- `OwnerBranch.backup: Option<OwnerBackup>`; `lib.rs` re-exports; `HubStatus`-side `UploadStatus` in `crates/arena-core/src/status.rs` gains `last_snapshot_at`, `failed_at`; filled in `SnapshotReader::upload_progress` (`crates/arena-core/src/snapshot.rs`, reuses `latest_snapshot_log`, `read_backup_state`).
- `crates/arena-tasks/src/heartbeat.rs` -- builds the block for a paid Hub (`pending_batches` = pending rows / `arena_store::BATCH_MAX_ROWS`, rounded up); `crates/arena-tasks/src/upload.rs` -- `refresh_progress` after `note_outcome` so a failure is seen at once.
- `crates/arena-store/control-migrations/0032_hub_backup_view.sql`, `src/control/hubs.rs` -- `HubUploadView` gains `last_snapshot_at`, `pending_batches`, `failed_at`; `src/control/commands.rs` -- `last_applied_command_at(branch_id, kind)`.
- `crates/arena-cloud/src/api/heartbeat.rs` (`check_upload`) -- stores the block; `api/branches.rs` (`row`) -- fills `OwnerBranch.backup`; `api/commands.rs` (`post_command` loop) -- the 15-minute check.
- `apps/owner/src/lib/{BranchCard.svelte,commands.svelte.ts,api.ts}`, `packages/ui/src/messages/en.json`, `apps/owner/tests/backup-now.spec.ts` (new), `apps/owner/tests/stub.ts` (branch fixtures).

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto`, `arena-core`, `arena-tasks` -- heartbeat backup block from the Hub; tests for block contents (Free sends none) and for the failure time
- [x] `arena-store` -- migration, view fields, `last_applied_command_at`; `arena-cloud` -- store block, serve on `OwnerBranch`, rate-limit `backup.now`; tests (limit, replay, rejected does not count, all-branches skip, Free refusal unchanged)
- [x] Owner app -- backup line and "Back up now" on the branch card, `backupNow` helper, messages, Playwright tests; regenerate types

**Acceptance Criteria:**
- Given the owner app Account -> Branches on a paid tier, when it opens, then each branch shows last upload, last snapshot and pending batches from the latest heartbeat with their age.
- Given a paid branch, when the owner taps "Back up now", then `backup.now` is sent with a command-row (Queued on this device -> Waiting for branch -> Applied) and the Hub runs its cycle at once.
- Given a `backup.now` applied less than 15 minutes ago, when another is sent, then it is rejected `RATE_LIMITED` with "A backup ran at <time>. Try again after <time>."
- Given the triggered upload fails, when the Hub reports, then the command is rejected with the transport error, its row shows "Couldn't apply at branch" + Retry, and the branch's backup line shows "Backup failed" with the time.
- Given a Free branch, when the owner looks for "Back up now", then it shows `tier-lock` "Available on Basic".

## Spec Change Log

## Review Triage Log

- Implementation pass only; the independent code review is the next step (story status `review`).
- Known, not from this story: `arena-tasks` `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::...a_printer_that_stops_reading_times_out` fail on the baseline on Windows (checked on a stash); `arena-core` `proptest_apply` failed 1 of 8 re-runs on an `owner_alert` row in `notify_outbox` (apply/alert code, which this story does not touch).
- Known limit: the 15-minute check is read-then-insert, so two simultaneous posts with different keys could both queue (the spec counts applied commands only).

## Design Notes

Times in the heartbeat block are Hub-clock ms, like `last_upload_at` already is; the owner sees ages, so a small skew only shifts them. The failure time is `backup_state.upload_error_at` while `upload_error` is set; Story 9.16 clears it when a cycle ends well, so "Backup failed" leaves with the next good cycle. Key-backup failures are not in the block (the cycle that runs for `backup.now` reports the transport error).

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-cloud`, `cargo clippy` on them `--all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run gen:types`, `pnpm run check:strings`, `npx playwright test tests/backup-now.spec.ts` in `apps/owner`

## Code Review (2026-10-31)

- Reviewed the 9.17 diff (heartbeat block, store view, rate limit, Owner app). No blocking findings; no code changes needed.
- Checks: `cargo fmt --check`, `cargo clippy -D warnings`, tests for arena-proto/store/core/cloud (owner_commands, backup) and Playwright `backup-now` + `branches` all green.
- Not from this story (Windows/environment, code untouched here): `arena-cloud` `binary::unreachable_at_start_retries_then_exits_1...`, `arena-tasks` `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::...times_out`; `backup::a_hung_run_times_out_with_its_own_code` is timing-flaky (passed on re-run).
- Accepted limit: the 15-minute check is read-then-insert (documented above).
