---
title: 'Story 3.21: Time Packs, Blocks and Passes per Device Group'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_revision: 'cf3d210'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-4-bundles-priced-on-their-own.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A café can only bill postpaid by the hour (2.25). Owners cannot define the prepaid plans they sell (3.22), start sessions on (3.24, 3.27) or extend with (3.28), and nothing stops a console plan from being offered on a PC.

**Approach:** `plan.upsert` (Manager, `catalog.edit`) stores a plan in new change-log tables `plans` and `plan_groups` (migration `0033_plans.sql`). A plan is a `pack` (`minutes`, `validity_days`), a `block` (a start window `window_start_min`..`window_end_min`, minutes from local midnight; the session will end at the window end) or a `pass` (`validity_days` and a play window that may wrap midnight, e.g. 22:00–06:00). The intent field is `plan_kind` (a bare `kind` would collide with the intent tag). `group_ids` are the allowed groups, replaced as a set on every save like bundle components. Edits carry `base_rev` (`STALE`); the kind of a plan cannot change; a deactivated plan (`active = false`) stays listed. Terms are not copied anywhere yet: Story 3.22 snapshots them into `player_packs` when it sells one, so a later edit cannot change what was sold. `GET /ui/plans` lists plans with their groups. Settings → Plans is the Hub screen; a Counter's save opens `inline-pin`.

## Boundaries & Constraints

**Always:**
- Money is integer minor units; `price_minor` is 1..=`MAX_PRICE_MINOR`. Field errors name the field and nothing is written: `price_minor`, `minutes` (pack, 1..=100000), `validity_days` (pack and pass, 1..=3650), `window_start_min`/`window_end_min` (block and pass, 0..1439, start differs from end), `group_ids` (at least one, at most 50, no duplicates, each a live group), `name`, `plan_kind`, `id`.
- Fields that do not belong to a kind must be absent: a pack has no window; a block has neither `minutes` nor `validity_days`; a pass has no `minutes`.
- Groups that mix console and any other kind are refused `GROUP_KIND_MISMATCH` on `group_ids`; a combo is two plans.
- `plan.upsert` joins the Story 1.8 proptest generator and `arena-replay`. No code comments (user rule).

**Never:**
- Selling a plan, owned-plan rows, plan checks at session start (`PLAN_NOT_FOR_GROUP`), window enforcement, expiry, and a delete intent (3.22 to 3.27).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pack | 5 h, 300.00, 7 days, Basic | plan row + one group row | No error |
| Block | 18:00–23:30 | window stored, no minutes or validity | No error |
| Pass wrap | 22:00–06:00, 30 days | stored as 1320 and 360 | No error |
| Mix | PC and console group | nothing written | `GROUP_KIND_MISMATCH` |
| Bad terms | price ≤ 0, hours ≤ 0, start = end, no group | nothing written | `VALIDATION_FAILED` with field |
| Edit | `base_rev` = rev | rev + 1, groups replaced | `STALE` when behind |
| Kind change | edit a pack as a pass | nothing written | `VALIDATION_FAILED plan_kind` |
| Deactivate | `active = false` | still listed with "Not offered" | No error |
| Counter | no `catalog.edit` | PIN asked in place, then applied with ticket | `PERMISSION_DENIED` without |

</intent-contract>

## Code Map

- `crates/arena-store`: `migrations/0033_plans.sql`, `plans.rs` (`PlanRow`, queries), `branch.rs` (`Table::Plans`, `Table::PlanGroups` unversioned like `bundle_components`), `lib.rs` (`Store::plans`).
- `crates/arena-proto`: `plans.rs` (`PlanUpsert`, `PlanView`, limits), `intent.rs` (`plan.upsert`, `CatalogEdit`).
- `crates/arena-core`: `apply/plans.rs` (+ `mod`, `validate`, `guards`, `mutate`), `snapshot.rs` (`Reader::plans`).
- `crates/arena-api/src/ui_auth.rs` -- `GET /ui/plans`.
- `crates/arena-replay`, `crates/arena-core/tests/proptest_apply.rs` -- fixtures.
- `apps/hub/src/routes/settings/plans/+page.svelte`, `src/lib/plans.ts`, `routes/settings/+layout.svelte`, `packages/ui/src/messages/en.json`, `tests/plans.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration, change-log tables and store queries
- [x] Protocol types, intent kind, generated types
- [x] `plan.upsert` apply logic, `GET /ui/plans`
- [x] Rust tests (matrix, permissions, API), proptest op, replay fixture
- [x] Settings → Plans page, nav link, strings, inline PIN
- [x] Playwright spec

**Acceptance Criteria:**
- Given Settings → Plans, when a Manager creates a time pack, then name, minutes, price, validity days and groups are stored in `plans` and `plan_groups`.
- Given a block, then it stores price and a start window, and the session will end at the window end; given a pass, then it stores price, validity days and a window that may wrap midnight.
- Given groups mixing console and PC kinds, then the save is refused `GROUP_KIND_MISMATCH`.
- Given zero or negative price or hours, a window with start = end, or no group, then a field error shows and nothing is written.
- Given an edit, then it carries `base_rev` (`STALE` on conflict); a deactivated plan stays listed and is no longer offered.
- Given a Counter, when they save a plan, then `inline-pin` opens for manager elevation.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test` in `apps/hub`.

## Auto Run Result

Status: review
Verified: all commands above pass (`arena-kiosk-svc` test targets fail to compile on unused imports unrelated to this story, as before).
Left for later: selling plans and owned copies (3.22), plan checks and windows at session start (3.24, 3.27), Owner-app plan management (8.23), CSV plan import (5.34).
