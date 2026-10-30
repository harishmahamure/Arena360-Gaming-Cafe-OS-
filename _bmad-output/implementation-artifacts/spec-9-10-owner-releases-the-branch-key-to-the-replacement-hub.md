---
title: 'Story 9.10: Owner releases the branch key to the replacement Hub'
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_revision: '62c50d21fbac5046a4b0c03a1cb26352133b2159'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-9-a-replacement-hub-opens-in-restore-this-branch-and-serves-no.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-8-owner-replaces-a-failed-hub-and-gets-a-new-pairing-code.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-2-player-keys-are-backed-up-and-the-branch-key-is-escrowed.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A replacement Hub waits in "Restore this branch" with a `pending` row in `key_requests` (Story 9.9), but nothing lets the owner see it, approve or deny it, or hand the escrowed branch key (Story 9.2) to that Hub.

**Approach:** The owner app shows "Release branch key to <PC name>" with Approve or Deny. Both send the reserved sensitive, cloud-applied kind `backup.restore` (step-up MFA, a `command-row`). On Approve the control role unwraps the escrowed key with the escrow identity and re-wraps it to the request's one-time public key, in one transaction that also decides the row and writes one `admin_audit` row. The waiting Hub fetches the blob once, unwraps it with its private key, stores the key in the OS keystore and enables "Restore from a file".

## Boundaries & Constraints

**Always:**
- Payload `{request_id, decision: "approve"|"deny"}`, strict (`deny_unknown_fields`). `backup.restore` becomes `cloud_applied`, still sensitive, tier Free, exactly one branch, never `HANDLED_COMMAND_KINDS` on the Hub. Mirror the `hub.revoke` path (`apply_hub_revoke`, `post_cloud_command`, `step_up_check`, idempotency key, `elevated_by{kind: mfa}`): the `commands` row, the `key_requests` decision and the audit row commit together.
- The escrow private key is the cloud secret `ARENA_KEK_ESCROW_KEY` (an `AGE-SECRET-KEY-1…` string) held in `AuthConfig` as a redacted newtype; unset means Approve fails `STEP_NOT_READY` with nothing changed. Tests inject their own pair. The key bytes live in zeroizing buffers, and the key never appears in a log, error, response (other than the re-wrapped blob) or audit row.
- The escrow row used is the branch's newest `kek_escrow` (written by an earlier Hub instance). None means `STEP_NOT_READY`.
- Control DB: migration `0030` adds `key_requests.wrapped_kek TEXT`, `fetched_at INTEGER`, `pc_name TEXT`. `KekRequest` gains an optional `pc_name` (the Hub sends its sanitised host name; the app falls back to "the new PC"). Pending rows older than 7 days become `expired` (swept before every list, decision and fetch). `denied` and `expired` rows stay decided (the owner replaces the Hub again for a fresh instance).
- `POST /v1/kek/fetch` (Hub token; same auth as `/v1/kek/request`) returns `{state, wrapped_kek?}`: the blob only for an `approved` row not yet fetched, and it sets `fetched_at` in the same transaction; a second fetch is `KEY_ALREADY_RELEASED`. `pending`, `denied`, `expired` return the state with no blob.
- New error code `KEY_ALREADY_RELEASED` (409): a request whose public key was already released for the branch (any instance) cannot be approved, and a fetched key cannot be fetched again. A revoked Hub is refused `TOKEN_REVOKED` on fetch and on Approve; a request of another tenant's branch is `NOT_FOUND`/`FORBIDDEN`; a request not `pending` cannot be decided (`STEP_NOT_READY`).
- Audit: exactly one `admin_audit` row per release, denial or expiry (branch, Hub instance, owner, outcome); `actor_kind` is `'system'` with the owner as `actor_id` (the check constraint allows only `admin|system`). Expiry writes its row when the sweep flips the row.
- Owner app: `OwnerBranch` gains `key_request?: {id, pc_name, requested_at}` (the newest unexpired pending row). `BranchCard` shows the prompt with Approve and Deny, each a `command-row` through the step-up dialog. All strings are in `packages/ui/src/messages/en.json`.
- Hub: `KeyRequestTask` keeps asking while `pending` (poll with its existing backoff, and after the WAN returns), then fetches, unwraps with the one-time identity, stores the key as the branch key in the keystore, deletes the one-time key, and sets `restore_key_released` (`HubStatus`, snapshot, regenerated TS types). At start the flag is true when `restore_pending` and the keystore already holds the key. The released key is never logged or written to the database. `denied` or `expired` keeps showing "Waiting for the owner to approve on their phone".
- Hub screen: `RestoreBranch.svelte` enables "Restore from a file" once released; before it, the waiting copy stays.

