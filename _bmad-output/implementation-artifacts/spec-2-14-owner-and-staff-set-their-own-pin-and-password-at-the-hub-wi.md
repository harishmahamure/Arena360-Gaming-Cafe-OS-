---
title: 'Story 2.14: Owner and staff set their own PIN and password at the Hub with a one-time setup code'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '5b177e5'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-13-branch-bootstrap-applies-the-onboarding-profile-at-first-pai.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-11-staff-sign-in-to-the-hub-with-roles-and-permissions.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The owner's staff row (Story 2.13) has a setup-code hash and no password or PIN, and nothing at the Hub lets anyone use the code. Staff invited later have no way to choose their own credentials.

**Approach:**
- The Hub sign-in screen gets "First time here? Use a setup code": login (phone), the 8-character code, then a password and a PIN entered twice. One loopback call, `POST /ui/auth/setup-code`, checks the code, hashes the secrets and submits the new intent `staff.set_credentials`, then signs the person in.
- `staff.set_credentials` is the only writer of a first password and PIN from a code. It runs in one transaction that re-checks the stored hash, the expiry on trusted time and that no password exists, so a code works once even under concurrent submissions.
- The verifier push to kiosks is the existing Story 1.20 fan-out: the change touches `staff`, so a changed manager verifier is pushed at once.

## Boundaries & Constraints

**Always:**
- **No migration.** `staff.setup_code_hash` and `setup_expires_at` exist (Hub 0018). Hub migrations on master are now up to 0019 (Story 5.4).
- **Intent** `staff.set_credentials` (category Staff, requirement `System`), payload `StaffSetCredentials{staff_id, setup_code_hash, trusted_now_ms, password_hash, pin_hash}`. The permission step also accepts a `staff` actor whose id equals `payload.staff_id` (the member acts as themself, never elevated). Owner, device, admin and other staff are `PERMISSION_DENIED`. `/ui/intent` refuses the kind (`UNSUPPORTED`); only the setup-code route builds it.
- **apply()**: the member must exist, be active, have no `password_hash`, hold `setup_code_hash` equal (constant-time compare) to the payload's, and `setup_expires_at > trusted_now_ms`; any failure, including an unknown id, is the one `SETUP_CODE_INVALID` (new error code, 401) with a fixed message. Hashes must be argon2id PHC strings (`VALIDATION_FAILED`). Mutation stores both hashes, bumps `pin_verifier_version`, clears `setup_code_hash` and `setup_expires_at`; one `staff` update change row with `actor{staff, id = self}`. The verifier history refreshes in the same transaction.
- **Route** `POST /ui/auth/setup-code {login, code, password, pin}` (no bearer):
  1. A follower answers `NOT_PRIMARY` before anything else.
  2. Per-login attempt throttle (the sign-in `Lockout`, own instance, keyed by login without case): 5 consecutive failures pause that login for 60 s, `AUTH_LOCKED` with `retry_after_s`; attempts in flight count.
  3. `StaffDirectory::check_setup_code` normalises the code (spaces, `-`, `_` stripped, upper-cased), always does exactly one argon2id verification (a dummy hash for an unknown login, an inactive or credentialed member, an expired or absent code, or a malformed code) and answers `(staff_id, stored hash)` or nothing. Expiry is judged on trusted time (`HubStatus::trusted_now`: cloud time plus monotonic time, the wall clock only before any cloud time is known), never the wall clock alone. At most 2 such checks run at once (the PIN-check semaphore).
  4. A refused code is one `SETUP_CODE_INVALID` line, identical for wrong, used, expired, unknown and malformed, and counts as a failure.
  5. Only after a valid code: the password is 10–128 characters (`PASSWORD_TOO_SHORT` / `VALIDATION_FAILED` on `password`), the PIN 4–6 digits (`VALIDATION_FAILED` on `pin`). Those refusals do not count as failures and do not spend the code.
  6. Both secrets are hashed with argon2id outside the transaction, the intent is submitted, a racing loser (`SETUP_CODE_INVALID` from apply) counts as a failure, success resets the count, a session is opened exactly as for sign-in and `SignInOk` is returned.
