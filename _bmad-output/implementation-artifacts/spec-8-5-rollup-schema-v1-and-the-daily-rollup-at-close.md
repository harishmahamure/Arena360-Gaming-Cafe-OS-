---
title: 'Story 8.5: Rollup schema v1 and the daily rollup at close'
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: '9f7ff0a'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The owner's reports must be the Hub's own numbers (AD-35), but the Hub has no fixed, versioned day file to compute and later publish; the Z-report is per shift and the Reports screen per range.

**Approach:** Define rollup schema v1 in `arena-proto` (generated to TS) and a rollup engine in `arena-core` that, for one `business_day`, reads stamped columns and stored totals on the snapshot reader and returns the v1 document. It writes no row and makes no network call; publishing is Story 8.6.

## Boundaries & Constraints

**Always:**
- All money is integer minor units. Document: `{schema: 1, branch_id, business_day, currency, revenue: {total, by_type: {play, products, plans, other}, by_method: {cash, upi, card, wallet, credit}}, refunds, discounts, voids, sessions: {count, minutes}, shifts: [...], stock: {levels, waste, expiring}, credit: {outstanding, aged: {d0_7, d8_14, d15p}, players: [...]}, play_hours: [{game_id, minutes}]}`.
- `revenue.total` is the settled sales' stored totals (the Z-report `sales_minor`); `by_type` splits it as the Z-report does, `other` = discounts (negative) plus tax added on tax-exclusive bills, so the four add up to `total`; `by_method` is the positive payment rows of the day's settled bills, top-ups and credit collections (the Z-report `payments`). `refunds`, `discounts`, `voids` are `{count, amount}`, amounts non-negative (refunds are the day's credit notes; voids the lines of voided orders).
- Undone sales, voided orders and orphaned sessions never count: only `settled` orders, as in the Reports screen.
- `shifts` are the shifts closed on that business day (`shift_id, staff_name, opened_at, closed_at, float, expected, counted, variance, reason?, approver_name?`); `stock.levels` every tracked product (`product_id, name, qty, low_qty?`); `stock.waste` per product and reason, net of undone waste; `stock.expiring` lots with stock left expiring within the branch's warn days of the day (`product_id, lot, qty, expires_on`); `credit` is as of the day, aged FIFO, with at most 500 players listed (largest first) as `{player_id, username, outstanding, oldest_unpaid_on, consent}` (`consent` = latest transactional consent granted); `play_hours` minutes per game id from plays that ended that day.
- ~20 queries on the reader's own connection; never inside `apply()`; deterministic ordering so the same data gives byte-identical JSON.
- A day with no data gives a valid document of zeros and empty lists. `None` before the branch is bootstrapped.

**Never:**
- Publishing, `rollup.publish`, `rollup_log` rows, catch-up, schema bumps (8.6, 8.7), the snapshot (8.8), the read API (8.9).
- Player names, phones or emails in the file (a username and consent flag only, per the signed-off field list).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Busy day | sales, refund, void, undone sale, waste, credit | v1 JSON; totals equal that day's Z-report | none |
| Empty day | no rows | zeros, empty lists, stock levels still listed | none |
| Not bootstrapped | no branch row | `None` | none |
| Bad date | `2026-13-40` | `ValidationFailed` on `business_day` | error returned |

</intent-contract>

## Code Map

- `crates/arena-proto/src/rollup.rs` (new) -- `RollupV1` and parts, `ROLLUP_SCHEMA`; export in `lib.rs`; generated TS.
- `crates/arena-store/src/rollup.rs` (new) -- row loaders (payments by method, voids, refunds, sessions, closed shifts, expiring lots, credit players, game plays); reuse `reports.rs` (`sales_orders`, `sales_lines`, `received`, `refunded`), `stock_reports.rs`, `credit.rs` aging, `players.rs` `latest_consent`.
- `crates/arena-core/src/rollup.rs` (new) -- `RollupRows` fold + `build_rollup`; reuse `reports::daily_sales`, `stock_report::waste_report`, `locale::business_day_fn`.
- `crates/arena-core/src/snapshot.rs` -- `SnapshotReader::daily_rollup(day, now_ms)` next to `branch_summary`.
- `crates/arena-core/src/apply/tests/{rollup,z_report}.rs` -- tests; `fixture_shift` made `pub(super)`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- rollup v1 types, constant, TS export.
- [x] `crates/arena-store` -- `RollupRows` loader for one business day.
- [x] `crates/arena-core` -- fold (`build_rollup`), `SnapshotReader::daily_rollup`.
- [x] Tests -- golden JSON (fixed rows, exact), real-ledger parity with the Z-report (revenue, payments, cash, refunds, voids), empty day, bad date, busy-day speed (1,000 sessions, 500 orders under 5 s).

**Acceptance Criteria:**
- Given `arena-proto`, when v1 is defined, then it is a versioned Rust type generated to TypeScript with the field list above.
- Given a business day, when the engine runs, then it returns the v1 document and writes no row and makes no network call.
- Given the fixed fixture, when built, then the JSON equals the golden file exactly, and revenue, payments and cash totals equal that day's Z-report.
- Given 1,000 sessions and 500 orders, when it runs, then it completes in under 5 s (debug allowance in tests).

## Spec Change Log

## Review Triage Log

## Design Notes

Loading (SQL) and folding (pure) are split like the Z-report: the golden test feeds the fold fixed rows; the ledger test proves the loaders against the Z-report. The Story 4.17 device-farm 3 GB check is a CI job outside this repo's unit tests.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core`; `cargo clippy` on them with `-D warnings`; `cargo fmt --check`; `pnpm run check:generated`.

## Auto Run Result

Status: in-review (implemented and verified; the review pass is the next iteration's).

- Summary: rollup schema v1 (`RollupV1` and parts, generated TS) and a rollup engine: `arena-store` loads one business day's rows in one snapshot (`Store::rollup_rows`), `arena-core::rollup` folds them (`fold_rollup`, pure) and `SnapshotReader::daily_rollup(day, now_ms)` builds the document. It writes no row and makes no network call.
- Files: `crates/arena-proto/src/{rollup,lib}.rs` (+ generated TS); `crates/arena-store/src/{rollup,shifts,lib}.rs`; `crates/arena-core/src/{rollup,rollup/tests,snapshot,lib}.rs`, `tests/golden/rollup_v1.json`, `src/apply/tests/{rollup,z_report}.rs`.
- Verification: fmt and clippy `-D warnings` clean (proto, store, core, tasks); 1,265 arena-core tests, arena-proto, arena-store green; golden JSON equal; real-ledger test shows revenue, payments, cash, refund, void, shift and waste equal the Z-report; busy day (1,000 sessions, 500 orders) well under 5 s; `check:protocol`, `check:layers`, `format:check`, `check:no-manifest` ok; `check:generated` reads the git tree, so it is clean once the regenerated TS is committed.
- Failing before and after (not touched): `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings` (its seed has no `library_entries` row).
- Decisions: `refunds`, `discounts`, `voids` are `{count, amount}` with non-negative amounts; `by_method` is the Z-report's payments (so it includes top-ups and credit collections); `other` = discounts + tax added; shifts, waste, voids and plays are read by a UTC window and kept by business day; `stock.expiring` lists lots expiring on or before the day plus the warn days (expired stock with units left included); credit is the current balance aged FIFO as of the day, so a regenerated past day shows today's balances; session `minutes` are whole minutes of session-segment time.
- Residual risks: the 3 GB Android device-farm check (Story 4.17) is not part of this repo's unit tests; the 5 s budget is asserted in a debug build only; credit for a past day reflects current balances.
