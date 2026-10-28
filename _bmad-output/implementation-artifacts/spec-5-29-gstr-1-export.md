---
title: 'Story 5.29: GSTR-1 export'
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
baseline_commit: '01816a0'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-24-refund-a-settled-order-with-a-credit-note.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-27-reports-screen-with-daily-sales-and-sessions.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner's accountant files GSTR-1 from the invoices the Hub issued, but the Hub has no way to hand a month of them over, so the accountant retypes them.

**Approach:** Reports gains a GST entry in a GST branch only (`GET /ui/reports/gst?month=YYYY-MM`, `report.view`) that shows the month's totals per section, and an Export (`POST /ui/reports/gst/export {month, folder}`) that writes one ZIP of CSVs in the GST Offline Tool template format to a folder the manager types (or the saved backup folder), with no network call: `b2b.csv`, `b2cs.csv`, `cdnr.csv`, `hsn.csv`, `docs.csv` and a `summary.csv` that reconciles the sections. The files are built from stored rows only (`arena-store::gst` loaders folded by `arena_core::gst`); nothing is recomputed from current rates.

## Boundaries & Constraints

**Always:**

- A month is the business days `YYYY-MM-01` to its last day. Invoices are settled `session`, `plan_sale` and `pos_sale` orders that hold an `invoice_no`; credit notes are `refunds` rows (`credit_note_no`) by `business_day`. An order with a `buyer_gstin` is B2B; every other order is B2C. A credit note takes the buyer of its original order.
- Tax comes from the order's (or credit note's) stored `tax_breakup` per rate (`taxable_minor`, `cgst_minor`, `sgst_minor`). Session and plan orders store none (Stories 3.x and 5.12), so they are listed at 0% with the stored total as taxable value, and the screen says how many invoices are listed that way.
- B2B and CDNR rows carry the place of supply of the buyer's GSTIN state code (`29-Karnataka`); an inter-state document reports its tax as integrated (central plus state) in `hsn.csv` and `summary.csv`. B2CS carries the branch GSTIN's state code. B2C credit notes net into B2CS (negative); credit notes to registered buyers go to CDNR with positive values.
- `hsn.csv` is net of credit notes: each document's per-rate taxable value and tax are shared over its lines of that rate by largest remainder (so the sums equal the stored breakup exactly), keyed by HSN (a product's `hsn`, `branch.tax.sac` for play time and plans) and rate.
- `docs.csv` lists, per `(fy, segment)` of the month, the invoices (first, last, count) and the credit notes (first, last, count); gaps in the month's range of that segment count as Cancelled on the invoices row. An empty month writes headers only and the screen says "No invoices in <month>."
- The ZIP is written beside nothing else: `GSTR1_<branch code>_<YYYY-MM>.zip` into the folder (a plain absolute path that is a writable directory), through a temp name and a rename. No network call is made.
- Text cells that start with `=`, `+` or `@` get a leading `'` so a spreadsheet cannot run them.

**Never:**

- Writing to the database, recomputing tax from today's rates, I/O inside `apply()`, an export for a simple-tax branch (`VALIDATION_FAILED` on `tax`; the Reports list does not show GST there).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fixture month | B2B + B2C invoices at 5% and 18%, a credit note to each, two epochs | sections reconcile: B2B + B2CS - CDNR = HSN net; Documents issued lists each segment's first and last, none missing | none |
| Inter-state B2B | buyer state differs from the branch | place of supply = buyer state; integrated tax in HSN and summary | none |
| Empty month | no invoices | files hold headers only, screen says "No invoices in <month>." | none |
| Simple-tax branch | `gst` false | GST not on the list | `VALIDATION_FAILED` `tax` |
| Bad month | `2026-13` | none written | `VALIDATION_FAILED` `month` |
| Bad folder | relative, missing, a file, read-only, a tree URI | none written | `VALIDATION_FAILED` `folder` with the reason |
| No `report.view` | counter role | API 403 | `PERMISSION_DENIED` |

</intent-contract>

## Code Map

- `crates/arena-store/src/gst.rs` (new) -- `GstRows` loader over a month; `lib.rs` -- `Store::gst_rows`.
- `crates/arena-proto/src/gst.rs` (new) -- `GstMonth`, `GstSection`, `GstExportRequest`, `GstExported`; generated TS.
- `crates/arena-core/src/gst.rs` (new) -- `build`, CSV text, state table, stored ZIP writer; `snapshot.rs` -- `gst_month`, `gst_export`.
- `crates/arena-api/src/ui_report.rs` + `ui_auth.rs` routes -- the two GST routes (the export writes the file off the writer's thread).
- Hub: `lib/reports.ts` (GST entry shown for a GST branch, loaders), `routes/reports/+page.svelte`, `routes/reports/gst/+page.svelte`, `packages/ui/src/messages/en.json`.
- Tests: core `gst.rs` unit (golden fixture, ZIP reader), store loader, API test, Playwright `reports.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] Store: month loader for invoices, lines and credit notes
- [x] Protocol: `GstMonth`, `GstExported`, generated TS
- [x] Core: state table, fold, CSVs, stored ZIP, snapshot reads
- [x] API: month and export routes
- [x] Hub UI: GST entry (GST branches only), month picker, totals, export to a folder
- [x] Tests: golden fixture, edge matrix, ZIP, API, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given a GST branch, then Reports lists GST; given a simple-tax branch, it does not.
- Given a month and a folder, when a manager exports, then one ZIP of CSVs (B2B, B2CS, CDNR, HSN, Documents issued) is written there with no network call.
- Given a fixture month with B2B and B2C invoices, two tax rates, a credit note and an epoch change, then taxable value, CGST and SGST per section reconcile to the invoices minus credit notes, and Documents issued lists each segment's first and last number with none missing.
- Given a month with no invoices, then the files hold headers only and the screen says "No invoices in <month>."

## Spec Change Log

## Review Triage Log

## Design Notes

The GST Offline Tool's B2B, B2CS and CDNR templates carry rate and taxable value only (it works the tax out itself); the per-section CGST and SGST that must reconcile live in `hsn.csv` (which has the tax columns) and in `summary.csv`, an extra file for the accountant that the tool ignores. A credit note takes its number from the same `(fy, segment)` counter as the invoices, so the invoice range and the credit-note range of a segment interleave; Documents issued therefore counts each nature and reports numbers in the month's range that belong to neither as Cancelled.

Deferred: writing to an Android tree URI (needs the Kotlin bridge to write a file, not only probe), session and plan invoices at their real GST rate (the Hub does not store a breakup for them), the B2CL and exempt sections.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`; `pnpm gen:types`, `check:generated`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `reports.spec.ts` in `apps/hub`.
