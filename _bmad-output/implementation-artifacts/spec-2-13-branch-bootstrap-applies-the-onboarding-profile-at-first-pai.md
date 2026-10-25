---
title: 'Story 2.13: branch.bootstrap applies the onboarding profile at first pairing, with branch code lock'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '76957d3'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-12-hub-verifies-its-licence-against-trusted-time-and-enforces-i.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-10-hub-installer-pairs-with-the-code-and-retries-in-place.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A paired Hub has an identity and a licence but no branch. The owner's café name, timezone, currency and tax details live only in the cloud, the owner has no staff row at the Hub, and the branch code can change forever.

**Approach:**
- `POST /v1/pair` also returns a `branch.bootstrap` command built from the cloud branch and owner. The setup code never travels, only its Argon2id hash and expiry.
- The Hub applies it through `apply()` with `actor{owner, tenant_id}`, in the same transaction that stores the identity and the licence. One intent writes the branch row, the settings, the built-in roles and the owner's staff row (no password or PIN).
- The cloud keeps the owner's plaintext setup code sealed in `gz_control`; the wizard shows it once after step 3 and deletes it.
- The branch code becomes a setting that `settings.set` edits until a system function `lock_branch_code()` sets `branch.code_locked_at`.

## Boundaries & Constraints

**Always:**
- **Hub migration** `0018_staff_setup_code.sql` (0016 is Story 2.23, 0017 Story 5.3): `staff.setup_code_hash`, `staff.setup_expires_at`, both nullable. **Control migration** `0010_owner_setup_codes.sql`: `owner_setup_codes(branch_id PK, code_hash, hub_expires_at, ciphertext, nonce, created_at, shown_at)`.
- **Command** (`arena-proto`): `BranchBootstrapCommand{idempotency_key, profile{name, branch_code, currency, tz, day_start_min, country, gst_registered, gstin, tax_mode}, owner_staff{name, login = owner phone, role = "Owner", setup_code_hash, setup_expires_at}}`, a new `bootstrap` field of `HubPairResponse`.
  - `idempotency_key` is a ULID derived from the branch id, so every pairing of one branch sends the same key.
  - `day_start_min` is 0 (the cloud holds no day start; the owner changes it in Settings → Branch). `country` is added to the AC's profile so the Hub can seed `branch.tax.country` (the Story 5.1 deferral).
  - `setup_expires_at` = pairing time + 72 h (the staff setup code lifetime). The first pairing creates the code; a re-pair reuses the stored hash.
- **Hub apply**: `BranchBootstrap` gains `onboarding: Option<BootstrapOnboarding{country, gst_registered, gstin, tax_mode, owner_staff}>`. `None` is the dev fixture and behaves as before.
  - An `owner` actor may submit `branch.bootstrap` only with `onboarding`; `system` still may always.
  - Validation: branch code 3–6 upper-case letters or digits; `branch.tax` from `{country, gst, gstin}` must pass `TaxSettings::check`; `tax_mode` must equal `gst_in` only for a GST-registered Indian branch; login E.164; name 1–80; role `Owner`; hash starts `$argon2id$`.
  - Rows: branch, all default settings (`branch.tax` from the profile), `branch.code` (the code) and `branch.code_locked_at` (empty), roles, and the owner staff row with `password_hash` and `pin_hash` null. All change rows share the one intent id and the owner actor.
  - `record_pairing(identity, licence, Some(command))` runs identity, licence and bootstrap in one transaction: any failure leaves the Hub unpaired. The same key replays its stored outcome; another key fails `ALREADY_BOOTSTRAPPED` and changes nothing.
