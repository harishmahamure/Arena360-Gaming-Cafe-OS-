---
title: "Story 8.25: Step-up MFA for sensitive commands"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '3494619a7a034b0e5f22b0565208dfb27d3fc114'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-24-manage-products-and-bundles.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `sensitive` exists in the owner command catalogue but nothing checks it: a stolen session could post a sensitive kind (staff, roles, device revoke) with no second factor, and the Hub records no proof of who approved it.

**Approach:** The cloud refuses a sensitive post `MFA_REQUIRED` unless the session holds a step-up proof younger than 5 minutes. `POST /v1/session/stepup` (TOTP, backup code or SMS) sets the proof; the post then records `elevated_by` on the command row and the Hub copies it into the change row. The Owner app asks for the code in a dialog and re-sends the same command with the same key; a sensitive command queued offline is held until the owner passes step-up.

## Boundaries & Constraints

**Always:**
- Proof = `owner_sessions.stepup_at` (migration 0021); valid for `STEPUP_TTL_MS` = 5 min. A session refresh rotates the row, so the proof is carried to the new row.
- Only a sensitive kind needs it; non-sensitive kinds never prompt. The check runs after payload validation and before any row is written; an "all branches" post is checked once.
- Wrong codes: 5 failures in 15 min (per owner, any factor, separate from the sign-in counter) pause step-up with `RATE_LIMITED` + `Retry-After`; the session is untouched. A wrong code never says which factor failed (`TOTP_INVALID`, field `code`, same copy for every method). A TOTP step or backup code used for step-up cannot be used again (same single-use rules as sign-in). SMS only when `owners.sms_fallback`.
- `commands.elevated_by` holds the owner id when the post was elevated (kind is always `mfa`); `CommandWire.elevated_by` (optional) carries it to the Hub, whose `command_intent` builds `ElevatedBy{kind: mfa, id, mode: hub, verifier_version: 0}` and `apply()` accepts it only for an `Owner` actor whose id equals `elevated_by.id`; it lands in `changes.elevated_by_*`.
- The sensitive fixture kind lives only in a test catalogue: `AuthState::extra_kinds` (empty in production, set by tests) is searched after `OWNER_COMMAND_KINDS`; its payload need only be an object.

**Never:** real sensitive kinds (8.26/8.27), the Hub gating on MFA itself, a step-up that ends the sign-in session, sending a sensitive queued command without a fresh proof.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| No proof | sensitive post, no/old `stepup_at` | 403 `MFA_REQUIRED`, nothing queued | app opens the dialog |
| Step-up ok | correct TOTP/backup/SMS code | 204, `stepup_at = now`; the re-sent post queues rows with `elevated_by = owner id` | — |
| Non-sensitive | price/plan/... post, no proof | queued, no prompt, `elevated_by` null | — |
| 5 wrong codes | 6th attempt | 429 `RATE_LIMITED` with `Retry-After` ≈ 900 s; session still valid | dialog shows the time |
| Replay | same TOTP step / backup code twice | second is `TOTP_INVALID` | generic copy |
| Offline queue | sensitive item queued, back online | held "Queued on this device"; dialog on Send; never posted without proof | cancel keeps it held |

</intent-contract>

## Code Map

