---
title: 'Story 2.6: Owner signs in on a new device with TOTP, backup code or SMS fallback'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: '591ffb2651c8628007d3f9ddbf668bad48dc857a'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-4-owner-signs-up-with-phone-otp.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-5-owner-sets-a-password-and-enrols-totp-with-backup-codes.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** An enrolled owner can't sign in again. There's no sign-in, no trusted devices, no lockout and no way to sign out of every session.

**Approach:**
- Sign-in uses phone or email plus password, then a second factor on any browser that isn't trusted: TOTP, a backup code, or an SMS code when the owner has turned that on. The owner can trust a device for 30 days.
- Accounts pause after 5 consecutive failures. Account → Security manages SMS fallback and trusted devices, and has "Sign out everywhere".
- The 2.4 refresh rotation and reuse revocation stay as they are and get covered end to end.

## Boundaries & Constraints

**Always:**
- **Migration** `0004_owner_signin.sql`:
  - `owners` gains `sms_fallback` (INTEGER 0/1, default 0), `failed_signins` (INTEGER, default 0) and `locked_until` (ms, nullable).
  - New `signin_challenges`: id, owner_id, created_at, expires_at (+5 min), attempts, consumed_at, `sms_code_hash` (nullable), `sms_sent_at`.
  - New `owner_devices`: id, owner_id, `token_hash`, label, created_at, last_seen_at, expires_at (+30 d), revoked_at.
  - All SQL lives in `arena_store::control`, over both backends.
- **`POST /v1/signin {login, password}`:**
  - `login` is an E.164 phone (the same normalising as 2.4) or an email (case-insensitive).
  - **Unknown login, wrong password, or no password set:** 401 `AUTH_FAILED`, one fixed message.
    - For an unknown login the server still runs one argon2 verify against a fixed dummy hash, so timing is similar.
    - For a known owner it increments `failed_signins`.
  - **Correct password while `locked_until` is in the future:** 429 `AUTH_LOCKED` plus `Retry-After`. Nothing is revealed before the password is checked, so only a lockable known account can answer `AUTH_LOCKED`.
  - **Correct password and owner `pending`:** a session plus `{next: "account_setup"}`, since setup can still be finished.
  - **Correct password and owner `enrolled`:**
    - If the request carries a valid `arena_trusted` cookie for this owner (hash match, not expired or revoked), it issues a session, updates `last_seen_at`, resets `failed_signins`, and answers `{next: "done"}`.
    - Otherwise it creates a `signin_challenge` and answers 200 `{challenge_id, methods: ["totp","backup_code"] + ["sms" if sms_fallback]}`. No session.
- **`POST /v1/signin/sms {challenge_id}`:**
  - Only when `sms_fallback` is on (else 403 `FORBIDDEN`).
  - Sends a 6-digit code, hashed as in 2.4 (HMAC, pepper), valid 5 min, with the 2.4 per-phone rate limit and a 30 s resend floor.
  - Answers 202 `{resend_after_s}`.
- **`POST /v1/signin/verify {challenge_id, method, code, trust_device, device_label?}`:**
  - **`totp`:** 2.5 verification, including the ±1 step and the replay refusal via `last_step`.
  - **`backup_code`:** checks the normalised code against the owner's unused hashes and marks the match `used_at`. The response includes `backup_codes_remaining`.
  - **`sms`:** the challenge's SMS code.
  - **Failure:** 401 `AUTH_FAILED` (the same message as a wrong password, so the error never says which part failed). It increments `failed_signins` and the challenge `attempts`. 5 attempts void the challenge, and the next verify is `AUTH_FAILED`.
  - **Success:** the challenge is consumed, `failed_signins` is reset, and a session is issued with the 2.4 cookies.
    - When `trust_device` is set, it creates `owner_devices` with a 32-byte token and sets `arena_trusted` (HttpOnly, Secure, SameSite=Lax, `Path=/v1/signin`, Max-Age 30 d).
    - The label is the given `device_label` (≤ 60 chars), or one derived from the User-Agent ("Chrome on Android" style, coarse).
  - Answers `{next: "done", backup_codes_remaining?}`.
- **Lockout:**
  - `failed_signins` counts consecutive failures across password and second factor.
  - Reaching 5 sets `locked_until = now + 15 min` and resets the counter, so the next window starts fresh.
  - While locked, `/signin` with a correct password answers `AUTH_LOCKED` plus `Retry-After`, and so does `/signin/verify`.
  - A successful sign-in resets the counter.
  - Cloudflare per-IP rules for `/v1/signin*` are documented next to the 2.4 rules.
