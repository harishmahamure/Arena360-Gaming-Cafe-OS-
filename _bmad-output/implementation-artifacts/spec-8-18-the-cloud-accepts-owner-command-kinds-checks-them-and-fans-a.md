---
title: 'Story 8.18: The cloud accepts owner command kinds, checks them, and fans "all branches" out'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '3eafeba'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-15-owner-changes-reach-the-hub-through-the-command-mailbox-with.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The command mailbox (Story 2.15) takes only the seven setup kinds and no owner can post to it: nothing checks a change against a catalogue, a branch's tier or its Hub's version, and "all branches" has no meaning.

**Approach:** `arena-proto` gains an owner command catalogue (`rate.upsert`, `plan.upsert`, `rule.upsert`, `coupon.upsert`, `product.upsert`, `backup.now`; `hub.revoke` and `backup.restore` reserved as sensitive with no handler). `POST /v1/commands` validates a post for a signed-in owner, checks the branch(es) are the café's, entitled and on a supported command schema, and queues one `commands` row per branch; `GET /v1/commands` (owner session) lists the café's commands newest first. The Story 2.15 setup kinds and the Hub's mailbox are unchanged.

## Boundaries & Constraints

**Always:**
- Catalogue entry = `kind`, `schema_version`, `min_tier`, optional licence `flag`, `sensitive`, `handled`. Payload schema = the matching intent payload in `arena-proto` (`RateUpsert`, `PlanUpsert`, `RuleUpsert`, `CouponUpsert`, `ProductUpsert`; `backup.now` takes no fields). `base_rev` rides beside the fields.
- A kind outside the catalogue, or reserved without a handler, is `UNSUPPORTED`; so is every setup kind through this path.
- Single branch: not the café's is `NOT_FOUND`; a Free branch is `FEATURE_NOT_ENTITLED`; a Hub that reported a command schema below the kind's is the new `BRANCH_UPDATE_REQUIRED` (409), for that branch only.
- `all`: one row per branch that passes, each with `group_id` = the owner's key and a ULID key derived from key + branch; refused branches come back in `skipped` with code and message. With no row created the first skip's error is returned.
- Same `(branch_id, key)` twice returns the existing row and inserts nothing.
- Per-tenant limit of 60 posts a minute, `RATE_LIMITED` with `Retry-After`.
- `GET /v1/commands?status=&branch=&group=&before=&limit=` is the owner list (tenant-scoped via the branches join, `next_before` pages); a request carrying `Authorization` or an `epoch` query is still the Hub's fetch.
- The heartbeat carries optional `command_schema`; a Hub that never sent one counts as 1.

**Never:** Hub handlers for the new kinds (Story 8.19), the sweeper/expiry/fencing (8.20), step-up MFA (8.25), the owner UI (8.21+).

## Code Map

- `crates/arena-proto/src/owner_commands.rs` (new), `error.rs` (`BranchUpdateRequired`), `pairing.rs` (`HeartbeatRequest.command_schema`), `lib.rs`; generated TS.
- `crates/arena-store`: `control-migrations/0019_owner_commands.sql` (`commands.group_id`, `hubs.command_schema_max`), `control/owner_commands.rs`, `control/commands.rs` (`NewCommand.group_id`).
- `crates/arena-cloud`: `api/commands.rs` (post, list, shared GET), `api/heartbeat.rs`, `api/error.rs`, `api/wizard_staff.rs`; `crates/arena-api`, `arena-kiosk-core` (exhaustive `ErrorCode` matches); `crates/arena-tasks/src/heartbeat.rs` (Hub sends `command_schema`).
- `crates/arena-cloud/tests/owner_commands.rs` (new).

## Tasks & Acceptance

- [x] Catalogue, payload validation and wire types in `arena-proto`; `BRANCH_UPDATE_REQUIRED`.
- [x] Migration, store queries (owner list, Hub command schema), `group_id` on insert.
- [x] `POST /v1/commands`, owner `GET /v1/commands`, heartbeat `command_schema`.
- [x] Tests: catalogue and validation (proto), 13 cloud tests (idempotency, UNSUPPORTED, validation, entitlement, tenant, fan-out, schema range, rate limit, list, Hub path unchanged).

**Acceptance Criteria:**
- Given the catalogue, then the six owner kinds are accepted and `hub.revoke`/`backup.restore` are `UNSUPPORTED`, as is anything else.
- Given a post for one branch, then a `pending` row is inserted with `issued_by`; the same key again returns it.
- Given `all`, then one row per entitled branch shares `group_id`, others are listed as skipped.
- Given a Hub below the kind's schema, then that branch is refused `BRANCH_UPDATE_REQUIRED` only.
- Given a tenant over the limit, then `RATE_LIMITED`.
- Given the owner list, then rows come newest first with state, error, `queued_at` and `done_at`.

## Verification

- `cargo fmt --check`; `cargo clippy --locked --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`; `cargo test` for `arena-proto`, `-store`, `-cloud`, `-tasks`, `-api`, `-kiosk-core`, `-hub`; `pnpm run check:protocol|strings|layers|secrets`; `gen:types`.
- Pre-existing, unrelated failures: `arena-e2e` and `arena-kiosk-svc` do not build on Windows; `arena-cloud` tests `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start_*` fail without this change; `check:deploy`/`test:scripts` deploy tests need `kubectl`; `check:generated` lists the new generated files until they are committed.

## Design Notes

- `rate.upsert` etc. validate by deserialising the stored intent payload, so a payload that passes here is one the Hub's `IntentPayload` can parse once Story 8.19 adds the handlers.
- Until 8.19, a Hub that fetches an owner-kind command records it `rejected` `UNSUPPORTED` (its handled list is the setup kinds).
- Per-branch keys must be ULIDs (the Hub requires it), so they are the first 125 bits of SHA-256(key, branch) as a ULID.
- The mailbox shares one path: Hub GET is told by `Authorization` or `epoch`; the owner list needs a session.
- `min_tier`/`flag` are enforced from the entitlement tier and the licence claims' flags; all six kinds need Basic and no flag.

Known limits: a repeated key with a different payload returns the first row (as in Story 2.15); listing is not rate limited.
</intent-contract>
