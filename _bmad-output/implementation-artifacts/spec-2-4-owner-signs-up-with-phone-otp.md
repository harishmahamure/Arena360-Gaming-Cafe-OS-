---
title: 'Story 2.4: Owner signs up with phone OTP'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: 'bc1fd92535e6de79f1f18d65ee49df79b61e7031'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-2-cloud-error-envelope-local-dev-mode-and-single-sqlite-linkag.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-3-control-data-is-backed-up-nightly-and-the-cloud-is-observabl.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** There's no way for a café owner to create an account. The onboarding app is a placeholder, and the cloud has no owners, OTP or sessions.

**Approach:**
- The onboarding SvelteKit app gets a "Get started" form and a 6-cell code step, plus a shared footer `theme-toggle`.
- The cloud gets `/v1/signup/*`:
  - an OTP challenge (hashed, 6 digits, 5 min, 5 attempts) sent through a messaging adapter (Gupshup plus a recorded fake);
  - per-phone rate limits in `rate_limits`;
  - on success, an `owners` row and a rotating cookie session.
- It never reveals whether a phone already has an account.

## Boundaries & Constraints

**Always:**
- **Migration** `crates/arena-store/control-migrations/0002_owner_signup.sql` creates four tables. Enums are TEXT with a CHECK constraint, times are UTC ms, ids are ULID.
  - `owners`: id, name, phone_e164 UNIQUE, email, cafe_name, city, `mfa_state` ∈ {pending, enrolled}, created_at.
  - `otp_challenges`: id, purpose ∈ {signup}, phone_e164, code_hash, payload_json (the submitted form), created_at, expires_at, attempts, consumed_at.
  - `owner_sessions`: id, owner_id, family_id, access_hash, access_expires_at, refresh_hash, refresh_expires_at, rotated_at, revoked_at, created_at.
  - `rate_limits`: key, window_start, count, PRIMARY KEY(key, window_start).
- **Store:** typed functions in `arena_store::control` (owner, otp, session and rate-limit modules) over both backends. All SQL lives here.
- **OTP:**
  - The code is 6 digits from the OS CSPRNG.
  - `code_hash` = HMAC-SHA256(`ARENA_OTP_PEPPER`, challenge_id ‖ code), compared in constant time. In `--dev` a fixed dev pepper is used.
  - A challenge expires after 5 min. 5 wrong attempts void it. A consumed or void challenge never verifies.
- **Rate limit:**
  - At most 5 sends per phone per rolling hour (sum over minute buckets in `rate_limits`; buckets older than 2 h are pruned on write).
  - The 6th send gets 429 `RATE_LIMITED` with a `Retry-After` header (seconds until the oldest counted send leaves the hour). Nothing is sent.
  - Resend is allowed 30 s after the previous send of the same challenge (the server enforces it with `RATE_LIMITED` plus `Retry-After`).
  - Per-IP limits are Cloudflare rules, documented in `deploy/README.md` (signup start, verify, resend).
- **New codes** in `arena_proto::ErrorCode` (regenerate TS): `RATE_LIMITED` (429), `OTP_INVALID` (400, wrong code, attempts left), `OTP_EXPIRED` (400, expired, void or consumed: request a new code).
- **API** (DTOs in `arena-proto`, ts-rs exported; every error uses the 2.2 envelope):
  - `POST /v1/signup/start {name, phone, email, cafe_name, city}`:
    - validates first: name, café and city 1–80 characters after trim; phone E.164 (`+` and 8–15 digits; `+91` needs 10 digits starting 6–9); email is a pragmatic `local@domain.tld` within 254 characters. A failure is 422 `VALIDATION_FAILED` with `field`.
    - On success it creates the challenge and sends `"<code> is your Arena 360 code. It expires in 5 minutes."`, then answers 202 `{challenge_id, expires_in_s: 300, resend_after_s: 30}`. The response is identical whether or not the phone already has an owner.
  - `POST /v1/signup/resend {challenge_id}`: issues a new code on the same challenge, resetting expiry and attempts, and counts against the phone limit.
  - `POST /v1/signup/verify {challenge_id, code}`:
    - **wrong code:** `OTP_INVALID`; on the 5th, `OTP_EXPIRED`.
    - **correct code:**
      - The challenge is consumed atomically with the owner insert and the session creation.
      - **New phone:** insert `owners` (`mfa_state = pending`), then 200 `{next: "account_setup"}` plus cookies.
      - **Existing owner, `pending`:** a session for that owner with `{next: "account_setup"}`. No duplicate row, and the stored details are not overwritten.
      - **Existing owner, `enrolled`:** **no session**, `{next: "sign_in"}`, because sign-in with a second factor is Story 2.6. The OTP alone never bypasses TOTP.
  - `GET /v1/session`: answers `{owner: {name, mfa_state}}`, or 401 `AUTH_FAILED`.
  - `POST /v1/session/refresh`:
    - rotates the refresh token: it issues new access and refresh tokens and marks the old refresh `rotated_at`;
    - **reuse:** presenting an already-rotated refresh token revokes the whole family and returns 401 `AUTH_FAILED`.
