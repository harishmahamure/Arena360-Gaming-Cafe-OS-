---
title: 'Story 2.5: Owner sets a password and enrols TOTP with backup codes'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: 'bd0a36e898b907988b96cc143f5f66e6908dd7af'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-4-owner-signs-up-with-phone-otp.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** After sign-up, an owner's account is protected only by their phone. There's no password, no second factor and no recovery path.

**Approach:**
- Account setup makes the owner set a password, then enrol an authenticator app (TOTP, shown as a QR plus copyable text), then save 10 single-use backup codes before any wizard step opens.
- Account → Security can regenerate the backup codes once the owner confirms with a current TOTP code.

## Boundaries & Constraints

**Always:**
- **Migration** `0003_owner_mfa.sql`:
  - `owners` gains `password_hash TEXT` (nullable) and `password_set_at`.
  - New `owner_mfa`: owner_id PK, `secret_ct` BLOB, `secret_nonce` BLOB, created_at, `confirmed_at`, `last_step` INTEGER, `backup_ack_at`.
  - New `owner_backup_codes`: id, owner_id, `code_hash`, created_at, `used_at`, plus an index on owner_id.
  - All SQL lives in `arena_store::control`, over both backends.
- **Password:**
  - At least 10 characters (Unicode scalar count) and at most 128.
  - Rejected when its lowercase form is in a bundled common-password list.
  - Hashed with argon2id (the workspace `argon2` pin, OWASP parameters m=19 MiB, t=2, p=1, PHC string).
  - It can be set only once here, while `password_hash IS NULL`. Changing a password is not in this story.
  - **New codes:** `PASSWORD_TOO_SHORT`, `PASSWORD_COMMON` (422, `field:"password"`).
- **Common-password list:**
  - `crates/arena-cloud/data/common-passwords.txt`: the 10,000 most common passwords, lowercased, one per line.
  - Taken from an MIT- or CC0-licensed dictionary package on the npm registry (for example `@zxcvbn-ts/language-common`, pinned), never fetched from GitHub. Its provenance and licence go in a header comment and the README pin table.
  - Loaded once, with `include_str!`, into a `HashSet`.
- **TOTP:**
  - RFC 6238 with SHA-1, 6 digits and a 30 s step, accepting ±1 step.
  - Implemented with `ring` (pin `=0.17.14`, already in the lockfile; `hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY`) and the RFC 6238 test vectors as unit tests. No TOTP crate.
  - The secret is 20 random bytes (`ring::rand::SystemRandom`), shown as RFC 4648 base32 without padding, in groups of 4 for display.
  - It's encrypted at rest with AES-256-GCM (`ring::aead`) under `ARENA_MFA_KEY`: 32 bytes, base64, a secret env var, with a fixed dev key in `--dev`. The owner id is the AAD.
  - A code whose step is ≤ `last_step` is refused (no replay), and `last_step` is updated when a code is accepted.
  - **New code:** `TOTP_INVALID` (400, `field:"code"`).
- **QR:** the server renders `otpauth://totp/Arena%20360:<phone_e164>?secret=…&issuer=Arena%20360&algorithm=SHA1&digits=6&period=30` as an SVG string with the `qrcode` crate (pinned, SVG renderer only, no image crate). The UI shows it as an `<img>` with alt text and the secret text beside it, plus a Copy button.
- **Backup codes:**
  - 10 codes of 10 Crockford-base32 characters, shown as `XXXXX-XXXXX` and stored only as argon2id hashes. They're normalised on input (uppercase, hyphens and spaces stripped, I/L→1, O→0).
  - They're shown once, in the confirm (or regenerate) response, and never again.
  - Regenerating deletes every old row, then inserts 10 new ones in one transaction.
