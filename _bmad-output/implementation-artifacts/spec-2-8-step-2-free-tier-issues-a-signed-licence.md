---
title: 'Story 2.8: Step 2 — Free tier issues a signed licence'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '24328da06270811517357d105b8e35405fe9ca5f'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-7-resumable-setup-wizard-and-step-1-café-and-first-branch.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A branch has no entitlement and no licence, so nothing tells the Hub what the branch may do.

**Approach:**
- Wizard step 2 shows the three tiers from one shared pricing table. Free writes an entitlement immediately; Basic and Premium stay disabled until billing (Epic 10).
- A licence module signs EdDSA JWTs from the entitlement, with tier defaults from one `arena-proto` table, and verifies them against a two-key set.
- The audited operator CLI `grant-comp` gives a branch a comp Basic or Premium entitlement.

## Boundaries & Constraints

**Always:**
- **Migration** `0006_entitlements.sql` (number reserved; 2.7 holds 0005):
  - `entitlements`: branch_id PK, tier ∈ {free, basic, premium}, source ∈ {free, comp, paid}, addons_json (default `[]`), issued_at, until (nullable), updated_at.
  - `ops_audit`: id, at, operator, action, target, reason, details_json.
  - All SQL lives in `arena_store::control`, over both backends.
- **Tier table** (`crates/arena-proto/src/tier.rs`, ts-rs exported). One `TIERS` definition per tier, shared by cloud and Hub:
  - **free:** `flags []`, `features []`, `upload_interval_s 0`, `summary_interval_s 0`, `rollup_schema 1`, `retention_days 0`, `limits {devices: 20, users: 3, branches: 1}`.
  - **basic:** `flags ["uploads", "owner_reports"]`, `features []`, `upload_interval_s 300`, `summary_interval_s 900`, `rollup_schema 1`, `retention_days 365`, `limits {devices: 60, users: 10, branches: 1}`.
  - **premium:** `flags ["uploads", "owner_reports", "kiosk_orders", "kitchen_display", "standby"]`, `features []`, `upload_interval_s 300`, `summary_interval_s 900`, `rollup_schema 1`, `retention_days 1095`, `limits {devices: 200, users: 50, branches: 1}`.
  - The Free values and the paid device limits come from the story. The other paid values are provisional and recorded for review in Epic 10.
- **Pricing table** (`packages/ui/src/pricing.ts`, a single source exported from `@arena/ui`):
  - **free:** ₹0, 1 branch, "Ads on the Android Hub".
  - **basic:** ₹200 per branch per month + 18% GST; yearly ₹2,000.
  - **premium:** ₹1,999 per month + 18% GST; yearly ₹19,990.
  - The cards render only from this table, and a unit test pins the values.
