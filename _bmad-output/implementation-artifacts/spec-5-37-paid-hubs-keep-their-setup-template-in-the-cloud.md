---
title: 'Story 5.37: Paid Hubs keep their setup template in the cloud'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
baseline_commit: '56c725c'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-36-export-and-import-a-branch-setup-file.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A café owner with a paid branch has to export the Story 5.36 setup file by hand and carry it to the next branch. Story 5.38 wants the cloud to already hold an up-to-date copy.

**Approach:** A paid, paired, primary Hub watches its catalogue tables and, within 5 min of a change, sends the 5.36 setup file (`SnapshotReader::setup_export`) to `PUT /v1/branches/{id}/setup-template` with its device token. The cloud checks the token, that the branch is the token's own, the branch's `gz_control` entitlement and the 1 MB limit, then stores the file and a small metadata object under the branch's prefix of the object store (R2). Free Hubs upload nothing.

## Boundaries & Constraints

**Always:**

- Endpoint: `PUT /v1/branches/{id}/setup-template`, `Authorization: Bearer <hub token>` (same `authenticate` as heartbeat and commands), body = the setup file text (`application/json`, at most `SETUP_MAX_BYTES` = 1 048 576 bytes). Order of checks: token (`TOKEN_INVALID` 401, `TOKEN_REVOKED` 401), the branch in the path equals the token's branch else `FORBIDDEN` 403 (another branch, same or other tenant), entitlement from `gz_control` (`Entitlement::tier_at(now)` is not Free, else `FEATURE_NOT_ENTITLED` 403), then size (`PAYLOAD_TOO_LARGE` 413, a new error code; a declared or actual body over the limit) and shape (`format` is `arena360-setup` and `version` a whole number, else `VALIDATION_FAILED` 422 on `file`). A refusal never touches the stored object.
- Storage: `branches/<branch id>/setup-template.json` holds the body byte for byte; `branches/<branch id>/setup-template.meta.json` holds `{version, sha256, bytes, uploaded_at}` and is written last, so a half-written upload is never described. The answer is `SetupTemplateStored {version, sha256, bytes, uploaded_at}` (200). A second upload replaces both; an identical body (same sha256) is accepted and only refreshes `uploaded_at`.
- The object store comes from the same `ARENA_BACKUP_S3_*` bucket variables the backup job uses. `--dev` keeps templates in memory. A cloud with neither answers 503 (`INTERNAL`, the dependency-unavailable envelope) and logs it.
- Hub task `SetupTemplateTask`: waits until paired; reacts to `Applied` changes of `device_groups`, `rates`, `products`, `bundle_components`, `plans`, `plan_groups`, `pricing_rules`, `coupons`, `coupon_targets` (a lagged receiver counts as a change); sends the first file shortly after start (the cloud may hold an older one) and then at most once per `UPLOAD_DEBOUNCE` (30 s, always inside 5 min) after the last change. It only runs on the primary Hub with a writer, a trusted paid tier (`HubStatus.tier` is `Some` and not Free) and a pairing that is not revoked. On Free it uploads nothing and keeps the pending change, so an upgrade uploads once.
- The task exports with `SnapshotReader::setup_export`, hashes the text (SHA-256) and skips the upload when it equals the hash of the last accepted upload, so unchanged catalogues (coupon use counts, price-neutral edits) never cost a request. A failed upload (cloud unreachable, 5xx, 429) keeps the change pending and retries with the heartbeat's `backoff`; `TOKEN_INVALID`/`TOKEN_REVOKED` marks the pairing revoked as the command task does; `FEATURE_NOT_ENTITLED` (the cloud thinks the branch is Free) waits `NOT_ENTITLED_RETRY` (15 min) and keeps the change pending; a file over the limit (`PAYLOAD_TOO_LARGE`, or a local text over 1 MB) is logged and dropped until the next change.

**Never:**

