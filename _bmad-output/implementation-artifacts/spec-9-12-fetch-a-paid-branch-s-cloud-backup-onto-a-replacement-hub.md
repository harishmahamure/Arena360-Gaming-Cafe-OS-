---
title: "Story 9.12: Fetch a paid branch's cloud backup onto a replacement Hub"
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '4b548cfeea4789c82e64219eb301a7bfb15588f4'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-10-owner-releases-the-branch-key-to-the-replacement-hub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-11-upgrading-from-free-uploads-a-full-snapshot-before-the-first.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-9-a-replacement-hub-opens-in-restore-this-branch-and-serves-no.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A replacement Hub waiting in "Restore this branch" holds the released branch key (Story 9.10) but has none of the branch's history. A paid branch's history sits in the cloud bucket (newest snapshot, batches, `keys/player-keys.bin`), and nothing lets the Hub list it, download it safely or say what is missing.

**Approach:** `POST /v1/restore/plan` (Hub token) lists the branch's own prefix and answers with 10-minute read URLs. The Hub downloads into a staging folder, checks every object, resumes after a dropped connection, stops with `RESTORE_GAP` when a batch is missing or damaged, and shows progress with Cancel. The Hub screen gets a second option, "Restore from cloud backup". Replaying the staged files is Story 9.13.

## Boundaries & Constraints

