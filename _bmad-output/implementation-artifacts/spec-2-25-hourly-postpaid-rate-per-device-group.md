---
title: 'Story 2.25: Hourly postpaid rate per device group'
type: 'feature'
created: '2026-10-26'
status: 'in-review'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-20-device-groups-with-six-swatches-held-at-the-hub.md'
---

<intent-contract>

## Intent

**Problem:** Device groups have no price, so a session start has no rate to read and setup status cannot say which groups still need one.

**Approach:** A Hub `rates` table with one active `price_per_hour_minor` row per group, written by a `rate.upsert` intent (permission `device.manage`, Counter elevates through `inline-pin`) and edited per group in Settings → Layout. The Hub counts groups without an active rate into `SetupStatus`.

## Boundaries & Constraints

**Always:**
- Hub migration `0023_rates.sql`: `rates(rate_id, group_id FK, price_per_hour_minor > 0, active, rev, created_at, updated_at)` with a partial unique index on `group_id` where `active = 1`. `rates` joins the change log (`Table::Rates`).
- `rate.upsert {group_id, price_per_hour_minor}`: the group must be live; the price is an integer from 1 to 999,999,999,999 (else `VALIDATION_FAILED` field `price_per_hour_minor`, nothing written); a change deactivates the old row (kept for history) and inserts the new active row in the same transaction.
- Snapshot groups carry `rate_minor` (the active rate or null); `SetupStatus.groups_without_rate` counts live groups without one. A `rates` change also fires `groups.changed`.
- Hub UI: each group row in Settings → Layout shows its rate in tabular numerals with the branch symbol, or "No rate yet", and a field to set it. Counter edits open `inline-pin`; the audit already records both actors.
- Every string lives in `en.json`.

**Never:**
- Session pricing or billing maths (Epic 3), pack or dynamic rates, rate deletion, hard deletes of rate history.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First rate | group with none, 12000 | one active row | N/A |
| Change | active 12000, new 15000 | old row inactive, new row the only active one | N/A |
| Zero / negative / non-numeric | 0, -5, "abc" | nothing written | `VALIDATION_FAILED` field `price_per_hour_minor` |
| Unknown or deleted group | bad id | nothing written | `VALIDATION_FAILED` field `group_id` |
| Counter | no ticket | refused | `PERMISSION_DENIED`; UI opens `inline-pin` |
| Setup status | 2 groups, 1 rated | `groups_without_rate: 1` | N/A |

</intent-contract>

## Code Map

- `crates/arena-store/src/branch.rs` -- `Table`/`RowKey` change-log tables; add `Rates`.
- `crates/arena-store/src/devices.rs`, `lib.rs`, `migrations.rs` -- group rows and store accessors; add rate queries.
- `crates/arena-proto/src/intent.rs`, `lib.rs`, `snapshot.rs`, `operating.rs` -- `IntentKind`/`IntentPayload`, `SnapshotGroup`, `SetupStatus`.
- `crates/arena-core/src/apply/{mod,validate,mutate,guards,groups}.rs` -- the apply pipeline; `maintenance.rs` is the template for a new target.
- `crates/arena-core/src/{snapshot,setup}.rs` -- snapshot and setup status builders.
- `crates/arena-api/src/ui_events.rs` -- `groups.changed` trigger tables.
- `apps/hub/src/routes/settings/layout/+page.svelte`, `apps/hub/src/lib/products.ts` (`parsePrice`, `priceText`) -- UI and money parsing.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store/**` -- migration 0023, `Table::Rates`, rate queries.
- [x] `crates/arena-proto/**` (+ generated TS) -- `rate.upsert`, `rate_minor`, `groups_without_rate`.
- [x] `crates/arena-core/**`, `crates/arena-api/src/ui_events.rs` -- apply handler, snapshot, setup status, tests.
- [x] `apps/hub/**`, `packages/ui/src/messages/en.json` -- rate field per group, Playwright test.

**Acceptance Criteria:**
- Given a Manager on Settings → Layout, when they set 12000 on a group and later 15000, then only the 15000 row is active, the 12000 row is kept inactive, and the group shows the new rate.
- Given a group with no rate, when setup status is computed, then it is counted in `groups_without_rate` and the UI shows "No rate yet".

## Spec Change Log

## Review Triage Log

Not reviewed yet. Decisions: no `base_rev` on `rate.upsert` (the new row replaces the active one, last write wins, so there is no STALE path); an identical price still writes a new history row (the Hub UI is the only caller and does not special-case it). Verify gate: `cargo test --workspace` (all pass), `arena-api --features dev-fixtures` pass, clippy `-D warnings` and fmt clean, `pnpm --filter @arena/hub check` 0 errors, Hub Playwright 539 passed / 11 skipped, every `pnpm run check:*` and `test:scripts` pass (`check:generated` is git-status based and passes once the regenerated types are committed). Known unrelated failure: `arena-hub --features dev-fixtures --test dev_seed` (2 tests, `USER_LIMIT` staff limit) fails identically on a clean tree.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check`.
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell`.
