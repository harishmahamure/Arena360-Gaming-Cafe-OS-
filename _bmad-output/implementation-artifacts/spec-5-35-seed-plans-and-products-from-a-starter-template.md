---
title: 'Story 5.35: Seed plans and products from a starter template'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
baseline_commit: '7b91fe0'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-33-csv-import-with-dry-run-starting-with-products.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-34-import-players-and-plans-by-csv.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A new café owner opens Settings with no plans and no products and has to type every one, or find a CSV, before the first sale.

**Approach:** Settings → Import gains a Templates tab with three starter templates (Indian café, Console lounge, VR arcade). `GET /ui/import/templates` lists them with the plans and products each adds, `POST /ui/import/templates/preview {template_id}` is a dry-run that writes nothing, `POST /ui/import/templates/commit {template_id, import_id}` adds what the dry-run lists through the existing `import.products` (chunk 0) and `import.plans` (chunk 1) intents. No new intent kind.

## Boundaries & Constraints

**Always:**

- A template is static data in `arena_core::import_starter`; prices are written in major units and scaled to the branch currency's decimals. Product rows leave `tax_rate` empty (each takes the branch's product rate from Story 5.1) and carry an HSN code, so the same rows pass in GST and simple-tax branches.
- Plans take **all live device groups of the template's kind** (`pc`, `console`, `vr`; capped at 50). A plan with no such group is skipped (`no_group`). A plan whose exact name exists is skipped (`exists`, so an edited plan is never overwritten). A product whose name or barcode already exists (the Story 5.33 `resolve` match) is skipped (`exists`). A row that fails the import rules is skipped (`invalid`). The dry-run reports each item as added or skipped with its reason.
- Rows are checked with the CSV importers' own rules (`import_plans::check_row`, `import_products::check_row`), so the preview is what the intents do.
- Commit reads the template again against the Hub, so a second commit of the same template adds nothing (422 `template_id` "already set up"). Products are chunk 0 and plans chunk 1 of the `import_id` whatever is empty, so a retry after a half-finished attempt reuses the same keys. Plan names differ between templates; product names may overlap and are then skipped.
- Needs `catalog.edit`. Wizard step 6 completes by itself from the heartbeat's `products_count` (Story 5.3), so nothing else is written for it.

**Never:**

- Writing anything in a preview, or touching players, stock, tax settings or groups.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Clean apply | Indian café, a PC group, empty catalogue | 5 plans + 9 products added; `chunks 2` | none |
| No matching group | Indian café, only a console group | plans `skipped (no_group)`, products still added | none |
| Name taken | plan `PC 1 hour pack`, product `Samosa` exist | those two `skipped (exists)`, rest added | none |
| Applied twice | same template again | `new 0`; commit refused | 422 on `template_id` |
| Other template | Console lounge after Indian café | 5 plans, only products not on the menu | none |
| Unknown id | `nope` | refused | 422 on `template_id` |
| No permission | role without `catalog.edit` | refused | 403 |

</intent-contract>

## Code Map

- `crates/arena-proto/src/import.rs` -- `StarterTemplateInfo`, `StarterPlanInfo`, `StarterProductInfo`, `StarterPreviewRequest`, `StarterCommitRequest`, `StarterItem`, `StarterPreview`, `StarterResult` (+ generated TS).
- `crates/arena-core/src/import_starter.rs` -- the three templates, `templates(exponent)`, `analyse`; `snapshot.rs` `starter_templates`, `starter_import`.
- `crates/arena-api/src/ui_import.rs` -- `starter_list`, `starter_preview`, `starter_commit` (`submit_chunks` now takes the chunk number), routes in `ui_auth.rs`.
- Hub: `lib/StarterPanel.svelte`, `lib/import.ts` (`loadStarterTemplates`, `previewStarter`, `commitStarter`), `routes/settings/import/+page.svelte` (tab), `en.json`.
- Tests: core (`import_starter::tests`), API (`a_starter_template_is_previewed_then_added_once`), Playwright `import.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- starter request, preview and result types, generated TS
- [x] `arena-core` -- templates, group mapping, skip rules, reader methods
- [x] `arena-api` -- list, preview and commit routes through the existing intents
- [x] Hub -- Templates tab, strings
- [x] Tests -- core, API, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Settings → Import → Templates, when a Manager opens it, then it offers Indian café, Console lounge and VR arcade, each listing the plans and products it will add.
- Given a template chosen, when the dry-run runs, then plans are mapped to existing groups of the matching kind (PC, console, VR); plans with no matching group, and products whose name or barcode already exists, are listed as skipped.
- Given commit, when it applies, then products are created through the Story 5.33 import intent and plans through the Story 5.34 plan import intent, with prices in the branch currency and the branch's product tax rate, and the wizard's step 6 completes once products exist (heartbeat `products_count`).
- Given a template already applied, when it is applied again, then nothing is duplicated.

## Spec Change Log

## Review Triage Log

Code review (2026-10-28): no blocking findings. fmt, clippy -D warnings, cargo tests (proto, core, api) and Hub svelte-check re-run green.

Unattended build: fmt, clippy (-D warnings) and cargo tests for proto/store/core/api, Hub `svelte-check`, Playwright `import.spec.ts` and `settings.spec.ts`, `format:check`, `check:strings`, `check:protocol`, `check:layers`, `check:hex` green. `check:generated` passes once the new generated files are committed. `check:tokens-used` fails on `--ad-reserved` in `routes/floor/+page.svelte` and `arena-e2e` does not build on Windows (unix-only); both fail without this story's changes.

## Design Notes

Plan prices are in major units of the branch currency scaled by its decimals, so a currency with no decimals gets the same numbers (60 for a one-hour pack); the manager edits them afterwards. A plan is skipped when its name exists rather than updated, unlike the CSV importer, so a template never overwrites a price the owner changed. Because commit re-reads the Hub, an `import_id` retry after a complete attempt is answered 422 ("already set up") rather than replayed; the screen re-runs the dry-run after an add and shows nothing left to add.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`; `pnpm gen:types`, `check:generated` (after commit), `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `import.spec.ts`, `settings.spec.ts` in `apps/hub`.
