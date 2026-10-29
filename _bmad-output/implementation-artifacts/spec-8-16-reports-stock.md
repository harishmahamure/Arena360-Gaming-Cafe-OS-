---
title: 'Story 8.16: Reports — stock'
type: 'feature'
created: '2026-10-30'
status: 'review'
baseline_commit: 'd9e957346ceb77f38d861ef429eedefef0c2c865'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-15-reports-revenue-and-cash-reconciliation.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Reports has no Stock tab, and the owner read API passes no stock figures through, so an owner cannot see levels, low stock, waste or expiring lots without visiting the branch.

**Approach:** `GET /v1/branches/{id}/summary` also carries `stock` (newest levels and expiring lots, waste summed over the accepted days) and the Hub's rollup levels carry the product category. Reports gets a Stock tab on the shared range and branch pickers: levels with their rollup date, the live low-stock items, a Low stock list, Waste in the range, and Expiring lots in the next 7 days; All branches adds a column per branch and a total matched by name and category.

## Boundaries & Constraints

**Always:**
- Branches come from the shared live list; setup branches show the gate, Free branches the tier lock; neither is asked for figures.
- Currencies are never mixed: one section per currency, waste value summed only inside one.
- Strings only from the catalogue; money in integer minor units through `formatMoney`.

**Never:**
- Export (8.17), a change to the rollup schema number, a cloud copy of branch rows.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| One branch | rollup levels, waste, lots, live low-stock item | levels with date, "Low right now", Low stock (qty ≤ threshold), waste by product and reason with value, lots within 7 days |
| Levels window | any range | a second `summary` over the last 7 days feeds levels and expiring lots; the picked range feeds waste |
| All branches | same name and category at two branches | one row, a column per branch, a total; "—" where a branch lacks it |
| Many currencies | INR and AED branches | one section per currency |
| No tracked products | newest rollup has no levels | "No tracked products at this branch." |
| No rollup in the week | `levels_as_of` null | "No stock figures yet." |
| Expired lot | `expires_on` before today | listed with an "Expired" badge; lots beyond 7 days are not listed |
| Setup / Free branch | not live / Free | gate or tier lock; no summary or live call |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{rollup.rs,owner_read.rs,lib.rs}` -- `RollupStockLevel.category`, `OwnerRangeStock` (+ `OwnerRangeWaste`, `OwnerRangeExpiring`), `OwnerRange.stock`; generated TS.
- `crates/arena-store/src/stock.rs`, `crates/arena-core/src/rollup.rs` -- `ThresholdRow.category`, written into the rollup levels; golden file `tests/golden/rollup_v1.json`.
- `crates/arena-cloud/src/api/owner_read.rs` -- builds `stock` from the accepted days.
- `apps/owner/src/lib/{reports.ts,ReportsBoard.svelte,StockPanel.svelte}`.
- `packages/ui/src/messages/en.json` (`owner.reports.stock.*`, `owner.reports.tab.stock`).
- Tests: `apps/owner/tests/{reports.spec.ts,stub.ts}`, `crates/arena-proto/src/owner_read.rs`, `crates/arena-cloud/tests/owner_read.rs`, `crates/arena-core/src/rollup/tests.rs`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/*` -- category on rollup levels, `stock` on the range answer, generated types
- [x] `apps/owner` -- Stock tab: levels, live and rollup low stock, waste, expiring lots, per-branch columns and totals, per-currency sections, gates
- [x] `packages/ui` -- `owner.reports.stock.*` strings
- [x] Tests for each matrix row, axe in light/dark/HC for the Stock tab, Rust tests for the stock fold and the range answer

**Acceptance Criteria:**
- Given the Story 8.16 criteria in `epics.md`, when Reports → Stock runs against a stubbed `/v1`, then each is met and the suites pass.

## Spec Change Log

## Review Triage Log

Not yet reviewed.

## Design Notes

See the Story 8.16 entry in `epic-8-context.md`.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-cloud --all-targets -- -D warnings`; `cargo test -p arena-proto -p arena-store -p arena-core`; `cargo test -p arena-cloud --test owner_read --test branches`; `pnpm run gen:types`; `pnpm --filter @arena/owner check`; `pnpm --filter @arena/owner test`; `pnpm run check:strings check:layers check:protocol check:no-manifest format:check`

## Auto Run Result

Status: review.

- Summary: Reports → Stock on the Owner app, with the cloud range answer carrying stock and the Hub's rollup levels carrying the product category.
- Verification: fmt and clippy clean; arena-proto (272), arena-store and arena-core (1280) tests pass; cloud `owner_read` (16) and `branches` pass; owner app `check` 0 errors and Playwright 328 passed (7 new for Stock, axe on the tab in all themes); `check:strings`, `check:layers`, `check:protocol`, `check:no-manifest`, `format:check` ok; `check:generated` lists the new generated files until they are committed.
- Residual risks: rollups built before this change carry no category, so those products match across branches by name alone; expiring lots are limited to the branch's own warning window (default 3 days), so lots 4–7 days out may be missing; the full cloud suite was not rerun (known unrelated failures in `tests/binary.rs` and `tests/backup.rs`).