- **API** (each call needs a session from 2.4; DTOs in `arena-proto`, generated to TypeScript):
  - `POST /v1/account/password {password}` → 204. It answers 409 `CONFLICT`-class `ALREADY_SET` when a password already exists; add `ALREADY_SET`.
  - `POST /v1/account/mfa/totp/start` → `{secret, otpauth_uri, qr_svg}`.
    - It requires a password (else 403 `PASSWORD_REQUIRED`; add the code) and `mfa_state = pending`.
    - Every call mints a **new** secret and replaces the unconfirmed one, which is how "leave and come back gets a new secret" works.
  - `POST /v1/account/mfa/totp/confirm {code}`: on a valid code, `mfa_state = enrolled`, `confirmed_at` is set, and it returns `{backup_codes:[10]}`; else `TOTP_INVALID` and the state stays pending.
  - `POST /v1/account/mfa/backup-codes/ack` → 204, sets `backup_ack_at`.
  - `POST /v1/account/mfa/backup-codes/regenerate {code}` (enrolled only, needs a valid TOTP that isn't a replay) → `{backup_codes:[10]}`.
  - `GET /v1/session` now returns `owner: {name, mfa_state, has_password, backup_codes_acked}`.
- **Gate:** `crates/arena-cloud/src/api/` gets a reusable `RequireSetupComplete` extractor (has a password, `enrolled`, acked), failing with 403 `SETUP_INCOMPLETE` (add the code). The wizard routes in 2.7 use it; a test route under `--dev` proves it.
- **Onboarding app**, under `/account-setup`, with steps chosen from `GET /v1/session`:
  1. **Password:**
     - a new shared `PasswordField` in `packages/ui`: a TextField with a Show/Hide toggle button that has an `aria-pressed` state and an accessible name;
     - `autocomplete="new-password"`, paste allowed;
     - a hint listing the rule;
     - errors mapped from the codes.
  2. **TOTP:** the QR, the grouped secret with Copy, and a 6-cell `CodeField`. `TOTP_INVALID` gives an inline error and clears the cells.
  3. **Backup codes:**
     - the 10 codes as a list;
     - Copy (all codes), and Download `.txt` (a Blob named `arena-360-backup-codes.txt`);
     - a required checkbox "I've saved my backup codes" that enables Continue, which acks;
     - this step is shown only right after confirm or regenerate, because the codes are never re-fetchable.
  4. **Done:** "Setup wizard starts here" until 2.7.
  - Returning while pending with a password resumes at TOTP with a fresh `start`.
  - Returning enrolled but not acked shows an explanation and a "Regenerate backup codes" path, because the codes can't be shown again.
  - **`/account/security`:** "Regenerate backup codes" opens a dialog that asks for a current TOTP code, then shows the new codes as in step 3. Shown only to enrolled owners.
  - Every string goes in `en.json`.
- **Logging:** never passwords, secrets, codes or the otpauth URI.
- **Tests:**
  - **Rust** (`crates/arena-cloud/tests/mfa.rs`, in-process, local backend): every matrix row, the RFC 6238 vectors, replay refusal, the AES-GCM round trip and a wrong key, the gate extractor, and a log scrub covering the password and the codes.
  - **Playwright** (stubbed API, extending 2.4's suite): every step, the show/hide toggle, copy and download, the ack gate, the resume behaviour, regenerate, and axe in light and dark at 360 px.

**Never:**
- Storing a plain password, TOTP secret or backup code.
- Showing backup codes after their one response.
- Opening any wizard route without a completed setup.
- Accepting a replayed TOTP step.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Short password | 9 chars | 422 `PASSWORD_TOO_SHORT` | N/A |
| Common password | `password123` | 422 `PASSWORD_COMMON` | N/A |
| Set password | valid, none set | 204; argon2id PHC stored | N/A |
| Set again | already set | 409 `ALREADY_SET` | N/A |
| TOTP before password | no password | 403 `PASSWORD_REQUIRED` | N/A |
| Start twice | two starts | the second secret replaces the first; the first's codes fail | N/A |
| Confirm ok | current code | `enrolled`, 10 codes, hashes stored | N/A |
| Confirm ±1 step | the previous step's code | accepted | N/A |
| Confirm wrong | wrong code | `TOTP_INVALID`, still pending | N/A |
| Replay | the same step again (regenerate) | `TOTP_INVALID` | N/A |
| Regenerate | valid code | old codes deleted, 10 new | N/A |
| Gate | enrolled, not acked | `SETUP_INCOMPLETE` | N/A |
| Gate ok | password + enrolled + acked | passes | N/A |
| Wrong MFA key | DB secret under another key | decrypt fails → 500 `INTERNAL`, logged | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-store/src/control*.rs` + `control-migrations/` -- the 2.4 owner/session modules and migration 0002; add 0003 and typed queries.
- `crates/arena-proto/src/error.rs`, `crates/arena-proto/src/signup.rs` (2.4 DTOs) -- add codes + account/MFA DTOs; regenerate TS.
- `crates/arena-cloud/src/api/{signup.rs,session.rs}` (2.4) -- session extractor and cookie handling to reuse; add `account.rs`, `mfa.rs`, `gate.rs`; `src/config.rs` adds `ARENA_MFA_KEY`.
- `crates/arena-cloud/tests/signup.rs` (2.4) -- helpers for an in-process app with a signed-in owner.
- `packages/ui/src/components/{TextField.svelte,CodeField.svelte}` (2.4) -- base for `PasswordField`; `messages/en.json`.
- `apps/onboarding/src/routes/account-setup/` (2.4 placeholder), `apps/onboarding/src/lib/api.ts`, `apps/onboarding/tests/` -- extend.
- `Cargo.toml` + README pin table -- `ring` direct pin, `qrcode` (default-features off, svg), data-file provenance row.
- `deploy/README.md`, `deploy/secrets/README.md`, `deploy/k8s/base/deployment.yaml` -- `ARENA_MFA_KEY` secret ref (+ `check:deploy` if it lists env).

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store/control-migrations/0003_owner_mfa.sql`, store modules + tests.
- [x] `crates/arena-proto/src/*`, `packages/ui/src/generated/*` -- codes + DTOs.
- [x] `crates/arena-cloud/src/{totp.rs,crypto.rs,passwords.rs,api/account.rs,api/mfa.rs,api/gate.rs,config.rs}`, `data/common-passwords.txt` -- logic + endpoints.
- [x] `crates/arena-cloud/tests/mfa.rs` -- matrix + vectors + scrub.
- [x] `packages/ui/src/components/PasswordField.svelte`, `index.js`, `messages/en.json` -- shared field.
- [x] `apps/onboarding/src/routes/account-setup/**`, `apps/onboarding/src/routes/account/security/**`, tests -- UI flow.
- [x] `deploy/**`, `README.md`, `Cargo.toml` -- secret, pins, docs.

**Acceptance Criteria:**
- Given `--dev` and the onboarding dev server, when a new owner signs up, sets a password, scans the QR with an authenticator (or uses the secret) and saves the codes, then `GET /v1/session` reports `enrolled` with `backup_codes_acked: true` and the dev gate route passes.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`) -- pass (known `arena-hub-ui` sidecar failure excepted); clippy incl. `-p arena-store --features control`; fmt; `scripts/check-sqlite-linkage.sh`.
- `pnpm --filter @arena/onboarding check && pnpm --filter @arena/onboarding build && pnpm --filter @arena/onboarding test` -- pass.
- every `pnpm run check:*`, `pnpm run test:scripts` -- pass.

## Implementation Notes

- **Store:** `control/mfa.rs`. `start_owner_mfa` is one guarded upsert (`INSERT … SELECT … WHERE` the owner is `pending` with a password, `ON CONFLICT DO UPDATE … WHERE confirmed_at IS NULL`, clearing `last_step`). `confirm_owner_mfa` binds the `secret_nonce` the code was checked against, so a start that replaced the secret meanwhile makes it fail; it sets `confirmed_at`/`last_step`, flips the owner to `enrolled` and replaces the codes in one transaction. Every code-accepting write requires `step > last_step` in the same statement. `Owner` gained `password_set_at` (the hash is read only via `owner_password_hash`). Tested on both backends (`tests/control_mfa.rs`).
- **Cloud:** `totp.rs` (RFC 6238/4226 on `ring`, base32, otpauth URI, `qrcode` SVG), `crypto.rs` (`MfaKey`, AES-256-GCM, random 96-bit nonce per start, owner id as AAD; a small base64 decoder instead of a new crate), `passwords.rs` (rule, `include_str!` list parsed once into a `HashSet`, argon2id m=19456/t=2/p=1 explicit, backup codes). `api/session.rs` now exposes the `SignedIn` extractor; `api/gate.rs` `RequireSetupComplete`; `GET /v1/dev/setup-gate` under `--dev`. Hashing runs on the blocking pool. Secret/code answers carry `Cache-Control: no-store`.
- **Decisions:** a password over 128 is 422 `VALIDATION_FAILED` (`field: password`); start/confirm for an `enrolled` owner are 409 `ALREADY_SET`; ack/regenerate for a non-enrolled owner are 403 `FORBIDDEN`; a malformed TOTP code is `TOTP_INVALID` (not `VALIDATION_FAILED`). The `+` of the phone is percent-encoded (`%2B`) in the otpauth label. Regenerating keeps `backup_ack_at`. `ARENA_MFA_KEY` sits with the other `arena-cloud-secrets` refs in the Deployment. The data file's provenance is checked by `check:pins` (a reader for its `# source:` line).
- **Frontend:** `PasswordField` (toggle text and `aria-pressed` both change), app-local `BackupCodesPanel` and `TotpSetup`; the saved-codes checkbox is a 44 px transparent input over a drawn box. The regenerate dialog is a native `<dialog>`; Escape is blocked while new codes are shown.
- **Verified:** `arena-cloud --role all --dev` driven over HTTP with an independent TOTP (Python stdlib): sign-up → password → start → confirm → ack → session `enrolled` + `backup_codes_acked: true`, `/v1/dev/setup-gate` 200, a replayed step refused, no secret in the log. `check:generated` lists the new generated files until committed.


## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; on Sonnet). P = patch, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/VG/ECH: no per-owner attempt limit on TOTP confirm/regenerate | high | P | `api/mfa.rs` has no counter; a stolen session can brute-force |
| 2 | VG/ECH: regenerate keeps `backup_ack_at`; ack without codes | medium | P | `regenerate_backup_codes` leaves the ack; ack has no codes check |
| 3 | BH: unbounded concurrent argon2 (19 MiB each) | medium | P | `spawn_blocking` with no limiter |
| 4 | BH: argon2 for backup codes is heavy; HMAC would do | low | R | the frozen spec requires argon2id |
| 5 | BH: nothing consumes a backup code | false | R | consumption is Story 2.6 |
| 6 | BH: accepting `now+1` stores `last_step` and blocks the current-step code for up to 30 s | low | R | ±1 window per the spec; impact ≤ 30 s |
| 7 | BH/VG: non-constant-time compare | low | P | direct fix with `ring::constant_time` |
| 8 | BH/ECH: whitespace-only password; no NFC; no personal-info check | low | P (trim) / R (NFC needs a new crate; personal info not in the spec) | |
| 9 | BH: extra DB round trips per request | low | R | performance only |
| 10 | BH/ECH: base64 accepts extra padding | low | P | direct fix |
| 11 | BH: no key rotation/version; no dev-key refusal test | low | R | documented limitation |
| 12 | ECH: security page uses `getSession` | medium | P | expired access cookie bounces to `/` |
| 13 | ECH: 401 mid-flow on setPassword/start/confirm/ack/regenerate | medium | P | no refresh-and-retry |
| 14 | ECH: no retry control after a failed `startTotp` | low | P | direct UI fix |
| 15 | ECH: repeated Escape closes the dialog with unsaved codes | low | P | old codes are already deleted |
| 16 | ECH: refresh `info()` failure after rotation → 500 | low | P | fallback |
| 17 | ECH: two-tab start invalidates the first QR | low | R | rare; confirm fails visibly with `TOTP_INVALID` |
| 18 | ECH: gate reads owner and owner_mfa in two queries | low | R | momentary inconsistency only |
| 19 | BH: no password reset, re-enrol, notification or audit | low | R | outside the story's scope |
| 20 | ECH: lost confirm response → wait for a fresh step to regenerate | low | R | the unacked path recovers within 30–60 s |
