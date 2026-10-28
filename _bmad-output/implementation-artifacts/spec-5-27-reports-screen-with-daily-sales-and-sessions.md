---
title: 'Story 5.27: Reports screen with daily sales and sessions'
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
baseline_commit: '98b2236697f8f55165a3002fed82c028672a33bf'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-26-close-a-shift-with-a-blind-count-and-a-signed-variance.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub has money, sessions and stock but nowhere to see how a day went: the Reports rail entry is a placeholder.

**Approach:** The Reports screen (permission `report.view`) lists reports and later report stories add entries to the same list (`REPORTS` in `lib/reports.ts`). Two reports ship: Daily sales (`GET /ui/reports/daily-sales?from=&to=`) and Sessions (`GET /ui/reports/sessions?from=&to=`). Both read stored rows by their stamped `business_day` column over an inclusive range of business days, on the reader connection (never `apply()`).

## Boundaries & Constraints

**Always:**

- A range is two `YYYY-MM-DD` business days, `from` <= `to`, at most 90 days (`REPORT_MAX_DAYS` = 93 server-side slack); otherwise `VALIDATION_FAILED` on `from` or `to` (HTTP 422). The Hub picker is in branch local time and defaults to the current business day (local date of now minus the day start).
- Only `settled` session, plan-sale and counter-sale orders are sales. An undone settle reopens its order and a voided order is `void`, so reversed pairs net to zero and voids are excluded by construction. Amounts are the stored `total_minor` (tax included), never recomputed.
- Daily sales rows add up to `sales_minor`: play time + products + plans (order line amounts) - coupons and manual discounts + tax added (the residual a tax-exclusive bill adds on top). Refunds (credit notes dated in the range) are shown apart: `net = sales - refunds`. Top-ups and credit collections are listed under "Payments received, not sales", never in sales.
- Sessions: count, billed minutes (the time lines of the session's settled bill; the time on the device when the bill has none, such as a pack or pass or an unsettled bill) and revenue (those time lines' amounts) by device and group; utilisation per local hour per group = minutes in session / (devices x days x 60).
- Hour of day uses the branch time zone in effect now (`locale::current_tz`).

**Never:**

- Writing anything, holding the writer, or recomputing prices; showing the report to a member without `report.view`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Day with sales | cash sale 40.00 + UPI sale 80.00, second undone | 1 order, 40.00 sales; hour bucket of its settle | none |
| Counter sale | `pos_sale`, no device | group id `""`, shown as "Counter" | none |
| Tax-exclusive plan | plan 1,000.00 + tax 180.00 | plans 1,000.00, tax added 180.00, total 1,180.00 | none |
| Session settled | 41 min billed to 45 at 100.00/h | 1 session, 45 min, revenue = time lines | none |
| No `report.view` | counter role | list says so; API 403 | `PERMISSION_DENIED` |
| Reversed or 91+ day range | `from` > `to` | message on the picker, nothing sent | `VALIDATION_FAILED` |

</intent-contract>

## Code Map

- `crates/arena-proto/src/report.rs` -- `DailySales`, `SessionsReport` and their row types, key constants; generated TS.
- `crates/arena-store/src/reports.rs` + `lib.rs` -- row loaders and `sales_report_rows` / `sessions_report_rows` (one read snapshot each, tolerant of an older schema).
- `crates/arena-core/src/reports.rs` -- `parse_range`, `daily_sales`, `sessions_report` (hour bucketing, utilisation); `snapshot.rs` -- `SnapshotReader::daily_sales` / `sessions_report`; `locale.rs` -- `current_tz`.
- `crates/arena-api/src/ui_report.rs` -- the two GET routes (`report.view`).
- Hub: `routes/reports/+page.svelte` (list), `routes/reports/daily-sales/`, `routes/reports/sessions/`, `lib/reports.ts`, `lib/ReportRange.svelte`, `lib/ScrollRegion.svelte`, `messages/en.json`.
- Tests: `reports.rs` unit tests, `pos_sale.rs` and `session.rs` core tests (including the 90-day load test), `ui_auth.rs` API test, Playwright `reports.spec.ts` (and `shell.spec.ts`, Reports is no longer a placeholder).

## Tasks & Acceptance

**Execution:**

- [x] Protocol: report types, generated TS
- [x] Store: row loaders and consistent-read methods
- [x] Core: range check, Daily sales and Sessions builders, reader methods, branch time zone
- [x] API: `GET /ui/reports/daily-sales` and `/ui/reports/sessions`
- [x] Hub UI: Reports list, range picker, Daily sales, Sessions
- [x] Tests: unit, core with real settles, 90-day load, API, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Reports on the rail with `report.view`, it lists Daily sales and Sessions with a business-day or date-range picker in branch local time; later report stories add entries to the same list.
- Given Daily sales for a business day, it shows totals by hour, by device group and by product category, and sales by type with play time, products and plans as separate rows; top-ups and credit collections appear under "Payments received, not sales".
- Given Sessions for a range, it shows count, billed minutes and revenue by device and group, and utilisation per hour for each group.
- Given undone actions in the range, reversed pairs net to zero and voided orders are excluded, with all figures from stamped `business_day` columns and stored totals.
- Given a 90-day range on a seeded branch (43,000 sessions, 60 devices' worth), both reports build within 2 s without touching `apply()`.

## Spec Change Log

## Review Triage Log

## Design Notes

The store returns raw rows and `arena-core` aggregates them, because only the core knows the branch time zone (`chrono-tz`) and the store has no date dependency. A report is two or three indexed range scans (`orders_kind_day`, `sessions_business_day`, `session_segments_session`) plus a Rust fold, so a 90-day range costs well under the 2 s budget even in a debug build. The tax-added row is a residual on purpose: it keeps the rows summing to the stored total whatever mix of lines, coupon and discount a bill had.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay`, `arena-kiosk-core`; `pnpm gen:types`, `check:generated`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `reports.spec.ts shell.spec.ts` in `apps/hub`.