- **Cookies:**
  - `arena_access`: an opaque 32-byte token, stored hashed (SHA-256), 15 min, `Path=/v1`.
  - `arena_refresh`: 30 days, `Path=/v1/session/refresh`.
  - Both are `HttpOnly; Secure; SameSite=Lax` and host-only on the API host.
- **CORS:** `ARENA_WEB_ORIGINS` is an exact-match origin list (`--dev` defaults to `http://localhost:5179`).
  - The cloud answers preflight requests and adds `Access-Control-Allow-Credentials: true` for listed origins only.
  - State-changing `/v1` requests with an `Origin` that isn't listed get 403 `FORBIDDEN`.
- **Messaging adapter** (`crates/arena-cloud/src/messaging/`): `trait Sms { async fn send(&self, to_e164, text) -> Result<(), SmsError> }`, chosen by `ARENA_SMS_PROVIDER` ∈ {fake, gupshup}. `--dev` forces fake.
  - **Fake:** records `{to, text, at}` in memory. Under `--dev`, `GET /v1/dev/sms` returns the recorded messages for tests and local use.
  - **Gupshup:** an Enterprise SMS HTTP API client over `reqwest`. Its config comes from `ARENA_GUPSHUP_*` (endpoint, userid, password, sender, DLT template id), and the secrets are never logged.
    - It's covered by a unit test against a local HTTP stub that asserts the request shape.
    - The real provider is not called; a live check is an operator step.
  - A send failure answers 503 `INTERNAL`, keeps the challenge, and doesn't count against the limit.
- **Logging:** a phone appears in logs only as `phone_tag` (the last 2 digits plus the first 8 hex characters of SHA-256 with the pepper). Never the email, name, code or tokens. A test runs the signup flow with a captured tracing subscriber and asserts that none of the submitted values or the code appear.
- **Onboarding app** (`apps/onboarding`):
  - Routes:
    - `/` "Get started": an `h1`, the fields name, phone (a country-code select defaulting to +91, plus the number), email, café name and city, and one primary "Send code" button;
    - the code step on the same route;
    - `/account-setup`: a placeholder "Account setup continues here", gated by `GET /v1/session`, which redirects to `/` on 401.
  - Validation runs on submit and on blur. Focus moves to the first invalid field (`focusFirstInvalid`), and errors are announced by `TextField`. Nothing is sent while the form is invalid. Server `VALIDATION_FAILED.field` maps to that field.
  - **Code step:**
    - A new shared `CodeField` in `packages/ui`: 6 visible digit cells, `inputmode="numeric"`, `autocomplete="one-time-code"`, paste of 6 digits fills every cell, auto-submits when full, cells clear on `OTP_INVALID`.
    - Errors are inline and plain.
    - "Resend code" is disabled with a countdown until `resend_after_s` passes, announced at 30 s and 0, not every tick.
    - `OTP_EXPIRED` shows "This code no longer works. Send a new code." with the resend enabled.
    - `RATE_LIMITED` shows "Too many codes requested. Try again in m:ss." from `Retry-After`.
    - `{next: "sign_in"}` shows "You already have an account. Sign in to continue." (the sign-in link target lands in 2.6, so for now it goes to `/`).
  - **Shared `ThemeToggle`** in `packages/ui`: built on `TabControl`, System · Light · Dark, System by default. System tracks `prefers-color-scheme` live, the choice is stored per device in `localStorage` (try/catch), and it sets `data-theme` on `<html>` before first paint via a small inline script in `app.html`. It's placed in the onboarding footer.
  - **API client:** `apps/onboarding/src/lib/api.ts` calls `fetch(`${PUBLIC_API_BASE}/v1/...`, {credentials: "include"})`, where `PUBLIC_API_BASE` defaults to `""`. The Vite dev and preview proxy sends `/v1` to `http://127.0.0.1:8080`. It decodes the `AppError` envelope with the generated types.
  - Every string lives in `packages/ui/src/messages/en.json` (`check:strings`).
