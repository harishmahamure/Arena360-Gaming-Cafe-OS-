---
title: 'Story 8.1: Presign issues batch and summary upload URLs under the branch''s own R2 prefix'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '001a4d5bb08d78333922583b20b950dfd6bfcdb5'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A paid Hub has nowhere to send change-log batches, the branch summary, rollups or snapshots: the cloud issues no upload links, and no code builds the `tenants/<t>/branches/<b>/…` keys (AD-4, AD-36).

**Approach:** Add `POST /v1/presign` to the control role. A device-token Hub gets 10-minute presigned PUT URLs, built with `aws-sdk-s3` against R2 (MinIO in dev), for keys under its own branch prefix. The cloud applies the Free-tier gate, epoch fencing and a per-branch hourly limit, and counts R2 PUTs per tenant and branch.

## Boundaries & Constraints

**Always:**
- `tenant_id` and `branch_id` come only from the device token (hub row, then branch row); any such field in the body is ignored.
- Keys: `changes/<epoch>/<first:012>-<last:012>.zst`, `state/latest.json`, `rollups/<yyyy-mm-dd>.json`, `snapshots/<yyyy-mm-dd>.db.zst` under `tenants/<t>/branches/<b>/`; every URL valid 600 s.
- Presigner: region `auto`, path-style, `request_checksum_calculation = WhenRequired`. Config from `ARENA_DATA_S3_{ENDPOINT,BUCKET,ACCESS_KEY_ID,SECRET_ACCESS_KEY}` (never logged); `--dev` falls back to the MinIO dev compose credentials. No config and not dev answers 503.
- Epoch is `<counter>:<instance>`; "below `branch_fence`" compares `(counter numeric, instance)`. Below fence: summary/rollup/snapshot are `FENCED`, batch is still issued.
- Free (tier_at(now) = Free) is `FEATURE_NOT_ENTITLED` before anything else is issued.
- Rate limit key `presign:<tenant>:<branch>`, one hit per issuing call, 1 h window, limit `4 × 3600 / max(upload_interval_s, 30)` from the licence claims; refused `RATE_LIMITED` with Retry-After; refusals are counted.

**Never:**
- The Grafana/OpenTelemetry exporter (deferred by the user with Story 2.3; this story keeps the per-tenant PUT counters in process and logs them).
- A cloud copy of branch rows, a manifest, or the Hub-side uploader (Story 8.2).
- Trusting any key component from the request: epoch and date are validated; anything with path characters is refused, never sanitised.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Batch + summary | paid, epoch ≥ fence, `batch{first,last}`, `summary:true` | `{batch_url, summary_url}` for the two keys | none |
| Rollup / snapshot | `kind:rollup\|snapshot`, `date` | `{url}` for the dated key | other kind: `UNSUPPORTED` |
| Traversal | epoch or date with `/`, `\`, `..` | nothing issued | `FORBIDDEN` + `admin_audit` row naming the hub id |
| Stale epoch | epoch below fence | batch URL issued; summary/rollup/snapshot refused | `FENCED` |
| Free | Free tier | nothing issued | `FEATURE_NOT_ENTITLED` |
| Over limit | limit reached in the hour | nothing issued | `RATE_LIMITED`, `Retry-After`, refusal counted |
| Bad shape | `first > last`, bad date, nothing requested | nothing issued | `VALIDATION_FAILED` |

</intent-contract>

## Code Map

- `crates/arena-cloud/src/api/heartbeat.rs` -- `authenticate` (device token → `HubRow`), rate-limit use of `RateRule`/`ceil_s`; pattern to follow.
- `crates/arena-cloud/src/api/setup_template.rs` -- Free gate (`entitlement` → `tier_at`), `ApiError::forbidden`; sibling route.
- `crates/arena-cloud/src/api/mod.rs`, `error.rs` -- route merge; `status_of` must map the new code.
- `crates/arena-cloud/src/store.rs` -- `S3Config`, `S3Store::new` (region `auto`, `WhenRequired`, path style); the presigner reuses the same client settings.
- `crates/arena-cloud/src/templates.rs`, `lib.rs` `serve` -- `OnceLock` store set at start-up; same pattern for the presigner.
- `crates/arena-cloud/src/licence.rs` `claims_for` -- `upload_interval_s` for the limit.
- `crates/arena-store/src/control/hubs.rs` -- `branch_fence` reads/writes; add a read of the fence epoch.
- `crates/arena-store/src/control/admin_audit.rs` -- `append_admin_audit` for the `FORBIDDEN` event.
- `crates/arena-proto/src/error.rs` -- `ErrorCode` (add `Fenced`); copies of the status map in `crates/arena-api/src/ui_auth.rs`, `crates/arena-kiosk-core/src/queue.rs`; `packages/ui/src/generated` by `pnpm gen:types`.
- `crates/arena-cloud/tests/setup_template.rs`, `tests/support/s3.rs` -- harness to copy; MinIO helper for the PUT test.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/error.rs` (+ status maps, generated TS) -- add `ErrorCode::Fenced` (`FENCED`) -- epoch fencing needs its own code.
- [x] `crates/arena-proto/src/presign.rs`, `lib.rs` -- `PresignRequest`, `PresignBatch`, `PresignResponse`, `PRESIGN_TTL_S` -- the wire shape.
- [x] `crates/arena-store/src/control/hubs.rs` -- `branch_fence_epoch(branch_id)`.
- [x] `crates/arena-cloud/src/presign.rs` -- `Presigner` (S3 config, key builders, `presign_put`), epoch parse/compare, `PutMetrics` per tenant/branch -- the core.
- [x] `crates/arena-cloud/src/api/presign.rs`, `api/mod.rs`, `auth.rs`, `lib.rs`, `config.rs` -- route, state, start-up wiring.
- [x] `crates/arena-cloud/tests/presign.rs` and unit tests -- every matrix row, body ids ignored, MinIO PUT round trip (skipped without `ARENA_TEST_S3_URL`).
- [x] Docs: `deploy/` env note and README pin table entry if one lists cloud env vars.

