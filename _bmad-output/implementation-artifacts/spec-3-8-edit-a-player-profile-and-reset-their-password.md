---
title: 'Story 3.8: Edit a player profile and reset their password'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_commit: '96d116b570e81b2c5cd479f9bac9bba2274d9426'
baseline_revision: '96d116b570e81b2c5cd479f9bac9bba2274d9426'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-6-create-a-player-profile-with-protected-personal-data-and-con.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-7-player-lookup-by-phone-guest-and-the-players-screen.md'
warnings: []
deferred:
  - 'Erasure filtering of the index: Story 9.1.'
  - 'Players signing in with the new password on a kiosk: Epic 6.'
---

<intent-contract>

## Intent

**Problem:** Staff can create and find a player (3.6, 3.7) but cannot correct a profile or give back an account whose password was forgotten.

**Approach:** The profile panel gets an Edit profile form (phone, name, email, DOB, notes, tags, consent) and a Reset password form. `POST /ui/players/update` re-seals every personal field under the player's existing key and submits `player.update` with `base_rev`. `POST /ui/players/password` hashes the new password (Argon2id) and submits `player.password_reset`. The lookup index re-reads any player whose `rev` changed, so a new phone or name is found at once and the old phone no longer matches.

## Boundaries & Constraints

**Always:**
- Both routes need `player.manage`; a member without it gets `inline-pin` and the `elevate()` ticket flow with intent kind `player.update` / `player.password_reset`. The actor is the session.
- `player.update` carries `base_rev`; a mismatch is `STALE` and the UI reloads the profile and shows the current values with a notice. Missing player is `NOT_FOUND`.
- Phone uniqueness: a phone held by another non-deleted player is `PHONE_TAKEN` on `phone`; keeping one's own phone is fine. Username is not editable.
- Consent: the form's checkbox is the desired state. When it differs from the latest consent row, a new `consents` row is appended (id `{player}:transactional:{new rev}`, current policy version, `now`); earlier rows stay. Unchanged consent writes no row.
- Plaintext (name, phone, email, DOB, notes, password) never reaches `apply()`, the change log or logs; the change row holds ciphertext, `phone_lookup` and the new hash. The audit viewer already redacts these columns.
- Password: 6 to 128 characters and both entries equal, else `VALIDATION_FAILED` on `password` / `confirm` and nothing changes.
- All new strings in `packages/ui/src/messages/en.json`; regenerate TS with `pnpm gen:types`.

**Never:**
- No raw `player.update` / `player.password_reset` on `/ui/intent` (`UNSUPPORTED`): ciphertext and hashes are produced by the Hub only.
- No username change, no erasure (9.1), no player-side password change.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Edit | new name and phone, current `base_rev` | fields re-sealed, `rev` + 1, index finds new phone, not the old | none |
| Stale | `base_rev` behind | refused, current values shown | `STALE` |
| Phone of another player | edit to a held number | refused on phone | `PHONE_TAKEN` |
| Own phone, other formatting | same number | applies | none |
| Consent flipped | checkbox changed | extra `consents` row, earlier rows kept | none |
| Reset | matching 6+ char passwords | new Argon2id hash, change row has no plaintext | none |
| Reset mismatch / short | differing or < 6 chars | inline error, nothing sent or changed | `VALIDATION_FAILED` |
| No permission | role lacks `player.manage` | inline PIN, then applies with `elevated_by` | `PERMISSION_DENIED` without ticket |

</intent-contract>

## Code Map

- `crates/arena-proto/src/players.rs`, `intent.rs`, `lib.rs` -- `PlayerEditForm`, `PlayerPasswordForm`, `PlayerUpdate`, `PlayerPasswordReset`, request types, shared `Details::check`, two intent kinds, extended `PlayerProfile` (`rev`, email, dob, notes).
- `crates/arena-store/src/players.rs`, `lib.rs` -- `phone_holder`, `update_player`, `set_player_password`, `list_player_revs`, `find_player` (replace rowid listing).
- `crates/arena-core/src/players.rs` -- `seal_details`/`reseal`, rev-based `PlayerIndex::sync`, free `profile`; `apply/players.rs` + `validate.rs`, `mutate.rs`, `guards.rs`, `mod.rs` arms; `snapshot.rs` passthroughs.
- `crates/arena-api/src/ui_auth.rs` -- `POST /ui/players/update`, `POST /ui/players/password`, shared `player_write` prelude, raw-intent refusal.
- `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-replay/src/lib.rs` -- edit and reset in the generator and the seeded fixture.
- `apps/hub/src/lib/auth.ts`, `lib/players/EditProfile.svelte` (new), `routes/players/+page.svelte`, `lib/audit/labels.ts`, `messages/en.json`, `apps/hub/tests/players.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- payloads, forms, kinds, shared validation; `pnpm gen:types`
- [x] `arena-store` -- update/reset/rev queries
- [x] `arena-core` -- reseal, apply (base_rev, validate, mutate, consent append), rev-synced index, tests, proptest ops
- [x] `arena-api` -- two routes, permission gate, raw-intent refusal, API tests
- [x] `apps/hub` -- `EditProfile`, profile actions, stale handling, strings, audit labels, Playwright

**Acceptance Criteria:**
- Given a profile, when staff edit phone, name, email, DOB, notes or tags and save, then changed data is re-encrypted with the player's key, `base_rev` is checked (`STALE` shows current values) and uniqueness still applies.
- Given an edit, when committed, then a search by the new phone or name finds the player and the old phone no longer does.
- Given Reset password with two equal valid entries, then a new Argon2id hash is stored and no password appears in any log or change row; unequal or short entries show an inline error and change nothing.
- Given the consent checkbox changed, then a new `consents` row is appended and earlier rows are kept.
- Given a Counter without `player.manage`, then `inline-pin` opens in place.

## Design Notes

Every personal field is re-sealed on each edit (fresh nonces, same key) instead of diffing against the stored ciphertext: the form always carries the full profile, and a field cleared in the form clears the column. The index no longer keys on rowid: it compares `(player_id, rev)` on every read and re-reads only the players whose `rev` moved, so edits, creates and replicated rows share one mechanism.

## Verification

**Commands:**
- `pnpm run gen:types && pnpm run check:protocol && pnpm run check:strings` -- expected: pass (`check:generated` passes once the generated files are committed)
- `cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace` -- expected: pass
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub exec playwright test tests/players.spec.ts --project=desktop` -- expected: pass
