---
title: 'Story 8.19: The Hub applies owner commands exactly once, and the owner''s change wins'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '389fb68539f7c69d407de94b59293e0b858858aa'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-18-the-cloud-accepts-owner-command-kinds-checks-them-and-fans-a.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-15-owner-changes-reach-the-hub-through-the-command-mailbox-with.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The cloud now queues `rate.upsert`, `plan.upsert`, `rule.upsert`, `coupon.upsert`, `product.upsert` and `backup.now` (Story 8.18), but the Hub's handled list is the seven setup kinds, so every one of them is recorded `rejected` `UNSUPPORTED`. Nothing makes an owner edit beat a staff edit of the same row, and staff are never told.

**Approach:** Extend the Hub's command path (`arena-core` `command_intent`/`owner_wins`, `Writer::apply_command`, `CommandTask`) so the five upsert kinds run as `actor{owner}` intents that always apply over a newer row `rev`, record the overwritten change, and raise an "updated by owner" event the Hub UI shows as a toast; `backup.now` runs an upload + summary cycle at once and is reported by that cycle's outcome.

## Boundaries & Constraints

**Always:**
- Handled kinds = the seven existing + the six above, at `COMMAND_SCHEMA_VERSION`. Any other kind, or a newer `schema_version`, stays `rejected` `UNSUPPORTED` with nothing changed. Keep the Hub's list and `arena_proto::OWNER_COMMAND_KINDS` (`handled: true`) in agreement (a test asserts it).
- The payload object is the matching `IntentPayload` struct with `base_rev` beside the fields (as 8.18 validates). The inner intent runs through `apply()` with `actor{kind: owner, id: issued_by}` and the command's idempotency key; the `applied_commands` row (key, `applied_seq`, epoch check) is written in the same transaction (existing `apply_command` behaviour, kept).
- Owner wins: for `plan.upsert`, `rule.upsert`, `product.upsert`, `coupon.upsert` the intent's `base_rev` is replaced by the target row's current `rev` before `apply` (extend `owner_wins`), so a stale `base_rev` never yields `STALE`. `rate.upsert` has no `rev` (it retires the active rate and inserts a new one) and always applies. A create (no existing row) is unchanged.
- Override record: when the command carried a `base_rev` lower than the row's current `rev` (never for `rate.upsert`, which has no `rev`), the same transaction stores the overwritten change — its `intent_id`, `actor_kind`, `actor_id` and `at`, taken from the newest `changes` row of that table/row before this apply — keyed by the owner change's `(epoch, seq)`; the audit keeps both changes (nothing is deleted). Only the `base_rev` field decides; no `base_rev` means no override.
- A failed inner intent (validation, missing group, permission, `STALE` from other causes) posts `rejected` with the `AppError`, changes nothing, and the Hub does not retry (existing `apply_command` error path; add tests for each new kind, e.g. a plan for a deleted group).
- Replaying the same key or command id returns the stored result and writes nothing.
- Toast: a new SSE event `updated_by_owner` `{kind, entity, row_id, overwritten?}` on the Hub's UI event stream after a committed owner command of the five kinds, with the copy keyed by kind in one table: "Price updated by owner" (`rate.upsert`), "Plan updated by owner", "Pricing rule updated by owner", "Coupon updated by owner", "Product updated by owner". `overwritten` is present only for an override and names the overwritten change (intent id, actor, time). The Hub shell shows the toast (text from the strings catalogue, `check:strings` clean) and lists the overwritten change where the toast opens its detail; later kinds add a line to the table.
- Propagation: an applied `rate.upsert`/`rule.upsert`/`plan.upsert` reaches floor tiles, the header `chip-rule` and kiosk attract through the existing `groups.changed`/`catalog.changed`/`settings.changed` and attract tick paths; an integration test shows the new rate on the floor read and the attract view within 1 minute of apply (use the manual clock; no sleeping).
- `backup.now`: no fields (extra fields → `VALIDATION_FAILED`). Applying it wakes the upload task (and the summary engine) for an immediate cycle and waits for that cycle's result: `applied` when its uploads succeeded (nothing pending counts as success), else `rejected` with the transport error code. The wait is bounded (30 s → `rejected` `UNAVAILABLE`/the existing timeout code), and a branch with no uploader (Free) rejects `FEATURE_NOT_ENTITLED`. Writes no change rows, so `applied_seq` is null.
- `proptest` generator (`crates/arena-core/tests/proptest_apply.rs`) gains owner commands (`plan`/`product`/`rule`/`coupon` upserts with stale, correct and missing `base_rev`) interleaved with staff edits of the same rows; invariant: after an owner command that followed a staff edit, the row equals the owner command's values; the change log stays gap-free and replay reproduces every table.