- The PIN, password and code are never logged, stored or put in a change row, a request `Debug` or an error. Change rows hold only the argon2id hashes.
- **UI**: sign-in gets a secondary button "First time here? Use a setup code" that swaps in the setup form: step 1 login (phone) and code ("Enter your setup code, then choose your PIN and password."), step 2 password, PIN and PIN again. Shape errors are caught client-side; a refused code returns to step 1 with one line; a pause shows the same countdown as sign-in. All strings in `en.json`.
- **Tests**: apply matrix; route matrix (success, one-time, uniform refusal, 5-failure pause and 60 s release, policy not spending the code, trusted-time expiry both ways, `/ui/intent` refusal, no secret in the change row); LAN verifier push; Hub Playwright incl. axe.

**Never:**
- A second way to set a first password from a code, or a code that works twice.
- A response that distinguishes wrong, used, expired or unknown codes, or that does less hashing for one of them.
- Judging expiry on the wall clock when a trusted time exists.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Valid code | unexpired hash, no password | hashes stored, code cleared, `pin_verifier_version + 1`, signed in | N/A |
| Same code again | after success | `SETUP_CODE_INVALID` | counts as a failure |
| Wrong / unknown login / malformed | any | the same `SETUP_CODE_INVALID` line, one argon2id check | counts as a failure |
| Expired | `setup_expires_at <= trusted_now` | `SETUP_CODE_INVALID` | counts as a failure |
| Credentials already set | code present, password set | `SETUP_CODE_INVALID` | N/A |
| 5 failures for a login | 6th attempt, right code | `AUTH_LOCKED retry_after_s 60` (counting down) | works again after 60 s |
| Weak password / PIN | valid code | 422 on `password` / `pin`, code still valid | not a failure |
| Manager or Owner sets a PIN | commit | new verifier version pushed to every connected kiosk | N/A |
| Follower Hub | any | `NOT_PRIMARY` | N/A |
| `/ui/intent` with the kind | signed in | `UNSUPPORTED` | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/{intent,error,ui_auth,lib}.rs`; `packages/ui/src/generated` (regenerated).
- `crates/arena-store/src/staff.rs` (`StaffRow.setup_*`, `set_staff_credentials`).
- `crates/arena-core/src/apply/staff_credentials.rs` (new) with `mod/guards/validate/mutate`, `rbac.rs`, `staff.rs` (`check_setup_code`, `check_new_credentials`, `setup_code_invalid`), `status.rs` (`trusted_now`).
- `crates/arena-api/src/ui_auth.rs` (`setup_code`, `open_session`, `/ui/intent` refusal), `ui_setup_tests.rs`, `tests/lan.rs`; `crates/arena-cloud/src/api/error.rs`, `crates/arena-kiosk-core/src/queue.rs` (new code in total matches).
- `apps/hub/src/lib/{auth.ts,SetupCode.svelte}`, `routes/sign-in/+page.svelte`, `tests/setup-code.spec.ts`, `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

- [x] proto, store, core, api.
- [x] Hub sign-in UI.
- [x] tests.

**Acceptance Criteria:** the Given/When/Then of Story 2.14 in `epics.md`, as mapped by the matrix above.

## Verification

`cargo test --workspace --locked`; `cargo test -p arena-hub -p arena-api --features dev-fixtures`; clippy with dev-fixtures `-D warnings`; `cargo fmt --check`; every `pnpm run check:*`; `pnpm run test:scripts`; prettier; Hub `test:shell` and gallery `test:a11y` (unique `ARENA_PW_PORT`); app builds except marketing; `pnpm check:generated` after the commit.

## Accepted limits

- A PIN or password changed later by the member is not built here (the intent only serves a first set from a code); a Hub UI for it and Story 2.16's reissue (which must clear the member's credentials to let a code work again) are later.
- Online guessing is bounded per login (5 per 60 s); there is no global limit across logins. The code is 40 bits and lives 72 h.
- The code is checked at the final submit of the form, not when the first step is "Continue"; there is no separate verify call to rate-limit or to act as an oracle.
- The setup-code throttle is in memory and resets when the Hub restarts.
- `trusted_now_ms` in the payload is set by the Hub's own route; the kind is refused on every client path.

## Review Triage Log

(pending)
