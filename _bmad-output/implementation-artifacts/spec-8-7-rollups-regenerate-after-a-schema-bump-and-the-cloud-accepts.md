---
title: 'Story 8.7: Rollups regenerate after a schema bump, and the cloud accepts only the current and previous schema'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: 'f2de38b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub always writes the rollup schema it was built with, so a schema bump would leave every past day in the old shape, and the cloud has no rule for which schemas it reads (AD-35, NFR25).

**Approach:** The Hub emits the schema its licence pins (`rollup_schema`), held within the writers it keeps (N and N−1), and a paid primary Hub rebuilds published days whose `rollup_log.schema` is below that target, oldest first, 10 a minute, as part of the uploader cycle. The cloud reads a rollup only at its newest schema N or N−1; any other day counts as missing with `ROLLUP_SCHEMA_OLD` recorded for the branch and day, and the per-branch missing-day count is exported.

## Boundaries & Constraints

**Always:**
- Target schema = `emitted_rollup_schema(pin, newest)`: the licence's `rollup_schema` (the tier default if the licence cannot be verified) clamped to `[max(N−1, 1), N]`. New days, late-change regenerations and old-day regenerations all use it.
- Regeneration: published daily rows (`yyyy-mm-dd`, never `latest`) with `schema < target`, oldest first, at most 10 in any rolling 60 s of the Hub clock, after the cycle's batches, summary and due days; leftovers wake the task when the minute frees a place. A day that cannot be built is passed over for the run and does not hold up the days behind it. A failed PUT stops the pass like any rollup PUT.
- Resume: the `rollup_log` is the only state. A restart starts at the oldest day still below the target and a rebuilt day (its row now carries the target schema) is never rebuilt again.
- The Hub builds a rollup only in a schema it has a writer for (`build_rollup` refuses others with `ValidationFailed` on `schema`).
- Cloud: `rollup_schema_accepted(schema, current)` is N or N−1; `current` is the highest `rollup_schema` any tier pins. `ROLLUP_SCHEMA_OLD` (HTTP 409) is the code for a file outside that; the day counts as missing until the Hub regenerates it; a readable file clears the record.

**Never:**
- A second writer (the v2 and v3 shapes are Epics 13 and 14), rewriting `apply()`, a cloud copy of rows, the snapshot (8.8) or the read API (8.9).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Bump | Hub writes N=2, licence pins 2, 14 days at v1 | 10 rebuilt oldest first, the other 4 a minute later | none |
| Pin behind | Hub writes 2, licence pins 1 | new days at v1, nothing rebuilt | none |
| Pin ahead | licence pins 9, Hub writes 2 | emits 2 | none |
| Restart | 10 of 12 rebuilt | resumes at day 11, rebuilt days untouched | none |
| Unbuildable | one old day cannot be built | skipped, the others go | logged |
| Cloud reads | file schema 1 with current 3 | not read, `ROLLUP_SCHEMA_OLD` recorded, day missing | 409 |

</intent-contract>

## Code Map

- `crates/arena-proto/src/rollup.rs` -- `ROLLUP_MIN_SCHEMA`, `emitted_rollup_schema`, `rollup_schema_accepted`; `error.rs` -- `ErrorCode::RollupSchemaOld`.
- `crates/arena-store/src/summary.rs` -- `Store::rollup_days_below`.
- `crates/arena-core/src/rollup.rs`, `snapshot.rs` -- `build_rollup`/`daily_rollup` take the schema; `rollup_days_below`.
- `crates/arena-tasks/src/upload.rs` -- `rollup_schema` (the licence pin), `regenerate_old_days` (rate window), schema passed through `publish_rollup`.
- `crates/arena-cloud/src/rollups.rs` -- `current_rollup_schema`, `check_rollup_schema`, `rollup_days`, `RollupMetrics` (`record_file`, `missing_days`, `export_missing`), on `AuthState`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- schema helpers and `ROLLUP_SCHEMA_OLD`, generated TS.
- [x] `crates/arena-store`, `crates/arena-core` -- old-day query, schema-aware builder.
- [x] `crates/arena-tasks` -- licence pin, regeneration at 10 a minute, resume, skip of unbuildable days.
- [x] `crates/arena-cloud` -- N / N−1 gate, `ROLLUP_SCHEMA_OLD` record, missing-day count.
- [x] Tests -- schema helpers; builder refuses unknown schemas; uploader (bump, 10 a minute, once only, restart, pin behind/ahead, unbuildable); cloud (accept rule, listing, per-branch record and metric).

