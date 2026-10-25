---
title: 'Story 2.10: Hub installer pairs with the code and retries in place'
type: 'feature'
created: '2026-10-25'
status: 'in-progress'
baseline_commit: '597c0954c8feaccabf7a1f4dbd3a9e4c929e2422'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-9-step-3-single-use-8-character-hub-pairing-code.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-8-step-2-free-tier-issues-a-signed-licence.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A freshly installed Hub has no way to join a branch. The 2.9 code exists, but nothing redeems it.

**Approach:**
- The cloud gets `POST /v1/pair`. It redeems the code atomically, refuses a Windows Hub on Free, mints a Hub device token (Argon2id-hashed in a `hubs` row), and returns the token once along with ids, tier, `branch_code` and the first licence.
- The Hub shows an always-dark "Pair this Hub" screen until paired. It stores the token in the OS keystore and the licence in `licence_cache`, and retries in place on a bad code or no network.
- Rate limits apply per code and per IP.

## Boundaries & Constraints

**Always:**
- **Control migration** `0008_hubs.sql` (number reserved; 2.9 holds 0007): a `hubs` table with id, branch_id, platform ∈ {windows, android}, hub_instance_id, version, token_hash, paired_at, last_seen_at, revoked_at.
  - One active Hub per branch for now. Re-pairing the same branch revokes the previous `hubs` row; standby Hubs come in a later epic.
  - All SQL lives in `arena_store::control`.
- **`POST /v1/pair`** is unauthenticated but rate-limited. Body: `{code, platform, hub_instance_id, version}`.
  - **In one transaction:**
    1. Look up the code with 2.9's `redeem` logic. A wrong code is `PAIR_CODE_INVALID` (401). A used or invalidated code is also `PAIR_CODE_INVALID`. An expired one is `PAIR_CODE_EXPIRED` (401, new code).
    2. **Entitlement check before consuming:** if the branch's effective tier (2.8 `tier_at(now)`) is Free and `platform = windows`, answer 403 `FEATURE_NOT_ENTITLED` (new code) with `field: "platform"` and leave the code **unconsumed**.
    3. Otherwise mark the code used and lock the branch code (`branches.code_locked_at`, via 2.7 `lock_branch_code`).
    4. Mint a 256-bit token (`hub_<id>.<base64url>`) and store its Argon2id hash in `hubs` (reuse the 2.5 semaphore).
    5. Sign the first licence with the 2.8 signer.
  - **Success:** 200 `{hub_id, token, tenant_id, branch_id, tier, branch_code, licence}`. The token is returned once and never logged.