- **Account → Security** (session required; enrolled owners only, else 403 `SETUP_INCOMPLETE`):
  - `PUT /v1/account/security/sms-fallback {enabled}` → 204.
  - `GET /v1/account/devices` → `[{id, label, last_seen_at, created_at, current}]` (non-revoked, unexpired).
  - `DELETE /v1/account/devices/{id}` → revokes one.
  - `POST /v1/account/sign-out-everywhere` → revokes every session family and every trusted device for the owner, and clears this browser's cookies, so the next `/v1/session` is 401.
- **Session extension:** `GET /v1/session` adds `sms_fallback`. The 2.4 refresh rotation and family revocation on reuse stay unchanged, with an end-to-end test across a sign-in.
- **Onboarding app:**
  - **`/sign-in`:** fields for "Phone or email" and Password (2.5 `PasswordField`, `autocomplete="current-password"`), and a primary "Sign in" button.
  - **Challenge step:**
    - TOTP by default (`CodeField`);
    - links "Use a backup code" (one text field, normalising input) and "Text me a code" (only when offered; it shows the 2.4 code step with resend);
    - a checkbox "Trust this device for 30 days".
  - **Messages:** `AUTH_FAILED` shows one plain message. `AUTH_LOCKED` shows "Sign-in is paused. Try again in m:ss." from `Retry-After`, announced once.
  - **After a backup-code sign-in:** if 3 or fewer codes remain, an interstitial "You have N backup codes left" offers "Regenerate now", which goes to the 2.5 security page, or "Later".
  - **Account → Security** (`/account/security`, extending 2.5): an SMS fallback switch (labelled, `aria-checked`), a trusted-devices list with label, "last seen" in the locale format and a "Remove" button, and "Sign out everywhere" with a confirm dialog.
  - 2.4's `{next: "sign_in"}` message now links to `/sign-in`.
  - Every string lives in `en.json`.
- **Logging:** login values only as `phone_tag`, or an email hash tag. Never passwords, codes or tokens.
- **Tests:**
  - **Rust** (`crates/arena-cloud/tests/signin.rs`): every matrix row; timing parity of an unknown login (the argon2 dummy runs, checked through a counter hook rather than wall time); refresh reuse after sign-in; sign-out everywhere; the log scrub.
  - **Playwright** (stubbed API): sign-in, each factor, trust, lockout, the backup-low prompt, the security page, and axe in light and dark at 360 px.

