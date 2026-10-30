---
title: "Story 8.37: The builder serves audit queries from a local cache"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '0ec525af'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
deferred: []
---

<intent-contract>

## Intent

**Problem:** A built audit month sits in R2 as a compressed SQLite file nobody can read: an owner or a support agent cannot ask what happened in it, and support has no recorded, reasoned way in.

**Approach:** `GET /v1/branches/{id}/audit?month=&q=` answers only the questions of a fixed `arena-proto` query catalogue over a locally unpacked copy of the built month (50 GiB least-recently-used cache), and admin-scoped endpoints let a staff credential build and search with a recorded reason, each call written to `admin_audit` first.

## Boundaries & Constraints

**Always:**
- A search names one catalogue entry (`q` = `recent`, `actor`, `module`, `intent`, `entity`, `time`) and only the parameters that entry takes; an unknown `q` or an extra parameter is `UNSUPPORTED`, a malformed or missing value `VALIDATION_FAILED`. Pages are 50 rows, newest first, keyset-paged by an opaque `cursor`.
- A `ready`, fresh month is searched; `queued`/`building` answers `AUDIT_BUILDING` with the progress, `failed` answers `AUDIT_BUILD_FAILED`, a month never built is `NOT_FOUND`. A month past 24 hours, or whose R2 object is gone, is queued again (`AUDIT_BUILDING`).
- Results are change metadata only (who, intent, table, row id, when): no row image, no ciphertext, players by id. Changes a fork point kept out of state come back in `orphaned`, on the first page, under the same filters.
- The admin endpoints need a staff bearer credential and a non-empty `reason`; each call writes one `admin_audit` row (`actor_kind = admin`, staff id, branch, month, query, reason) before any data is returned.

**Never:** arbitrary SQL or free-text shapes, row images in a result, a standing copy beyond the local cache, the admin app UI (Epic 11).

## Spec Change Log

- The epic has "the builder" serve the query. The builder has no Service and no owner session, so the control role, which authenticates the caller and already holds the data bucket, serves it and keeps the local cache (`ARENA_AUDIT_CACHE_DIR`, an emptyDir on the control Deployment). `--role builder` still only builds.
- The epic puts the admin endpoints behind Cloudflare Access. Staff SSO and the admin app are Epic 11, so the endpoints take a staff bearer token whose SHA-256 is listed in `ARENA_ADMIN_TOKENS`; with none configured they answer 404.

## Design Notes

The local copy is named `<month>-<built_at>.db`, so a rebuilt month never reads an older file; use refreshes the file's modified time and eviction removes the oldest until the total fits (the month in use is never evicted). The catalogue and its parameter lists live in `arena_proto::AuditSearch`; the SQL is `arena_store::list_audit` (extended with intent, table and row filters) and `list_orphaned`.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-replay -p arena-cloud` -- expected: pass except known unrelated failures
- `cargo clippy -p arena-proto -p arena-store -p arena-replay -p arena-cloud --all-targets -- -D warnings` and `cargo fmt --check` -- expected: clean
- `pnpm check:generated && pnpm check:strings && pnpm check:pins` -- expected: ok once the generated files are committed

## Auto Run Result

Status: done
Implemented: `arena-proto` query catalogue (`AuditSearch`, `AuditQueryKind`, `AuditSearchPage`); `arena-store` `AuditFilter` intent/table/row filters and `list_orphaned`; cloud `audit_query` (local LRU cache, search), `GET /v1/branches/{id}/audit`, `admin` credentials and `/v1/admin/branches/{id}/audit[/build]` with `admin_audit` rows, `request_rebuild`; control Deployment audit-cache volume and optional data-bucket/admin env. Tests: proto catalogue unit tests, store filter test, admin token unit tests and 7 new cases in `crates/arena-cloud/tests/audit_build.rs` (catalogue queries and paging, unsupported shapes, build states, orphans, gone object, LRU eviction and refetch, support path and audit rows). Known unrelated failures: arena-replay `replaying_the_seeded_log_reproduces_branch_and_settings`, arena-cloud `unreachable_at_start_retries_then_exits_1_without_logging_the_token` (and `a_hung_run_times_out_with_its_own_code` on a slow run). `check:deploy` could not run (no kubectl).

## Review Result

Code review passed. One blocking finding fixed: `the_support_path_needs_a_staff_credential_and_a_reason_and_is_audited` was flaky because the test clock is fixed and `admin_audit_rows` orders by `(at, id)` with random ULIDs; the test now advances the clock between recorded calls. fmt, clippy (-D warnings), arena-proto/store/cloud tests and `pnpm check:generated/strings/pins` are green. Noted, not blocking: an in-flight search can lose its cache file to eviction or a rebuild of the same month (it answers a 500 and the next call refetches).
