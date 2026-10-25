---
title: 'Story 2.16: Step 7 — invite staff by phone with a role and a one-time setup code'
type: 'feature'
created: '2026-10-26'
status: 'done'
baseline_commit: '170fdba'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-15-owner-changes-reach-the-hub-through-the-command-mailbox-with.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-14-owner-and-staff-set-their-own-pin-and-password-at-the-hub-wi.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The owner cannot add a manager, counter or kitchen member during setup: there is no step 7, no invite, and the Hub's `staff.upsert` cannot create a member who has no password yet.

**Approach:**
- The cloud keeps a `staff_invites` row per invite and queues one `staff.upsert` command carrying only `setup_code_hash` and `setup_expires_at = now + 72 h`; the plain code goes back once in the HTTP answer and is never stored.
- The Hub's `staff.upsert` accepts a hash-only create or a re-issue, refuses a taken login (`LOGIN_TAKEN`) and a full licence (`USER_LIMIT`).
- The onboarding app gets step 7 (form, invite list with status, one-time code panel, New code, Skip for now) and `tier-lock` gets the shared billing flag.

## Boundaries & Constraints

**Always:**
- **Cloud migration** `0012_staff_invites.sql`: `staff_invites(id ULID = the Hub's staff_id, branch_id, name, phone, role Manager|Counter|Kitchen, command_key, setup_expires_at, created_at)`; `onboarding_progress.state` also takes `skipped` (table rebuilt).
- **Invite API** (owner session, branch must have a seen Hub, else `STEP_NOT_READY`): `GET /v1/wizard/staff`, `POST /v1/wizard/staff` (name, phone E.164, role), `POST /v1/wizard/staff/{id}/code` (new code), `POST /v1/wizard/staff/skip`. The invite and its command are written in one transaction. A status is derived from the command row: pending/fetched `waiting`, applied `applied`, rejected `failed` with the Hub's code and message.
- **Hub `staff.upsert`:** `setup_code_hash` (Argon2id) with `setup_expires_at` creates a member with no password; a `staff_id` that names no row is accepted as the new id only with a setup code and a ULID. `role_id` may name a built-in role (the cloud does not know the Hub's role ids). A login is the E.164 phone. A taken login is `LOGIN_TAKEN`; a create beyond the licence's `users` limit is `USER_LIMIT`. Re-issue: an existing member without a password gets the new hash and expiry (the old code stops working); one with a password is refused.
- **Step 7 done** when a Manager invite's command is reported applied; "Skip for now" stores `skipped` (never over `done`); a skipped step counts as passed for `current`.
- **`tier-lock`** shows the lock, "Available on <tier>" and, when the shared billing flag is on, an underlined "Upgrade" link; with the flag off (the default until Story 10.5) it shows "Available soon" in its place.

**Never:** the plain setup code in a command, log, change row or the database; fan-out to several branches; step-up MFA; the Hub-side staff screen.

## Code Map

- `crates/arena-proto`: `error.rs` (`LoginTaken`, `UserLimit`), `intent.rs` (`StaffUpsert`), `wizard.rs` (`StepState::Skipped`), `staff_invite.rs` (new), generated TS.
- `crates/arena-core/src/apply/staff_roles.rs`, `tests/`; `crates/arena-store/src/staff.rs` is unchanged (`set_staff_setup_code` exists).
- `crates/arena-store`: `control-migrations/0012_staff_invites.sql`, `control/staff_invites.rs`, `control/tenant.rs`.
- `crates/arena-cloud`: `api/wizard_staff.rs` (new), `api/commands.rs`, `api/mod.rs`, `api/error.rs`.
- `crates/arena-api/src/ui_auth.rs`, `crates/arena-kiosk-core/src/queue.rs` (new error codes).
- `packages/ui`: `TierLock.svelte`, billing flag, messages; `apps/gallery`; `apps/onboarding`: `StaffStep.svelte`, `api.ts`, `wizard.svelte.ts`, `setup/[step]/+page.svelte`, tests.

## Tasks & Acceptance

- [x] Proto types and error codes; generated TS.
- [x] Hub `staff.upsert`: hash-only create, re-issue, `LOGIN_TAKEN`, `USER_LIMIT`, E.164 login, role by name.
- [x] Cloud migration, store module, invite API, step completion on applied Manager invite, skip.
- [x] `tier-lock` billing flag, "Upgrade"/"Available soon", gallery entry.
- [x] Onboarding step 7 and tests (Rust: core, store, cloud; Playwright: step 7).

**Acceptance Criteria:**
- Given step 7 on a paired branch, when it opens, then it shows name, phone, role (Manager preselected for the first invite), "Invite", the invite list with status and "Skip for now".
- Given valid details, when "Invite" is pressed, then one `staff.upsert` is queued with only the hash and a 72 h expiry and "Setup code for <name>: XXXX XXXX…" is shown once with Copy.
- Given a login already at the branch, when the Hub applies the command, then it is rejected `LOGIN_TAKEN` and the row shows "Couldn't apply at branch" with the reason.
- Given an unused invite, when "New code" is chosen, then a fresh hash goes out by a new command, the old code stops working on apply, and the new code is shown once.
- Given a Manager invite applied, then step 7 is done; given "Skip for now", then it is skipped.
- Given a full Free licence, when an invite is applied, then the Hub rejects `USER_LIMIT` and the row shows `tier-lock`.

## Verification

- `cargo fmt --check`; clippy for the workspace; `cargo test --workspace --locked`; `pnpm run check:generated`, `check:strings`, `format:check`; onboarding and gallery type checks and Playwright specs.

</frozen-after-approval>

## Design Notes

Decisions: the cloud picks the Hub's `staff_id` (a ULID kept in `staff_invites`) so a re-issue can edit the same member without the Hub reporting ids back; `role_id` in the command is the built-in role name, resolved at the Hub, because role ids are per-branch ULIDs; `skipped` became a third `onboarding_progress` state (table rebuilt) and counts as passed for `current`; the Hub counts every active staff row against `users` (the Owner included), so test fixtures that seed more than three staff raise the cap with `licence.apply`; the property test seeds Owner, Manager and Counter only.

Known limits: an invite for the owner's own phone is only refused at the Hub (`LOGIN_TAKEN`); the plain setup code cannot be shown again once the page is left; the list refreshes every 5 s only while an invite is waiting; the Upgrade link's `href` default is a placeholder until Story 10.5; a `USER_LIMIT` rejection is shown as `tier-lock` but the cloud does not know the licence limit up front.

## Review Triage Log

- Patched: role-by-name resolves built-in roles only; an inactive create takes no licence seat; the wizard no longer auto-advances while the one-time code is on screen (Continue button instead); a failed first list load shows an error; tests for unknown `staff_id` and re-issue at the users limit.
- Deferred: New code offered on applied invites (Hub refuses once a password exists); re-issue overwrites name/role/active on a passwordless member; no duplicate-phone check in the cloud; no expiry bound at the Hub; no `expired` status; TierLock tier fixed to Basic; migration 0012 data-preservation test; proptest no longer seeds Kitchen; HTTP status assertions for LOGIN_TAKEN/USER_LIMIT; rate-limit test.
