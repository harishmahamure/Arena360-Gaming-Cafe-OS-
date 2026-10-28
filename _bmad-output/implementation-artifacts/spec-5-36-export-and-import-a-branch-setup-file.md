---
title: 'Story 5.36: Export and import a branch setup file'
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
baseline_commit: '86c3511'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-33-csv-import-with-dry-run-starting-with-products.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-35-seed-plans-and-products-from-a-starter-template.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A café owner with a second branch re-types the first branch's groups, rates, plans, rules, coupons and menu, or relies on the cloud template (Stories 5.37-5.38) which needs the internet and a paid tier.

**Approach:** Settings → Import gains a Branch setup tab. `POST /ui/import/setup/export {folder}` writes `arena360-branch-setup.json`, a versioned file defined in `arena-proto` (`SetupFile`). `POST /ui/import/setup/preview {json}` is a dry-run that writes nothing; `POST /ui/import/setup/commit {json, import_id}` sends one new `import.setup` intent that creates what the dry-run lists through `apply()`.

## Boundaries & Constraints

**Always:**

- The file has `format` "arena360-setup", `version` 1 (`SETUP_VERSION`), the branch `currency` and lists of `groups` (name, kind, swatch, order, optional hourly `rate_minor`), `products`, `bundles`, `plans`, `rules` and `coupons`. Prices are minor units of `currency`. Everything that points at something else does so by **name** (a plan lists its group names, a bundle its component product names, a rule its group, a coupon targets by kind and name). Product categories are the label on a product, not a separate list. No players, devices, payments, stock, images or coupon use counts are exported; export orders every list and sub-list the same way on every Hub (components, plan groups and coupon targets by name), so exporting a copy gives the same file.
- Import is **create-only**. An entry that exists by name is skipped (`exists`) and never updated: group and plan names without case, a product or bundle by barcode else name, a rule by name within its group, a coupon by code. An existing group with no active rate takes the file's rate. An entry that breaks the rules of its own intent is skipped (`invalid`); one that names a group, product or plan that neither the Hub nor the file creates is skipped (`no_group`, `no_product`, `no_plan`).
- The dry-run and the intent run the same `setup_file::analyse`, which uses the single-item rules (`group.upsert`, `rate`, `product.upsert` with bundle components, `plan.upsert`, `rule.upsert`, `coupon.upsert`). Steps run in dependency order (groups, rates, products, bundles, plans, rules, coupons) and each goes through its intent's own load, validate and mutate inside the one `import.setup` transaction: all of it applies or none of it does, and the idempotency key makes a retry a no-op.
- A file whose `version` is above this build's is refused `UNSUPPORTED` with "Update this Hub to import this file."; a file that is not a setup file, is over 1 MB, has a list over 2 000 entries, or is priced in another currency than the branch is `VALIDATION_FAILED` (fields `file`, `currency`).
- Needs `catalog.edit`; a file that creates groups also needs `device.manage` (the actor's or the elevating manager's). Commit reads the file again against the Hub, so a second commit of the same file is 422 on `file`.

**Never:**

- Writing anything in a preview, or touching players, stock, devices, tax settings, money rows or images.
- Updating an entry that already exists.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Clean copy | File from Hub A, empty Hub B | every entry added; B exports the same file | none |
| Name taken | group `PC`, plan `2 hour pack` exist | skipped `exists`; a plan or rule for that group attaches to the existing group | none |
| Existing group, no rate | `pc` exists, file has a rate for `PC` | rate added to the existing group | none |
| Missing reference | plan names group `Nowhere` | plan skipped `no_group`, rest added | none |
| Imported twice | same file again | preview `added 0`; commit refused | 422 `file` |
| Newer format | `version: 2` | refused | 400 `UNSUPPORTED`, "Update this Hub to import this file." |
| Other currency | file in USD, branch INR | refused | 422 `currency` |
| No permission | role without `catalog.edit` | refused | 403 |

</intent-contract>

## Code Map

- `crates/arena-proto/src/setup_file.rs` -- `SetupFile` and its entries, `ImportSetup`, requests, `SetupPreview`, `SetupFileResult` (+ generated TS); `intent.rs` -- `import.setup` kind (catalog, `catalog.edit`).
- `crates/arena-core/src/setup_file.rs` -- `export`, `parse`, `Context`, `analyse` (the plan of steps and the preview); `snapshot.rs` `setup_export`, `setup_import`.
- `crates/arena-core/src/apply/import_setup.rs` -- load/validate/mutate over the single-item intents; small `check_import`/`price_ok`/`check_bundle` helpers in `groups`, `rates`, `rules`, `coupon`, `products`; `guards.rs` `importing_setup`.
- `crates/arena-api/src/ui_import.rs` -- `setup_export`, `setup_preview`, `setup_commit`; routes in `ui_auth.rs`.
- Hub: `lib/SetupPanel.svelte`, `lib/import.ts` (`exportSetup`, `previewSetup`, `commitSetup`; a 400 answer now shows its message), `routes/settings/import/+page.svelte` (tab), `en.json`.
- Tests: core (`apply::tests::import_setup`), API (`a_branch_setup_file_is_exported_and_added_once_at_another_hub`), Playwright `import.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- setup file format, `import.setup` intent, request and answer types, generated TS
- [x] `arena-core` -- export, parse (version check), dry-run analysis, `import.setup` through `apply()`
- [x] `arena-api` -- export, preview and commit routes
- [x] Hub -- Branch setup tab, strings
- [x] Tests -- core apply, API round trip, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Settings → Import → Branch setup, when a Manager chooses Export, then the Hub writes a versioned JSON file with groups, rates, plans, pricing rules, coupons, product categories, products (without stock or images) and bundles, and no players, devices, money or stock rows.
- Given a setup file, when a Manager imports it at another Hub, then a dry-run lists what will be created and what is skipped because it already exists (by name), and commit creates them through `apply()`.
- Given a file from a newer format version, when imported, then it is refused `UNSUPPORTED` with "Update this Hub to import this file."

## Spec Change Log

## Review Triage Log

Unattended build: `cargo fmt --check`, `cargo clippy --locked -D warnings --all-targets` and `cargo test` for proto/store/core/api, Hub `svelte-check`, Playwright `import.spec.ts` and `settings.spec.ts`, `format:check`, `check:strings`, `check:protocol`, `check:layers`, `check:hex` green. `check:generated` passes once the new generated files are committed. (`check:tokens-used` and the Windows `arena-e2e` build fail without this story's changes, as noted for Story 5.35.)

## Design Notes

`import.setup` carries the whole file and re-reads it against the Hub inside the transaction, instead of carrying chunks of rows like the CSV imports, because the entries depend on each other (a plan on its groups, a coupon on its plan) and all of it has to apply or none. Group ids are made by the group intent, so the steps look the group up by name after it is created. A file may hold at most 1 MB / 2 000 entries per list so the intent stays small; Story 5.37 stores the same file in the cloud (≤ 1 MB) and Story 5.38 applies it after bootstrap through this intent. Product tax rates are exported as stored and re-checked against the importing branch's tax mode; a product whose rate or HSN does not fit there is skipped `invalid` rather than rewritten.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`; `pnpm gen:types`, `check:generated` (after commit), `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `import.spec.ts`, `settings.spec.ts` in `apps/hub`.
