---
title: 'Story 2.9: Step 3 — single-use 8-character Hub pairing code'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '6ddae81aab1fb8f6d60289112286ae37af85d38e'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-8-step-2-free-tier-issues-a-signed-licence.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** After choosing a tier, the owner has no way to tie a Hub installation to their branch.

**Approach:** Wizard step 3 (`install_hub`) issues a single-use, 15-minute, 8-character code. It's stored only as a hash and shown large in a `pairing-code` card with Copy and an accessible countdown. Regenerating invalidates every earlier unused code for the branch. The Hub side that redeems the code is Story 2.10.

## Boundaries & Constraints

**Always:**
- **Migration** `0007_pairing_codes.sql` (number reserved; 2.8 holds 0006): `pairing_codes` with id, branch_id, `code_hash` UNIQUE, `tier_at_issue`, created_at, `expires_at` (+15 min), `used_at` NULL and `invalidated_at` NULL, plus an index on branch_id. All SQL lives in `arena_store::control`, over both backends.
- **Code:**
  - 8 characters from the alphabet `23456789ABCDEFGHJKMNPQRSTVWXYZ`, which excludes 0, O, 1, I, L and U.
  - Drawn uniformly from the OS CSPRNG (rejection sampling, no modulo bias).
  - Displayed as 4 + 4.
  - `code_hash` is HMAC-SHA256 with the server pepper (2.4's `ARENA_OTP_PEPPER` with a new domain label `"pair\0"`) over the normalised code: uppercase, space and hyphen stripped.
  - The code encodes nothing; the branch, tenant and tier come only from lookup.
- **API** (requires `RequireSetupComplete` plus `OwnedBranch`, with steps `cafe` and `tier` done; otherwise 409 `STEP_NOT_READY`, a new code):
  - `POST /v1/wizard/install-hub/code`. In one transaction it invalidates every unused, unexpired code for the branch, then inserts a new one. It answers `{code, expires_at}`. The plain code appears only in this response and is never logged.
  - `GET /v1/wizard/install-hub` → `{active: {expires_at} | null, hub_paired: bool}`. It never returns the code. Reloading the page while a code is live **issues a new code**, which invalidates the old one, as the story requires.
  - `hub_paired` is false until 2.10. The step completes only on pairing (2.10/2.11), not here.
- **Store helper for 2.10:** `redeem_pairing_code(code) -> Result<{branch_id, tenant_id, tier}, Invalid>`. In one transaction it marks `used_at` only if the code is unused, not invalidated and not expired. It's unit-tested here: valid, used, invalidated, expired, and a concurrent double redeem where exactly one wins.
- **Onboarding UI** (`/setup/install_hub`):
  - **Three numbered parts:**
    1. "1 · Download": "Hub for Windows 10 or 11", with the download link from `PUBLIC_HUB_DOWNLOAD_URL` (env, default `/download/hub`), plus the link "Installing on an Android tablet or phone instead?" pointing to `/help/android-hub`.
    2. "2 · Install".
    3. "3 · Type the code".
  - **Shared `PairingCode` card** in `packages/ui`:
    - the code in `web-display` tabular figures, grouped 4 + 4 and selectable;
    - a secondary "Copy code" button;
    - a "Single use" label;
    - "Expires in mm:ss".
    - No QR anywhere.
  - **Countdown:**
    - It's announced in a polite live region only at 10, 5 and 1 minutes remaining and at expiry, never on each tick.
    - At zero the code is struck through with an "Expired" `StatusBadge`, the message "This code has expired. Generate a new code and type it into the Hub installer." appears, and "Generate a new code" becomes the one primary action.
  - **Copy:** a successful write announces "Code copied" politely. On failure the code is selected for a manual copy, with the hint "Press Ctrl+C to copy".
  - Every string lives in `en.json`. axe passes in light and dark at 360 px. Targets are ≥ 44 px.
- **New code:** `STEP_NOT_READY`.
- **Tests:**
  - **Rust:**
    - the alphabet, length and uniformity sanity (no excluded characters across 10k codes);
    - only the hash is stored (scan the table for the plain code);
    - regenerate invalidates the previous code;
    - a reload issues a new code;
    - expiry;
    - every redeem case;
    - cross-tenant `FORBIDDEN`;
    - `STEP_NOT_READY`.
  - **Playwright** (stubbed, with a controllable clock via `page.clock`): the grouping, copy ok and fail, announcements exactly at 10/5/1/0, the expired state and regenerate, no QR (no `<canvas>`/`<img>` with a code), axe.

**Never:**
- Storing or logging the plain code.
- Two valid codes for one branch.
- A QR code.
- Completing step 3 without pairing.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Issue | steps 1–2 done | 8-char code, +15 min; one row hashed | N/A |
| Not ready | `tier` not done | 409 `STEP_NOT_READY` | N/A |
| Regenerate | live code exists | old row invalidated; new code | N/A |
| Reload | GET then POST on load | new code; old invalid | N/A |
| Redeem ok | valid code | `{branch, tenant, tier}`; `used_at` set | N/A |
| Redeem used / invalidated / expired | — | `Invalid` | N/A |
| Race | two redeems | exactly one wins | N/A |
| Foreign | other tenant's branch | 403 `FORBIDDEN` | N/A |
| Countdown | 15:00 → 0 | announced at 10:00, 5:00, 1:00, 0 only | N/A |
| Copy fails | clipboard rejects | code selected + hint | N/A |

</frozen-after-approval>

## Code Map

Investigate from 2.7/2.8 (wizard API, `OwnedBranch`, `RequireSetupComplete`, `WizardProgress`, step routes) and 2.4 (`auth.rs` HMAC pepper with domain labels, CSPRNG helpers, `CodeField`). Control migration **0007 reserved**. `packages/ui` `StatusBadge`, `Button`, `en.json`.

## Tasks & Acceptance

**Execution:**
- [ ] `control-migrations/0007_pairing_codes.sql`, store + redeem helper + tests.
- [ ] `crates/arena-cloud/src/{pairing_code.rs,api/wizard_install.rs}`, `arena-proto` DTOs + `STEP_NOT_READY`, generated TS, `tests/pairing_code.rs`.
- [ ] `packages/ui/src/components/PairingCode.svelte`, `index.js`, `en.json`; `apps/onboarding/src/routes/setup/install_hub/**`, tests.
- [ ] `README.md`.

**Acceptance Criteria:**
- Given `--dev` and an owner past step 2, when step 3 opens, then an 8-character code shows grouped 4 + 4 with a 15:00 countdown; reloading shows a different code and the first one no longer redeems.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`); clippy incl. `-p arena-store --features control`; fmt; `scripts/check-sqlite-linkage.sh` -- pass (known flaky tests recorded).
- `pnpm --filter @arena/onboarding check && pnpm --filter @arena/onboarding build && pnpm --filter @arena/onboarding test`; every `pnpm run check:*`; `pnpm run test:scripts`.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; on Sonnet). P = patch, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH: redeem has no throttle | medium | R (handled in 2.10) | 2.10's spec defines per-code (5) and per-IP (20/h) limits on `/v1/pair` |
| 2 | BH/ECH: issue unthrottled | medium | P | per-branch `rate_reserve` |
| 3 | BH: no purge of old rows | low | P | |
| 4 | BH: no DB guarantee of one live code per branch | medium | P | partial unique index |
| 5 | BH: rollback on collision with a live code unproven | medium | P | test |
| 6 | BH: no `Cache-Control: no-store` on the code | medium | P | |
| 7 | BH/ECH: reload, second tab or navigation kills the shown code | low | R | the frozen spec says a reload issues a new code |
| 8 | ECH: lost regenerate response leaves a dead code shown | low | P | |
| 9 | BH/ECH: client clock skew | medium | P | `server_now` offset |
| 10 | BH: "moves on by itself" copy before 2.11 | low | R | 2.11 (next) adds the advance; the text is correct then |
| 11 | BH: default download links 404 | low | R | configured by env; download hosting is outside this story |
| 12 | BH/ECH: narrow normalising; no shape check before HMAC | low | P | |
| 13 | BH: no redeem logs | low | P | |
| 14 | BH/ECH/VG: global subscriber in test | low | P | |
| 15 | BH: UI coupling, retry key reuse, FORBIDDEN generic, multiple live regions | low | R | cosmetic; the regions are polite and silent until used |
| 16 | BH/ECH: `tier_at_issue` read outside the txn | low | P | |
| 17 | BH/ECH: redeem doesn't recheck tier or branch | low | R | 2.10 reads the effective tier at pairing |
| 18 | ECH: background throttling, stale Copied, focus at expiry | low–medium | P | |
| 19 | ECH: Query extraction rejection is plain text | false | R | the 2.2 layer wraps non-JSON 4xx in the envelope |
| 20 | ECH: `STEP_NOT_READY` vs `FORBIDDEN` with no branch | low | R | no branch means no owned branch, so `FORBIDDEN` is correct |
| 21 | VG: paired state untested | medium | P | |
| 22 | VG: `StepNotReady` status mappings untested in api and kiosk | low | P | |
