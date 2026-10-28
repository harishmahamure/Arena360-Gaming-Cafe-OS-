---
title: 'Story 5.34: Import players and plans by CSV'
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
baseline_commit: 'd3e2cb7'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-33-csv-import-with-dry-run-starting-with-products.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner moving from another system has to type every regular and every time plan into the Hub by hand, and the regulars lose the money they had on account.

**Approach:** Two more files go through the Story 5.33 shape: `POST /ui/import/players/{preview,commit,template}` and `POST /ui/import/plans/{preview,commit,template}`. A dry-run reads the file, checks every row and answers counts with each failure's row and reason, writing nothing. A commit applies the rows that passed as `import.players` / `import.plans` intents of 500 rows, each chunk with an idempotency key derived from the client's `import_id`. Settings → Import gains Players and Plans tabs beside Products.

## Boundaries & Constraints

**Always:**

- Players file: columns `phone, name, balance, credit_limit` (UTF-8, header row, BOM, CRLF/LF, quoted cells; `phone` and `name` required, any order, names case-insensitive). `phone` starts with `+` and has 8 to 15 digits (spaces and dashes are dropped); `name` is 1 to 80 characters; `balance` and `credit_limit` are major units with at most the currency's decimals, 0 or more, empty is 0.
- A row whose phone already has a profile is **matched**: left as it is, counted, never a failure (so a retry of a half-finished import is safe). A phone repeated in the file fails the later row (`duplicate_phone`). Unknown columns block "Import" (not "Import valid rows only"); a missing `phone`/`name` column, a repeated column, an empty or unreadable file make the file unusable.
- `import.players` (`player.manage`; also `wallet.topup` when any row brings a balance and `credit.manage` when any row sets a limit, on the actor or the elevating manager) carries 1 to 500 sealed rows and applies all or none. Each new player gets the `player.create` rows (profile with name and phone as ciphertext under a fresh key wrapped by the branch key, consent `transactional` recorded as **not granted**, key row, wallet), a generated username (the name's letters and digits joined by dots, at most 20, plus the phone's last four digits; `-1`, `-2` when taken), **no password** (`password_hash` is `!`, which never verifies), a `wallet_txns` row of reason `import` for a balance above 0 (no order, no cash event, no invoice, `order_id` NULL) and a credit account (limit, reminder days 3, 7, 14) for a limit above 0.
- Plans file: columns `name, type, hours, price, validity_days, window_start, window_end, groups`; `name`, `type` (`pack`, `block`, `pass`) and `price` required. `hours` is decimal hours that must come to whole minutes; `window_start`/`window_end` are `HH:MM`; `groups` are device-group names separated by `;` or `|`, matched exactly, else case-insensitively when only one group fits.
- One check serves the preview and the intent (`import_plans::check_row`): the `plan.upsert` rules (`apply::plans::check_import_row` — terms per kind, price above 0, at least one group, console and PC groups never mixed, the kind of an existing plan never changes). A row updates the plan with exactly its name (keeping whether it is on sale), else it is new; a name repeated in the file fails the later row. Unknown group names fail the row (`unknown_group`).
- `import.plans` (`catalog.edit`) carries 1 to 500 rows, validates all of them and writes each through `plans::mutate` (so `plan_groups` rows are added and removed as `plan.upsert` does).
- Chunk `n` of an import uses idempotency key = the `import_id` ULID with its last 16 bits set to `n` (intent id with bit 15 also set). Sealing is random, so the API keeps the sealed chunks of the last four player imports by `import_id` and file: the same attempt again sends the very same chunks and replays stored outcomes.
- Limits: 1.5 MB of CSV, 10,000 rows, 200 listed failures (counts stay complete). Both templates are the exact header and two sample rows and pass their own dry-run (the plans template names the groups `PC` and `Vip`).

**Never:**