- Uploading from a Free, unpaired, follower or revoked Hub, or anything but the 5.36 setup file (no players, devices, money, stock or images are in it).
- Sending an unchanged file twice, or deleting a stored template on a refusal.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Paid upload | Basic branch, valid file | 200 `SetupTemplateStored`; file and meta stored under the branch prefix | none |
| Free branch | no entitlement / Free tier | nothing stored | 403 `FEATURE_NOT_ENTITLED` |
| Too large | body > 1 MB | stored file unchanged | 413 `PAYLOAD_TOO_LARGE` |
| Revoked token | Hub revoked | stored file unchanged | 401 `TOKEN_REVOKED` |
| Other branch | path id is another branch (any tenant) | stored file unchanged | 403 `FORBIDDEN` |
| Not a setup file | body is JSON of another shape | stored file unchanged | 422 `file` |
| Catalogue edit on paid Hub | plan added | one upload within 5 min | retry on failure |
| Edit on Free Hub | plan added | no request | none |
| WAN down, then back | several edits while offline | one upload of the latest file after it returns | backoff |
| Same file again | nothing changed | no request | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/error.rs` -- `ErrorCode::PayloadTooLarge` (`PAYLOAD_TOO_LARGE`); `setup_file.rs` -- `SETUP_TEMPLATE_PATH`, `SetupTemplateStored`, `SetupTemplateMeta` (+ generated TS in `packages/ui`).
- `crates/arena-cloud/src/templates.rs` -- `TemplateStore` (memory or S3 over `store::ObjectStore`), keys, `store_template`; `auth.rs` `AuthState.templates` (`OnceLock`, set by `serve`); `api/setup_template.rs` -- the route; `api/error.rs` -- 413 `PayloadTooLarge`; `api/mod.rs` mounts it.
- `crates/arena-tasks/src/cloud.rs` -- `CloudClient::put_setup_template`; `crates/arena-tasks/src/template.rs` -- `SetupTemplateTask`; wired in `crates/arena-hub/src/lib.rs` beside `CommandTask`.
- Tests: cloud `tests/setup_template.rs`; Hub task unit tests in `template.rs`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- `PAYLOAD_TOO_LARGE`, template types, generated TS
- [x] `arena-cloud` -- template store, `PUT /v1/branches/{id}/setup-template`, serve wiring, tests
- [x] `arena-tasks` / `arena-hub` -- client call, `SetupTemplateTask`, wiring, tests
- [x] Verification gate

**Acceptance Criteria:**

- Given a paired Hub on a paid tier, when its catalogue changes, then within 5 min it sends the setup file (≤ 1 MB) to `PUT /v1/branches/{id}/setup-template` with its device token, and the cloud stores it under the branch's prefix with its format version and upload time.
- Given a Hub on Free, when its catalogue changes, then it uploads nothing, and the cloud rejects any upload from a Free branch with `FEATURE_NOT_ENTITLED` checked from `gz_control` entitlements.
- Given an upload larger than 1 MB, from a revoked token, or for another tenant's branch, when it reaches the cloud, then it is rejected (`PAYLOAD_TOO_LARGE`, `TOKEN_REVOKED`, `FORBIDDEN`) and the stored file is unchanged.
- Given the WAN is down when the catalogue changes, when it returns, then the Hub uploads the latest setup file once, and repeated unchanged uploads are skipped by content hash.

## Spec Change Log

## Design Notes

Metadata is a sidecar object rather than a `gz_control` table: it needs no control migration (every control table is listed in the backup and restore tests) and Story 5.38 can read it, or list `branches/*/setup-template.meta.json`, through the same store. The cloud does not parse the setup file beyond `format` and `version`; the Hub that imports it (5.38) runs the full 5.36 checks.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-cloud`, `arena-tasks`, `arena-hub`, `arena-api`, `arena-kiosk-core`; `pnpm gen:types`, `check:generated`, `check:protocol`, `check:layers`, `check:strings`, `format:check`.

## Review Triage Log

Unattended build: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked --no-fail-fast` for proto/store/cloud/tasks/hub/api/kiosk-core green except four tests that fail the same way on a clean HEAD or only flake in a shared test process: `printer::lan::tests::a_printer_that_stops_reading_times_out`, `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed`, `arena-cloud/tests/binary.rs unreachable_at_start_retries_then_exits_1_without_logging_the_token` (all three fail on HEAD without this story's changes on this Windows host) and `ui_auth::tests::lock_and_unlock_log_the_staff_ids_and_never_the_pin` (passes alone, fails when run beside other logging tests). `check:protocol`, `check:layers`, `check:strings` and `format:check` pass; `check:generated` passes once the regenerated `ErrorCode.ts`, `index.ts` and new `SetupTemplateStored.ts` are committed.

Not done here: an S3/MinIO round trip of `TemplateStore` (the memory store is tested; the S3 store is the backup job's already-tested one), and a Hub status line for "last uploaded" (not in the acceptance criteria).

Code review (unattended, 2026-10-28): no blocking findings. Check order, storage layout, debounce/hash-skip/back-off behaviour match the contract. fmt, clippy (-D warnings) and the story's cloud and Hub task tests pass; `backup::a_hung_run_times_out_with_its_own_code` (zero-duration timer on this Windows host) and `binary::unreachable_at_start...` fail independently of this story's files.
