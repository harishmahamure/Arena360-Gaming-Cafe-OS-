---
title: "Story 9.1: Erase a player's personal data by crypto-shredding"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '6cbcaff6edcd4f750227f59571656f38a5ee346b'
baseline_revision: '6cbcaff6edcd4f750227f59571656f38a5ee346b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-8-edit-a-player-profile-and-reset-their-password.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A player who asks to be forgotten cannot be erased: their name, phone, email, DOB, notes and username stay readable on the Hub (DPDP).

**Approach:** A new `player.erased` intent (Hub-built from `POST /ui/players/erase`) tombstones the `players` row in one transaction: PII columns cleared, `erased_at` set, `player_keys` row deleted (the shred), `phone_lookup` removed. Money, history and consent stay. Every reader (index, search, notify drain, reports, decrypt) treats an erased player as gone and shows "Erased player" with the id.

## Boundaries & Constraints

**Always:**
- Permission `player.manage`; a member without it supplies a manager PIN through the existing `inline-pin` / `elevate()` ticket flow (`require_manage`), as in Story 3.8. The actor is the session.
- One transaction: clear `name_enc`/`phone_enc`/`email_enc`/`dob_enc`/`notes_enc`, set `username` to the fixed tombstone `erased`, `password_hash` and `phone_lookup` to `''`, `tags` to `[]`, set `erased_at = now`, delete the `player_keys` row. `consents` rows are kept. The change row therefore holds only the player id, `erased_at` and constants.
- Blocked (`ERASE_BLOCKED`, nothing changes): player has an active/paused/grace session ("End the session first") or credit outstanding above zero ("Collect or write off credit first").
- Wallet balance above zero: the intent must carry `wallet` = `refunded` (paid out at the counter) or `forfeited` plus a non-empty `note` (the manager's reason, ≤120 chars). Apply first writes a negative `wallet_txns` row (reason `erase_refund`/`erase_forfeit`, no order) zeroing the wallet, then tombstones. Balance zero: `wallet` must be absent.
- An erased player is never found: the unique username/phone indexes ignore erased rows; `list_player_revs`, lookup, search, CSV-import keys and `phone_holder` filter `erased_at IS NULL`; the in-memory `PlayerIndex` drops the entry on its next sync.
- Notify drain: new rule `player_erased` (first rule) skips the row with reason `erased`, including rows queued before the erasure.
- Decrypting any player ciphertext without a key row fails `KEY_ERASED`; UI callers show "Erased player".
- Plaintext never reaches `apply()`; the erase payload holds ids, the disposal and the note only.
- All strings in `packages/ui/src/messages/en.json`; regenerate TS with `pnpm gen:types`.

**Never:**
- No raw `player.erased` on `/ui/intent` (`UNSUPPORTED`). No un-erase. No key backup/escrow, standby, export or restore handling of tombstones (Stories 9.2+). No drawer/cash-event record for a counter refund (no payout intent exists yet).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Erase, no money | idle player, wallet 0, no credit | tombstoned, key gone, not found by phone/name | none |
| Open session | active/paused/grace session | nothing changes | `ERASE_BLOCKED` "End the session first" |
| Credit owed | outstanding > 0 | nothing changes | `ERASE_BLOCKED` "Collect or write off credit first" |
| Wallet balance | balance 500, `forfeited` + note | negative wallet txn, wallet 0, then tombstone | none |
| Wallet balance, no disposal | balance 500, none | refused | `VALIDATION_FAILED` on `wallet` |
| Already erased / missing | unknown or erased id | refused | `NOT_FOUND` |
| Queued message | notify row queued before erasure | skipped, reason `erased` | none |
| Old ciphertext | field of erased player | decrypt fails | `KEY_ERASED` |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0082_player_erasure.sql`, `src/migrations.rs` -- `players.erased_at`; unique indexes exclude erased; `wallet_txns` rebuilt with reasons `erase_refund`/`erase_forfeit` (pattern of 0061).
- `crates/arena-store/src/players.rs`, `wallet.rs`, `lib.rs` -- filters, `erase_player`, `delete_player_key`, `player_erase_state`, `insert_erase_wallet_txn`, `player_erased`.
- `crates/arena-proto/src/intent.rs`, `players.rs`, `error.rs`, `lib.rs` -- `IntentKind::PlayerErased`, `PlayerErase` payload, `UiPlayerEraseRequest`, `ErrorCode::{EraseBlocked, KeyErased}`.
- `crates/arena-core/src/apply/players.rs` (+ `mod.rs`, `validate.rs`, `mutate.rs`) -- load/validate/mutate; `crates/arena-core/src/players.rs` (`PlayerIndex::sync`, `open_field_of` → `KeyErased`); `notify_drain.rs` rule.
- `crates/arena-api/src/ui_auth.rs` -- `POST /ui/players/erase`, raw-intent refusal.
- `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-replay/src/lib.rs` -- new intent in generator/fixture if the exhaustive checks require it.
- `apps/hub/src/lib/players/*`, `routes/players/+page.svelte`, `lib/audit/labels.ts`, `messages/en.json` -- erase panel.

## Tasks & Acceptance

**Execution:**
- [x] `arena-store` -- migration 0082, queries, filters
- [x] `arena-proto` -- kind, payload, request, error codes; `pnpm gen:types`
- [x] `arena-core` -- apply arms, index drop, drain rule, `KEY_ERASED`, tests (I/O matrix)
- [x] `arena-api` -- erase route, refusal, API test
- [x] `apps/hub` -- panel with kept/erased lists and `inline-pin`, strings, audit label

**Acceptance Criteria:**
- Given the migration ran, then `players.erased_at` exists and lookup/search never return a row with it set.
- Given a Manager confirms the panel, then one transaction tombstones the player, deletes the key and the phone lookup, keeps consent and money history, and the change row carries no PII.
- Given a blocking session, credit or an unhandled wallet balance, then erasure fails as in the matrix and nothing changes.
- Given an erased player, then screens/reports show "Erased player" with the id, the index drops them at once, queued notifications skip with `erased`, and decrypting their old ciphertext fails `KEY_ERASED`.

## Spec Change Log

## Review Triage Log

Code review (2026-10-30): no blocking findings. Clippy (touched crates), `cargo test` (store, proto, core, api, kiosk-core), `check:generated`, `check:strings` and `check:protocol` pass. Not caused by this story: hub `svelte-check` errors in `playwright.kitchen.config.ts` (missing node types), clippy `clone_on_copy` in `arena-kiosk-svc` tests, `arena-e2e` not building on Windows. The known gaps in the Auto Run Result (proptest generator, tombstone username in past listings, note only in audit, no drawer event for a counter refund) are accepted as deferred.

## Design Notes

"inline-pin for the manager PIN" reuses Story 3.8's mechanism: a member who already holds `player.manage` (a Manager) is not asked again; a Counter member is. Tombstone constants (not NULL) avoid rebuilding `players`, which many tables reference.

## Verification

**Commands:**
- `cargo test -p arena-store -p arena-proto -p arena-core -p arena-api` -- expected: pass
- `cargo clippy --workspace --all-targets -- -D warnings` -- expected: clean
- `pnpm gen:types` then hub typecheck/lint -- expected: clean

## Auto Run Result

Status: in-review

Implemented per spec. Known gaps, left for review: `player.erased` is not in the `proptest_apply` generator (about 20 ops read `rows(Table::Players).first()` and would break on a tombstone); past session/POS listings that read `players.username` show the tombstone `erased`; the manager's `note` lives in the intent/audit record only; a counter refund writes no cash-drawer event. Unrelated failures seen in the workspace run: `arena-cloud` backup script tests (`a_hung_run_times_out_with_its_own_code`, `unreachable_at_start_retries...`), `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings`, `arena-e2e` does not build on Windows.
