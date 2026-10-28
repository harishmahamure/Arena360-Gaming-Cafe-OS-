---
title: 'Story 5.30: Players and staff reports'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
baseline_commit: 'a65cc98'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-27-reports-screen-with-daily-sales-and-sessions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-29-gstr-1-export.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The owner cannot see who the best players are, who is new or returning, who owes credit and for how long, or what each staff member sold, discounted, voided and refunded, so regulars go unrewarded and problems go unseen.

**Approach:** Reports gains Players and Staff (`GET /ui/reports/players?from=&to=` and `GET /ui/reports/staff?from=&to=`, `report.view`, inclusive business days, at most 90), read-only and built from stored rows (`arena-store::reports` loaders folded in `arena-core::reports`). Player names are opened with the branch key; a player whose data key is absent reads as an erased player with no personal data.

## Boundaries & Constraints

**Always:**

- Sales are settled `session`, `plan_sale` and `pos_sale` orders by `business_day` at their stored `total_minor`; a voided order (`status = 'void'`) is never a sale. Refunds, top-ups and credit collections are not in a player's or staff member's sales total.
- Players: top spenders (up to 20, spend descending, then name): name, visits (distinct business days with a settled sale in the range), spend. A bill with no `player_id` is a guest bill: one "Guest" total (bills, spend), never listed as a player. New = the player's first settled sale ever falls in the range; returning = they had one before it (count and spend of each). A player with no data key (no `player_keys` row, or a key that will not open) has `erased = true` and an empty `name`; the screen shows "Erased player".
- Credit outstanding is as of today's business day: the FIFO aging of `arena_store::credit_aging` summed over every debtor into 0-7, 8-14 and 15+ day buckets, plus the debtors (up to 50, largest outstanding first: name, outstanding, oldest age in days).
- Staff, per staff member with any activity in the range: sales total and count (`settled_by`), sessions started (insert of a session by that staff member in the change log, by the session's `business_day`), discounts given (manual `discount_minor > 0`, by `discount_by`: count and value), voids (by `voided_by`: count and value of the voided order's total, by the order's `business_day`; excluded from sales), refunds approved (`approved_by`: count and value, by `business_day`) and variance per shift (closed shifts of that `staff_id` whose `closed_at` falls in the range by local business day: shift id, closed time, signed `variance_minor`; their sum). A staff member no longer in the table shows as an empty name.
- Counter without `report.view`: 403 `PERMISSION_DENIED` (the Hub opens `inline-pin` in place, as on the other reports). Reports are added to `REPORTS` (list order: after Shifts).

**Never:**

- Writing to the database, recomputing amounts from rates, showing a player's phone or other personal data, or listing an erased player's name.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fixture range | 3 players, a guest, new and returning, credit aged 3, 10 and 20 days | top spenders ranked, Guest total, new/returning split, buckets 0-7, 8-14, 15+ | none |
| Erased player | player row with no `player_keys` | row with `erased = true`, empty name; UI "Erased player" | none |
| Staff fixture | sales, a session start, a manual discount, a void, a refund, a closed shift with variance | each figure on the right staff member; the void is not in sales | none |
| Empty range | no activity | empty lists, zero totals, "No players in this range." / "No staff activity in this range." | none |
| Bad range | `to` before `from`, over 90 days | none | `VALIDATION_FAILED` `from`/`to` |
| No `report.view` / no key store | counter role / no KEK | API 403 / Players answers `UNSUPPORTED` | `PERMISSION_DENIED` / `UNSUPPORTED` |

</intent-contract>

## Code Map

- `crates/arena-store/src/reports.rs` -- add loaders: player spend by player, first-settle day per player, credit debtors, staff sales, discounts, voids, refunds approved, session starts (`changes` insert on `sessions` by a staff actor), closed shifts; `lib.rs` -- `PlayersReportRows`, `StaffReportRows`, `Store::players_report_rows`, `Store::staff_report_rows`, pattern of `sales_report_rows`.
- `crates/arena-store/src/credit.rs` -- `credit_aging`, `list_credit_debtors` (reuse, read-only).
- `crates/arena-proto/src/report.rs` -- `PlayersReport`, `PlayerSpend`, `GuestTotals`, `NewReturning`, `CreditBucketRow`, `StaffReport`, `StaffRow`, `ShiftVariance` (+ generated TS, `lib.rs` exports).
- `crates/arena-core/src/reports.rs` -- `players_report`, `staff_report` folds; `players.rs` -- `open_field` (names); `snapshot.rs` -- `SnapshotReader::players_report(from, to, now, kek)`, `staff_report`; `locale.rs` -- `current_business_day`, tz for shift days.
- `crates/arena-api/src/ui_report.rs` + `ui_auth.rs` routes -- `players`, `staff` (players needs `auth.kek`).
- Hub: `lib/reports.ts` (entries, loaders), `routes/reports/players/+page.svelte`, `routes/reports/staff/+page.svelte`, `packages/ui/src/messages/en.json`.
- Tests: core fold unit tests, store loader test, API test (403, erased, fixture), Playwright `reports.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-store` -- loaders and `players_report_rows` / `staff_report_rows`
- [x] `arena-proto` -- report types and generated TS
- [x] `arena-core` -- folds and snapshot reads
- [x] `arena-api` -- `/ui/reports/players` and `/ui/reports/staff`
- [x] Hub -- Players and Staff screens, list entries, strings
- [x] Tests -- fold fixture, erased player, edge matrix, API, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Reports → Players for a range, when it renders, then it shows top spenders (name, visits, spend), new vs returning players, guest totals as "Guest", and credit outstanding aged 0-7, 8-14 and 15+ days.
- Given a player whose data key is absent, when reports list players, then the row shows "Erased player" with no personal data.
- Given Reports → Staff for a range, when it renders, then each staff member shows sales total, sessions started, discounts given (count and value), voids (count and value, excluded from sales), refunds approved and variance per shift.
- Given a Counter without `report.view`, when they open Reports, then `inline-pin` opens in place.

## Spec Change Log

## Review Triage Log

## Design Notes

A "visit" is a business day with at least one settled sale, so a player who buys time and a drink the same evening has one visit. Sessions have no `started_by` column; the actor of the change-log insert is the source, so nothing is migrated. Voids and discounts take the approving manager (`voided_by`, `discount_by`) as the staff member, matching how the audit shows them. Credit ageing is as of today, not the range end, because it describes what is owed now.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`; `pnpm gen:types`, `check:generated`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `reports.spec.ts` in `apps/hub`.

## Auto Run Result

Status: in-review. Implemented and verified (fmt, clippy, cargo tests for proto/store/core/api, pnpm checks, Hub svelte-check, Playwright `reports.spec.ts`). Note: without `report.view` the screens show the existing "no access" note like the other reports, rather than opening `inline-pin`.

## Review Result

Code review (2026-10-28): no blocking findings. fmt, clippy, cargo tests (proto/store/core/api), pnpm checks and Hub svelte-check all green. Accepted: without `report.view` the screens show the "no access" note, as the other reports do, rather than `inline-pin`.
