---
title: 'Story 5.31: Stock reports with waste by reason'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
baseline_commit: 'c56679d'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-27-reports-screen-with-daily-sales-and-sessions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-30-players-and-staff-reports.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A manager cannot see what stock the branch holds and what it is worth, how stock moved, how much was lost to waste and why, or which lots are about to expire, so losses and dead stock go unseen.

**Approach:** Reports gains Stock, one screen with four tabs backed by read-only endpoints (`report.view`), built from stored rows (`arena-store::stock_reports` loaders folded in `arena-core::stock_report`): `GET /ui/reports/stock` (on hand), `/stock/moves?from=&to=&product=&reason=`, `/stock/waste?from=&to=` (inclusive business days, at most 90) and `/stock/expiring` (the existing `ExpiryView`).

## Boundaries & Constraints

**Always:**

- On hand: every live tracked non-bundle product (a product with no `stock` row reads as 0), by name: quantity, average cost, value = quantity x average cost, and the total value.
- Movements: every `stock_moves` row whose local business day (the branch locale at the move's time) is in the range, newest first, optionally of one product and one move reason; at most 1,000 lines with `total` and `truncated`. Quantity is the signed `delta`. Value is at cost with the quantity's sign: the stored `value_minor`, or for a row that stores none (a stock-in) the quantity at the unit cost.
- Waste: waste moves (`waste_reason` set) and expired write-offs (`reason = 'expired'`) in the range, at cost, net of undone waste (a `waste_reverted` move counts negatively). Always the five reasons in order Spoiled, Broken, Free, Cancelled order, Expired write-off (zero when none), the total, and per product (largest value first) with its split by reason; products that net to nothing are left out.
- Expiring: lots with stock inside the warning window and expired lots not yet written off, as the Inventory lists show them (`catalog::expiry_view`).
- A member without `report.view` gets 403 `PERMISSION_DENIED`; Reports adds a Stock entry (after Staff).

**Never:**

- Writing to the database, or recomputing a value from a price: only stored cost is shown.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| On hand | 20 Cola at 15.00, tracked Apple with no stock | Apple 0 / 0.00, Cola 300.00, total 300.00 | none |
| Movements | stock-in, waste, undone waste, sale | newest first, signed quantity and value, former staff shown as "Former staff member" | none |
| Filter | product and/or reason | only matching moves | none |
| Waste | spoiled 4 then 1 undone, broken 1, expired 5 | spoiled 3, broken 1, expired 5, totals net of the undo | none |
| Empty | no stock or moves | empty lists, five zero reasons, "No tracked products yet." / "No movements in this range." / "No waste in this range." | none |
| Bad range | `to` before `from`, over 90 days | none | `VALIDATION_FAILED` `from`/`to` |
| No `report.view` | counter role | API 403; the screen shows the "no access" note | `PERMISSION_DENIED` |

</intent-contract>

## Code Map

- `crates/arena-store/src/stock_reports.rs` -- loaders `stock_levels`, `stock_moves_between`; `lib.rs` -- `Store::stock_levels`, `Store::stock_moves_rows`, row types.
- `crates/arena-proto/src/stock_report.rs` -- `StockReport`, `StockLevel`, `StockMoves`, `StockMoveLine`, `WasteReport`, `WasteReasonRow`, `WasteProductRow` (+ generated TS).
- `crates/arena-core/src/stock_report.rs` -- `stock_report`, `stock_moves`, `waste_report` folds; `snapshot.rs` -- `SnapshotReader::stock_report`, `stock_moves`, `waste_report` (`expiry` reused).
- `crates/arena-api/src/ui_report.rs` + `ui_auth.rs` routes -- `stock`, `stock_moves`, `stock_waste`, `stock_expiring`.
- Hub: `lib/reports.ts` (entry, loaders, `MOVE_REASONS`), `routes/reports/stock/+page.svelte` (`TabControl`), `packages/ui/src/messages/en.json`.
- Tests: store loader test, core fold tests, API test, Playwright `reports.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-store` -- stock level and move loaders, `Store` reads
- [x] `arena-proto` -- stock report types and generated TS
- [x] `arena-core` -- folds and snapshot reads
- [x] `arena-api` -- `/ui/reports/stock`, `/moves`, `/waste`, `/expiring`
- [x] Hub -- Stock screen with On hand, Movements, Waste and Expiring tabs, list entry, strings
- [x] Tests -- store, folds, API, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Reports → Stock, when it renders, then it shows current stock per tracked product with quantity, average cost and value, and the total stock value.
- Given Movements for a range, when it renders, then it lists every `stock_moves` row (product, reason, quantity, value, staff, time), filterable by product and reason.
- Given Waste for a range, when it renders, then it shows totals by reason (Spoiled, Broken, Free, Cancelled order, Expired write-off) with quantity and value at cost, and per product.
- Given Expiring, when it renders, then it lists lots expiring within the warning window and expired lots not yet written off.

## Spec Change Log

## Review Triage Log

Review (unattended): folds, loaders and endpoints checked against the stock waste, undo and expiry writers (apply/stock.rs); no blocking findings. fmt, clippy, cargo tests, pnpm checks and Hub svelte-check green.

## Design Notes

Stock moves carry no business day, so the range is read with a slack window around the UTC bounds (`shift_read_window`) and each move is kept by `business_day_fn(at)`, as the Staff report does for shifts. A waste undo is a `waste_reverted` move with the same `waste_reason` and the original value, so the fold negates it. Expired write-offs have no `waste_reason`; they are their own reason in the waste report. The product filter is built from the On hand list (tracked products), loaded once when Movements is first opened. The Expiring tab reuses the Inventory view rather than a second implementation, so both always agree.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`; `pnpm gen:types`, `check:generated`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `reports.spec.ts` in `apps/hub`.

## Auto Run Result

Status: in-review. Implemented and verified (fmt, clippy, cargo tests for proto/store/core/api, pnpm checks, Hub svelte-check, Playwright `reports.spec.ts`).