**Never:** the sweeper, 7-day expiry and epoch fencing of the mailbox (8.20), the owner UI and preview (8.21+), step-up MFA (8.25), new cloud endpoints, new command kinds beyond the six, any Hub-side retry of a rejected command.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Plain apply | `product.upsert` for a row at rev 3, `base_rev` 3 | Applied as owner; `applied_commands` row with `applied_seq` and epoch; `updated_by_owner` without `overwritten` | none |
| Owner wins | `plan.upsert` `base_rev` 2, plan now rev 4 (staff edited) | Applies; row has owner values; override stored and `updated_by_owner.overwritten` names the staff intent, actor and time; audit has both | none |
| Create | `coupon.upsert` with no id | Inserts; no override | none |
| Rate | `rate.upsert` for a group | Old active rate retired, new inserted; toast "Price updated by owner" | none |
| Invalid inner intent | `plan.upsert` naming a deleted group | `rejected` with the `AppError`, no change rows, not retried | `VALIDATION_FAILED` reported |
| Unknown kind / newer schema | `rule.delete`, or `schema_version` 2 | `rejected` `UNSUPPORTED`, nothing changes | `UNSUPPORTED` |
| Duplicate | same command id or key again | Stored result returned, no new change | none |
| backup.now ok | uploads succeed | `applied`, `applied_seq` null | none |
| backup.now fails | cloud unreachable in the cycle | `rejected` with the transport code | reported to the cloud |

</intent-contract>

## Code Map

- `crates/arena-core/src/commands.rs` -- `HANDLED` list, `command_intent` (kind mapping, `base_rev` split), `owner_wins`; add the six kinds and the rev lookups (`find_plan`/`find_pricing_rule`/`find_product`/coupon finders in `arena-store`).
- `crates/arena-core/src/writer.rs` (`apply_command` ~460, `reject_command`) -- same-transaction override record; `backup.now` bypasses `run()`.
- `crates/arena-core/src/apply/{plans,rules,products,coupon,rates}.rs` -- the existing owner-tolerant `base_rev` guards (plans/rules/products reject any mismatch even for the owner, hence `owner_wins`); read-only unless a bug appears.
- `crates/arena-store/src/{commands.rs,migrations.rs}` + new migration after `0076_snapshot_log.sql` -- `owner_overrides` table and its read/insert; `changes` (intent_id, actor_kind, actor_id, at) is the source of the overwritten change.
- `crates/arena-tasks/src/commands.rs` (`CommandTask::run_one`, `offline_target`) and `upload.rs` (`UploadTask::run`/`cycle`) -- `backup.now`: a shared poke + result channel into the upload loop; `crates/arena-hub/src/lib.rs` (~501-520) wires the task handles.
- `crates/arena-api/src/ui_events.rs` (~504 `groups.changed`/`owner_wrote`, `Reader::owner_wrote`) -- emit `updated_by_owner`; `packages/ui` generated types via `gen:types`; `apps/hub/src/lib/events.ts`, `apps/hub/src/routes/+layout.svelte` (~435 `onGroupsChanged`) and the strings catalogue -- toast.
- `crates/arena-proto/src/owner_commands.rs` -- `handled` flags (already true for the six); add the event wire type and toast-copy key table here or beside `HubEvent`.
- `crates/arena-core/src/apply/tests/commands.rs`, `crates/arena-tasks/src/commands.rs` tests, `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-api` tests -- extend.