**Never:**
- No clearing of `restore_pending`, no restore from file in the waiting state (a waiting Hub has no staff session or Owner PIN; the Story 9.7 endpoints stay as they are, and restore completion belongs to Stories 9.12 and 9.13), no cloud-backup restore, no change to first-Hub bootstrap, no reader of the escrow other than the release path, no new endpoint that returns the escrowed bytes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Approve | pending row, escrow present, fresh step-up | row `approved`, re-wrapped blob stored, command `applied` with `elevated_by`, one audit row | none |
| Hub fetches | `approved`, not fetched | blob returned once; Hub keystore holds the key; screen enables the option | none |
| Second fetch | already fetched | none returned | `KEY_ALREADY_RELEASED` |
| Deny | pending row | row `denied`, audit row, Hub never gets a blob and keeps waiting | none |
| 7 days pass | pending older than 7 days | row `expired`, audit row; Approve refused | `STEP_NOT_READY` |
| Replay | public key already released on the branch | refused | `KEY_ALREADY_RELEASED` |
| Revoked / foreign | revoked Hub instance, or other tenant's branch | refused, nothing written | `TOKEN_REVOKED` / `NOT_FOUND` |
| No step-up / no escrow key configured | stale step-up / env unset | refused, nothing changed | `MFA_REQUIRED` / `STEP_NOT_READY` |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{error.rs,owner_commands.rs,kek.rs,wizard.rs,snapshot.rs}` -- `KeyAlreadyReleased` (variant, `as_str`, test table); `backup.restore` -> `cloud_applied("backup.restore")` (l.101; flip the tests at ~l.501) and a `deny_unknown_fields` payload checked in `validate_owner_payload` (l.204); `KekRequest.pc_name`, `KekFetchResponse`, `PendingKeyRequest`; `OwnerBranch.key_request` (`wizard.rs:339`); snapshot `restore_key_released`. Regenerate `packages/ui/src/generated`.
- `crates/arena-core/src/commands.rs:488` -- add `backup.restore` to the list of kinds the Hub refuses.
- `crates/arena-store/control-migrations/0030_key_release.sql`, `src/control/{key_requests.rs,kek_escrow.rs,commands.rs,admin_audit.rs}` -- columns; list-pending, expire sweep, `latest_kek_escrow` (only reader), `apply_key_release` (modelled on `apply_hub_revoke` l.146), fetch-once; audit via `insert_stmt`. Update the module doc of `kek_escrow.rs`.
- `crates/arena-cloud/src/{config.rs,backup.rs,api/kek.rs,api/commands.rs,api/branches.rs,api/error.rs}` -- escrow key config and env doc; reuse `backup::{decrypt, seal}`-style age helpers to unwrap and re-wrap; `/kek/fetch` route; `post_cloud_command` (l.574) dispatches by kind; `row()` (`branches.rs:31`) fills `key_request`; status mapping 409.
- `crates/arena-tasks/src/{restore_wait.rs,cloud.rs,keystore.rs,keys.rs}` -- `KeyRequestTask` fetch loop (`wait_in_restore` l.107, `ask` l.136), `CloudClient::kek_fetch` (beside `kek_request`, l.246), keystore write for `KEK_FILE`, delete `RESTORE_KEY_FILE`; send `pc_name`.
- `crates/arena-core/src/status.rs`, `crates/arena-hub/src/lib.rs:272-309,601-620`, `crates/arena-api/src/health.rs` -- status field, start-up derivation, snapshot.
- `apps/owner/src/lib/{commands.svelte.ts,BranchCard.svelte}`, `apps/hub/src/lib/RestoreBranch.svelte`, `packages/ui/src/messages/en.json` -- `KINDS` label, `releaseKey`/`denyKey` copied from `revokeHub`, prompt and rows, enabled option and strings.
- Tests: `crates/arena-cloud/tests/stepup.rs` (pattern: 9.9 `kek/request` tests), `crates/arena-store/src/control/key_requests.rs`, `crates/arena-tasks` unit tests, `crates/arena-hub/tests/pairing.rs`, `apps/hub/tests/restore-pending.spec.ts`, owner Playwright/unit tests beside existing `hub.revoke` ones.

## Tasks & Acceptance

**Execution:**
- `arena-proto` -- error code, command kind and payload, request/fetch types, `OwnerBranch.key_request`, snapshot flag; regenerate types
- `arena-store` (control) -- migration 0030, sweep and list, escrow reader, `apply_key_release`, fetch-once, audit rows
- `arena-cloud` -- escrow key config, unwrap and re-wrap, `backup.restore` through `post_cloud_command`, `/v1/kek/fetch`, `key_request` on branches
- `arena-tasks` / `arena-core` / `arena-hub` / `arena-api` -- fetch loop, keystore, status and snapshot flag
- owner app and Hub UI -- prompt, Approve/Deny command rows, enabled option, strings (`pnpm run check:strings`)
- Tests for every matrix row at the layer that owns it

**Acceptance Criteria:**
- Given a pending request, when the owner opens Account -> Branches, then "Release branch key to <PC name>" shows the PC name and request time with Approve and Deny.
- Given Approve with fresh step-up, then a `backup.restore` `command-row` applies, the Hub receives only the re-wrapped blob, stores the key and enables "Restore from a file".
- Given a denial or no approval within 7 days, then the Hub shows "Waiting for the owner to approve on their phone" and never receives a key.
- Given any release, denial or expiry, then one `admin_audit` row records branch, Hub instance, owner and outcome and the key appears nowhere else.
- Given a revoked Hub, a foreign branch or a replayed public key, then the request is refused (`TOKEN_REVOKED`, `FORBIDDEN`/`NOT_FOUND`, `KEY_ALREADY_RELEASED`) and no key leaves the control role.

## Spec Change Log

## Review Triage Log

## Design Notes

The Hub's one-time key pair is the replay guard: a public key is used for one release. A lost fetch response leaves the owner to replace the Hub again (a new instance, a new pair), which is safer than a key that can be fetched twice. Restore from file in the waiting state needs its own Owner authorisation on a Hub with no staff; that is left to the restore-completion stories and noted as a gap, not guessed here.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api`, `cargo test -p arena-hub --features dev-fixtures --test pairing`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run check:strings`, `check:layers`, `check:protocol`, `format:check`; Playwright `restore-pending` in `apps/hub` and the owner app's command specs

## Auto Run Result

Status: in-review

Implemented per spec. Verified: fmt, clippy `-D warnings` (proto, store, cloud, core, tasks, api, hub), cargo tests for proto/store/core/cloud/tasks/api, the Hub `pairing` integration test, Owner and Hub Playwright specs, `check:strings/layers/protocol`, `format:check`.

Failures not caused by this story: `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start...`, `arena-tasks` `heartbeat::trusted_time...` and `printer::lan::a_printer_that_stops_reading_times_out`; `arena-e2e` does not compile on Windows.

Judgement calls and risks:
- The enabled "Restore from a file" button has no action in the waiting state; restore completion (Stories 9.12, 9.13) needs an Owner authorisation path on a Hub with no staff.
- A lost fetch response leaves the key unrecoverable (`KEY_ALREADY_RELEASED`); the owner replaces the Hub again.
- Expiry counts from `requested_at`, which the Hub's re-asks refresh, so a restarting Hub keeps a pending request alive past 7 days.
- `ARENA_KEK_ESCROW_KEY` is not yet in the deploy manifests, so production Approve returns `STEP_NOT_READY` until it is wired.