**Acceptance Criteria:**
- Given a Hub whose rollup schema is N and a licence pinning N, when it runs, then days with `schema < N` are rebuilt oldest first, at most 10 a minute, without blocking `apply()` or the upload cycle.
- Given a licence pinning below the Hub's schema, when a rollup is built, then the pinned schema is emitted.
- Given a rollup file at neither N nor N−1, then the cloud counts the day as missing with `ROLLUP_SCHEMA_OLD` recorded for the branch and day.
- Given a missing or old day, when the daily pass runs, then the per-branch missing-day count is exported.
- Given a regeneration interrupted by a restart, then it resumes from the oldest day still below N and rebuilt days are not rebuilt.

## Spec Change Log

## Review Triage Log

- Review (2026-10-30): no blocking findings. fmt, clippy -D warnings and the proto, store, core, tasks and cloud suites pass except the five failures that predate the story (heartbeat trusted_time, printer lan timeout, cloud backup hung-run, cloud binary unreachable, replay seeded log), none in files this story touches.

## Design Notes

Only schema v1 exists, so N = N−1 = 1 in this release and nothing is regenerated in production yet; the uploader tests stand in a second writer (`newest_schema = 2`, a builder that stamps v2) to prove the machinery before Epic 13 ships the real one. The cloud's "N" is the highest pin any tier carries, so a Hub release that adds a field needs no cloud deploy until a tier's pin rises. The cloud has no object reader yet: `RollupMetrics::record_file` and `export_missing` are what the owner read API (8.9) and the nightly metrics pass (11.31) call for each file they open, and the metric is logged like the heartbeat-gap metric until the Story 2.3 exporter lands.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-replay -p arena-hub -p arena-api -p arena-cloud`; `cargo clippy` on them with `-D warnings`; `cargo fmt --check`; `pnpm run check:generated`; `pnpm run check:protocol`; `pnpm run check:no-manifest`; `pnpm run check:layers`; `pnpm run format:check`.

## Auto Run Result

Status: in-review (implemented and verified; the review pass is the next iteration's).

- Summary: the Hub emits the licence-pinned rollup schema (clamped to the writers it keeps) and rebuilds published days below it, oldest first, 10 per rolling minute, resuming from the `rollup_log`; the cloud has the N / N−1 rule, `ROLLUP_SCHEMA_OLD`, and the per-branch missing-day count.
- Files: `crates/arena-proto/src/{rollup,error,lib}.rs` (+ generated `ErrorCode.ts`); `crates/arena-store/src/summary.rs`; `crates/arena-core/src/{rollup,snapshot}.rs`, `src/apply/tests/rollup.rs`; `crates/arena-tasks/src/upload.rs`; `crates/arena-cloud/src/{rollups,auth,lib}.rs`, `src/api/error.rs`; `crates/arena-api/src/ui_auth.rs`; `crates/arena-kiosk-core/src/queue.rs`.
- Verification: fmt and clippy `-D warnings` clean (proto, store, core, tasks, replay, hub, api, cloud, kiosk-core); all suites green except the failures below; `check:protocol`, `check:no-manifest`, `check:layers`, `format:check`, `check:strings` ok; `check:generated` reads the git tree, so it is clean once the regenerated `ErrorCode.ts` is committed.
- Failing before and after (not touched): `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings`; `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-cloud` `tests/backup.rs` `a_hung_run_times_out_with_its_own_code` (fails with the whole file, passes alone) and `tests/binary.rs` `unreachable_at_start_retries_then_exits_1_without_logging_the_token`.
- Decisions: the cloud's current schema is the highest tier pin; regeneration is gated by a rolling 60 s window on the Hub clock rather than a per-cycle count; `emitted_rollup_schema` takes the newest writer as an argument so tests can simulate a bump.
- Residual risks: no real v2 writer exists, so regeneration is proven only with a stand-in builder; the cloud side is a library with no caller until 8.9 and 11.31, and its record is in process (lost on restart, rebuilt from the next read).
