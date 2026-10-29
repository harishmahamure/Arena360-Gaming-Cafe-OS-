---
title: 'Story 8.6: The daily rollup publishes at close and catches up missed days'
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: '3b47e5b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub can build a daily rollup (8.5) but never sends one, so the owner's reports have no days (AD-35, FR167).

**Approach:** In the uploader cycle, a paid primary Hub plans which business days are due (closing time + 30 min, the last open shift closing, or the day roll + 30 min with no closing time), builds each on the snapshot reader, PUTs it to `rollups/<date>.json` via presign `kind: rollup`, then applies the system intent `rollup.publish`, which upserts `rollup_log(date, schema, built_at, key)` and so appears in the audit. Missed days are caught up oldest first; a ledger change stamped to an already-rolled day regenerates and overwrites its file.

## Boundaries & Constraints

**Always:**
- Paid tier, primary role only (the uploader's existing gate); a Free Hub, a follower and an unpaired Hub build nothing.
- A day is due the first time at the first of: its closing time (opening hours, weekday of the business day, wrapping past midnight when `close < open`) + 30 min, never later than the day roll + 30 min; a closed day or an open-round-the-clock day (`0`..`1439`) has no closing time, so it is the roll + 30 min; or no shift open now and a shift closed on that business day.
- Catch-up: every business day from the one the branch went paid (the first accepted upload, `upload_cursor.first_uploaded_at`, migration 0075) up to today with no `rollup_log` row is due, oldest first; at most 10 per cycle, then the next cycle follows in 60 s. Nothing is due before the first upload.
- Regeneration: a published day is due again when a change to a ledger table (`orders`, `order_lines`, `payments`, `refunds`, `sessions`, `session_segments`, `shifts`, `cash_events`, `stock_moves`, `game_plays`) was stamped to it (the change row's `business_day`, or for orders/refunds/sessions the touched row's own day). Configuration, credit balances and stock levels never re-open a day. A day that could not be re-published stays pending.
- `rollup.publish` is System-only, `{date, schema, built_at, key}`, no `base_rev`, upserts only that day's change-logged `rollup_log` row (the audit and replay see it). `date` must be `yyyy-mm-dd` (`latest` is refused).
- The publish is applied only after the PUT answered 2xx. A failed PUT leaves no row; the day is planned again next cycle with backoff. A day that cannot be built is skipped and logged.
- The heartbeat reports `last_rollup_date` (the latest dated `rollup_log` row).
- The cycle wakes for the next clock-due rollup, and within 30 s of a shift closing, rather than a whole interval later.

**Never:**
- Schema bumps, `rollup_schema` pinning, regeneration rate limits (8.7), the snapshot (8.8), the read API (8.9), a change to `apply()` beyond the new intent, or a cloud copy of rows.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Closing | open until 23:00, now 23:30 | the day's rollup is built, PUT, recorded | none |
| Last shift | shift closed, none open | today's rollup now | none |
| 24-hour | no closing time | previous day 30 min after the roll | none |
| Off for days | 3 days without rows | three PUTs, oldest first | none |
| Late Undo | change stamped to a rolled day | rebuilt, same key overwritten | none |
| WAN down | PUT 503 | no row, backoff, planned again | none |
| Free / never uploaded | tier Free, or no cursor | nothing | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/rollup.rs` -- `RollupPublish`, `rollup_object_key`, `is_rollup_date`; `intent.rs` -- `rollup.publish` (System).
- `crates/arena-store/migrations/0075_upload_cursor_first.sql`, `src/upload.rs` (`read_paid_since`), `src/rollup.rs` (`RollupFacts`, `ChangedRow`, `ROLLUP_LEDGER_TABLES`, `load_rollup_facts`), `src/summary.rs` (`last_rollup_date`).
- `crates/arena-core/src/rollup_plan.rs` -- pure planner (`plan`, `due_at`) and `plan_rollups`; `snapshot.rs` -- `rollup_plan`, `UploadStatus.last_rollup_date`; `apply/summary.rs` -- shared publish apply; `writer.rs` -- `publish_rollup`.
- `crates/arena-tasks/src/upload.rs` -- `publish_rollups`/`publish_rollup`, the heard-changes inbox, shift-close wake; `heartbeat.rs` -- `last_rollup_date`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- `RollupPublish`, `rollup.publish` intent kind and payload, generated TS.
- [x] `crates/arena-store` -- migration 0075 (`first_uploaded_at`), paid-since, rollup facts (published days, closed shifts, changed days).
- [x] `crates/arena-core` -- planner, `SnapshotReader::rollup_plan`, `WriterHandle::publish_rollup`, the apply step for `rollup.publish`.
- [x] `crates/arena-tasks` -- rollup publish in the uploader cycle, catch-up, regeneration, early wakes, heartbeat field.
- [x] Tests -- planner units (closing, shift, 24 h, wrap, catch-up, DST), real-ledger planner, apply (upsert, system-only, validation), replay of a dated row, uploader (catch-up order, 30-min rule, failed PUT, late change, Free/unpaid, long catch-up).

**Acceptance Criteria:**
- Given closing + 30 min passes or the last open shift closes, when the uploader cycles, then `rollups/<date>.json` is PUT via presign `kind: rollup` and `rollup.publish` upserts `rollup_log` and appears in the audit.
- Given a 24-hour branch, when the business day rolls, then the previous day's rollup runs 30 minutes after the roll.
- Given the Hub was off or the WAN down, when it next runs with WAN, then every past business day since going paid with no `rollup_log` row is generated and uploaded, oldest first.
- Given a late change stamped to an already-rolled day, when it commits, then that day's rollup is regenerated and the file overwritten.

## Spec Change Log

## Review Triage Log

## Design Notes

"Since the branch went paid" is the business day of the first accepted upload (the Hub has no other marker); a Free-history backfill is the full snapshot's job (9.11). A late Undo issued after the day roll is caught through the touched order's own `business_day`, not only the change row's stamp. A shift closing mid-day with no other shift open publishes early; later sales stamped to that day regenerate it, at most once per cycle.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-replay -p arena-hub -p arena-api`; `cargo clippy` on them with `-D warnings`; `cargo fmt --check`; `pnpm run check:generated`; `pnpm run check:protocol`; `pnpm run check:no-manifest`; `pnpm run check:layers`; `pnpm run format:check`.

## Auto Run Result

Status: in-review (implemented and verified; the review pass is the next iteration's).

- Summary: a paid primary Hub plans the daily rollups each upload cycle (`arena_core::rollup_plan`), builds each due day on the snapshot reader, PUTs it to `rollups/<date>.json` through a `kind: rollup` presign and records it with the system intent `rollup.publish` (the dated `rollup_log` row). It catches up missed days oldest first (10 a cycle), regenerates a published day when a ledger row stamped to it changes, and wakes for the next clock-due day or a closing shift.
- Files: `crates/arena-proto/src/{rollup,intent,lib}.rs` (+ generated TS); `crates/arena-store/migrations/0075_upload_cursor_first.sql`, `src/{upload,rollup,summary,lib,migrations}.rs`; `crates/arena-core/src/{rollup_plan,rollup_plan/tests,snapshot,status,writer,lib}.rs`, `src/apply/{summary,mod,mutate,validate}.rs`, `src/apply/tests/rollup.rs`; `crates/arena-tasks/src/{upload,heartbeat}.rs`; `crates/arena-replay/src/lib.rs`.
- Verification: fmt and clippy `-D warnings` clean (proto, store, core, tasks, replay, hub, api, cloud); arena-proto 264, arena-store 105, arena-core 1,278, arena-hub, arena-api and the 31 uploader tests green; `check:protocol`, `check:no-manifest`, `check:layers`, `format:check` ok; `check:generated` reads the git tree, so it is clean once the regenerated TS is committed.
- Failing before and after (not touched): `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings` (its seed has no `library_entries` row); `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-kiosk-svc` and `arena-e2e` do not compile on this Windows machine.
- Decisions: "went paid" is the first accepted upload (new `upload_cursor.first_uploaded_at`, backfilled by the migration); `UploadStatus` is no longer `Copy` (it carries `last_rollup_date`); locale (zone, day start) is the one in effect now for all days of the plan; the opening-hours weekday is the business day's date; late changes are matched by the change row's `business_day` plus, for orders, refunds and sessions, the row's own day; a burst of more than 5,000 heard ledger changes triggers a two-day rescan.
- Residual risks: the real R2 presign/PUT of a rollup is not exercised (a local HTTP server stands in); the cloud's rate limit on presigns is shared with batches (10 rollups a cycle at most); a shift closing mid-day with none open publishes that day early and each later sale regenerates it once per cycle; the `Hub off past the undo window` case relies on the touched row's own day, so a changed row of a table outside orders/refunds/sessions is matched only by its change stamp.
