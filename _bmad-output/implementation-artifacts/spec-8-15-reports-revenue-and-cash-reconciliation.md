---
title: 'Story 8.15: Reports — revenue and cash reconciliation'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: 'e1284006ed5d985a7ca2ad7827671d592b9bd487'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-9-the-owner-read-api-serves-the-live-summary-and-rollup-ranges.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-14-the-owner-app-stays-in-setup-mode-until-a-branch-goes-live-t.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Owner app's Reports screen is a placeholder, and the owner read API returns no shift rows, so an owner cannot see revenue over a range or any shift's cash count.

**Approach:** `GET /v1/branches/{id}/summary` also lists the shifts closed on the accepted days, and the Hub's rollup flags each shift over the branch threshold. Reports gets a range picker, a branch picker on `liveBranches()`, and two tabs: Revenue (totals by type and method, refunds/discounts/voids, by-day and by-branch lists, "So far today" from the live summary, missing days, partial label) and Cash reconciliation (one card per shift).

## Boundaries & Constraints

**Always:**
- Branches come from the shared live list; setup branches say "Available after <branch> goes live", Free branches show the tier lock; neither is asked for figures.
- Currencies are never mixed: one section per currency.
- Strings only from the catalogue; money in integer minor units through `formatMoney`.

**Never:**
- Stock reports (8.16), export (8.17), a change to the rollup schema number, a cloud copy of branch rows.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Range | Today / This week / This month / Custom | `summary?from&to&group_by=day` per picked branch; Custom refuses an inverted or over-400-day range with no call |
| Today in range | live `business_day` in range, no rollup for it | "So far today" row with data age; not a missing day |
| Missing days | days without an accepted rollup | muted line with the dates; totals marked Partial |
| Shifts | rollups carry shifts | card per shift; "Over threshold" only when `over_threshold` is true |
| Many currencies | All branches over INR and AED | one section per currency |
| Setup / Free branch | not live / Free | gate or tier lock; no summary or live call |
| Offline | cloud unreachable after a first open | saved copy shown with its age |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{rollup.rs,owner_read.rs,lib.rs}` -- `RollupShift.over_threshold`, `OwnerRangeShift`, `OwnerRange.shifts`; generated TS.
- `crates/arena-store/src/rollup.rs`, `crates/arena-core/src/rollup.rs` -- `RollupRows.variance_threshold`, flag in `fold_rollup`.
- `crates/arena-cloud/src/api/owner_read.rs` -- collects the shifts of accepted days.
- `apps/owner/src/lib/{reports.ts,ReportsBoard.svelte,RevenuePanel.svelte,CashPanel.svelte}`, `src/routes/reports/+page.svelte`.
- `packages/ui/src/messages/en.json` (`owner.reports.*`).
- Tests: `apps/owner/tests/{reports.spec.ts,stub.ts,shell.spec.ts,setup.spec.ts}`, `crates/arena-core/src/rollup/tests.rs`, `crates/arena-cloud/tests/owner_read.rs`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/*` -- over-threshold flag on rollup shifts, shifts on the range answer, generated types
- [x] `apps/owner` -- Reports screen: pickers, Revenue and Cash tabs, So far today, missing days, per-currency totals, gates
- [x] `packages/ui` -- `owner.reports.*` strings
- [x] Tests for each matrix row, axe in light/dark/HC for both tabs, Rust tests for the flag and the shift list

**Acceptance Criteria:**
- Given the Story 8.15 criteria in `epics.md`, when Reports runs against a stubbed `/v1`, then each is met and the suites pass.

## Spec Change Log

## Review Triage Log

Reviewed (code review, 1 pass). Over-threshold comparison matches shift-close semantics; no blocking findings. Fixed: Reports board could leave the loading status stuck when a run was superseded by an invalid range or empty pick.

## Design Notes

See the Story 8.15 entry in `epic-8-context.md`.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-cloud --all-targets -- -D warnings`; `cargo test -p arena-proto -p arena-store -p arena-core`; `cargo test -p arena-cloud --test owner_read --test branches`; `pnpm run gen:types`; `pnpm --filter @arena/owner check`; `pnpm --filter @arena/owner test`; `pnpm run check:strings check:layers check:protocol check:no-manifest format:check`

## Auto Run Result

Status: done (reviewed).

- Summary: Reports → Revenue and Cash reconciliation on the Owner app, with the cloud range answer carrying shifts and the Hub flagging shifts over the threshold in each rollup.
- Verification: fmt and clippy clean; arena-proto/store/core tests pass (1280 in core); cloud `owner_read` (15) and `branches` pass; owner app `check` 0 errors and Playwright 310 passed (15 new in `reports.spec.ts`, axe on both tabs in all themes); `check:strings`, `check:layers`, `check:protocol`, `check:no-manifest`, `format:check` ok; `check:generated` lists the new generated files until they are committed.
- Residual risks: rollup files built before this change carry no `over_threshold`, so past shifts are never flagged until regenerated; the threshold is the one in force when the rollup is built, not when the shift closed; today's closed shifts appear only once the day's rollup is published; the full cloud suite was not rerun (known unrelated failures in `tests/binary.rs` and `tests/backup.rs`).
