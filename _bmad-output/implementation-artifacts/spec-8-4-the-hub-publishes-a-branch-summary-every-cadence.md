---
title: 'Story 8.4: The Hub publishes a branch summary every cadence'
type: 'feature'
created: '2026-10-30'
status: 'review'
baseline_commit: 'dc67fa6'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A paid Hub ships its change log (8.1–8.3) but nothing tells the owner's phone what the branch looks like right now, and the cloud holds no queryable copy of branch rows (AD-35).

**Approach:** In the same cycle as the uploader, the paid primary Hub builds `state/latest.json` (a short, schema-versioned picture: open, live sessions, revenue so far, drawer cash, shifts, staff on shift, attention, and the owner-editable catalogue with each row's `rev`), PUTs it to the `summary_url` the batch presign brought along (or a presign of its own), then applies the system intent `summary.publish`, which writes only the `rollup_log` row `latest`.

## Boundaries & Constraints

**Always:**
- Paid tier, primary role only; a Free Hub, a follower and an unpaired Hub build nothing. The interval is the licence `summary_interval_s` (tier default if unreadable), never under 30 s, and the first cycle of a run publishes at once.
- The batch presign of a due cycle also asks `summary: true`; the link it returns is kept for that cycle (valid 600 s, dropped 60 s early). A cycle with no batch makes one summary-only presign. A `summary_fenced` answer publishes nothing.
- The summary is built on the snapshot reader (never inside `apply()`), from counts, stored totals (`daily_sales` net of refunds for the business day) and configuration. No transaction row, player name, phone, email or credential is in it; an attention item is `{kind, ref, text_params}` by id and display parameters.
- Attention: shifts closed in the last 24 h with a variance over `shift.variance_threshold_minor`, low-stock products, players 15+ days overdue (player id and two numbers), and open Hub alerts (not `stock_low`, which the low-stock items already name). Each list is capped at 500.
- `summary.publish` is a System-only intent (`{schema, built_at, key}`), no `base_rev`, that inserts or updates `rollup_log(date = 'latest')`, a change-logged table (migration 0074), so it appears in the audit and replays. No business row changes.
- The publish is applied only after the PUT answered 2xx. A failed PUT is not queued or backfilled: the cycle backs off like a failed batch and the next due cycle builds a fresh summary.
- The heartbeat carries `last_summary_at` (the `rollup_log` `latest` `built_at`).

**Never:**
- The daily rollup, snapshot or `rollup.publish` (8.5–8.8), the owner read API (8.9), or a cloud copy of rows.
- A change to the batch upload's keys, cursor or gap handling.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Due, rows waiting | paid, 3 rows pending | one presign (`batch` + `summary`), batch PUT, summary PUT, `rollup_log` row | none |
| Due, nothing waiting | paid, no rows | one summary-only presign, summary PUT | none |
| Not due | published less than an interval ago | no summary request | none |
| Free | tier Free | nothing built or requested | none |
| WAN down | summary PUT fails | no `rollup_log` row, backoff | next due cycle builds fresh |
| Fenced epoch | `summary_fenced: true` | no PUT, no row | logged |

</intent-contract>

## Code Map

- `crates/arena-proto/src/summary.rs` -- `BranchSummary`, `SummaryCatalogue`, `SummaryPublish`, constants; `intent.rs` -- `summary.publish` (System).
- `crates/arena-store/migrations/0074_rollup_log.sql`, `src/branch.rs` (`Table::RollupLog`), `src/summary.rs` -- the table, its row key, summary queries and the catalogue read.
- `crates/arena-core/src/summary.rs` -- `build_summary`; `src/apply/summary.rs` -- the intent; `writer.rs` `publish_summary`; `snapshot.rs` `branch_summary`, `UploadStatus.last_summary_at`.
- `crates/arena-tasks/src/upload.rs` -- shared presign, `publish_summary`, schedule; `heartbeat.rs` -- `last_summary_at`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- summary types, `summary.publish` intent kind and payload, generated TS.
- [x] `crates/arena-store` -- migration 0074, `Table::RollupLog` / `RowKey::RollupLog`, `rollup_log` helpers, summary queries and catalogue.
- [x] `crates/arena-core` -- `build_summary`, the apply step, `WriterHandle::publish_summary`, `SnapshotReader::branch_summary`.
- [x] `crates/arena-tasks` -- summary publish in the uploader cycle, `last_summary_at` in the heartbeat.
- [x] Tests -- apply (insert/update/only-system/validation), builder (shift, catalogue, attention, no secrets, 60 devices x 1,500 sales within budget), replay of the `rollup_log` row, uploader (shared presign, own presign, not due, Free, failed PUT, fenced), heartbeat.

**Acceptance Criteria:**
- Given a paid Hub whose summary interval has passed, when the uploader cycles, then `state/latest.json` is PUT to the `summary_url` (the batch presign's when a batch was sent) and `summary.publish` writes only `rollup_log` `latest`.
- Given the summary, then it carries the fields of the story and the catalogue rows with `rev`, and no credential or player data.
- Given the WAN is down, then nothing is queued and the next successful cycle publishes a fresh summary.
- Given a Free Hub, then no summary is built.
- Given 60 devices and a full business day of sales, then building the file takes under 50 ms (release build; the test allows 500 ms in debug).

## Spec Change Log

## Review Triage Log

## Design Notes

`rollup_log` is change-logged because the audit is a view over `changes`; each publish therefore adds one change row, shipped by the next upload cycle. The summary reads each query on its own snapshot: it is a picture of right now, not a ledger. Staff display names appear (shift owner, staff on shift); players never do.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-replay -p arena-api -p arena-hub -p arena-cloud`; `cargo clippy` on those crates with `-D warnings`; `cargo fmt --check`; `pnpm run check:generated`; `pnpm run check:protocol`; `pnpm run check:no-manifest`.

## Auto Run Result

Status: in-review (implemented and verified; the review pass is the next iteration's).

- Summary: a paid primary Hub now builds the branch summary each `summary_interval_s`, PUTs it to `state/latest.json` through the link the batch presign brought along (or its own presign), and records the publish with the system intent `summary.publish`, which writes only the change-logged `rollup_log` row `latest`. A failed PUT is not queued. The heartbeat reports `last_summary_at`.
- Files: `crates/arena-proto/src/{summary,intent,lib}.rs` (+ generated TS); `crates/arena-store/migrations/0074_rollup_log.sql`, `src/{branch,migrations,summary,lib}.rs`; `crates/arena-core/src/{summary,snapshot,status,writer,lib}.rs`, `src/apply/{summary,mod,guards,mutate,validate}.rs`, `src/apply/tests/summary.rs`; `crates/arena-tasks/src/{upload,heartbeat}.rs`; `crates/arena-replay/src/lib.rs`.
- Verification: fmt and clippy `-D warnings` clean; new tests green (apply 5, uploader 5, heartbeat 1, replay 1); `check:protocol`, `check:no-manifest`, `check:layers`, `format:check` ok; `check:generated` reads the git tree, so it is clean once the regenerated TS is committed.
- Failing before and after (not touched): `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-cloud` `binary::unreachable_at_start_…` and `backup::a_hung_run_times_out_with_its_own_code`; `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings` (its seed has no `library_entries` row); `arena-kiosk-svc` and `arena-e2e` do not compile on this Windows machine.
- Decisions: `open` means a shift is open or a session is running; variance attention covers shifts closed in the last 24 h (an open shift has no variance yet); catalogue rows are `{id, name, rev, active?, detail?}` (configuration only); the rollup_log `key` is the branch-relative `state/latest.json`; the 50 ms benchmark is a unit test with a debug allowance, not a separate CI job.
- Residual risks: the 50 ms figure was not measured on a release build here; each publish adds one change row to the next upload; the real R2 summary PUT is not exercised (tests use a local HTTP server); the first cycle after a restart republishes at once.
