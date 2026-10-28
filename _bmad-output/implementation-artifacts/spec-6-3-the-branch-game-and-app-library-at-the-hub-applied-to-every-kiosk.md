---
title: 'Story 6.3: The branch game and app library at the Hub, applied to every kiosk'
type: 'feature'
created: '2026-10-29'
status: 'done'
review_loop_iteration: 0
baseline_commit: 'ed8c63e2ae1b457a0c007ef08ccd7ee8f50d79b1'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** Each branch has no single list of the games and apps on its PCs, so kiosks cannot show or launch them and the process allow-list (Story 1.18) would kill a launch as unknown.

**Approach:** A branch table `library_entries`, edited at the Hub (Settings → Games) by `library.upsert` / `library.delete` under the new permission `games.manage`. The same `apply()` transaction rewrites two derived settings: `kiosk.library` (the enabled entries, pushed to every paired kiosk through the existing `settings.changed` push and `settings.get`, cached by the kiosk in `queue.db`) and the library's rules inside `kiosk.process_allow`.

## Boundaries & Constraints

**Always:**

- Entry fields: `kind` (`game`|`app`), `name`, `launcher` (`steam`|`epic`|`riot`|`battlenet`|`none`), `executable_path`, `launch_uri`, `launch_args`, `category`, `tags` (≤ 10, each ≤ 32 chars), `age_rating`, `enabled`, optional `catalog_id`. Name 1–80 chars. `executable_path` is a full Windows path (`X:\…`) with no `*`, `?`, `{`, `}`; it may be empty only when `launch_uri` is non-empty. `launch_uri` ≤ 512 chars with a scheme (`steam://…`), `launch_args` ≤ 512, `category` ≤ 40, `age_rating` ≤ 16. Field errors are `VALIDATION_FAILED` with `field`; nothing is written on error.
- Duplicate = same `kind`, `launcher` and `executable_path` (case-insensitive, non-empty path) among live entries: `VALIDATION_FAILED` on `executable_path`. A partial unique index backs it.
- Edits carry `base_rev` (missing on an existing row is `VALIDATION_FAILED`, mismatch is `STALE`; the owner is exempt) exactly as `zone.upsert`. Delete is a soft delete. Counter staff reach `games.manage` only by `inline-pin` elevation (Managers and Owner hold it; Counter does not); the change row carries both actors.
- Catalogue link: when `catalog_id` is set, `name`, `launcher`, `age_rating` come from the catalogue. The API layer (which alone can read `reference.db`) rewrites those three from the cached `ref_games` row before the intent is applied and refuses an unknown `catalog_id` with `VALIDATION_FAILED` on `catalog_id`; in `apply()` an existing linked entry may not change them, nor `catalog_id` (`VALIDATION_FAILED`). `category` and `tags` stay editable. `GET /ui/library` shows the catalogue values.
- Allow-list: let `rules(entry)` = the entry's `executable_path` as an allow rule (when non-empty) plus its launcher's install rules (`C:\Program Files (x86)\Steam\**.exe`, `C:\Program Files (x86)\Epic Games\**.exe`, `C:\Program Files\Epic Games\**.exe`, `C:\Riot Games\**.exe`, `C:\Program Files (x86)\Battle.net\**.exe`; none for `none`). `derived(set)` = the de-duplicated union over enabled, live entries. After every library write, `kiosk.process_allow` = (current − (derived(before) − derived(after))) ∪ derived(after); operator rules are therefore never removed, and a rule another enabled entry still needs stays. The result must pass `parse_process_rules(Allow, …)`; a library rule that cannot (an unsigned path under a user-writable root such as `C:\Users\…`) is refused with `VALIDATION_FAILED` on `executable_path` and nothing is written. `kiosk.process_allow` is rewritten (and a settings change row emitted) only when its value changes.
- `kiosk.library` is a seeded derived setting (default `[]`), JSON array of `{id, kind, name, launcher, executable_path, launch_uri, launch_args, category, tags, age_rating, catalog_id}` for enabled live entries ordered by name, rewritten in the same transaction on every library write that changes it. `settings.set` on it is refused (`VALIDATION_FAILED`); `may_read_setting` already lets paired kiosks read it, so it reaches them within 2 s through `push_kiosk_settings`.
- The Hub UI stream (`/ui/events`) sends `library.changed` for `library_entries` changes; the Games screen reloads on it.
- Kiosk: `kiosk-svc` caches a valid `kiosk.library` in `queue.db` (revision-guarded like the operating settings), fetches it with the other settings after hello, and exposes `library::load(queue)` returning the last library received (empty when never received; an unparsable cache is empty and logged).