- **Hosting:** production onboarding is at `start.arena360.cloud` (staging `start.staging.arena360.cloud`), recorded in the README and in the CORS docs.
- **Tests:**
  - **Rust** (`crates/arena-cloud/tests/signup.rs`, in-process, local backend, fake SMS): every matrix row, plus cookie attributes, refresh rotation and reuse revocation, CORS allow and deny, and the log scrub.
  - **Playwright** (`apps/onboarding/tests/`, with the API stubbed by `page.route`, the hub pattern):
    - the form validation and focus;
    - the code step: paste, a wrong code clearing the cells, the expired state, the rate-limited message, the resend countdown, landing on `/account-setup`;
    - the theme toggle persisting;
    - axe in light and dark: landmarks, one `h1`, labelled fields, targets ≥ 44 px, zero serious or critical issues;
    - a 360 px phone project.
  - Add the onboarding app to the CI a11y job.

**Never:**
- Creating an `owners` row before a correct code.
- Revealing whether a phone exists (start and resend answer the same either way).
- Issuing a session to an enrolled owner from an OTP alone.
- Storing a plain code or token.
- PII in logs.
- Calling the real Gupshup in tests or CI.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Invalid field | phone `+91123` | 422 `VALIDATION_FAILED` `field:"phone"`; no SMS | N/A |
| Start | valid new phone | 202; 1 fake SMS with a 6-digit code; no `owners` row | N/A |
| Verify ok (new) | correct code ≤5 min | owner `pending`, challenge consumed, 2 cookies, `next:"account_setup"` | N/A |
| Wrong code | 1st–4th wrong | `OTP_INVALID` | N/A |
| 5th wrong | 5th wrong | `OTP_EXPIRED`; the correct code is refused afterwards | N/A |
| Expired | correct code after 5 min | `OTP_EXPIRED` | N/A |
| Reuse consumed | verify again | `OTP_EXPIRED` | N/A |
| Existing pending | phone of a pending owner | same 202; verify → session for that owner, row count unchanged | N/A |
| Existing enrolled | phone of an enrolled owner | same 202; verify → `next:"sign_in"`, no cookies | N/A |
| 6th send/hour | 6th start or resend for a phone | 429 `RATE_LIMITED`, `Retry-After` > 0, no SMS | N/A |
| Early resend | resend < 30 s | 429 `RATE_LIMITED` with `Retry-After` ≤ 30 | N/A |
| SMS down | provider fails | 503 `INTERNAL`; not counted | logged with `phone_tag` |
| Refresh | valid refresh | new pair; the old refresh marked rotated | N/A |
| Refresh reuse | rotated refresh again | 401 `AUTH_FAILED`; family revoked (new access fails) | N/A |
| Bad origin | POST with an unlisted Origin | 403 `FORBIDDEN` | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-store/src/control.rs` + `control-migrations/` -- `ControlDb` (remote libsql + local rusqlite), shared runner; add `0002_owner_signup.sql` and typed query modules (keep SQL in arena-store; layer rules).
- `crates/arena-proto/src/error.rs` -- `ErrorCode` (+ `as_str`); add codes; DTOs in a new `crates/arena-proto/src/signup.rs` with ts-rs export; regen `packages/ui/src/generated` (`pnpm gen:types`, `check:generated`).
- `crates/arena-cloud/src/api/` (Story 2.2) -- `ApiError`, `ApiJson`, envelope layer, dev routes (mounted only with `--dev`); add `signup.rs`, `session.rs`, `cors.rs`; `src/config.rs` for new env (`ARENA_OTP_PEPPER`, `ARENA_WEB_ORIGINS`, `ARENA_SMS_PROVIDER`, `ARENA_GUPSHUP_*`); `src/lib.rs` wiring of app state (db handle, clock, sms) into the router.
- `crates/arena-cloud/tests/api_envelope.rs` -- spawn/request helpers to reuse.
- `crates/arena-api/src/ui_auth.rs` -- Hub cookie/session code as a reference only (no dependency allowed).
- `packages/ui/src/{index.js,components/TextField.svelte,components/InlinePin.svelte,components/TabControl.svelte,forms.js,messages/en.json,tokens/tokens.css,base.css}` -- reuse TextField/Button/focusFirstInvalid; InlinePin is the pattern for CodeField; TabControl for ThemeToggle; light/dark tokens exist (`[data-theme]`); add components + export.
- `apps/onboarding/{vite.config.js,src/routes/+layout.svelte,src/routes/+page.svelte,package.json}` -- placeholder app on adapter-cloudflare, port 5179.
- `apps/gallery/playwright.config.ts`, `apps/gallery/tests/a11y.spec.ts:34,127,145` and `apps/hub/tests/auth.spec.ts:83,311` -- Playwright/axe patterns; `.github/workflows/ci.yml` a11y job (~l.216-246).
- `deploy/README.md`, `deploy/secrets/README.md`, `deploy/k8s/base/deployment.yaml` -- add `ARENA_OTP_PEPPER`, `ARENA_GUPSHUP_*`, `ARENA_WEB_ORIGINS`, `ARENA_SMS_PROVIDER` (secretKeyRef / env) and Cloudflare per-IP rule docs; keep `check:deploy` passing (extend if it asserts env lists).

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store/control-migrations/0002_owner_signup.sql`, `crates/arena-store/src/control*.rs`, tests -- schema + typed queries on both backends.
- [x] `crates/arena-proto/src/{error.rs,signup.rs,lib.rs}`, `packages/ui/src/generated/*` -- codes + DTOs.
- [x] `crates/arena-cloud/src/{messaging/*,api/signup.rs,api/session.rs,api/cors.rs,config.rs,lib.rs}` -- endpoints, adapter, cookies, CORS, phone_tag logging.
- [x] `crates/arena-cloud/tests/signup.rs` -- matrix + cookies + CORS + log scrub + Gupshup stub test.
- [x] `packages/ui/src/components/{CodeField.svelte,ThemeToggle.svelte}`, `index.js`, `messages/en.json` -- shared components + strings.
- [x] `apps/onboarding/**` (routes, `src/lib/api.ts`, `src/app.html`, vite proxy, playwright config + tests, package.json scripts) -- the flow.
- [x] `.github/workflows/ci.yml`, `deploy/**`, `README.md` -- a11y job, env/secrets, docs.