- **Branch code**: settings `branch.code` (rule: 3–6 upper-case letters or digits) and `branch.code_locked_at` (system managed; empty or the lock time in ms). `settings.set` on `branch.code` fails `BRANCH_CODE_LOCKED` (new code, 409) once locked. `arena_core::lock_branch_code(tx, now)` sets the lock idempotently and returns the changed `RowKey` for the caller's change rows. The snapshot reads `branch.code` and reports `code_locked`.
- **Owner setup code (cloud)**: generated at the first pairing (8 characters, pairing alphabet), sealed with the AES-256-GCM key already used for TOTP, with the branch id as AAD. `POST /v1/wizard/setup-code/reveal` (owner session, own branch only) answers it once after the Hub's first heartbeat (`STEP_NOT_READY` before) and wipes the plaintext; later calls and calls past 24 h answer `NOT_FOUND`. A heartbeat also wipes an expired plaintext. The hash row stays.
- **UI**: the onboarding step-4 page shows "Your Hub sign-in code: XXXX XXXX. Enter it at the Hub to choose your PIN and password." with Copy. The Hub tax screen shows "Locked after the first invoice" under the invoice prefix once the code is locked.
- **Release builds** have no dev seed; an unpaired Hub has no branch and no staff (test).
- **Tests:** apply matrix (owner bootstrap rows, tax mode, replay, different key, validation, permission), branch code edit/lock/`lock_branch_code`; cloud (command content, GST/non-GST, re-pair stability, reveal once, 24 h, heartbeat wipe, tenant isolation); Hub end to end (rows after pairing, unpaired start empty); onboarding and Hub Playwright.

**Never:**
- The setup code in plaintext in a command, a change row, a log or an unsealed column.
- A half-paired Hub (identity without branch).
- Editing the branch code once locked.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First pairing | valid code | identity, licence, branch, settings, roles, owner staff in one commit | N/A |
| GST-registered Indian branch | `gst_registered`, valid GSTIN | `tax_mode gst_in`, `branch.tax` gst true | N/A |
| Non-Indian branch | `country AE` | `simple`, `branch.tax.country AE` | N/A |
| Same key again | retry | stored outcome, nothing written | N/A |
| Other key | branch exists | `ALREADY_BOOTSTRAPPED`, Hub stays unpaired | surfaced to the installer |
| tax_mode mismatch | `gst_in` for a non-GST branch | `VALIDATION_FAILED` on `tax_mode`, nothing written | N/A |
| Re-pair in cloud | second code | same key, same owner hash | N/A |
| Reveal before heartbeat | step 3 open | `STEP_NOT_READY` | N/A |
| Reveal twice / after 24 h | second call | `NOT_FOUND` | N/A |
| Code edit, unlocked | `branch.code` valid | applied | N/A |
| Code edit, locked | any | `BRANCH_CODE_LOCKED` | nothing written |
| `lock_branch_code` twice | N/A | first sets, second no-op | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/{pairing,intent,tax,error,snapshot,wizard}.rs`.
- `crates/arena-store/migrations/0018_staff_setup_code.sql`, `control-migrations/0010_owner_setup_codes.sql`, `src/{staff,branch}.rs`, `src/control/{setup_codes,hubs,owner}.rs`.
- `crates/arena-core/src/apply/{validate,mutate,settings_set,seed_defaults,branch_code}.rs`, `rbac.rs`, `settings_defaults.rs`, `writer.rs`, `snapshot.rs`.
- `crates/arena-cloud/src/api/{pair,wizard_install,error}.rs`; `crates/arena-tasks/src/cloud.rs`.
- `apps/onboarding/src/lib/SetupCodeNotice.svelte`, `apps/hub/src/routes/settings/branch/tax/+page.svelte`, `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

- [x] proto, store, core, cloud, tasks.
- [x] onboarding notice and Hub lock note.
- [x] tests.

**Acceptance Criteria:** the Given/When/Then of Story 2.13 in `epics.md`, as mapped by the matrix above.

## Verification