**Acceptance Criteria:**
- Given a paid Hub, when it posts `{epoch, batch, summary:true}`, then it gets both URLs for its own prefix, valid 600 s, and a PUT to each succeeds on MinIO.
- Given the matrix rows above, when each runs, then the code and audit/metric effects match.

## Spec Change Log

## Review Triage Log

## Design Notes

Presigning is local signing (no network), so tests assert on the URL (path, `X-Amz-Expires=600`, prefix) and one MinIO test does the real PUT. The OpenTelemetry/Grafana export of `PutMetrics` is deferred with Story 2.3's exporter.

## Verification

**Commands:**
- `cargo test -p arena-cloud -p arena-proto -p arena-store`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check`; `pnpm run check:generated`; `pnpm run check:protocol`.

## Auto Run Result

Status: in-review (implemented; verification run on the touched crates).

- Not run here: the MinIO PUT round trip (no Docker/MinIO on this machine; the test returns early without `ARENA_TEST_S3_URL`). Run it in CI or locally with `deploy/dev/compose.yaml`.
- Deferred: the Grafana/OpenTelemetry export of the per-tenant PUT counters (with Story 2.3's exporter); `ARENA_DATA_S3_*` in the Kubernetes control Deployment (production answers 503 until set).
- Decisions: a stale epoch asking for batch plus summary gets the batch URL with `summary_fenced: true`; a stale epoch asking for only summary, rollup or snapshot gets `FENCED` (409); `kind` together with `batch` or `summary` is `VALIDATION_FAILED`.
- Two `arena-cloud` tests fail on a clean checkout too (`binary::unreachable_at_start_…`, `backup::a_hung_run_times_out_with_its_own_code`); workspace clippy fails in `arena-kiosk-svc` and `arena-e2e`, untouched by this story.

## Review Triage Log (code review)

- Reviewed the diff of 910829e against the spec and I/O matrix: no blocking findings. fmt and clippy (arena-cloud, arena-proto, arena-store) are clean; the presign tests (11) and unit tests pass.
- Remaining failures are pre-existing and unrelated: `binary::unreachable_at_start_…` and `backup::a_hung_run_times_out_with_its_own_code`. The MinIO PUT round trip is still unrun here (no MinIO).
