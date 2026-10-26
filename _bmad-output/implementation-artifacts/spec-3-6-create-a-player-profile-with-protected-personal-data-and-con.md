---
title: 'Story 3.6: Create a player profile with protected personal data and consent'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_commit: '1bc0ba31a1ac106281dc761aabddfc369e173484'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-5-stock-in-with-expiry-lots-and-weighted-average-cost.md'
warnings:
  - oversized
deferred: []
---

<intent-contract>

## Intent

**Problem:** Counter staff cannot create a player. No `players`, `consents` or `player_keys` tables exist, nothing encrypts personal data, and the Players screen is a placeholder.

**Approach:** A Players screen with a "Create profile" form posts plaintext to a new `POST /ui/players`. The Hub validates, hashes the password (Argon2id), encrypts name/phone/email/DOB/notes with a fresh per-player AES-256-GCM key wrapped by a branch KEK in the OS keystore, and submits one `player.create` intent that carries only ciphertext, a keyed phone hash and the wrapped key. `apply()` writes `players`, an append-only `consents` row and `player_keys` (outside the change log).

## Boundaries & Constraints

**Always:**
- Form fields: username, password (show/hide, paste allowed), phone (default +91), name; optional email, DOB, notes, tags; consent checkbox unticked, text = existing `player.signup.consent`. Focus starts in username; on a field error focus moves to that field and nothing is written.
- `player.create` needs `player.manage` (Counter, Manager, Owner hold it); a member without it gets `inline-pin` and the existing `elevate()` ticket flow. The actor is the session, never the client.
- Plaintext never reaches `apply()`, the change log, the idempotency store or logs: change rows hold only ciphertext (base64url text), `phone_lookup` (HMAC-SHA256 under a key derived from the KEK), the Argon2id hash and ids. `player_keys` is not a change-log table. Audit viewer redacts `password_hash`, `phone_lookup` and every `*_enc` column and shows the consent row (player id, purpose, version, state).
- Password >= 6 chars; phone `+` and 8-15 digits after stripping spaces/dashes; username 3-32 chars `[a-z0-9._-]` lower-cased; name 1-80; email must contain one `@`; DOB a valid past date; notes <= 500; <= 10 tags of <= 24 chars. Violations are `VALIDATION_FAILED` naming the field. Username or phone held by a non-deleted player: `USERNAME_TAKEN` / `PHONE_TAKEN` on that field, nothing written.
- Consent row: purpose `transactional`, `policy_version` from the catalogue (`player.policy_version`), granted 0/1, timestamp. Consents are append-only (DB triggers).
- All new strings in `packages/ui/src/messages/en.json`; regenerate TS with `pnpm gen:types`.

**Never:**
- No lookup index or search (Story 3.7), edit/reset (3.8), erasure (9.1), key backup/escrow (9.2), Android keystore.
- No client-supplied ciphertext: `/ui/intent` refuses `player.create`.
- No KEK, DEK, nonce or randomness inside `apply()` (replay must reproduce the rows).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Happy path | valid form, consent ticked | player + granted consent rows; key row; UI shows created | none |
| No consent | valid form, unticked | consent row with granted 0 | none |
| Username taken | same username (any case) | refused | `USERNAME_TAKEN` field `username` |
| Phone taken | same number, other formatting | refused | `PHONE_TAKEN` field `phone` |
| Short password | 5 chars | refused client + server | `VALIDATION_FAILED` field `password` |
| Bad phone | `+91 12` | refused | `VALIDATION_FAILED` field `phone` |
| No permission | role lacks `player.manage` | inline PIN, then applies with `elevated_by` | wrong PIN applies nothing |

</intent-contract>

## Code Map

- `crates/arena-proto/src/players.rs` (new) -- `PlayerForm` (plaintext request), `PlayerCreate` (sealed intent payload), limits, `PlayerForm::check`; `intent.rs` -- `player.create` kind/payload/requirement; `error.rs` -- `USERNAME_TAKEN`, `PHONE_TAKEN`.
- `crates/arena-store/migrations/0024_players.sql`, `migrations.rs`, `src/players.rs` (new), `branch.rs` (`Table::Players`, `Table::Consents`, `RowKey`), `lib.rs` exports.
- `crates/arena-core/src/players.rs` (new) -- `Kek` source trait, `seal`/`open_field`; `apply/players.rs` (new) + `mod.rs`, `guards.rs`, `validate.rs`, `mutate.rs` arms; `audit.rs` `REDACTED`.
- `crates/arena-api/src/ui_auth.rs` -- `POST /ui/players`, `run_intent` extracted from `intent()`, `with_player_kek`; `lan/registry.rs`/`cloud` exhaustive `ErrorCode` matches.
- `crates/arena-tasks/src/keystore.rs` -- named keystore file; `crates/arena-hub/src/lib.rs` -- KEK from keystore, wired into `UiAuth`.
- `crates/arena-core/tests/proptest_apply.rs`, `arena-replay/src/lib.rs` -- player op in generator and seeded fixture.
- `apps/hub/src/lib/auth.ts` (`createPlayer`), `apps/hub/src/lib/players/CreateProfile.svelte` (new), `routes/players/+page.svelte`, `messages/en.json`, `apps/hub/tests/players.spec.ts` (new).

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- payloads, limits, validation, error codes, kind wiring; `pnpm gen:types`
- [x] `crates/arena-store` -- migration 0024, rows, queries, `Table` entries
- [x] `crates/arena-core` -- sealing, apply (validate/mutate), audit redaction, tests, proptest op
- [x] `crates/arena-api`, `arena-tasks`, `arena-hub` -- `/ui/players`, KEK wiring, refuse raw `player.create`, API test
- [x] `apps/hub` -- form component, Players screen, `createPlayer`, strings, Playwright spec incl. <15 s

**Acceptance Criteria:**
- Given the Players screen, when staff press Create profile, then the form opens with focus in username, an unticked consent box and +91 in phone.
- Given valid input, when submitted, then `players`, `consents` and `player_keys` rows exist and the `changes` rows contain no name, phone, email or password in clear.
- Given a duplicate username or phone, when submitted, then the field shows the error, focus moves to it and nothing is written.
- Given the audit viewer, when a profile is created, then it shows the player id and consent purpose, version and state.
- Given a Playwright run, when the required fields are typed and submitted, then it completes in under 15 s from opening the form.

## Spec Change Log

## Review Triage Log

## Design Notes

DEK is random per player so deleting its `player_keys` row shreds the data (Story 9.1). Sealing happens in the API layer before `apply()` so `apply()` stays deterministic; a retry with a new sealed payload under the same idempotency key is refused by dedupe, so the UI mints a fresh key per submit.

## Verification

**Commands:**
- `pnpm run gen:types && pnpm run check:generated && pnpm run check:protocol && pnpm run check:strings` -- expected: pass
- `cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace` -- expected: pass
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub exec playwright test tests/players.spec.ts --project=desktop` -- expected: pass
