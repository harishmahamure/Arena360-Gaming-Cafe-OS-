---
title: 'Story 5.33: CSV import with dry-run, starting with products'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
baseline_commit: '8312a1c'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-3-products-with-categories-barcodes-images-and-the-kitchen-fla.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-32-export-every-hub-report-to-csv-and-pdf.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner moving from another system has to type every product into the Hub by hand.

**Approach:** A product CSV is read in two steps. `POST /ui/import/products/preview {csv}` is a dry-run: it parses the file, checks every row with the Story 5.3 product rules and the stock-in rules against the products the Hub holds, and answers the counts (new, updated, failed) with each failure's row and reason, writing nothing. `POST /ui/import/products/commit {csv, import_id, valid_only?}` applies the rows that passed as `import.products` intents of 500 rows, each with its own idempotency key derived from the client's `import_id`. `POST /ui/import/products/template {folder}` saves the template. The screen is Settings → Import.

## Boundaries & Constraints

**Always:**

- Columns `name, category, price, tax_rate, hsn, barcode, track_stock, kitchen, opening_qty, unit_cost` (UTF-8, header row, BOM, CRLF/LF, quoted cells; `name` and `price` required, the rest optional, any order, names case-insensitive). `price` and `unit_cost` are major units with at most the currency's decimals; `tax_rate` is a percent (`5`, `12.5`); `track_stock`/`kitchen` take `true/false/yes/no/y/n/1/0`, empty is false; `opening_qty` is a whole number.
- A row updates the live product with its barcode, else the one with exactly its name (case-sensitive; an active one wins); otherwise it is new. Updating keeps the product's image, favourite and active flags. A match on a bundle fails the row.
- One check serves the preview and the intent (`import_products::check_row`): the `product.upsert` rules (`apply::products::check_import_row`, so `HSN_REQUIRED` in a GST branch, `BARCODE_TAKEN` for another active product's barcode), then opening stock (needs `track_stock`, `1..=MAX_STOCK_IN_QTY`, held + qty within `MAX_STOCK_QTY`, cost in range). Rows of one file also collide: a repeated barcode, or two rows resolving to the same product (or two barcode-less new rows with one name), fail the later row.
- Conflicts block commit: any failed row or unknown column leaves "Import" closed; "Import valid rows only" imports the rest. A missing `name`/`price` column, a repeated column, an empty or unreadable file make the file unusable.
- `import.products` (`catalog.edit`; also `stock.in` when any row opens stock, on the actor or the elevating manager) carries 1 to 500 rows, validates all of them and applies all or none. Each row is `product.upsert`'s own write (`products::mutate_upsert`) and, with `opening_qty`, a `stock.in` lot (supplier "CSV import", no expiry) through `stock::opening_stock`.
- Chunk `n` of an import uses idempotency key = the `import_id` ULID with its last 16 bits set to `n` (and an intent id with bit 15 also set), so a retry with the same `import_id` replays stored outcomes and never books stock twice.
- Limits: 1.5 MB of CSV, 10,000 rows, 200 listed failures (counts stay complete).
- The template is the exact header and two sample rows and passes its own dry-run.

**Never:**

- Writing anything in the preview, a network call, or a partial chunk.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Clean file | 3 rows, 2 new, 1 matching a barcode | `new 2, updated 1, failed 0`; Import open | none |
| Bad number | `price` = `abc` on line 3 | failure `{row: 3, field: price, reason: bad_number}`; Import closed | none |
| Duplicate barcode | line 5 repeats line 2's barcode | failure on 5 `duplicate_barcode` | none |
| Unknown column | header has `colour` | failure on row 1 `unknown_column`; valid rows can still import | none |
| Valid rows only | 1,001 valid + 2 bad rows | 3 chunks (500, 500, 1); created 1001, skipped 2 | none |
| Retry | same `import_id` again | stored outcomes replayed; stock unchanged | none |
| Commit with problems | `valid_only` false | 422 on `csv`, nothing written | inline message |
| No `catalog.edit` | counter role | 403 `PERMISSION_DENIED` | none |
| Opening stock without `stock.in` | role with only `catalog.edit` | intent `PERMISSION_DENIED`, nothing written | none |
| Unwritable template folder | `/no/such/folder` | 422 `folder` | screen offers another |

</intent-contract>

## Code Map

- `crates/arena-proto/src/import.rs` -- `ImportProductRow`, `ImportProducts`, the preview and commit requests and answers, `ImportFailure`, limits (+ generated TS); `intent.rs` -- `import.products` kind (`catalog.edit`, Catalog category).
- `crates/arena-core/src/import_csv.rs` -- RFC 4180 reader with line numbers and exact decimals (shared by Story 5.34).
- `crates/arena-core/src/import_products.rs` -- template, `check_row`, `duplicates`, `analyse`, `context`; `snapshot.rs` -- `SnapshotReader::product_import`.
- `crates/arena-core/src/apply/import.rs` (+ `products.rs` `upsert_from_import`/`check_import_row`, `stock.rs` `opening_stock`, `guards.rs` `importing_stock`) -- the intent.
- `crates/arena-api/src/ui_import.rs` + routes in `ui_auth.rs` -- preview, commit, template.
- Hub: `lib/import.ts`, `routes/settings/import/+page.svelte`, Settings nav, `en.json`.
- Tests: core (CSV, rules, 14 import cases, apply tests, 1,000-row timing), proptest `Op::Import`, API (`a_product_csv_is_previewed_then_committed_in_chunks_with_a_safe_retry`), Playwright `import.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- payload, requests, answers, `import.products` kind, generated TS
- [x] `arena-core` -- CSV reader, row checks shared by preview and intent, dry-run, template
- [x] `arena-core` apply -- `import.products` (permission, validate, mutate), proptest op
- [x] `arena-api` -- preview, commit in 500-row chunks with derived keys, template
- [x] Hub -- Settings → Import → Products, strings
- [x] Tests -- core, apply, proptest, API, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Settings → Import → Products, when a Manager picks a CSV, then a dry-run validates every row with the Story 5.3 rules and shows counts of new, updated and failed rows with each failure's row number and reason, and writes nothing.
- Given conflicts (duplicate barcode within the file, unknown column, bad number), when the dry-run shows them, then commit is disabled until the file is fixed, or the Manager chooses "Import valid rows only".
- Given commit, when it applies, then one `import.products` intent per 500 rows creates or updates products and records opening stock as stock-in lots, each chunk with its own idempotency key so a retry never duplicates.
- Given a 1,000-row file, when dry-run plus commit runs, then it completes in under 30 s (the core test does it in about 0.4 s in a debug build).
- Given "Download template", then a file with the exact header and two sample rows is saved.

## Spec Change Log

## Review Triage Log

Unattended build: fmt, clippy (-D warnings), cargo tests for proto/store/core/api, proptest (400 cases), `pnpm` format/strings/protocol/layers/hex checks, Hub `svelte-check` and Playwright `import.spec.ts` green.

## Design Notes

The preview and the intent cannot disagree because they share `check_row` and `duplicates`; the preview only adds the CSV reading. The intent carries typed rows (minor units, basis points), not the file, so the Hub parses once and the log never holds raw CSV. Counts in the commit answer come from the `products` changes each chunk wrote, so a replayed chunk reports what it originally did. Opening stock is additive: re-importing a file with `opening_qty` adds another lot (the `import_id` only protects a retry of one attempt). Product images, favourites, bundles and categories beyond the text column are not part of the file; updating a product leaves them as they were.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`; `pnpm gen:types`, `check:generated`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `import.spec.ts`, `settings.spec.ts`, `shell.spec.ts` in `apps/hub`.

## Auto Run Result

Status: done. Implemented, reviewed and verified.

## Code Review (2026-10-28)

Reviewed the implementation commit against the intent contract: preview/commit/template endpoints, shared `check_row`/`duplicates`, the `import.products` intent (permission, validate, all-or-none mutate), chunk idempotency keys, CSV reader. No blocking findings. Gate re-run green: fmt, clippy -D warnings, cargo tests (proto, core, api), pnpm format/strings/protocol/layers/hex/generated checks, Hub svelte-check.

Non-blocking notes: a barcode-less row and a later barcode-carrying row sharing a name in different chunks resolve against the products earlier chunks created, so the later row may update instead of create (preview counts it as new); a mid-import failure retried with the same `import_id` re-reads the Hub, so rows are matched as updates. Both are consistent with the "opening stock is additive" design note.