`cargo test --workspace --locked`; `cargo test -p arena-hub -p arena-api --features dev-fixtures`; clippy with dev-fixtures `-D warnings`; `cargo fmt --check`; every `pnpm run check:*`; `pnpm run test:scripts`; prettier; Hub `test:shell` and onboarding Playwright (unique `ARENA_PW_PORT`); app builds except marketing; `pnpm check:generated` after the commit.

## Accepted limits

- No editable branch code field in Settings → Branch yet (the intent path and the lock exist); deferred.
- A branch code edited at the Hub is not reported to the cloud, whose `(tenant, branch_code)` is unique.
- `branch.seed_defaults` backfills `branch.code` from the branch row but cannot seed the tax country (an upgraded branch has no cloud profile).
- A lost response to the reveal call loses the one-time code; staff setup-code reissue (Story 2.16) is the recovery.
- A Hub that fails after the cloud committed the pairing still burns the pairing code (deferred from 2.10).

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter with this spec, Verification Gap; on Sonnet). P = patch, D = defer, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH: a profile the Hub rejects (or a cloud-side build failure) surfaced after the pairing code was spent | high | P | one shared `arena_proto::check_bootstrap` used by the Hub's validate and by the cloud before `pair_hub`; test `a_profile_the_hub_would_refuse_fails_before_the_pairing_code_is_spent` |
| 2 | BH/ECH: a re-pair after 72 h handed the Hub an already expired owner code | medium | P | re-pair refreshes `hub_expires_at` to now + 72 h; test `a_re_pair_after_the_hub_expiry_hands_over_a_fresh_seventy_two_hours` |
| 3 | BH/ECH: reveal cleared the ciphertext before it was opened; a failed open or a transient client failure lost the code | medium | P | `peek_setup_code`, open, then `consume_setup_code` (exactly one wins); the client retries on `INTERNAL`; test `two_simultaneous_reveals_show_the_code_to_exactly_one` |
| 4 | ECH: `Outcome::Replayed(Err)` from the bootstrap would commit a half-paired Hub | medium | P | mapped to `Err`, rolling everything back; test `pairing_with_a_bootstrap_is_all_or_nothing` |
| 5 | ECH: `BRANCH_CODE_LOCKED` came after `STALE`/`VALIDATION_FAILED` | medium | P | lock checked in `settings_set::load`; test `a_locked_branch_code_answers_locked_before_any_other_check` |
| 6 | VG: no test for rollback, fy-month/country mapping, one event, seed_defaults backfill values, owner-actor limits, 24 h edge, concurrent reveal, leak check using the real code | high | P | tests added in `apply/tests.rs`, `arena-cloud/tests/pair.rs`, `arena-hub/tests/pairing.rs` |
| 7 | BH: `rbac` trusts a client-supplied owner actor | high | R | no ingress builds an owner actor from client input (UI: staff session; LAN: device); owner is accepted only with `onboarding` and only before a branch exists |
| 8 | BH/ECH: `lock_branch_code` has no caller and does not bump `rev` | medium | D | the invoice story adds the returned `RowKey` to its `written` list, which `stamp` versions |
| 9 | BH/ECH: `branch.code` and the `branch.branch_code` column can drift; the cloud is not told of an edit | medium | D | cloud uniqueness per tenant |
| 10 | ECH: legacy backfilled `branch.code` can break its own rule | low | R | dev-only branches |
| 11 | BH: `setup_expires_at` not checked against time; hash not parsed | low | D | |
| 12 | BH/ECH: plaintext window runs from pairing; a re-pair needs the new Hub's first beat to reveal; no sweeper for a Hub that never beats | low | D | |
| 13 | BH: 40-bit code behind a 72 h hash is brute-forceable offline from change rows or backups; `mfa_key` reused without a purpose label; FK has no cascade | low | D | |
| 14 | VG: error-code status mappings, `CloudPairer` `ALREADY_BOOTSTRAPPED` mapping, Playwright copy/denied-clipboard states, boundary values for phone length | low | D | |

Verification after fixes: see the commit message and the final report.