- **Licence** (`crates/arena-proto/src/licence.rs` for the claims and verify; `crates/arena-cloud/src/licence.rs` for signing):
  - **Library:** `jsonwebtoken` with its rust-crypto EdDSA backend (pin 11.x per the architecture; pick the newest 11.1.x and record it). Add `ed25519-dalek` 3.x only if the key handling needs it.
  - **Claims:** `{tenant_id, branch_id, tier, flags, features, upload_interval_s, summary_interval_s, rollup_schema, retention_days, limits, iat, exp = iat + 7 d}` plus a `kid` header. The values come from the tier table plus the entitlement.
  - **Signing key:** `ARENA_LICENCE_SIGNING_KEY` (PKCS#8 PEM Ed25519, a secret) with `ARENA_LICENCE_KID`. `--dev` uses a fixed dev keypair committed under `crates/arena-cloud/dev/` and clearly named dev.
  - **Verify set:** `arena_proto::licence::verify(token, keys: &[VerifyKey], now)`, with `VerifyKey {kid, public_key}`. It checks the signature, the kid and `exp`/`iat`.
    - **Production:** `LICENCE_VERIFY_KEYS` (current plus next, two slots) is a constant in `arena-proto`. It ships **empty**, and an empty set refuses every token with a clear error.
    - **Dev:** the dev public key is used by dev and test builds only, behind `cfg(any(test, feature = "dev-keys"))`.
  - **Key generation:** `arena-cloud ops licence-keygen` prints a new PKCS#8 private PEM (to stdout, for SOPS) and the public key and kid to paste into `LICENCE_VERIFY_KEYS`. It's documented as an operator step.
  - Signing an entitlement never runs at the HTTP layer in this story. The heartbeat in 2.11 delivers licences.
- **Wizard step 2** (`tier`; requires `cafe` done):
  - `GET /v1/wizard/tier` → `{tiers: [{key, available, reason?}], billing_enabled}`.
    - **Free:** available only if the tenant has no other Free entitlement, else reason `one_free_branch`.
    - **Basic and Premium:** unavailable with reason `billing_disabled` while `ARENA_BILLING_ENABLED` is not `true`. It's off by default, and no code path turns it on in Epic 2.
  - `PUT /v1/wizard/tier {tier}`:
    - **free:** writes the entitlement (source `free`) and marks `tier` done, in one transaction.
    - **basic/premium:** while billing is disabled, 409 `BILLING_DISABLED` (new code), nothing written.
    - **free when another Free exists:** 409 `ONE_FREE_BRANCH` (new code).
  - Idempotent: picking Free again for the same branch is a no-op 200.
  - The `OwnedBranch` scope from 2.7 applies.
- **Onboarding UI** (`/setup/tier`):
  - A Monthly · Yearly `TabControl`, and three equal cards (a CSS grid with equal heights; it stacks on a phone).
  - Each card has the tier name, the price for the selected period, "+ 18% GST" where it applies, and its feature lines from the pricing table.
  - Disabled cards show their inline reason ("One Free branch per account", or "Paid plans are coming soon").
  - Continue is enabled only for an available choice, which triggers the 2.7 advance announcement. Every string lives in `en.json`.
- **Operator CLI:** `arena-cloud ops grant-comp --branch <id> --tier basic|premium --until <YYYY-MM-DD> --operator <name> --reason <text>`.
  - It uses the configured DB (env, or `--dev --data-dir`) and upserts the entitlement (source `comp`, tier, `until` at the end of that date in Asia/Kolkata).
  - It writes an `ops_audit` row (operator, action `grant_comp`, target branch, reason, details), marks `tier` done, and prints a summary.
  - **Exit codes:** 64 for a usage error, 1 for an unknown branch, a past date or a DB failure.
- **New codes:** `BILLING_DISABLED`, `ONE_FREE_BRANCH`.
- **Tests:**
  - **Rust:** the tier table values; the claims for each tier and source; sign → verify with the current key, the next key, an unknown kid, an edited claim, an expired token and an empty set; the wizard tier matrix; `grant-comp` (audit row, limits in the signed claims, step done, exit codes); cross-tenant `FORBIDDEN`.
  - **Playwright** (stubbed): cards and period switch, values from the pricing table, Free continue, disabled reasons, axe in light and dark at 360 px.

**Never:**
- Asking for payment.
- Writing an entitlement for Basic or Premium through the wizard in Epic 2.
- Committing a production private key, or any key other than the clearly named dev key.
- Hard-coded tier values outside the two tables.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Tier list | first branch | free available; basic/premium `billing_disabled` | N/A |
| Pick Free | `PUT tier free` | entitlement free/free; `tier` done | N/A |
| Free again | same branch | 200 no-op | N/A |
| Second Free | other branch of same tenant has Free | 409 `ONE_FREE_BRANCH` | N/A |
| Pick paid | basic, billing off | 409 `BILLING_DISABLED`; nothing written | N/A |
| Before cafe | no branch | 403 `FORBIDDEN` / redirect | N/A |
| Free claims | sign free | limits 20/3/1, intervals 0, no flags, `exp = iat + 7 d` | N/A |
| Comp | grant-comp premium | source comp, limits devices 200, audit row, `tier` done | N/A |
| Verify current/next | either slot key | ok | N/A |
| Unknown kid | other key | error | N/A |
| Edited claim | tier changed | error | N/A |
| Empty set | production set empty | error "no licence verify keys" | N/A |

</frozen-after-approval>

## Code Map

- 2.7 wizard (`crates/arena-cloud/src/api/{wizard.rs,owned.rs}`, `crates/arena-proto/src/wizard.rs`, store tenants/branches/progress, `apps/onboarding/src/routes/setup/**`, `packages/ui` `WizardProgress`) -- extend for step `tier`.
- 2.3 `ops restore-control` CLI parsing in `crates/arena-cloud/src/config.rs` -- pattern for `ops grant-comp` / `ops licence-keygen`.
- `packages/ui/src/components/TabControl.svelte`, `messages/en.json`.
- `Cargo.toml` + README pin table -- `jsonwebtoken`, possibly `ed25519-dalek`; keep `scripts/check-sqlite-linkage.sh` and `check:pins` passing.
- `deploy/README.md`, `deploy/secrets/README.md`, `deploy/k8s/base/deployment.yaml` -- `ARENA_LICENCE_SIGNING_KEY`, `ARENA_LICENCE_KID` secret refs; operator keygen step.

## Tasks & Acceptance

**Execution:**
- [ ] `crates/arena-store/control-migrations/0006_entitlements.sql`, store modules + tests.
- [ ] `crates/arena-proto/src/{tier.rs,licence.rs,error.rs,lib.rs}`, generated TS.
- [ ] `crates/arena-cloud/src/{licence.rs,api/wizard_tier.rs,config.rs,main.rs}`, `dev/` keypair, `tests/{licence.rs,tier.rs}`.
- [ ] `packages/ui/src/pricing.ts`, `index.js`, `en.json`; `apps/onboarding/src/routes/setup/tier/**`, tests.
- [ ] `deploy/**`, `README.md`.

**Acceptance Criteria:**
- Given `--dev` and an owner at step 2, when they pick Free and continue, then the wizard shows step 3 current, and a licence signed for that branch verifies with the dev verify key and carries `limits.devices = 20`.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`) -- pass (known `arena-hub-ui` sidecar failures excepted); clippy incl. `-p arena-store --features control`; fmt; `scripts/check-sqlite-linkage.sh`.
- `pnpm --filter @arena/onboarding check && pnpm --filter @arena/onboarding build && pnpm --filter @arena/onboarding test`; every `pnpm run check:*`; `pnpm run test:scripts`.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; on Sonnet). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/VG/ECH: expired comp — stuck step, one-Free bypass | high | P | `tier_at` vs the raw `tier` column |
| 2 | ECH: comp licence outlives `until` by up to 7 d | medium | P | `exp` always iat+7d |
| 3 | BH/ECH: grant-comp overwrites a paid plan | medium | P | unconditional upsert |
| 4 | BH: no DB backstop; race | medium | P | no unique index or CHECKs |
| 5 | BH/ECH: billing flag on → GET/PUT disagree | medium | P | refuse the flag in Epic 2 |
| 6 | ECH: verify overflow | low | P | direct |
| 7 | BH/ECH: signer not self-tested; kid vs verify set | medium | P | |
| 8 | BH: dev private key compiled into release | low | R | the production verify set never trusts it; `is_dev_key` refuses it as a production signer |
| 9 | BH: PEM parsing quirks | low | R | the self-test (#7) catches unusable keys |
| 10 | BH: pricing.ts vs tier table drift | low | R | feature lines are marketing copy; limits come from the server |
| 11 | BH/ECH: branch_id not sent; non-deterministic first branch | medium | P | |
| 12 | BH: tier UI alert/selected/403 | low | P | direct |
| 13 | BH: licence env required for every role | low | R | only the control role exists |
| 14 | BH: operator identity spoofable | low | R | the CLI runs with operator DB credentials |
| 15 | ECH: AlreadyFree doesn't mark the step done; missing-env exit code | low | P | direct |
| 16 | ECH: two-slot set unenforced | low | P | const assert |
| 17 | VG: skew boundary untested | medium | P | |
| 18 | VG: 409 recovery branches in TierStep untested | low | D | narrow race; add a stub flag later |
| 19 | BH: verify doesn't check claims against the tier table | low | R | the signature binds the claims; a later Hub-side rule (2.12) |