**Never:**
- Telling the user whether the password or the second factor was wrong.
- Issuing a session to an enrolled owner without a second factor, unless a valid trusted-device cookie is present.
- Offering SMS when fallback is off.
- Storing plain tokens or codes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Unknown login | no such owner | 401 `AUTH_FAILED` (same message) | N/A |
| Wrong password | known | 401 `AUTH_FAILED`; `failed_signins`+1 | N/A |
| Ok, untrusted | enrolled, no cookie | challenge, methods `[totp, backup_code]`; no session | N/A |
| Ok, trusted | valid `arena_trusted` | session; no challenge | N/A |
| Trusted expired | cookie > 30 d or revoked | challenge | N/A |
| TOTP ok + trust | valid code, `trust_device` | session + `owner_devices` row + cookie | N/A |
| Backup code | unused code | session; that code used; `backup_codes_remaining: 9` | N/A |
| Backup reused | the same code again (new challenge) | 401 `AUTH_FAILED` | N/A |
| SMS off | `/signin/sms` with fallback off | 403 `FORBIDDEN`; `sms` not in methods | N/A |
| SMS on | fallback on | `sms` in methods; send → verify ok | N/A |
| 5 failures | 5 wrong (mixed password/factor) | locked 15 min; correct password → 429 `AUTH_LOCKED` + `Retry-After` | N/A |
| Lock expiry | after 15 min | sign-in works; counter reset on success | N/A |
| Pending owner | password ok, `pending` | session, `next:"account_setup"` | N/A |
| Sign out everywhere | 2 sessions + 1 trusted device | all revoked; both sessions 401; trusted cookie no longer skips TOTP | N/A |
| Refresh reuse | rotated refresh replayed | family revoked (2.4 behaviour) | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-store/src/control*` + `control-migrations/` -- 2.4 (owners, otp, sessions, rate limits) and 2.5 (password, owner_mfa, backup codes) modules; add 0004 and queries. **Migration number reserved: 0004.**
- `crates/arena-cloud/src/{auth.rs,api/signup.rs,api/session.rs,api/cors.rs,messaging/*}` (2.4) -- cookie/session issuance, HMAC OTP, rate limit, SMS adapter to reuse.
- `crates/arena-cloud/src/{totp.rs,crypto.rs,passwords.rs,api/account.rs,api/mfa.rs,api/gate.rs}` (2.5) -- TOTP verify + replay, argon2 helpers, backup-code normalising, gate extractor.
- `crates/arena-proto/src/{error.rs,signup.rs}` -- reuse `AUTH_FAILED`, `AUTH_LOCKED`, `FORBIDDEN`, `SETUP_INCOMPLETE`; add sign-in DTOs (new `signin.rs`), regenerate TS. **No new error codes needed.**
- `apps/onboarding/src/routes/{+page.svelte,account-setup/,account/security/}`, `src/lib/api.ts`, `tests/` -- extend; add `sign-in/`.
- `packages/ui/src/components/{CodeField,PasswordField,TextField,Button}.svelte`, `messages/en.json` -- reuse.

## Tasks & Acceptance

**Execution:**
- [ ] `crates/arena-store/control-migrations/0004_owner_signin.sql`, store modules + tests.
- [ ] `crates/arena-proto/src/signin.rs`, `lib.rs`, `packages/ui/src/generated/*`.
- [ ] `crates/arena-cloud/src/api/{signin.rs,security.rs}`, `src/useragent.rs` -- endpoints, lockout, trust, devices, sign-out-everywhere.
- [ ] `crates/arena-cloud/tests/signin.rs` -- matrix + scrub.
- [ ] `apps/onboarding/src/routes/sign-in/**`, `account/security/**`, tests, `en.json` -- UI.
- [ ] `deploy/README.md`, `README.md` -- Cloudflare sign-in rules, API docs.

**Acceptance Criteria:**
- Given `--dev` and the onboarding dev server, when an enrolled owner signs in on a fresh browser profile with password + TOTP and ticks trust, then a second sign-in in the same profile skips TOTP, and after "Sign out everywhere" it asks for TOTP again.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`) -- pass (known `arena-hub-ui` sidecar failure excepted); clippy incl. `-p arena-store --features control`; fmt; `scripts/check-sqlite-linkage.sh`.
- `pnpm --filter @arena/onboarding check && pnpm --filter @arena/onboarding build && pnpm --filter @arena/onboarding test`; every `pnpm run check:*`; `pnpm run test:scripts` -- pass.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; on Sonnet). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH: removing a trusted device leaves its sessions alive | high | P | `revoke_owner_device` only sets `revoked_at` on the device |
| 2 | ECH: sign-out-everywhere leaves open challenges | medium | P | not consumed in the txn |
| 3 | ECH: post-commit `?` on the remaining-codes query → 500, code burned | medium | P | after `complete_signin` |
| 4 | BH: a double-submit counts as a failed sign-in | medium | P | the race loser calls `record_failure` |
| 5 | BH: open challenges unbounded per owner | medium | P | one live challenge per owner |
| 6 | BH: backup verify costs ~10 argon2 per attempt | low | R | bounded by the 2.5 semaphore and the lockout |
| 7 | BH/ECH: anyone can lock an owner out | medium | D | the frozen spec defines a per-account lock; a mitigation (trusted-device exemption or per-client counting) needs an intent change |
| 8 | BH: the lock state reveals a correct password | low | R | spec design: the lock is only revealed after the password |
| 9 | BH: no step-up for SMS fallback; no security notifications or audit | low | R | step-up MFA and notifications belong to later epics (Epic 8) |
| 10 | BH: static trusted token; not revoked on password change | low | R | no password change exists yet |
| 11 | ECH: non-unique email resolves to another owner | medium | P | `owner_by_email` picks by preference; refuse ambiguity |
| 12 | ECH: duplicate trusted-device rows | low | P | refresh the presented device |
| 13 | ECH: unbounded input lengths | low | P | direct caps |
| 14 | ECH: leading-0 domestic numbers | low | P | direct parse fix |
| 15 | ECH: UI gaps (resend wait, dead challenge, malformed reply, remove double-click/current) | low–medium | P | direct UI fixes |
| 16 | ECH: iPad labelled macOS | low | R | cosmetic |
| 17 | ECH: lock-set race leaves the decision unlogged | low | R | the outcome is still a generic failure; the lock still applies |
| 18 | ECH: sign-in normalising is more lenient than sign-up | low | R | lenient login input is intended UX |
| 19 | ECH: TOTP limit and account lock have separate clocks | low | R | both answer `AUTH_LOCKED` with their own `Retry-After` |
| 20 | ECH: a pending owner can't sign out everywhere | low | R | pending owners have no trusted devices |
| 21 | VG: SMS send-failure path untested | medium | P | no `set_failing` in signin tests |
| 22 | VG: SMS expiry and wrong-SMS counting untested; conditional assertion | medium | P | add the tests |
