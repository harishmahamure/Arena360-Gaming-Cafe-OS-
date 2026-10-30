---
title: 'Story 9.8: Owner replaces a failed Hub and gets a new pairing code'
type: 'feature'
created: '2026-10-30'
status: 'review'
baseline_revision: 'e1dbb8a1384ab4acc08a5ced0bae3f30b4849109'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-25-step-up-mfa-for-sensitive-commands.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-27-manage-devices-and-groups-and-revoking-a-device.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-30-owner-manages-branches-create-rename-timezone-new-hub-pairin.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner whose Hub PC died or was stolen cannot stop it acting for the branch (its token stays valid), and the only way to a new Hub is a pairing code that revokes the old one as a side effect of pairing. `hub.revoke` is a reserved name that nothing accepts.

**Approach:** `hub.revoke` becomes a *cloud-applied* sensitive owner kind (any tier, no Hub needed). Posting it needs a step-up (Story 8.25); the control role then writes the command row already `applied` (with `elevated_by` = the owner) and revokes the branch's live Hub token in the same transaction. Account → Branches gets "Replace Hub": confirm, step-up, revoke, then the Story 2.9 pairing-code card with a fresh single-use code ("Get a new code" reuses the existing issue route and never revokes again). The old Hub's next heartbeat or command fetch is refused `TOKEN_REVOKED` (existing behaviour); its header alert now reads "This Hub was replaced by the owner".

## Boundaries & Constraints

**Always:**
- `OwnerCommandKind.cloud`: the cloud applies the kind itself. `hub.revoke` is `sensitive`, `min_tier` Free, payload `{}` only (any field is refused). `accepted_owner_kind` accepts `handled || cloud`; the Hub still treats `hub.revoke` as unsupported (it is never queued).
- One branch of the owner's own tenant per post: `"all"` is `VALIDATION_FAILED` field `branch_id`; another tenant's branch is `NOT_FOUND`; no step-up in 5 minutes is `MFA_REQUIRED` and nothing is revoked or written. The tier / flag / command-schema `refusal` does not apply to a cloud kind.
- `ControlDb::apply_hub_revoke`: one transaction inserts the `applied` row (only while a live Hub exists) and sets `revoked_at` on the live `hubs` row. The same idempotency key again returns its first row and revokes nothing (so it can never revoke the replacement Hub). A branch with no live Hub and no earlier row is `STEP_NOT_READY`.
- The cloud-side revoke is immediate: `authenticate` reads `revoked_at` from the database on every call, so the token cache does not delay it.
- Hub header alert text `hub.pairing.revoked` = "This Hub was replaced by the owner"; the existing `PairingState::Revoked` already stops uploads and command fetches. LAN trading is not changed.

**Never:**
- No restore state on the new Hub, no `key_requests`, no KEK release (Stories 9.9, 9.10); no change to `/v1/pair`; no queueing of `hub.revoke` on the owner's device while offline (the app shows "Couldn't replace the Hub").

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Replace | Owner, paired branch, fresh step-up | row `applied`, `elevated_by` owner, Hub token revoked; app shows the command-row then a new code | none |
| No step-up | no proof / older than 5 min | `MFA_REQUIRED`, nothing revoked | app opens the code prompt, resends the same key |
| Old Hub beats | heartbeat / command fetch with the old token | `TOKEN_REVOKED` (401); Hub shows the alert and stops | none |
| Same post again | same key | first row returned, nothing changes (even after a replacement paired) | none |
| No live Hub | different key | `STEP_NOT_READY` | none |
| "all" / other cafe | `branch_id` "all" / foreign | `VALIDATION_FAILED` / `NOT_FOUND` | nothing revoked |
| Code expired | owner taps "Get a new code" | a new code via the install-hub route, no second revoke | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/owner_commands.rs` -- `cloud` flag, `cloud_applied("hub.revoke")`, empty-payload check, tests.
- `crates/arena-store/src/control/commands.rs` -- `apply_hub_revoke`.
- `crates/arena-cloud/src/api/commands.rs` -- `post_cloud_command`; `refusal` skips cloud kinds.
- `apps/owner/src/lib/{BranchCard.svelte,commands.svelte.ts}`, `packages/ui/src/messages/en.json` -- "Replace Hub" flow, `revokeHub`, strings, header alert copy.
- Tests: `crates/arena-cloud/tests/{stepup.rs,owner_commands.rs}`, `apps/owner/tests/replace-hub.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- `cloud` kind flag, `hub.revoke` cloud-applied sensitive any-tier kind; unit tests
- [x] `arena-store` (control) -- `apply_hub_revoke` (row + revoke in one transaction, idempotent)
- [x] `arena-cloud` -- `POST /v1/commands` applies a cloud kind; tests (step-up, revoke at once, `TOKEN_REVOKED`, replay, replacement pairing, "all", foreign branch)
- [x] Owner app -- "Replace Hub" with confirm, step-up, command-row, pairing-code card; Playwright spec
- [x] Hub header alert copy "This Hub was replaced by the owner"

**Acceptance Criteria:**
- Given Account → Branches for a paired branch (any tier), when the owner chooses "Replace Hub", confirms and passes step-up, then `hub.revoke` is shown as an applied `command-row`, the old Hub's token is revoked and `elevated_by` is the owner.
- Given the token is revoked, when the old PC beats or fetches commands, then it is refused `TOKEN_REVOKED` and shows "This Hub was replaced by the owner", stopping uploads and fetches.
- Given the revoke applied, then a new single-use 8-character code is shown with the expiry card, and "Get a new code" replaces an expired one without a second revoke.
- Given no step-up, a foreign branch or "all", then it is refused (`MFA_REQUIRED`, `NOT_FOUND`, `VALIDATION_FAILED`) and nothing is revoked.

## Spec Change Log

## Review Triage Log

## Design Notes

The command row is written `applied` with `resolved_at` = now so the Owner app's existing list and `command-row` show "Applied" with no new status. The Hub-side `PairingState::Revoked` machinery from earlier stories (heartbeat, uploads, commands, catalogue, keys, templates) already stops all cloud traffic on `TOKEN_REVOKED`; this story only changes the alert copy. Every owner session is the tenant Owner until Epic 14 adds team roles, so "not the Owner" is covered by the tenant check.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-cloud -p arena-core`, `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-core --all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run check:strings`, `check:layers`, `check:protocol`, `format:check`; in `apps/owner`: `pnpm run check`, `pnpm exec playwright test`

## Auto Run Result

Status: review

Implemented per spec. Verified: fmt, clippy `-D warnings`, `cargo test` for proto/store/cloud/core, `check:strings/layers/protocol`, `format:check`, owner `svelte-check`, owner Playwright (replace-hub, branches, commands, stepup, devices, a11y, queue) and the Hub `connectivity` revoked-banner test all pass.

Failures not caused by this story: `arena-cloud` `backup` `a_hung_run_times_out_with_its_own_code` fails on a clean checkout too; `binary` `unreachable_at_start...` is the known Windows flake; `arena-core` `proptest_apply` failed once under load and passed on rerun.

Judgement calls and risks:
- The command row is created already `applied`; there is no "waiting" state, so a revoke of a Hub that is offline still applies at once (intended: a dead PC never answers).
- Commands still waiting for the old Hub are rejected `TOKEN_REVOKED` by the existing sweeper (within a minute) even if a replacement pairs soon after.
- `TOKEN_INVALID` also sets the Hub's revoked state, so the new alert text also appears for an unknown token.
- The disk was full during the run; the regenerable, git-ignored `target/debug/incremental` build cache (124 GB) was deleted to continue.