- Writing anything in a preview, a network call, a partial chunk, or a raw `import.players` through `POST /ui/intent` (it carries sealed data only the Hub's own import path may make).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Clean players | 3 rows, 1 phone known | `new 2, matched 1, failed 0`; Import open | none |
| Bad phone | `12345` on line 2 | failure `{row: 2, field: phone, reason: bad_phone}`; Import closed | none |
| Repeated phone | line 5 repeats line 2 (written differently) | failure on 5 `duplicate_phone` | none |
| Bad balance | `-5`, `abc`, `1.234` | `out_of_range`, `bad_number`, `bad_number` on `balance` | none |
| Valid rows only | 1,001 valid + 2 bad rows | 3 chunks (500, 500, 1); created 1001, skipped 2 | none |
| Retry | same `import_id`, same file | stored outcomes replayed; nobody added twice | none |
| Every phone known | file already imported | `new 0, matched N`; commit refused | 422 on `csv` |
| Balance without `wallet.topup` | role with only `player.manage` | intent `PERMISSION_DENIED`, nothing written | none |
| Plan, unknown group | `groups` = `Nowhere` | failure `{field: groups, reason: unknown_group}` | none |
| Plan, console + PC | `PC;PS5` | failure `group_kind` | none |
| Plan, same name | row names an existing pack | counted as updated; `active` kept | none |
| Plan, other kind | existing pack named in a `pass` row | failure `kind_change` | none |
| No key store | Hub without a branch key | players preview answers `UNSUPPORTED` | inline message |
| Unwritable template folder | `/no/such/folder` | 422 `folder` | screen offers another |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0061_import_wallet.sql` -- `wallet_txns.order_id` nullable and reason `import`; `wallet.rs` `insert_import_wallet_txn`, `players.rs` `player_import_keys` (+ `Store::player_import_keys`).
- `crates/arena-proto/src/import.rs` -- `ImportPlayerRow`/`ImportPlayers`, `ImportPlanRow`/`ImportPlans`, requests, previews and results, column lists (+ generated TS); `intent.rs` -- `import.players` (Player category, `player.manage`) and `import.plans` (Catalog category, `catalog.edit`).
- `crates/arena-core/src/import_file.rs` -- header and problem list shared by both files; `import_players.rs` -- template, `analyse`, `username_for`, `seal_rows`; `import_plans.rs` -- template, `check_row`, `duplicates`, `analyse`; `players.rs` `seal_import`; `snapshot.rs` `player_import`, `plan_import`.
- `crates/arena-core/src/apply/import_players.rs`, `import_plans.rs` (+ `plans.rs` `check_import_row`/`upsert_from_import`/`import_target`, `guards.rs` `importing_players`) -- the intents.
- `crates/arena-api/src/ui_import.rs` + routes in `ui_auth.rs` -- previews, commits (sealed-chunk cache), templates; `ui_auth.rs` also refuses a raw `import.players` on `/ui/intent`.
- Hub: `lib/ImportPanel.svelte` (the Story 5.33 screen made generic), `lib/import.ts`, `routes/settings/import/+page.svelte` (tabs), `en.json`.
- Tests: core (CSV rules, players and plans apply tests, 1,000-row timings), proptest `Op::ImportPlayers`/`Op::ImportPlans`, API (`a_player_csv_is_previewed_then_committed_in_chunks_with_a_safe_retry`, `a_plan_csv_is_previewed_then_committed_in_chunks_with_a_safe_retry`, no key store), Playwright `import.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-store` -- migration 0061, `import` ledger row, player keys read
- [x] `arena-proto` -- payloads, requests, answers, `import.players` / `import.plans` kinds, generated TS
- [x] `arena-core` -- shared file reader, player and plan rules shared by preview and intent, dry-runs, templates, sealing
- [x] `arena-core` apply -- `import.players`, `import.plans`, permission guard, proptest ops
- [x] `arena-api` -- previews, commits in 500-row chunks with derived keys, templates
- [x] Hub -- Players and Plans tabs, strings
- [x] Tests -- core, apply, proptest, API, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Settings → Import → Players with columns phone, name, balance, credit_limit, when the dry-run runs, then each row is checked for a valid phone, duplicates within the file and against existing players (by phone), non-negative balance and limit, and results show as in Story 5.33.
- Given commit, when it applies, then each player is created with encrypted personal data and a generated username, the balance is added as a `wallet_txns` row with reason `import` (no cash event, no invoice), credit is enabled with the limit if above zero, and consent is recorded as not granted.
- Given Settings → Import → Plans with columns name, type, hours, price, validity_days, window_start, window_end, groups, when the dry-run runs, then each plan is checked with the Story 3.21 rules and its group names are matched to existing groups; an unknown group or a mix of console and PC groups fails that row.
- Given a plans dry-run with no blocking failures (or "Import valid rows only" chosen), when the Manager commits, then one `import.plans` intent per 500 rows creates or updates plans (matched by exact name) and their `plan_groups` rows through the `plan.upsert` rules, each chunk with its own idempotency key so a retry never duplicates, and a summary shows created, updated and skipped counts.
- Given a 1,000-row players file or a 1,000-row plans file, when dry-run plus commit runs, then each completes in under 30 s (the core tests do it in well under 2 s in a debug build).

## Spec Change Log

## Review Triage Log

Unattended build: fmt, clippy (-D warnings), cargo tests for proto/store/core/api, proptest (400 cases), `pnpm` format/strings/protocol/layers/hex checks, Hub `svelte-check` and Playwright `import.spec.ts` green. `check:generated` compares against git, so it passes once the new generated files are committed.

## Design Notes

A phone that already has a profile is *matched*, not failed: re-running a file is then safe whatever happened to the first attempt, and Story 13.24's importers can keep the same words (new, matched, skipped, failed). The intent carries sealed rows (the branch key never reaches the log), so the dry-run and the commit both read the file with the key; only the commit seals. Wallet balances need a ledger row with no order, hence migration 0061 (the `orders` table has no kind for it and a fake order would leak into reports). Imported players have no password: `!` is not a PHC string, so `verify_secret` is always false until a manager resets it (Story 3.8). If the Hub restarts between two attempts of one player import, the sealed chunks are gone; the new attempt starts from the Hub's own state (finished chunks show as matched) and uses the same chunk keys, which can answer `IDEMPOTENCY_CONFLICT` for a chunk number the first attempt used — the screen asks for a new import then.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`; `pnpm gen:types`, `check:generated` (after commit), `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `import.spec.ts`, `settings.spec.ts`, `shell.spec.ts` in `apps/hub`.
