---
title: 'Story 9.9: A replacement Hub opens in "Restore this branch" and serves nothing until restored'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_revision: 'e49b586d9fbacdd69b1651ad169fa93c23722023'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-8-owner-replaces-a-failed-hub-and-gets-a-new-pairing-code.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-2-player-keys-are-backed-up-and-the-branch-key-is-escrowed.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A fresh Hub paired with the Story 9.8 code receives `branch.bootstrap` and starts an empty branch over the owner's real one, and nothing tells the cloud that a replacement Hub wants the branch key.

**Approach:** The cloud decides at pairing whether the branch has history (another Hub instance of it checked in, or the owner replaced a Hub with `hub.revoke`). Then `POST /v1/pair` answers `restore_pending: true` with no `bootstrap`. The Hub stores the identity and licence plus a durable `restore_pending` flag in one transaction, makes a one-time age key pair (private half in the OS keystore, never a branch key), and opens in "Restore this branch": every intent and every LAN `hello`/pairing is refused `RESTORE_PENDING`. While waiting it sends `POST /v1/kek/request {hub_instance_id, public_key}` (once per start, again when the WAN returns), which writes or refreshes one `pending` row in the new gz_control table `key_requests` for Story 9.10.

## Boundaries & Constraints

**Always:**
- History (`ControlDb::branch_has_history`, read before `pair_hub` revokes the earlier Hub): another `hub_instance_id` of the branch with `last_seen_at` set, or another instance plus an applied `hub.revoke` command. The same instance pairing again (retry, or re-pair after a lost keystore) is never a replacement.
- `HubPairResponse`: `bootstrap` is optional (absent when `restore_pending`), `restore_pending` defaults to false, `branch_name` is always sent.
- Hub: `hub_identity.restore_pending` / `restore_branch_name` (migration 0085) set in the same transaction as the identity; `HubStatus.restore_pending` is loaded at start, so a restart or a lost WAN still waits. `ApplyCtx.restore_pending` makes `apply::run` fail `RESTORE_PENDING` before any other step. LAN `hello`, kiosk pairing and kitchen pairing are refused `RESTORE_PENDING`.
- A waiting Hub makes no branch key, escrows nothing, backs up no keys and applies no owner commands (the key backup and command tasks idle).
- `key_requests (id, branch_id, hub_instance_id, public_key, state pending|approved|denied|expired, requested_at, decided_at, decided_by)`, unique per (branch, instance). Only a `pending` row is refreshed; a decided row stays. `POST /v1/kek/request` takes the Hub from its token, needs `hub_instance_id` to match it (`VALIDATION_FAILED` otherwise), a valid `age1…` key, a Hub that replaces another (`FORBIDDEN` otherwise), and is rate limited (30 an hour per branch).

**Never:**
- No key release, approval, denial, expiry or replay check (Story 9.10); "Restore from a file" stays disabled; no clearing of `restore_pending` (restore completion, Stories 9.10/9.12/9.13); no change to the first-Hub bootstrap.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Replacement pairs | branch replaced by `hub.revoke`, new instance, valid code | `restore_pending: true`, no `bootstrap`; Hub waits | none |
| First Hub / retry | no history, or same instance | bootstrap as before, `restore_pending: false` | none |
| Waiting Hub, hello / intent | any device or intent | `RESTORE_PENDING`, nothing written | none |
| Start / WAN returns | waiting Hub | `POST /v1/kek/request`; pending row written or refreshed | backoff on failure, `TOKEN_REVOKED` marks revoked |
| Restart | waiting Hub | still waiting, nothing written, request sent again | none |
| Request from a first Hub | no history | `FORBIDDEN` | no row |
| Bad key / foreign instance | not `age1…` / other instance | `VALIDATION_FAILED` | no row |
| Request after a decision | row approved/denied/expired | state returned, row unchanged | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{error,pairing,kek,snapshot}.rs` -- `RESTORE_PENDING`, pair response fields, `KekRequest`, `KeyRequestState`, snapshot `restore_pending`.
- `crates/arena-store/{control-migrations/0029_key_requests.sql,src/control/key_requests.rs}` -- the table, `branch_has_history`, `put_key_request`, `key_request`.
- `crates/arena-cloud/src/api/{pair,kek}.rs` -- history decision, `POST /v1/kek/request`.
- `crates/arena-store/{migrations/0085_restore_pending.sql,src/identity.rs}` -- the Hub's durable flag.
- `crates/arena-core/src/{status,writer}.rs`, `apply/mod.rs` -- status field, `record_replacement_pairing`, the intent guard.
- `crates/arena-tasks/src/{cloud,restore_wait,keys,commands,keystore}.rs` -- replacement pairing, one-time key, `KeyRequestTask`, idle tasks, `RESTORE_KEY_FILE`.
- `crates/arena-api/src/lan/{mod,session,pair,kds_pair}.rs`, `health.rs` -- LAN refusals, snapshot field.
- `crates/arena-hub/src/lib.rs` -- wiring; `apps/hub/src/{lib/RestoreBranch.svelte,routes/+layout.svelte}`, `packages/ui/src/messages/en.json` -- the screen and strings.
- Tests: `crates/arena-cloud/tests/{stepup,pair,backup}.rs`, `crates/arena-store/src/control/key_requests.rs`, `crates/arena-hub/tests/pairing.rs`, `crates/arena-core/src/apply/tests.rs`, `crates/arena-api/src/lan/session.rs`, `apps/hub/tests/restore-pending.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- `RESTORE_PENDING`, pair response fields, kek request types, snapshot field; regenerate types
- [x] `arena-store` (control) -- migration 0029 `key_requests`, history query, request upsert and reader
- [x] `arena-cloud` -- `/v1/pair` sends no bootstrap for a replacement; `POST /v1/kek/request`
- [x] `arena-store` (Hub) -- migration 0085, `mark_restore_pending`, `find_restore_pending`
- [x] `arena-core` -- `HubStatus.restore_pending`, `record_replacement_pairing`, intent guard
- [x] `arena-tasks` -- replacement pairing path, one-time key, `KeyRequestTask`, idle key backup and command tasks
- [x] `arena-api` / Hub UI -- LAN refusals, snapshot field, "Restore this branch" screen, strings
- [x] Tests at each layer

