---
title: 'Story 2.7: Resumable setup wizard and step 1 — café and first branch'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '2a793ad55dac1b50850dd305216c62450fb119ad'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-5-owner-sets-a-password-and-enrols-totp-with-backup-codes.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** An owner who has finished account setup has nowhere to go. There's no tenant, no branch and no way to resume a multi-sitting setup.

**Approach:**
- A 10-step setup wizard in the onboarding app, with its progress kept in the cloud, so it resumes on any device at the first incomplete step.
- Step 1 (Café details) creates the tenant and the first branch, with timezone, currency and GST, and gets a control-plane-assigned `branch_code` in one transaction.
- Every tenant and branch request is scoped by the session only.

## Boundaries & Constraints

**Always:**
- **Migration** `0005_tenants_branches.sql` (number reserved; 2.6 holds 0004):
  - `tenants`: id, owner_id UNIQUE (one tenant per owner for now), name, created_at.
  - `branches`: id, tenant_id, name, city, country (ISO 3166-1 alpha-2), currency (ISO 4217), timezone (IANA), gst_registered (0/1), gstin (nullable), branch_code, code_locked_at (nullable), created_at, updated_at, plus UNIQUE(tenant_id, branch_code).
  - `onboarding_progress`: branch_id, step, state ∈ {pending, done}, completed_at, PRIMARY KEY(branch_id, step).
  - `step` is TEXT with a CHECK over the 10 keys below. All SQL lives in `arena_store::control`, over both backends.
