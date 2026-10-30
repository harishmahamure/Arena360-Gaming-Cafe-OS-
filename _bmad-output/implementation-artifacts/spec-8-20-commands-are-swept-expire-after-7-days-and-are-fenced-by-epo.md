---
title: 'Story 8.20: Commands are swept, expire after 7 days, and are fenced by epoch'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '9f88ac9'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-19-the-hub-applies-owner-commands-exactly-once-and-the-owner-s.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The command mailbox (`commands` in `gz_control`) never lets go of a row: a `fetched` command whose result post was lost stays `fetched` for ever, a `pending` one waits for ever, and nothing stops a replaced Hub (lower epoch) from fetching or answering commands.

**Approach:** A run-once `--role jobs --task command-sweep` returns stale `fetched` rows to `pending`, expires week-old `pending` rows (`expired`, `COMMAND_EXPIRED`) and exports the command backlog; the command routes compare the Hub's epoch with `branch_fence` (`FENCED`, re-queue); a fence move re-queues commands applied past `fork_seq`; the relay returns every `pending` row regardless of the Hub's cursor, and the Hub demotes on `FENCED`.

## Boundaries & Constraints

**Always:**
- Sweeper (`ControlDb::sweep_commands(now)`): `fetched` rows with `fetched_at` older than 10 min and no result -> `pending` (`fetched_at` cleared); `pending` rows with `created_at` older than 7 days -> `expired`, `error` = `AppError{COMMAND_EXPIRED}` JSON, `resolved_at` = now. A later fetch never returns `expired`. Returns the counts; idempotent.
- `CommandStatus::Expired` (`expired`) and `ErrorCode::CommandExpired` (`COMMAND_EXPIRED`, HTTP 410/409 table entries kept in step with `Fenced`); the owner list filter accepts `expired`; `commands.status` CHECK allows it (table rebuild migration, also adds `applied_epoch`).
- Fetch (`GET /v1/commands?after=&epoch=`): epoch below `branch_fence` (`is_fenced`) -> `FENCED` (409) naming the current fence in its message, no rows, nothing marked `fetched`. Otherwise it returns every `pending` row of the branch in `id` order regardless of `after`, plus `fetched` rows with `id > after`.
- Result (`POST /v1/commands/{id}/result`): epoch below the fence -> `FENCED` and the row (if `fetched`/`pending`... or `applied`/`rejected` at that epoch) goes back to `pending`; otherwise it records `applied_epoch` = the body's epoch with the result. A result for an already answered command keeps the stored result.
- Fence move (`ControlDb::move_branch_fence(branch, epoch, hub_id, fork_seq, now)`): sets the new epoch and returns every command with `applied_epoch` = the old fence epoch and `applied_seq > fork_seq` to `pending` (result fields cleared). Commands with `applied_seq <= fork_seq` or null stay.
- Hub: a `FENCED` answer to a fetch or a result post closes the writer and publishes `Role::Follower` (LAN intents then answer `NOT_PRIMARY`); the Hub cursor no longer drops fetched rows with `id <= cursor` (the `applied_commands` dedupe reports the stored result without re-applying).
- Backlog metric: the sweep logs one line per branch with `command_backlog_pending`, `command_backlog_fetched` and `command_backlog_oldest_age_s` (the OpenTelemetry exporter is deferred per Story 8.13's convention); `ControlDb::command_backlog()` is the source.
- CronJob manifest every minute, `concurrencyPolicy: Forbid`, same image and env as the backup job.

**Never:** the owner UI (8.21+), claiming a higher fence from a Hub / CAS counters (9.3), the full self-lock screen (9.24), new owner endpoints, any change to how the Hub applies commands (8.19).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Stuck fetch | `fetched` 11 min ago, no result | Back to `pending`; next fetch redelivers; Hub reports its stored result | none |
| Recent fetch | `fetched` 9 min ago | Untouched | none |
| Old pending | `pending` 8 days old | `expired` with `COMMAND_EXPIRED`, never fetched | none |
| Fenced fetch | epoch `1:A`, fence `2:B` | `FENCED`, no commands, nothing `fetched` | Hub demotes |
| Fenced result | result at `1:A`, fence `2:B` | `FENCED`; the command is `pending` again | Hub demotes |
| Fence move | applied at `1:A` with seq 7 and 12, `fork_seq` 9, new `2:B` | seq 12 row `pending`; seq 7 stays `applied` | none |
| Requeued below cursor | `pending` row `id` 3, Hub cursor 10 | Returned; Hub dedupes | none |
| Burst of 100 | fence moves during the burst | Each command `applied` exactly once at the new epoch | none |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0020_command_sweeper.sql` (new) + `crates/arena-store/src/control.rs` -- rebuild `commands` with `expired` in the CHECK, add `applied_epoch`; register migration 20.
- `crates/arena-store/src/control/commands.rs` -- `fetch_commands`, `resolve_command` (epoch), `sweep_commands`, `command_backlog`, `requeue_command`; `move_branch_fence` in `hubs.rs` beside `branch_fence_epoch`.
- `crates/arena-proto/src/{commands.rs,error.rs,lib.rs}` -- `CommandStatus::Expired`, `ErrorCode::CommandExpired`, sweep constants (`COMMAND_FETCH_TIMEOUT_MS`, `COMMAND_EXPIRY_MS`); generated TS in `packages/ui/src/generated`.
- `crates/arena-cloud/src/api/commands.rs` -- fetch/result fence checks (reuse `crate::presign::is_fenced`, `Epoch`); `api/error.rs` and `arena-api/src/ui_auth.rs`, `arena-kiosk-core/src/queue.rs` -- status-code/match arms for the new code.
- `crates/arena-cloud/src/{config.rs,backup.rs,command_sweep.rs,lib.rs,main.rs}` -- `Task::CommandSweep`, `command-sweep` argument, run and alert path (`report_failure`).
- `crates/arena-tasks/src/commands.rs` -- drop the `id > cursor` filter, demote on `FENCED`.
- `deploy/k8s/base/cronjob-command-sweep.yaml` + `kustomization.yaml`.
- Tests: `crates/arena-store/tests/control.rs`, `crates/arena-cloud/tests/command_mailbox.rs` (new), `crates/arena-tasks/src/commands.rs`, `crates/arena-cloud/tests/config.rs`.

## Tasks & Acceptance

**Execution:**
- `arena-proto` -- add `Expired`, `CommandExpired`, constants; regenerate TS -- wire vocabulary.
- `arena-store` -- migration, sweep, backlog, fetch-all-pending, epoch-aware resolve, fence move -- AD-5 sweeper and fence move.
- `arena-cloud` -- fence checks on fetch and result; `command-sweep` job with alert/backlog log; CronJob -- AD-11/AD-38, NFR25.
- `arena-tasks` -- demote on `FENCED`, no cursor filter -- Story 1.6 demotion path.
- Tests for every matrix row (store + cloud HTTP), including the 100-command fence-move burst against the Hub-style loop (fetch, apply once by key, post result).

**Acceptance Criteria:**
- Given `fetched` commands older than 10 minutes, when the sweep runs, then they are `pending` and the next fetch returns them; an on-time Hub answers again without re-applying.
- Given `pending` commands older than 7 days, when the sweep runs, then they are `expired` with `COMMAND_EXPIRED` and never fetched.
- Given a fetch or result at an epoch below `branch_fence`, then `FENCED`, no commands for a fetch, and a result re-queues its command; the Hub closes its writer and goes follower.
- Given a fence move with `fork_seq`, then commands applied at the old epoch past `fork_seq` return to `pending` and a new-epoch Hub applies each exactly once.
- Given re-queued rows below the Hub's cursor, then the fetch returns them in `id` order.
- Given waiting commands, then the sweep logs pending + fetched counts and the oldest age per branch.

## Spec Change Log

## Review Triage Log

## Design Notes

- The fence is stored as text; `is_fenced` (Story 8.1) orders epochs by counter then instance. The old fence epoch is read inside the same transaction that moves it.
- `fetched_at` is kept at first fetch (`COALESCE`) so a row that keeps being redelivered is still swept; the sweep clears it so the next fetch stamps a fresh time.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy --locked --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings` -- expected: clean
- `cargo test -p arena-proto -p arena-store -p arena-cloud -p arena-tasks` -- expected: pass (known unrelated failures per Story 8.19)
- `pnpm run gen:types`, `pnpm run check:protocol` -- expected: clean

## Auto Run Result

Status: in-review. Implemented: migration 0020 (`expired` status, `applied_epoch`), `CommandStatus::Expired` / `ErrorCode::CommandExpired`, `ControlDb::sweep_commands` / `command_backlog` / `requeue_fetched_command` / `move_branch_fence`, the all-pending mailbox fetch, `FENCED` on fetch and result, the `command-sweep` jobs task with a per-minute CronJob, and the Hub's demotion on `FENCED` with no cursor filter. Verified: `cargo fmt --check`, clippy `-D warnings`, tests for arena-proto/-store/-cloud/-tasks/-kiosk-core, `check:protocol`, `check:strings`, `check:layers`, `check:secrets`. Known unrelated failures: arena-cloud `backup::a_hung_run_times_out_with_its_own_code` (flaky) and `binary::unreachable_at_start_*`; arena-tasks `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::a_printer_that_stops_reading_times_out`; `check:deploy` needs `kubectl`; `check:generated` lists the two regenerated files until committed.

## Code Review Result

Reviewed 2026-10-30: no blocking findings. fmt and clippy (`-D warnings`) clean; arena-proto/-store/-cloud/-tasks tests pass apart from the known unrelated failures (`backup::a_hung_run_times_out_with_its_own_code`, `binary::unreachable_at_start_*`, `heartbeat::trusted_time_*`, `printer::lan::a_printer_that_stops_reading_times_out`) and one load-sensitive timing flake (`a_double_submitted_code_signs_in_once_and_counts_no_failure`, passes alone). Noted, not blocking: `move_branch_fence` has no production caller yet (the takeover commit belongs to the fence-claim story, 9.3).
