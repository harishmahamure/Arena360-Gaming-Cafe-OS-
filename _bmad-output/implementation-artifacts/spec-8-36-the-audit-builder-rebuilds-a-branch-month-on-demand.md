---
title: "Story 8.36: The audit builder rebuilds a branch month on demand"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '4f3240c1'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner or support agent cannot look back at what happened in a branch month: the cloud keeps no copy of branch rows, only change-log batches, snapshots and rollups.

**Approach:** A `builder_jobs` queue in `gz_control`, an owner route that queues a month, and an `arena-cloud --role builder` worker that replays the nearest snapshot plus the batches after it (honouring fork points) with `arena-replay` into a temporary SQLite and stores `audit-cache/<yyyy-mm>.db.zst` for 24 hours.

## Boundaries & Constraints

**Always:**
- `POST /v1/branches/{id}/audit/build {month}` queues a job or reuses a queued, building or fresh ready one; the answer is `AuditBuildStatus` (202 `AUDIT_BUILDING` with progress, 200 ready). `GET` follows it; a failed job answers `AUDIT_BUILD_FAILED` and a new `POST` starts another attempt.
- The builder replays snapshot (newest on or before the month's first day, else none) plus batches after it, one batch at a time, honouring `fork` headers; orphaned rows go to `orphaned_changes`; replay stops at the first change of a later `business_day` month.
- A gap, a missing or tampered batch fails the job. A tenant may start 6 builds an hour (`RATE_LIMITED`); queue depth is logged.
- The cache object is deleted 24 hours after it was built.

**Never:** Player keys (the snapshot has none), a standing copy of branch data, arbitrary queries (Story 8.37), the support endpoints (Story 8.37).

## Spec Change Log

- The epic says the `audit-cache` prefix has a 24 h lifecycle rule; the object key starts with the branch prefix (AD-36), which no single R2 rule can match, so the builder deletes expired caches itself.

## Review Triage Log

- Fixed: `builder_jobs` missing from the control migration table list test; a failed rebuild now deletes a stale expired cache object; stale doc comments (lifecycle rule, claim).
- Accepted: sweep-vs-requeue race window is negligible.

## Design Notes

The audit database keeps the `changes` rows (snapshot's plus replayed) so the search of Story 8.37 can filter by actor, intent, entity and time. A job's `attempt` rises on every restart and claim, and every progress, finish and fail write names it, so a builder that was presumed dead cannot overwrite its successor.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-replay -p arena-cloud` -- expected: pass except known unrelated failures
- `cargo clippy -p arena-proto -p arena-store -p arena-replay -p arena-cloud --all-targets -- -D warnings` and `cargo fmt --check` -- expected: clean
- `pnpm check:generated && pnpm check:strings && pnpm check:pins` -- expected: ok once the generated files are committed

## Auto Run Result

Status: done
Implemented: proto types and codes, control migration 25 and `builder_jobs` queries, `arena-replay` `replay_history`/`MonthBuilder`/`state_checksum`, cloud `audit_build` (request, builder, expiry sweep), owner routes, `--role builder`, builder Deployment. Tests: replay unit tests (round-trip checksum, snapshot resume, forks, month cut, gap, snapshot above fork) and `crates/arena-cloud/tests/audit_build.rs` (11 cases). Known unrelated failures: arena-replay `replaying_the_seeded_log_reproduces_branch_and_settings` (a `library_entries` fixture gap, fails on a clean checkout), arena-cloud `unreachable_at_start_retries_then_exits_1_without_logging_the_token` (and `a_hung_run_times_out_with_its_own_code` on a slow run), arena-tasks `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `a_printer_that_stops_reading_times_out`. `check:deploy` could not run (no kubectl).