- **Steps** (a `WizardStep` enum in `arena-proto`, exported to TypeScript, in this order):
  1. `cafe`
  2. `tier`
  3. `install_hub`
  4. `devices`
  5. `rates`
  6. `products` (can skip)
  7. `staff` (can skip)
  8. `printer_payments` (can skip)
  9. `test_session`
  10. `go_live`

  **Current step:** the first one not done (`cafe` when there's no branch yet). Skippable steps count as done only once they're marked done or skipped; skipping is a later story.
- **Countries:** `arena-proto` gets a small `COUNTRIES` table: code, name, default currency, default timezone and `gst` (whether GST applies).
  - At launch: IN (INR, Asia/Kolkata, gst), NP, LK, BD, AE, SG, MY, GB, US (each with its primary timezone, no GST).
  - The currency must be in the existing `arena_proto::currency` list. The timezone must be any `chrono_tz` name; any IANA zone is allowed, and the country's zone is only the default.
  - `GET /v1/meta/countries` and `GET /v1/meta/timezones` serve the lists to the UI.
- **GSTIN:** required, and validated only when `gst_registered`.
  - Format: 15 characters, 2-digit state code 01–38, a PAN (5 letters, 4 digits, 1 letter), 1 entity character [1-9A-Z], `Z`, then a mod-36 checksum character (the official GSTN algorithm, unit-tested against known-valid numbers).
  - Uppercased before checking, and refused when `gst_registered` is false. Errors are 422 `VALIDATION_FAILED` with `field`.
- **`branch_code`:**
  - Derived from the branch name: uppercase ASCII letters and digits only. Multi-word names take the initials of up to 6 words. If that gives fewer than 3, it uses the first letters of the joined name; if still short, it pads with `X`.
  - Unique per tenant: on a clash it replaces the last character with 2..9 then A..Z.
  - Editable through `PUT /v1/branches/{id}/code {code}` (3–6 of `[A-Z0-9]`, unique per tenant) while `code_locked_at IS NULL`; once locked it's 409 `ALREADY_SET`. 2.10 locks it at Hub pairing.
- **Scope:**
  - The tenant always comes from the session owner.
  - A `RequireSetupComplete` session (2.5 gate) is required for every wizard and branch endpoint.
  - A branch id in a path that doesn't exist, or belongs to another tenant, answers 403 `FORBIDDEN` with nothing read beyond the ownership check and nothing written. A shared `OwnedBranch` extractor enforces this.
- **API** (DTOs in `arena-proto`, exported to TypeScript):
  - `GET /v1/wizard` → `{current, steps: [{key, state, can_skip}], tenant?: {id, name}, branch?: {id, name, city, country, currency, timezone, gst_registered, gstin, branch_code, code_editable}}`.
  - `PUT /v1/wizard/cafe {cafe_name, city, country, currency, timezone, gst_registered, gstin?}`:
    - **First call:** creates the tenant and branch, assigns the code, and writes all 10 progress rows with `cafe` done — all in one transaction.
    - **Later calls:** update the tenant name and the branch fields, and keep `cafe` done.
    - Answers the same body as `GET /v1/wizard`.
  - `PUT /v1/branches/{id}/code {code}` → `{branch_code}`.
- **Onboarding app:**
  - **Shared `WizardProgress`** in `packages/ui`.
    - **At 840 px and wider:** a list of 10 rows, each ≥ 44 px, showing the step name, an ok check when done, and a "Can skip" tag on products, staff and printer & payments. The current row has `aria-current="step"`. Done rows are links; rows after the current one are plain text, not links.
    - **Below 840 px:** 10 dots plus "Step N of 10 · <name>". Dots are presentational, and the text is the accessible label.
    - It's built in a `<nav aria-label>` with an ordered list.
  - **Routes:** `/setup` redirects to `/setup/<current>`, and `/setup/[step]` renders that step.
    - A step after the current one redirects to the current one.
    - A done step opens for edit. Saving keeps it done and returns to `/setup/<current>`.
  - **Step 1 form:**
    - café name and city are prefilled from sign-up (via `GET /v1/session`, which gains `cafe_name` and `city`);
    - a country select;
    - currency and timezone selects defaulted from the country, still changeable;
    - a GST yes/no;
    - a GSTIN field shown and required only when yes;
    - after creation, the `branch_code` with an "Edit" control while it's editable.
    - Validation runs on submit and blur, with focus moving to the first invalid field (as in 2.4).
  - Steps 2–10 render a heading plus "This step is coming soon" until their stories land.
  - **Self-advance helper:** when the wizard state changes (a save here, polling in later stories), focus moves to the new step's `h1` and a polite live region announces "Step N of 10: <name>" once. A Playwright test covers it via step 1's save.
  - 2.5's "Setup wizard starts here" now links to `/setup`.
  - Every string goes in `en.json`.
- **Tests:**
  - **Rust** (`crates/arena-cloud/tests/wizard.rs`): every matrix row, the GSTIN checksum vectors, the code derivation and clash cases, and cross-tenant `FORBIDDEN` with a second owner, asserting no write.
  - **Playwright** (stubbed): progress at desktop and 360 px, resume, the redirect on a future-step URL, edit and return, GST toggling, the code edit, the announcement and focus, axe in light and dark.

**Never:**
- A tenant id taken from the request body or query.
- Creating a second tenant for an owner.
- Opening a future step.
- Code changes after the code is locked.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| No tenant | enrolled owner | `current: cafe`; 10 steps pending | N/A |
| Not setup-complete | not acked | 403 `SETUP_INCOMPLETE` | N/A |
| Create | valid step 1, India, GST no | tenant + branch + 10 progress rows, `cafe` done, `current: tier`, code assigned | N/A |
| GST yes, no GSTIN | `gst_registered: true` | 422 `field:"gstin"` | N/A |
| Bad GSTIN checksum | wrong last char | 422 `field:"gstin"` | N/A |
| GSTIN when no | GST false + gstin | 422 `field:"gstin"` | N/A |
| Unknown tz/currency/country | `Mars/Base` | 422 with field | N/A |
| Edit done step | `PUT /v1/wizard/cafe` again | fields updated; still one tenant/branch; `cafe` done | N/A |
| Code derive | "Pixel Arena Gaming" | `PAG` | N/A |
| Code clash | second branch would also be `PAG` (unit test) | `PA2` | N/A |
| Code edit | `PUT code {"ARENA1"}` | updated | N/A |
| Code invalid | `"ab"` or a duplicate | 422 / 409-class | N/A |
| Code locked | `code_locked_at` set | 409 `ALREADY_SET` | N/A |
| Foreign branch | owner B's branch id | 403 `FORBIDDEN`; no write | N/A |
| Unknown branch | random id | 403 `FORBIDDEN` | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-store/src/control*` + `control-migrations/` -- 2.4–2.6 modules; add 0005 + `tenants`/`branches`/`progress` queries. **Migration 0005 reserved.**
- `crates/arena-proto/src/currency.rs` -- `CURRENCIES`, `is_known_currency`; chrono-tz is already an arena-proto dep (timezones). Add `wizard.rs` (steps, DTOs, `COUNTRIES`).
- `crates/arena-cloud/src/api/{gate.rs (RequireSetupComplete), session.rs (SignedIn), error.rs}` (2.4/2.5) -- reuse; add `wizard.rs`, `branches.rs`, `meta.rs`, and an `OwnedBranch` extractor.
- `packages/ui/src/components/*` (TextField, Button, TabControl, CodeField…), `forms.js` (`focusFirstInvalid`), `messages/en.json` -- add `WizardProgress.svelte` + export.
- `apps/onboarding/src/routes/{account-setup,account}/`, `src/lib/api.ts`, `tests/stub.ts` + specs -- extend; add `setup/`.

## Tasks & Acceptance

**Execution:**
- [ ] `crates/arena-store/control-migrations/0005_tenants_branches.sql`, store modules + tests.
- [ ] `crates/arena-proto/src/wizard.rs`, `lib.rs`, generated TS.
- [ ] `crates/arena-cloud/src/{gstin.rs,branch_code.rs,api/wizard.rs,api/branches.rs,api/meta.rs,api/owned.rs}`, `tests/wizard.rs`.
- [ ] `packages/ui/src/components/WizardProgress.svelte`, `index.js`, `en.json`.
- [ ] `apps/onboarding/src/routes/setup/**`, tests.
- [ ] `README.md` -- API docs.

**Acceptance Criteria:**
- Given `--dev`, when a set-up owner completes step 1, closes the browser and signs in again on another profile, then `/setup` opens on step 2 (`tier`) with step 1 shown done and editable.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`) -- pass (known `arena-hub-ui` sidecar failures excepted); clippy incl. `-p arena-store --features control`; fmt; `scripts/check-sqlite-linkage.sh`.
- `pnpm --filter @arena/onboarding check && pnpm --filter @arena/onboarding build && pnpm --filter @arena/onboarding test`; every `pnpm run check:*`; `pnpm run test:scripts` -- pass.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; on Sonnet). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH/VG: wizard singleton shows another owner's data; no cross-device refresh | high | P | `wizard.state` never cleared |
| 2 | BH: country/currency/tz freely editable after step 1 | medium | D | no downstream data yet; guard when rates and products land |
| 3 | BH/ECH/VG: clash helpers unused; README claim | low | P | wire `assign` in |
| 4 | BH: non-Latin names → `XXX` | medium | P | target markets |
| 5 | BH: locked-vs-invalid error order unspecified | low | R | either answer is safe |
| 6 | BH/ECH: tz aliases accepted | medium | P | canonical list only |
| 7 | BH: no concurrent first-save or rollback test | medium | P | |
| 8 | BH: no rate limits; meta lists uncached | low | P (cache) / R (limits: session-gated writes) | |
| 9 | BH/VG: duplicated GSTIN without shared vectors | medium | P | |
| 10 | BH: a11y gaps (Edit name, describedby, alert) | medium | P | |
| 11 | BH: hub-paired copy ships early | low | R | the strings are correct once 2.10 lands; unreachable now |
| 12 | BH: `first_branch` loads all; café rename renames the branch | low | R | one branch per tenant in Epic 2 |
| 13 | ECH: done later step redirects | medium | P | the spec allows editing done steps |
| 14 | ECH: `ALREADY_SET` refresh failure keeps Edit | low | P | |
| 15 | ECH: silent empty prefill | low | P | |
| 16 | ECH: save errors not routed | low | P | |
| 17 | ECH: tenant without branch → 500 forever | low | P | |
| 18 | ECH: ambiguous create-false cause | low | R | only the unique race yields false |
| 19 | ECH: 0-row code update misread as Taken | low | P | |
| 20 | VG: `ALREADY_SET`, load-failure and auth tests | medium | P | |
| 21 | VG: remote tenant test silently passes without sqld | false | R | CI sets `ARENA_REQUIRE_SQLD=1`, so absence fails there |