- `crates/arena-proto/src/error.rs` -- `ErrorCode::MfaRequired` (`MFA_REQUIRED`); status 403 in `arena-cloud` `api/error.rs`, `arena-api` `ui_auth.rs`, `arena-kiosk-core` `queue.rs` matches.
- `crates/arena-proto/src/{owner_commands,commands}.rs` -- `OwnerCommandKind.sensitive` doc, `CommandWire.elevated_by`, `StepupRequest`/`StepupSms`/`StepupSmsSent` wire types, `STEPUP_TTL_MS`.
- `crates/arena-store/control-migrations/0021_stepup.sql`, `control/{session,commands,signin}.rs` -- `stepup_at`, `commands.elevated_by`, `complete_stepup`, `NewCommand.elevated_by`.
- `crates/arena-cloud/src/api/{stepup.rs (new),commands.rs,session.rs,mod.rs}`, `auth.rs` (`extra_kinds`) -- gate, endpoints.
- `crates/arena-core/src/{commands.rs,apply/guards.rs}` -- copy `elevated_by`, accept owner MFA.
- `apps/owner/src/lib/{api.ts,commands.svelte.ts,StepUpDialog.svelte (new)}`, `routes/+layout.svelte`, `CommandPreview.svelte`/`CommandRow.svelte`, `packages/ui/src/messages/en.json` -- `retryAfter`, `MFA_REQUIRED` handling, held state, dialog.
- Tests: `crates/arena-cloud/tests/stepup.rs` (new), `arena-core` apply test, `apps/owner/tests/stepup.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto/store: error code, wire types, migration 0021, store calls; regenerate types.
- [x] Cloud: `extra_kinds`, MFA gate in `post_command`, `/v1/session/stepup[/sms]`, `elevated_by` on rows and the Hub wire; Rust tests.
- [x] Hub core: copy `elevated_by`, accept owner MFA in `guards.rs`; test that the change row records it.
- [x] Owner app: dialog (focus in, paste allowed, Esc cancels without sending, generic error, pause time), re-send with the same key, queue hold; strings; Playwright tests.

**Acceptance Criteria:**
- [x] Given a sensitive kind without a proof younger than 5 min, when posted, then `MFA_REQUIRED`; after a correct code the same key posts and the row records `elevated_by`, which the change row carries.
- [x] Given a non-sensitive kind, when sent with no step-up, then no prompt.
- [x] Given a sensitive command queued offline, when the queue drains, then it is held until step-up passes.
- [x] Given 5 wrong codes, then step-up pauses 15 min with the time shown and the session lives.
- [x] Given the dialog, then focus moves in, paste works, the error never names the factor, Esc cancels without sending.

## Spec Change Log

## Review Triage Log

## Design Notes

- `ElevationMode` has no cloud value and `changes` has a CHECK on it; MFA is recorded as mode `hub`, `verifier_version` 0 to avoid rebuilding the append-only table. The kind (`mfa`) is what tells the audit viewer it was a step-up.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-api`, `cargo clippy --all-targets -- -D warnings` (touched crates), `pnpm run gen:types`, `check:strings`, `check:layers`, `format:check`; in `apps/owner`: `pnpm run check`, `pnpm exec playwright test` -- expected: clean

## Auto Run Result

Status: in-review. Sensitive owner kinds are refused `MFA_REQUIRED` without a step-up proof younger than 5 minutes; `POST /v1/session/stepup` (+ `/sms`) takes a TOTP, backup or SMS code; the command row records `elevated_by`, the Hub copies it into the change row (kind `mfa`); the Owner app asks in a dialog, re-sends with the same key, and holds a sensitive queued change. Verification: clippy clean on the touched crates; cargo tests pass except `arena-cloud` `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token` (fails without this change too; process timing); owner `pnpm run check` and 488 Playwright tests pass; check:strings, layers, protocol and format:check pass. Not done: the code-review pass. Known limits: the app does not know which kinds are sensitive, so a queued sensitive change is posted once and held after the cloud refuses it; a pending SMS step-up code is lost if the session refreshes before it is entered.

## Code Review Result

Reviewed with four layers (blind, edge-case, verification-gap, acceptance). Fixed in the Owner app: a stale step-up reply no longer settles a newer prompt; a post is re-asked when the proof lapses while the owner types; `sendHeld` drains again if a drain already under way passed it by; a native dialog close declines the step-up; the pause test now uses a `Retry-After` other than the 900 s fallback.

Deferred (accepted): re-send of an already-queued key after the proof lapsed answers `MFA_REQUIRED` (needs a store lookup by key); a held change does not block later queued ones; `MFA_REQUIRED` posts count against the post rate limit; no Playwright cover for the SMS option or row-level Resend of a sensitive kind.
