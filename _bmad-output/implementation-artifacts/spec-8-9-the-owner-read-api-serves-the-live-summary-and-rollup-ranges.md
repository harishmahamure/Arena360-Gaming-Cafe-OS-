---
title: 'Story 8.9: The owner read API serves the live summary and rollup ranges from files'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: 'd32312f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub uploads a branch summary and a daily rollup, but the Owner app has no way to read them: the cloud holds no queryable copy of branch rows (AD-35), so the dashboard needs an API that reads the files, quickly, and only for the caller's own branches.

**Approach:** Two session-authenticated routes in `arena-cloud`: `GET /v1/branches/{id}/live` reads `state/latest.json` and adds `branch_online` and `last_heartbeat_at`; `GET /v1/branches/{id}/summary?from=&to=&group_by=` loads the `rollups/` files of the range, sums them in memory and lists `missing_days`. `all` replaces the id for every paid branch. Both read through an in-memory cache (30 s, then re-checked against the bucket's ETag).

## Boundaries & Constraints

**Always:**
- The R2 key is built from the session's tenant and the branch row found under it (`branch_prefix`), never from request input; a branch outside the owner's café, or one that does not exist, is `NOT_FOUND` (404) and nothing is fetched for it.
- A Free branch is `FEATURE_NOT_ENTITLED`; `all` lists paid branches only and is `FEATURE_NOT_ENTITLED` when there are none.
- Per-tenant limit of `OWNER_API_LIMIT_PER_MIN` (120) calls a minute through the control DB `rate_limits` table; over it, `RATE_LIMITED` with `Retry-After`. It is counted before the branch lookup, so probing ids is limited too.
- `live` serves from memory for `OWNER_READ_CACHE_MS` (30 s); after that a `HEAD` compares the ETag and a changed file is downloaded once. A branch with no summary answers `as_of: null`.
- `summary` caches per branch and month: one ETag-carrying listing per month per refresh; only files whose ETag changed are downloaded. A day with no file, a file at a schema other than N or N−1, or one in another currency than the branch's is in `missing_days` and not in the sums; every file opened goes through `RollupMetrics::record_file`.
- Money stays integer minor units; branches in different currencies are never summed (`all` carries one total per currency).

**Never:**
- A query of branch rows, a copy of them in the cloud, an owner write or command (8.18+), team roles (14.28), or a change to the Hub.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Live | paid branch, summary file, heartbeat 10 s ago | 200, summary, `as_of`, `branch_online: true`, `last_heartbeat_at` | none |
| Offline | last heartbeat over 3 min ago | `branch_online: false`, `last_heartbeat_at` kept | none |
| No summary | no `state/latest.json` | `as_of: null`, `summary: null` | none |
| Cached | second call within 30 s | no bucket call | none |
| Revalidated | after 30 s, file unchanged / changed | `HEAD` only / `HEAD` + one download | none |
| Range | `from`, `to`, `group_by=day\|week\|month` | totals, one row per period (zero rows for empty ones), `missing_days` | 422 on a bad date, order, `group_by`, or over 400 days |
| All | `branch=all` | one entry per paid branch, one total per currency | none |
| Stranger | another café's branch id, junk or `..` | 404 `NOT_FOUND`, nothing fetched | none |
| Free | Free branch | 403 `FEATURE_NOT_ENTITLED` | none |
| Flood | 121st call in a minute | 429 `RATE_LIMITED`, `Retry-After` | none |
| No bucket | no data bucket configured | 503 | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/owner_read.rs` -- constants, response types (`OwnerLive`, `OwnerLiveAll`, `OwnerRange`, `OwnerRangeAll`, `OwnerRangeTotals`…), `GroupBy`, the day arithmetic (`days_from_date`, `date_from_days`, `dates_between`); generated TS in `packages/ui/src/generated`.
- `crates/arena-cloud/src/store.rs` -- `S3Store::{head, read, list_etags}` and the `MemoryStore` counterparts with call counts (`StoreCalls`); `src/presign.rs` -- `Presigner::{head, read, list_etags}` over the data bucket.
- `crates/arena-cloud/src/owner_read.rs` -- `ReadCache` (`live`, `rollups`, month entries with per-day ETags); held in `AuthState::read_cache`.
- `crates/arena-cloud/src/api/owner_read.rs` -- the two routes, tenant rate limit, entitlement, range validation and summing; mounted in `api/mod.rs`.
- `crates/arena-cloud/tests/owner_read.rs` -- the matrix above against an in-memory bucket.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- response types, constants, date helpers, generated TS.
- [x] `crates/arena-cloud` -- ETag reads on the data bucket (S3 and memory), `ReadCache`, routes, rate limit, entitlement.
- [x] Tests -- proto date/period tests; cloud: live fields and online window, null `as_of`, 30 s cache then ETag, ranges per day/week/month with missing and old-schema days, regenerated day, other-currency day, month cache, `all` with two currencies and a Free branch, stranger 404 with no fetch, Free, rate limit, validation and 401, no bucket.

**Acceptance Criteria:**
- Given a signed-in owner and one of their paid branches, when the app calls `GET /v1/branches/{id}/live`, then the cloud reads `state/latest.json`, returns it with `as_of`, `branch_online` and `last_heartbeat_at`, and serves repeats within 30 s from memory checked against the ETag.
- Given `GET /v1/branches/{id}/summary?from=&to=&group_by=`, then it loads the `rollups/` files of the range (month cache with ETag), sums them in memory and returns totals plus per-period rows, with `missing_days` for any day without an accepted rollup.
- Given `branch=all`, then one entry per permitted branch plus one total per currency, never summed across currencies.
- Given a branch id not in the session's tenant, then `NOT_FOUND`, and keys come from the session only.
- Given a Free branch, then `FEATURE_NOT_ENTITLED`.
- Given a tenant above its limit, then `RATE_LIMITED` with a retry-after.
- Given a branch with no summary file, then `live` returns `as_of: null`.

## Spec Change Log

## Review Triage Log

Code review (4 layers) fixed: range expansion built every date before the 400-day cap (`dates_between` now stops at cap+1); sums saturate instead of overflowing and `add_rollup` delegates to `add` (one field list); `live` takes the branch row's currency and drops a summary of another currency or branch (as rollup days already were); a rollup day whose `branch_id`/`business_day` do not match its key counts as missing; an empty ETag never counts as unchanged. Tests added for every summed field, saturation and the cap.
Deferred: MinIO-backed tests of `S3Store::{head,read,list_etags}`; single-flight on cache refresh and per-key eviction; one failed day read failing the month; fan-out cost of `all` ranges against the rate limit; corrupt `latest.json` reading as "no summary"; 422 range errors answered before the rate counter (no data touched).

## Design Notes

`branch=all` is the path segment `all` (`/v1/branches/all/live`), as branch ids are ULIDs and cannot collide. `all` skips Free branches rather than failing the whole call; a single Free branch is `FEATURE_NOT_ENTITLED`. `as_of` is the Hub's own build time; `branch_online` and `last_heartbeat_at` come from the active hub row in the control DB. The Owner API limit is a rolling 60 s window per tenant (`Retry-After` can read 61, as the presign limiter's does). Files at schema N−1 parse as `RollupV1`, so they stay readable until a later schema adds a version-specific reader. The cache is process-local and emptied wholesale at 4096 entries; it holds Hub files only. `StoreCalls` exists so tests can prove "from memory" and "only changed files".

## Verification

**Commands:**
- `cargo test --no-fail-fast -p arena-proto -p arena-cloud`; `cargo clippy -p arena-proto -p arena-cloud --all-targets -- -D warnings`; `cargo fmt --check`; `pnpm run gen:types` then `check:generated` (clean once the new generated files are committed); `check:protocol`; `check:strings`; `check:layers`; `check:no-manifest`; `format:check`.

## Auto Run Result

Status: in-review (implemented and verified; the review pass is the next iteration's).

- Summary: `GET /v1/branches/{id}/live` and `/summary` (and `all`) read the Hub's summary and rollup files from the data bucket through a 30 s / ETag cache, for the signed-in owner's own paid branches only, with a per-tenant rate limit.
- Files: `crates/arena-proto/src/{owner_read,lib}.rs`; `crates/arena-cloud/src/{owner_read,store,presign,auth,lib}.rs`, `src/api/{owner_read,mod}.rs`, `tests/owner_read.rs`; `packages/ui/src/generated/*` (nine new types and the barrel).
- Verification: fmt and clippy `-D warnings` clean; arena-proto and arena-cloud suites green except two failures that predate the story and sit in untouched files (`tests/backup.rs` `a_hung_run_times_out_with_its_own_code`, `tests/binary.rs` `unreachable_at_start_retries_then_exits_1_without_logging_the_token`); `check:protocol`, `check:strings`, `check:layers`, `check:no-manifest`, `format:check` ok; `check:generated` lists only the new, uncommitted generated files.
- Decisions: `all` is a path segment and lists paid branches only; rollup days in another currency than the branch row count as missing; the rate limit is counted before the branch lookup; cache keys and bucket keys use only the session's tenant and the found branch row.
- Residual risks: the S3 `head`/`read`/`list_etags` paths are exercised only through the in-memory store here (no MinIO run); the 30 s window is per process, so several cloud instances each keep their own copy; `NFR7` latency is not measured on production-like data.
