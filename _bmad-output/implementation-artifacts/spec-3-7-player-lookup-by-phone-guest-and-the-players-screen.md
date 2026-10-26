---
title: 'Story 3.7: Player lookup by phone, Guest, and the Players screen'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_commit: '7d519de788ac9c4a78334be390d6d1918b6c99ca'
baseline_revision: '7d519de788ac9c4a78334be390d6d1918b6c99ca'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-6-create-a-player-profile-with-protected-personal-data-and-con.md'
warnings: []
deferred:
  - 'Embedding player-lookup in the tile sheet start area: the start area arrives with Story 3.9; the component is built reusable.'
  - 'Outstanding credit shown first in a result: credit arrives with Story 3.18.'
  - 'Erasure filtering of the index: Story 9.1.'
---

<intent-contract>

## Intent

**Problem:** Staff can create players (3.6) but cannot find one. The Players screen has no lookup, no profile view, no Guest choice and no Ctrl+K.

**Approach:** The Hub keeps an in-memory index (name, username, phone, tags) decrypted with the branch KEK, loaded incrementally from `players` by rowid. `GET /ui/players?q=` answers digit-substring matches and `GET /ui/players/profile?id=` answers one profile with its latest consent. A reusable `PlayerLookup` component drives both on the Players screen.

## Boundaries & Constraints

**Always:**
- Both reads need `player.view` and a non-locked session; otherwise `PERMISSION_DENIED` / `SESSION_LOCKED`.
- Plaintext lives only in process memory. Nothing decrypted is written to disk, the change log, logs or errors.
- Query is reduced to digits; an empty digit string answers an empty list. Matches are players whose phone digits contain the query; best match first (earliest position, then name), at most 8.
- The index syncs on every read (`rowid > last`), so creates on this or another path appear without restart; a first read builds it.
- Lookup field: matches shown below as typed (name + phone), Enter picks the first, a secondary Guest button sits beside the field. With >= 10 digits and no match: "No player with that number." with Create profile (3.6 form, phone prefilled) and Guest.
- A picked player opens a profile showing name, username, phone, tags and consent state (granted/not granted + policy version).
- Ctrl+K (focus not in a text field, never during idle lock) focuses the lookup field on the current screen, else opens Players with the field focused. Ctrl+3 already opens Players.
- All new strings in `packages/ui/src/messages/en.json`; regenerate TS with `pnpm gen:types`.

**Never:**
- No disk-persisted index, no erasure filter (9.1), no edit/reset (3.8), no session start (3.9).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Partial digits | `98765` | players whose phone contains it | none |
| Formatted input | `+91 98765-4` | same as digits `91987654` | none |
| No digits | `abc` | empty list | none |
| Created elsewhere | player added after index built | found on the next read | none |
| Unknown id | profile for missing id | `NOT_FOUND` | none |
| No permission | role lacks `player.view` | refused | `PERMISSION_DENIED` |
| 3,000 players | any query | answered in < 100 ms server-side | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/players.rs` -- add `PlayerHit`, `PlayerConsent`, `PlayerProfile`; `pnpm gen:types`.
- `crates/arena-store/src/players.rs`, `lib.rs` (`Store::player_rows_after`, `Store::latest_consent`, table-exists guards).
- `crates/arena-core/src/snapshot.rs` (`SnapshotReader` passthroughs), `crates/arena-core/src/players.rs` (`PlayerIndex`, `search`, `profile`, tests incl. 3,000-player timing).
- `crates/arena-api/src/ui_auth.rs` -- `GET /ui/players`, `GET /ui/players/profile`, index held in `UiAuth`; tests in `ui_player_tests.rs`.
- `apps/hub/src/lib/auth.ts` (`findPlayers`, `loadPlayer`), `apps/hub/src/lib/players/PlayerLookup.svelte` (new), `routes/players/+page.svelte`, `lib/players/CreateProfile.svelte` (phone prefill), `routes/+layout.svelte` + `lib/nav.ts` (Ctrl+K), `messages/en.json`, `apps/hub/tests/players.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto`, `arena-store`, `arena-core` -- types, queries, `PlayerIndex` + unit tests
- [x] `arena-api` -- two read routes, permission gate, API tests
- [x] `apps/hub` -- `PlayerLookup`, profile panel, Guest, no-match state, Ctrl+K, strings, Playwright

**Acceptance Criteria:**
- Given the Players screen, when staff type digits, then matches (name + phone) appear below, Enter picks the top one and a Guest button sits beside the field.
- Given 3,000 players, when a query is served, then the server answers in under 100 ms.
- Given no match for a complete number, when shown, then "No player with that number." appears with Create profile (phone prefilled) and Guest.
- Given a picked player, when the profile opens, then it shows name, username, phone, tags and consent state.
- Given focus outside a text field, when staff press Ctrl+K, then the lookup field is focused (Players screen opened if needed); Ctrl+3 opens Players.

## Design Notes

Sync-by-rowid replaces a separate "add on create" hook: one mechanism covers this Hub's creates and rows replicated to a follower. Erased players are not filtered here (Story 9.1 adds `players.erased_at`).

## Verification

**Commands:**
- `pnpm run gen:types && pnpm run check:generated && pnpm run check:strings` -- expected: pass
- `cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace` -- expected: pass
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub exec playwright test tests/players.spec.ts --project=desktop` -- expected: pass