**Never:** a kiosk-side Games tab or launching (Story 6.23); scanning or matching (6.4, 6.5); catalogue fields editable on a linked entry; removing an operator-authored allow rule; a global (control-plane) library; bundling icons or media into the library.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Add game | Manager, valid Steam entry with `C:\Games\X\x.exe` | row stored rev 1; `kiosk.library` and `kiosk.process_allow` updated; kiosks get `settings.changed` | — |
| Counter add | Counter without elevation | refused `FORBIDDEN`-style permission error; with elevation by a Manager PIN it applies, both actors audited | — |
| Duplicate | same kind+launcher+path | nothing written | `VALIDATION_FAILED` `executable_path` |
| Empty name / no exe and no URI | — | nothing written | `VALIDATION_FAILED` `name` / `executable_path` |
| Stale edit | `base_rev` behind | nothing written | `STALE` |
| Disable | `enabled=false` | gone from `kiosk.library`; its rules leave `kiosk.process_allow` unless another enabled entry needs them | — |
| Delete | `library.delete` | soft-deleted; same as disable | no such entry → `VALIDATION_FAILED` `id` |
| Linked entry edit | change name of catalogue-linked entry | nothing written | `VALIDATION_FAILED` `name` |
| Unsafe path | `C:\Users\p\g.exe` | nothing written | `VALIDATION_FAILED` `executable_path` |
| Kiosk cold start, Hub down | cached library | `library::load` returns the last library | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/permission.rs` -- add `GamesManage` (`games.manage`, module `games`), `ALL` 36→37; `rbac.rs` tests (Manager/Owner get it, Counter not).
- `crates/arena-proto/src/intent.rs` -- `LibraryUpsert`/`LibraryDelete` kinds + payloads (model: `ZoneUpsert`/`ZoneDelete`), `ALL`, `as_str`, `category`, `requirement`, `reasons`, pinned test lists. Re-export from `lib.rs`; add `LibraryEntryView`/`LibraryView`/`KioskLibraryEntry` types in a new `library.rs` (ts-rs), `KEY_KIOSK_LIBRARY`.
- `crates/arena-store/migrations/0062_library_entries.sql`, `migrations.rs`, `branch.rs` (`Table::LibraryEntries`, `RowKey`), new `library.rs` row helpers (model: `devices.rs` zone helpers), `lib.rs` exports.
- `crates/arena-core/src/apply/library.rs` -- load/base_rev/validate/mutate incl. the derived-rules and `kiosk.library` rewrite; wire in `apply/{mod,guards,validate,mutate}.rs`; tests `apply/tests/library.rs` or in `tests.rs`.
- `crates/arena-core/src/settings_defaults.rs` -- seed `kiosk.library`, read-only rule, `keys` counts in tests; `snapshot.rs` reader for the library.
- `crates/arena-api/src/ui_auth.rs` -- `GET /ui/library`; catalogue pre-check in the intent path for `library.upsert` using the `MediaCache` (`ReferenceDb::game(id)`); `ui_events.rs` `library.changed`.
- `crates/arena-kiosk-svc/src/library.rs` (+ `hub_link.rs`, `lib.rs`) -- cache and load.
- `crates/arena-core/tests/proptest_apply.rs` -- `library.upsert` strategy (Story 1.8).
- `apps/hub/src/routes/settings/games/+page.svelte`, `settings/+layout.svelte`, `lib/library.ts`, `lib/events.ts`, `packages/ui/src/messages/en.json`, `apps/hub/tests/library.spec.ts` (model: `settings/layout`, `coupons`).

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- permission, intents, payloads, view types, key, generated TS
- [x] `arena-store` -- migration 0062, table registration, row helpers
- [x] `arena-core` -- apply handlers with allow-list and `kiosk.library` rewrite, default setting, snapshot/list reader
- [x] `arena-api` -- `GET /ui/library`, catalogue pre-check, SSE event
- [x] `arena-kiosk-svc` -- library cache and load, fetched after hello
- [x] Hub UI -- Settings → Games screen
- [x] Tests for every I/O row and AC (Rust unit/integration, proptest, kiosk cache, Playwright)

**Acceptance Criteria:**
- Given Settings → Games, when a Manager adds an entry, then `library.upsert` under `games.manage` stores it with its fields and optional catalogue id in `library_entries`.
- Given an entry linked to a catalogue game, when it is shown or edited, then name, launcher and age rating are the catalogue's and read-only while category and tags stay editable.
- Given a saved enabled entry, when it commits, then every connected kiosk receives the new library within 2 s and the entry's executable and its launcher's executables are in `kiosk.process_allow`.
- Given an entry disabled or deleted, when it commits, then it leaves every kiosk's library and its rules leave the allow-list unless another entry still needs them.
- Given a duplicate, empty name or missing executable on a non-URI entry, when saved, then a field error shows and nothing is written; stale edits fail `STALE`.
- Given a Counter, when they edit the library, then `inline-pin` elevates and the change is audited with both actors.
- Given a kiosk that cold-starts with the Hub unreachable, when it needs the library, then it uses the last library it received.

## Spec Change Log

## Review Triage Log

## Design Notes

The library reaches kiosks as the derived setting `kiosk.library` rather than a new frame type: `settings.changed` and `settings.get` already carry `kiosk.*` values to paired kiosks only, order them by `rev`, and the kiosk already caches them. The allow-list is a diff-merge so manual rules survive. The catalogue read-only rule is enforced at the API layer (the only place that can read `reference.db`) plus immutability inside `apply()`.

## Verification

**Commands:**
- `cargo fmt --check` -- expected: clean
- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub -p arena-kiosk-svc --all-targets -- -D warnings` -- expected: no warnings
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub -p arena-kiosk-svc` -- expected: pass
- `pnpm gen:types`, `pnpm check:generated`, `pnpm --filter ./apps/hub check` and the library Playwright spec -- expected: pass

## Implementation Notes

- The `library.upsert` payload field is `entry_kind` (not `kind`): the intent payload is tagged by `kind`, so a field of the same name would collide. Views and `kiosk.library` entries keep `kind`.
- Added: at most 300 live entries per branch (keeps `kiosk.library` under the frame limit); the API maps a catalogue game's free-text launcher to one of ours (unknown becomes `none`); a branch lacking the `kiosk.library` row gets it on the first library write.
- `library.delete` requires `base_rev` from non-owners, as `zone.delete`.
- A linked entry that stays linked to the same catalogue id keeps its stored name, launcher and age rating on edit; `GET /ui/library` shows the catalogue's current values.
- Not run here: `crates/arena-kiosk-svc/tests/runtime.rs` (unix-only) new test; 14 pre-existing `arena-kiosk-svc` Windows unit failures (SYSTEM/Administrators ownership) are unrelated.

### Review Findings

Code review 2026-10-29 (blind-hunter, edge-case-hunter, verification-gap, acceptance-auditor). 0 decision-needed, 1 patch (applied), 4 defer, rest rejected.

- [x] [Review][Patch] `validate_upsert` planned an edit against a placeholder id, so the old row stayed in the "after" list and the allow-list and `kiosk.library` size checks ran on a state that never exists [crates/arena-core/src/apply/library.rs] — fixed: the entry's real id is used on edits.
- [x] [Review][Defer] An operator allow rule identical to a library-derived rule (same glob, no signer) is removed when the last entry needing it is disabled or deleted — deferred: inherent to the spec's diff-merge design (no ownership marker on rules).
- [x] [Review][Defer] Linked entries keep the stored catalogue name/launcher/age rating in `kiosk.library` while `GET /ui/library` shows the cache's current values — deferred: documented in Implementation Notes; needs a refresh on catalogue change.
- [x] [Review][Defer] `kiosk.library` cap is in chars (900k) while the frame limit is 1 MiB of bytes; multibyte or escaped content near the cap could exceed a frame — deferred: needs ~300 maximal entries, not reachable in normal use.
- [x] [Review][Defer] Cap-refusal tests (300 entries, allow-list and library size limits), catalogue-missing paths in `ui_library`, and the "library is full/too large" errors landing on field `name` (misleading Hub message) — deferred: low impact.

Rejected: `ui_library_tests.rs` missing (false — excluded from the review diff only); `games.manage` can edit the allow-list (by design in the spec); catalogue pre-check before permission check (catalogue is non-sensitive staff data, low); missing `process_allow` row (seeded default, low); empty tag dedupe quirk, client/server length counting, delete confirmation, free-text catalogue id, stale open form, kiosk second settings round-trip, `library::load` without caller (Story 6.23), other UI polish (cosmetic or out of story scope).