**Acceptance Criteria:**
- Given `arena-cloud --role all --dev` and `pnpm --filter @arena/onboarding dev`, when a developer signs up with a new phone and reads the code from `GET /v1/dev/sms`, then they land on `/account-setup` signed in.
- Given the onboarding Playwright suite, when it runs in CI, then the flow and axe checks pass in light and dark on desktop and 360 px.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`) -- pass (the known `arena-hub-ui` sidecar timing failure excepted); clippy incl. `-p arena-store --features control`; fmt; `scripts/check-sqlite-linkage.sh`.
- `pnpm --filter @arena/onboarding check && pnpm --filter @arena/onboarding build && pnpm --filter @arena/onboarding test` -- pass.
- every `pnpm run check:*`, `pnpm run test:scripts`, `pnpm --filter @arena/gallery test:a11y` -- pass.

## Implementation Notes

- **Store:** `ControlDb` gained private `execute` and `transact` (a statement list with row-count guards; remote transactions open their own Hrana stream). Typed modules `control/{owner,otp,session,rate_limit}.rs`. `complete_signup` is one transaction: a guarded consume (`code_hash`, unconsumed, attempts left, unexpired), `INSERT … ON CONFLICT(phone_e164) DO NOTHING`, and `INSERT … SELECT … WHERE mfa_state = 'pending'` for the session, so a concurrent double verify consumes once (tested on both backends). The last send time of a challenge is `expires_at - 300 s` (no extra column). Existing runner/dump tests that brought their own `owners` table now migrate with the baseline only, or use a renamed fixture table.
- **Cloud:** `auth.rs` (state, validation, HMAC/`phone_tag`, CSPRNG via `getrandom =0.3.4`, `hmac =0.12.1`; README pins added), `api/{signup,session,cors}.rs`, `messaging/{mod,fake,gupshup}.rs` (`Sms` uses a native async trait; `Messenger` enum dispatches). Sign-up settings are parsed by `config::auth_from_env` for serving roles only: without `--dev`, a missing/short `ARENA_OTP_PEPPER` (min 32), missing `ARENA_WEB_ORIGINS` or `ARENA_SMS_PROVIDER` exits 64. Resend sends first, then reissues, so a failed send keeps the challenge and its current code. A non-6-digit code is 422 `VALIDATION_FAILED` (`field: code`), not counted. Refresh failures also expire both cookies. `GET /v1/dev/sms` returns `[{to,text,at}]`. CORS also exposes `Retry-After`.
- **Deploy:** the pepper and `ARENA_GUPSHUP_*` are keys of the existing `arena-cloud-secrets` (no new Secret, so RBAC and the workflow are unchanged); `ARENA_SMS_PROVIDER=gupshup` and per-overlay `ARENA_WEB_ORIGINS` are plain values; `check:deploy` (+ tests) asserts them. Cloudflare per-IP rules documented in `deploy/README.md` step 6a.
- **Frontend:** SvelteKit 3 has no `$lib`/`$env/dynamic/public`; `PUBLIC_API_BASE` is declared in `src/env.ts` (`defineEnvVars`). Blur validation is skipped when focus moves to "Send code" (the error text would move the button under the pointer). A "Change details" button returns to the form. The rate-limit text is computed once from `Retry-After`.
- **Verified:** a real-browser run (`arena-cloud --role all --dev` + `pnpm --filter @arena/onboarding dev`, code read from `/v1/dev/sms`) landed on `/account-setup` with both cookies set; its log held only `phone_tag`s. `check:generated` reports the new generated files until they are committed.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; reviewers on Sonnet). P = patch, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH/VG: per-phone send limit is check-then-act; concurrent sends exceed 5/h | high | P | `check_send_limit` → send → `count_send`; atomic reservation |
| 2 | ECH: `count_send` failure after delivery → 500; resend send-then-reissue; concurrent resends pass the cooldown | medium | P | same root as #1 (ordering); a conditional reissue before send |
| 3 | ECH: provider failures leave unbounded challenge rows | low | P | delete the challenge on a failed start send |
| 4 | BH: no in-service cap against SMS pumping across phones | medium | P | per-phone only; add a global cap |
| 5 | BH/ECH: `fake` SMS accepted outside `--dev` | medium | P | `auth_from_env(false, …)` accepts it |
| 6 | BH/ECH: challenges/sessions never pruned; PII kept in `payload_json` | medium | P | no delete path |
| 7 | BH: retried verify → `OTP_EXPIRED` for a signed-in owner | medium | P | `NotConsumed` path |
| 8 | BH/ECH: two-tab refresh revokes the family | medium | P | no grace; 10 s no-revoke window |
| 9 | ECH: access token valid after rotation | low | P | add `rotated_at IS NULL` |
| 10 | ECH: refresh lookup after rotate can log out | low | P | return the owner from the txn |
| 11 | ECH: `complete_signup` outcome from post-commit reads | low | P | return from the txn |
| 12 | ECH: rate prune lacks a key filter | medium | P | latent counter loss for other limiters |
| 13 | ECH: `Retry-After` up to 60 s early | low | P | bucket start vs end |
| 14 | ECH: resend resets attempts (25 guesses/h) | low | R | 25/10^6 ≈ 2.5e-5 per phone-hour, bounded by the send cap |
| 15 | BH/ECH: Vary: Origin missing; POST without Origin skips the 403 | low | P (Vary) / R (no-Origin) | non-browser POSTs carry no CSRF risk |
| 16 | ECH: preflight Allow-Headers only content-type | low | R | no other headers in use |
| 17 | BH: pepper without domain separation; no rotation | low | P (separation) / R (rotation) | direct label prefix |
| 18 | BH/ECH: SMS text hard-codes 5 minutes | low | P | derive from `OTP_TTL_S` |
| 19 | ECH: Gupshup success prefix / unbounded body; timeout treated as not sent | low | P | cap + exact token; timeout keeps the reservation (#1) |
| 20 | BH/ECH: client/server validation divergence | medium | P | the spec requires the same rules; shared vectors |
| 21 | ECH: per-country phone validation | low | R | the spec rule is general E.164 + India |
| 22 | BH: kustomize env patch by index | medium | P | fragile `/env/9` |
| 23 | BH/ECH: no client refresh; offline → redirect | medium | P | 15-min bounce |
| 24 | BH/ECH: resend `OTP_EXPIRED` dead end; stale responses after Change details | low | P | direct UI fixes |
| 25 | BH: pending owner's new form silently dropped | low | R | the spec says don't overwrite existing details |
| 26 | BH/ECH: verify reveals the enrolled state after a correct code | low | R | only to the code holder; the spec accepts it |
| 27 | BH: no CORS cache-poison test | low | P (via #15) | covered by the Vary fix test |
- **Review fixes (pass 1):**
  - Sends are reserved atomically before the SMS goes out (`rate_reserve`: one transaction sums the window and increments the bucket only under the limit, for the phone rule and a new global `sms:global` rule, `ARENA_SMS_GLOBAL_PER_MIN`, default 60, `--dev` 10000). A definite provider failure releases the reservation; an uncertain one (timeout after sending) keeps it. A failed first send deletes the challenge; a resend reissues conditionally on the expiry it read (a losing concurrent resend is `RATE_LIMITED`) and reverts on a failed send. Windows count buckets whose end is inside them; `Retry-After` runs to the oldest bucket's end. Pruning is per limiter prefix.
  - `ARENA_SMS_PROVIDER=fake` exits 64 outside `--dev`.
  - `0002_owner_signup.sql` was edited before release: `payload_json` is nullable and nulled on consume or void (a voided challenge can no longer be resent: `OTP_EXPIRED`), plus indexes for pruning. Challenges expired over 24 h are deleted on insert; sessions whose refresh expired over a day ago on session create. A local `.arena-cloud-dev` created from the earlier draft must be deleted.
  - `complete_signup` and `rotate_owner_session` read their outcome (owner, session) inside the transaction. Access tokens of rotated sessions stop working. A rotated refresh presented within 10 s answers 401 without revoking or touching cookies.
  - A repeated verify of a used challenge from the browser it signed in answers `account_setup` again.
  - `Vary: Origin` on every `/v1` response; HMAC domain labels `otp\0` / `tag\0` (phone_tag is now an HMAC); `sms_text` minutes from `OTP_TTL_S`; Gupshup replies capped at 8 KiB and success only on an exact first field `success`.
  - Client and server validation share `tests/fixtures/signup-validation.json` (Rust unit test + `apps/onboarding/tests/validate.unit.spec.ts`).
  - Overlays set `ARENA_WEB_ORIGINS` by a strategic-merge patch keyed on the env name.
  - Onboarding: `/v1/session` 401 → one refresh and retry; offline/5xx shows a retry state; resend `OTP_EXPIRED` offers "Send a new code"; stale resend/verify answers are dropped.