**Always:**
- `POST /v1/restore/plan` body `{hub_instance_id, after?: {epoch, seq}}` (strict). Order of checks: Hub token (`TOKEN_REVOKED`/`TOKEN_INVALID`), `hub_instance_id` equals the token's (`VALIDATION_FAILED`), tier `Free` -> `FEATURE_NOT_ENTITLED`, the Hub replaces another Hub of the branch (`branch_has_history`, else `FORBIDDEN`), the owner's key was released to this Hub instance (`key_requests` row `approved` and `fetched_at` set, else the new `KEY_NOT_RELEASED`, 403), a rate limit of 60 calls an hour per branch. Nothing but the caller's `tenants/<t>/branches/<b>/` prefix is listed or signed; path characters in tenant or branch are an internal error.
- Answer `{snapshot?: {date, url}, keys_url?, batches: [{epoch, first, last, url}], batches_pending, expires_in_s}`. `snapshot` is the newest `snapshots/<date>.db.zst`; `keys_url` is present when `keys/player-keys.bin` exists. The cloud cannot read a snapshot's position, so: with a snapshot and no `after` the answer has `batches: []` and `batches_pending: true`; the Hub reads the position from the downloaded snapshot and asks again with `after`. With `after`, `batches` is every `changes/<epoch>/<first>-<last>.zst` after it in log order (a later epoch, or the same epoch with `last > seq`). With no snapshot at all, every batch is listed. Re-asking with the same `after` re-signs the URLs.
- Hub staging: `<data dir>/restore-staging/` holds `snapshot.db.zst`, `player-keys.bin`, `changes/<epoch>/<first:012>-<last:012>.zst` (the `:` of the epoch is written `@` because Windows file names cannot hold it). Each object is written to `*.part` and renamed once verified, so a file without `.part` is finished and is not fetched again after a restart or a dropped connection. Batches are checked with `decode_batch` (sha256, row count, sequence numbers) and against the name in the plan. The snapshot is checked by unpacking it and reading its last change `(epoch, seq)` (a database that opens and has a `changes` table); the key backup must be non-empty (Story 9.13 unwraps it).
- Network or 5xx failures retry with backoff, forever, keeping finished files; a `403/404/410` on an object, a failed check, or a hole in a same-epoch chain is a gap. `RESTORE_GAP` names the first bad range `from..to` (seq) and epoch, `last_good` = the seq of the last batch before it (or the snapshot's), and `lost` = the sum of `last - first + 1` of every listed batch from the bad one on. Nothing is replayed.
- Choosing "Restore up to <last good>" deletes staged batches after the good point, sets the staging state to `ready` with `partial: true` and keeps the rest. A complete download is `ready` with `partial: false`. Either way the Hub stays in "Restore this branch": 9.13 replays.
- Cancel (or a restart that does not resume) removes the staging folder; the Hub stays in "Restore this branch" with `restore_pending` unchanged.
- Hub UI control API (loopback only, like `/ui/pair-hub`, no staff session): `GET /ui/restore/cloud` -> the view or `null`, `POST /ui/restore/cloud` start, `POST /ui/restore/cloud/cancel`, `POST /ui/restore/cloud/partial`. Start is refused unless the Hub is in `restore_pending` with the key released (`KEY_NOT_RELEASED`) and not on Free (`FEATURE_NOT_ENTITLED`).
- Screen: second button "Restore from cloud backup" beside "Restore from a file", enabled once released, not shown on Free (tier from the snapshot). Confirm in place, then progress "Downloading 3 of 41" with bytes and percent, Cancel, the gap panel with "Restore up to <seq>" and the count that would be lost, and the ready summary. All strings in `packages/ui/src/messages/en.json`.
- Logs and errors carry object counts and seq ranges, never URLs, tokens or the key.

**Never:**
- No replay, no `arena-replay` call, no clearing of `restore_pending`, no epoch claim (Story 9.13); no change to `POST /v1/presign`, the uploader or the retention rules; no change to what 9.10 releases.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First ask | paid, key released, snapshot exists, no `after` | snapshot + keys URLs, `batches_pending` | none |
| Second ask | `after = snapshot position` | batches after it, in log order | none |
| No snapshot | batches only | every batch | none |
| Free / not released / not a replacement | token of such a Hub | no URLs | `FEATURE_NOT_ENTITLED` / `KEY_NOT_RELEASED` / `FORBIDDEN` |
| Dropped link | mid-download | retried; finished files kept | backoff |
| URLs expire | 403 after 10 min | plan asked again with the same `after`, finished files skipped | none |
| Missing / damaged batch | 404, bad sha, hole 12..20 | stops `RESTORE_GAP` 12..20, `last_good` 11, `lost` N | offer partial |
| Partial | Owner picks "up to 11" | later batches removed, `ready` partial | none |
| Cancel | any time | staging removed, state back to idle | none |
| Restart | files staged | idle, files kept; Start resumes | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{restore_plan.rs,error.rs,lib.rs}` -- plan/view types, `KEY_NOT_RELEASED`, `RESTORE_GAP`; regenerated TS types.
- `crates/arena-store/{src/control/key_requests.rs,src/snapshot.rs}` -- `key_released_to`, snapshot position of an unpacked copy.
- `crates/arena-cloud/src/{presign.rs,audit_build.rs,api/restore.rs,api/mod.rs,api/error.rs}` -- `presign_get`, shared batch listing, the endpoint.
- `crates/arena-core/src/restore_fetch.rs` -- `RestoreFetchControl` trait.
- `crates/arena-tasks/src/{cloud.rs,restore_fetch.rs,restore_fetch_tests.rs}` -- `restore_plan`, `download_object`, the downloader.
- `crates/arena-api/src/ui_restore_fetch.rs`, `crates/arena-hub/src/lib.rs` -- the loopback routes and wiring.
- `apps/hub/src/lib/RestoreBranch.svelte`, `apps/hub/src/routes/+layout.svelte`, `packages/ui/src/messages/en.json` -- the screen.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- types, error codes, regenerate TS
- [x] `arena-store` -- `key_released_to`, snapshot position
- [x] `arena-cloud` -- `presign_get`, `POST /v1/restore/plan`
- [x] `arena-tasks` / `arena-core` -- client calls, downloader, control trait
- [x] `arena-api` / `arena-hub` -- loopback routes and wiring
- [x] Hub screen and strings
- [x] Tests for every matrix row at the layer that owns it

**Acceptance Criteria:**
- [x] Given a Hub in "Restore this branch" with the key released on a paid tier, then "Restore from cloud backup" shows beside "Restore from a file"; on Free it is not shown.
- [x] Given Start, then `POST /v1/restore/plan` returns read URLs under the caller's branch prefix only for the newest snapshot, every batch after its seq across epochs, and `keys/player-keys.bin`.
- [x] Given the plan, then every batch's sha256 and header are verified, objects are staged, expired URLs are refreshed by asking the plan again, and a dropped connection resumes without re-downloading finished objects.
- [x] Given a missing or corrupt batch, then the Hub stops with `RESTORE_GAP` naming the seq range, offers "Restore up to <last good seq>" with the count lost, and replays nothing.
- [x] Given downloads, then the Hub shows "Downloading 3 of 41" with bytes and percent, and Cancel removes the staging folder and leaves the Hub in "Restore this branch".
- [x] Given a Free branch, a Hub without the released key, or a Hub that replaces nobody, then the plan is refused `FEATURE_NOT_ENTITLED`, `KEY_NOT_RELEASED` or `FORBIDDEN` with no URLs.

## Spec Change Log

## Review Triage Log

## Design Notes

The cloud keeps no manifest (Story 9.11) and a snapshot's position is inside the file, so the Hub supplies it: the first plan call returns the snapshot, the second the batches after it. This keeps the cloud from unpacking a snapshot per request. Fork headers travel inside each epoch's first batch (the Story 8.2 batch header); the Hub verifies them by decoding every batch, and Story 9.13 reads them when it replays. The Owner's PIN cannot be checked here: a replacement Hub has no staff or Owner PIN yet, and the Owner already authorised the restore with step-up MFA when releasing the key (Story 9.10), so the screen asks for an in-place confirmation on the loopback-only Hub UI instead. This is the same gap Story 9.10 recorded.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api`, `cargo clippy` on the touched crates `--all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run gen:types`, `check:strings`, `check:layers`, `check:protocol`, `format:check`

## Auto Run Result

Status: review

Implemented per spec. Verified: fmt, clippy `-D warnings` (proto, store, core, tasks, cloud, api, hub, kiosk-core), cargo tests for proto/store/cloud/core/tasks/api (new: cloud `tests/restore_plan.rs`, `api::restore`, store key-release and snapshot-position, tasks `restore_fetch` x10, api `ui_restore_fetch` x2), the Hub `pairing` integration test, Playwright `restore-pending` (desktop and phone, with axe), `check:protocol`, `check:layers`, `check:strings`, `format:check`. `check:generated` reports only the uncommitted regenerated types.

Failures not caused by this story (they fail the same way without it): `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start...`, `arena-tasks` `heartbeat::trusted_time...` and `printer::lan::a_printer_that_stops_reading_times_out`; `svelte-check` reports two `process` type errors in `apps/hub/playwright.kitchen.config.ts`.

Judgement calls and risks:
- The Owner's PIN cannot be checked on a replacement Hub (no staff, no Owner PIN yet); the screen asks for an in-place confirmation on the loopback-only route instead, and the key release (step-up MFA, Story 9.10) is the authorisation. The acceptance line "confirms with their PIN" is therefore not met as written.
- The plan is two calls (the cloud cannot read a snapshot's position); fork headers travel inside each epoch's first batch and are verified by decoding every batch, not repeated in the plan. Story 9.13 reads them when it replays.
- The "Restore up to <seq>" button is labelled "Restore up to the last good change" because `Button` labels take no parameters; the seq and the count lost are in the line above it.
- The gap check covers a batch missing from the bucket, a batch that fails its check, and a hole inside one epoch's chain; a hole between epochs is not detected (a new epoch's numbering after a fork is not fixed by this story).
- A `Gap` is held in memory: after a Hub restart the screen is idle and Start finds the gap again from the staged files.
- A failed local write (disk full) is retried like a network failure rather than shown as a distinct error.