## Tasks & Acceptance

**Execution:**
- `crates/arena-core/src/commands.rs` -- handle the five upsert kinds and `backup.now`; extend `owner_wins` to plan/rule/product/coupon -- AD-5 owner wins.
- `crates/arena-store` -- migration + store fns for `owner_overrides`; lookups of current `rev` and newest change per row -- AD-39.
- `crates/arena-core/src/writer.rs` -- record the override in the apply transaction -- audit keeps both changes.
- `crates/arena-tasks/src/{commands.rs,upload.rs}`, `crates/arena-hub/src/lib.rs` -- `backup.now` immediate cycle with bounded wait and result mapping.
- `crates/arena-api/src/ui_events.rs`, `crates/arena-proto`, generated TS, `apps/hub` -- `updated_by_owner` event and toast table keyed by kind.
- Tests: per-kind apply/override/invalid/duplicate/unsupported (arena-core), `backup.now` ok/fail/Free (arena-tasks), kind-list agreement, event + toast (arena-api, hub unit/e2e spec if the harness covers toasts), 1-minute propagation integration test, proptest extension.

**Acceptance Criteria:**
- Given a fetched `rate|plan|rule|coupon|product.upsert`, when the poller applies it, then the inner intent runs as `actor{owner,id}`, the key, `applied_seq` and epoch are recorded in one transaction, and the result is posted with epoch and `applied_seq`.
- Given a `base_rev` below the row's `rev`, when it applies, then it applies anyway, the event names the overwritten change (intent id, actor, time), both changes stay in the audit, and the toast reads the kind's line.
- Given an inner intent that fails validation, when applied, then `rejected` with the `AppError` code, nothing changes and no Hub retry.
- Given an applied rate/rule/plan, when it commits, then the new rate shows on the floor view and the kiosk attract view within 1 minute (integration test).
- Given `backup.now`, when it applies, then a cycle runs at once and the command is `applied` on success or `rejected` with the transport error.
- Given any other kind, then `rejected` `UNSUPPORTED` and nothing changes.
- Given the proptest generator, then the final row equals the last applied owner command where one followed the staff edit.

## Spec Change Log

## Review Triage Log

Code review (4 layers) found no blocking defects. Patched: a `backup.now` cycle with no hub token now reports `TOKEN_INVALID` (was generic `INTERNAL`); the owner-command proptest now accepts a refused product only for `HSN_REQUIRED`. Accepted as intended by the spec: `backup.now` tolerates a `base_rev` field (as 8.18 does), transient `backup.now` failures are a permanent rejection (no Hub retry). Deferred (non-blocking): task-level redelivery test for `backup.now`, summary-publish assertion, `groups.changed` `by_owner` assertion for rate/rule, toast timer pause and queueing, other-locale catalogues.

## Design Notes

- The override is decided in the Writer transaction (it has the row `rev` and the `changes` log); the UI event only reads it by the owner change's `(epoch, seq)`, so no field is added to `Applied`.
- Rates have no row version: the "override" concept does not apply to them, only the toast does.
- `backup.now` waits on the same cycle the timer would run, via a poke, so it respects tier, token and gap-resend logic; it does not build a second upload path.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy --locked --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings` -- expected: clean
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-api -p arena-hub -p arena-cloud` -- expected: pass (known unrelated failures per Story 8.18)
- `pnpm run gen:types`, `pnpm run check:protocol`, `check:strings`, `check:layers`, `check:secrets`; hub `svelte-check`/unit tests -- expected: clean

## Auto Run Result

Status: in-review. Implemented and verified (fmt, clippy `-D warnings`, tests for arena-proto/-store/-core/-api/-hub, checks). Known unrelated failures: arena-tasks `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::a_printer_that_stops_reading_times_out`; arena-cloud tests listed in Story 8.18; `ui_auth` lock/unlock test when arena-api/-hub/-cloud build together. Fixed a heartbeat test left stale by Story 8.18 (`command_schema`).