**Acceptance Criteria:**
- Given a fresh Hub paired with the Story 9.8 code of a branch with history, then no `branch.bootstrap` is applied, a one-time key pair exists (private half in the keystore) and the Hub opens in "Restore this branch".
- Given that state, then the screen shows the branch name, "This PC replaces the old Hub" and "Restore from a file" disabled with "Waiting for the owner to release the branch key".
- Given that state, a `hello` or any intent is refused `RESTORE_PENDING` and nothing is written.
- Given that state, the Hub sends `POST /v1/kek/request` once per start and again after the WAN returns, and a restart or lost WAN leaves it waiting with nothing written.
- Given the gz_control migration, `key_requests` exists and `POST /v1/kek/request` writes or refreshes one `pending` row per Hub instance.

## Spec Change Log

## Review Triage Log

Code review (2026-10-30): four layers (blind, edge-case, verification-gap, acceptance). Fixed:
- Android restart: a waiting Hub has no branch key, but `AndroidSecrets::lost` counted the missing key as lost, so a restart sent it to re-pairing (which minted a branch key). `lost_expecting(has_identity, expects_kek)` is used at start with `expects_kek = !restore_pending`; the re-pair path also skips minting while waiting.
- `HubPairResponse.branch_name` defaults; a non-replacement response without `bootstrap` is an error rather than an empty, unflagged pairing.
- Migration 0085 columns had no export-reference meanings (`export_reference` tests were red): meanings and `docs/export-format/reference.md` updated.
- Added `branch_has_history` unit tests (last_seen clause, same-instance exclusion, applied vs pending revoke) and a lost-key test for the waiting Hub.
Deferred (not blocking): revoke-only history for a Hub that never checked in is by design in the spec (owner escape hatch comes with restore stories 9.10/9.12/9.13); rate-limit, LAN kiosk/kitchen pairing refusal and `KeyRequestTask` WAN-return tests; background tasks (upload, housekeeping) while waiting; Revoked chip on the Restore screen.
Pre-existing failures on a clean checkout: `arena-tasks` `heartbeat::trusted_time...` and `printer::lan::a_printer_that_stops_reading_times_out`, `arena-cloud` `binary::unreachable_at_start...`.

## Design Notes

"Has history" is decided in the cloud, where the earlier Hubs and the `hub.revoke` row live, so the Hub needs no guess. A Hub that never checked in and was never replaced has nothing to restore, which keeps a failed first install (a retry with a new code) on the normal bootstrap path. The flag lives on `hub_identity`, outside the change log, so it survives restarts and never replicates.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api -p arena-kiosk-core`, `cargo test -p arena-hub --features dev-fixtures --test pairing`, `cargo clippy ... --all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run check:strings`, `check:layers`, `check:protocol`, `format:check`; in `apps/hub`: `pnpm exec playwright test restore-pending`

## Auto Run Result

Status: done

Implemented per spec. Verified: fmt, clippy `-D warnings`, `cargo test` for proto/store/core/tasks/kiosk-core/cloud/api, the Hub `pairing` integration test (real Hub binary against a local cloud: pair, replace, pair a second Hub, restart), `check:strings/layers/protocol`, `format:check`, and the Hub Playwright spec (desktop and phone, with axe).

Failures not caused by this story: `arena-cloud` `backup` `a_hung_run_times_out_with_its_own_code` and `binary` `unreachable_at_start...` fail on a clean checkout too; `arena-hub` `tests/android_fleet.rs` does not compile on a clean checkout (`SessionSettle` fields); `svelte-check` reports two `process` type errors in `apps/hub/playwright.kitchen.config.ts`.

Judgement calls and risks:
- Nothing clears `restore_pending` yet; restore completion (Stories 9.10, 9.12, 9.13) must.
- Between the pairing commit and the status flip a few microseconds pass in which an intent could be accepted; the Hub is brand new with no devices, so none exist.
- A replacement Hub still receives an owner setup code row from `pair_hub`; it is unused until the branch is restored.