- **Rate limits** (2.4's atomic `rate_reserve`):
  - **Per code:** 5 failed attempts invalidate the code (`invalidated_at`). A later correct entry is `PAIR_CODE_INVALID`.
  - **Per IP:** more than 20 `/v1/pair` attempts in a rolling hour answers 429 `RATE_LIMITED` with `Retry-After`. The IP comes from `CF-Connecting-IP` when the peer is in the committed Cloudflare ranges (`deploy/cloudflare/ips-*.txt`), otherwise the socket peer.
  - **Cloudflare rule:** committed as config in `deploy/cloudflare/rate-limits.json` (`/v1/pair`, 20 per hour per IP), documented in `deploy/README.md`, and checked by `check:deploy` for presence and threshold.
- **Hub side:**
  - **Hub migration**, next free number at landing: `hub_identity` (singleton: tenant_id, branch_id, hub_id, tier, paired_at) and `licence_cache` (singleton: jwt, received_at).
  - **Keystore** (`crates/arena-tasks/src/keystore.rs`, or reuse the kiosk's DPAPI helper if it can be shared without breaking layers): DPAPI `CryptProtectData` on Windows. Elsewhere, a `0600` file under the data dir for dev and tests. An Android Keystore stub interface is left for Epic 4.
  - **Cloud client:** `reqwest` (the 2.3 pin, rustls ring) with `--cloud-host` (the 1.22 flag). An `http://127.0.0.1…` cloud host is allowed only in dev-fixtures builds, for local `arena-cloud --dev`.
  - **Pairing endpoint:** `POST /ui/pair-hub {code}` on the loopback UI server. No staff session is needed, since this is before any staff exists.
    - It's accepted only while the Hub is unpaired; once paired it's 409 `ALREADY_SET`.
    - It normalises the code (uppercase; space and hyphen stripped), calls the cloud, and on success stores the token, `hub_identity` and `licence_cache` in one Hub DB transaction plus a keystore write.
    - It maps errors as follows:
      - invalid or expired → `PAIR_CODE_INVALID` to the UI;
      - `FEATURE_NOT_ENTITLED` passes through;
      - `RATE_LIMITED` passes through with `Retry-After`;
      - network errors → `HUB_SLOW`-class `CLOUD_UNREACHABLE` (new code).
  - **Gate:** while unpaired, `GET /ui/snapshot` includes `paired: false`, and every other `/ui/*` route answers 409 `HUB_NOT_PAIRED` (new code). Exempt are the `/healthz`, `/ui/pair-hub` and `/ui/snapshot` minimal fields, plus the `dev-fixtures` routes.
  - **Dev:** `dev seed` keeps working by writing a dev `hub_identity` (tenant, branch and hub ids, tier free, dev licence signed with the dev key, which the dev verify set trusts), so existing Epic 1 flows stay paired in dev.
  - **Restart:** an already-paired Hub skips the screen and never calls `/v1/pair` again.
- **Hub UI** (`apps/hub`):
  - When `paired: false`, render only the always-dark "Pair this Hub" screen. It ignores the theme setting (dark tokens always) and has:
    - one code field: 8 characters, case-insensitive, spaces ignored, `autocomplete="off"`;
    - a primary "Pair" button and "Quit setup", which closes the shell window through the existing Tauri command, or shows "You can close this window" in a browser.
  - Routing to any other route redirects to the pairing screen.
  - **Errors:**
    - **Invalid code:** clear the field, keep focus, and show "That code didn't work. Get a new code from your browser." It never says which kind of failure.
    - **`FEATURE_NOT_ENTITLED`:** "The Free plan runs on the Android Hub." plus a link to the Android download (`PUBLIC_ANDROID_HUB_URL`, default `https://arena360.cloud/download/android`).
    - **Unreachable:** "Can't reach Arena 360 — check the internet and try again", with the typed code kept.
    - **Rate limited:** "Too many tries. Try again in m:ss."
  - On success the normal shell loads.
  - Every string lives in `en.json`. axe passes.
- **New codes:** `PAIR_CODE_EXPIRED`, `FEATURE_NOT_ENTITLED`, `CLOUD_UNREACHABLE`, `HUB_NOT_PAIRED`. `PAIR_CODE_INVALID` already exists from 2.18 (reuse it).
- **Tests:**
  - **Cloud (Rust):**
    - every matrix row;
    - the token returned once and hashed;
    - the licence verifies with the dev key and carries the branch's tier;
    - `android` on Free accepted and `windows` on Free refused with the code unconsumed (a later android pair with the same code succeeds);
    - five failures invalidate the code;
    - the per-IP limit, honouring `CF-Connecting-IP` only from Cloudflare ranges;
    - re-pair revokes the previous Hub;
    - concurrent pairs with one code: exactly one succeeds;
    - the log scrub.
  - **Hub (Rust):** pairing against an in-process `arena-cloud --dev` app (real HTTP) stores the identity and licence; the gate on unpaired `/ui/*`; restart skips pairing; keystore round trip; unreachable mapping.
  - **Hub Playwright:** the screen, each error, retry in place, success, axe.

**Never:**
- Consuming a code on a refused Windows/Free pair.
- Returning the token twice or logging it.
- Any Hub screen reachable before pairing (other than the pairing screen).
- Plain HTTP to a non-loopback cloud.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pair ok (android, Free) | valid code | 200 token + licence (tier free); code used; branch code locked | N/A |
| Windows on Free | valid code | 403 `FEATURE_NOT_ENTITLED`; code still valid | N/A |
| Windows on comp Basic | valid code | 200 | N/A |
| Wrong code | random | 401 `PAIR_CODE_INVALID` | N/A |
| Expired | after 15 min | 401 `PAIR_CODE_EXPIRED` (UI shows the same line) | N/A |
| 5 failures | on one code | the code is invalidated | N/A |
| IP flood | 21st attempt/hour | 429 `RATE_LIMITED` + Retry-After | N/A |
| Race | two pairs, one code | exactly one 200 | N/A |
| Re-pair | branch already has a Hub | the old Hub is revoked; the new token works | N/A |
| Hub unpaired | open UI | pairing screen only; other `/ui/*` → `HUB_NOT_PAIRED` | N/A |
| Hub offline | cloud unreachable | `CLOUD_UNREACHABLE`; the code is kept in the field | N/A |
| Restart paired | Hub paired | no pairing screen; no `/v1/pair` call | N/A |

</frozen-after-approval>

## Code Map

Investigate from 2.9 (`pairing_codes`, redeem helper, pepper label), 2.8 (signer, `tier_at`, entitlement), 2.7 (`lock_branch_code`), 2.4 (`rate_reserve`, cookie-free API patterns), 2.17 (Hub `main.rs` wiring, `HubStatus`), 1.22 (`--cloud-host`, connectivity probe), 1.11–1.14 (`/ui/*` staff auth and routing), kiosk-svc DPAPI token store (2.18). Control migration **0008 reserved**; Hub migration: next free at landing.

## Tasks & Acceptance

**Execution:**
- [ ] cloud: control migration 0008, store, `api/pair.rs`, codes, rate limits, Cloudflare config + check:deploy.
- [ ] Hub: migration, keystore, cloud client, `/ui/pair-hub`, unpaired gate, dev seed identity.
- [ ] `apps/hub` pairing screen + route guard + tests; `en.json`.
- [ ] README / deploy docs.

**Acceptance Criteria:**
- Given `arena-cloud --role all --dev` with an owner at wizard step 3 and a fresh Hub (`--cloud-host http://127.0.0.1:8080`, android platform flag for Free in dev), when the step-3 code is typed into the Hub's pairing screen, then the Hub shows the normal shell, `hubs` has one row, and restarting the Hub skips the pairing screen.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`); dev-fixtures runs for hub, api, kiosk-svc; clippy incl. control + dev-fixtures; fmt; linkage script.
- `pnpm --filter @arena/hub check && ARENA_PW_PORT=54xx pnpm --filter @arena/hub exec playwright test --workers=2`; onboarding suite unaffected; every `pnpm run check:*`; `pnpm run test:scripts`.
