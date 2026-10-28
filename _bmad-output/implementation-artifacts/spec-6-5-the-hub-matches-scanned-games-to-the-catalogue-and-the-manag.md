---
title: 'Story 6.5: The Hub matches scanned games to the catalogue and the manager approves them in bulk'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '0f33df45991e07bd477767b1159858a2fa0d7749'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** A kiosk's scan (Story 6.4) sits in `kiosk_scans` unread: the manager still has to type every game into the library, and nothing links a found executable to the global catalogue's name and cover.

**Approach:** After a scan is stored the Hub (API layer, the only code that reads `reference.db`) matches its executables to the cached catalogue signatures and records the result per device with the system intent `game.matched` into `game_matches`. Settings → Games → "Found on PCs" lists what is not yet in the library and not dismissed, grouped across PCs. A Manager (or a Counter with a manager PIN) approves several with one `library.upsert` each, which now carries per-device install paths stored in `library_installs`; `library.dismiss` hides proposals until `library.dismiss_clear`. Later scans update install paths without new proposals, and the kiosk lists an entry only where an install path is known or its own path exists.

## Boundaries & Constraints

**Always:**

- **Match rule.** A scan executable matches a signature when its file name equals the signature's `exe_name` and every `product_name` / `publisher` the signature gives equals the executable's (all without case). The signature with the most such fields wins, then the lowest game id; an executable matches at most one game; a path is matched once. The scan carries no file hash (Story 6.4 never hashes), so a signature's `sha256` is unused until a kiosk reports one.
- **`game.matched`** (system actor, `Requirement::System`, no `base_rev`, not in any client scope): `{device_id, scanned_at, matches[{catalog_id, path}]}`. Refused `VALIDATION_FAILED` for an unknown or unpaired `device_id`, a PC with no scan, a `scanned_at` that is not the stored scan's, a `matches[i].path` the scan did not find, a malformed id or path, a repeated path, or matches identical to the ones already stored for that scan (field `matches`). It inserts (rev 1) or replaces the device's `game_matches` row. The LAN session submits it in the background after a `kiosk.scan_reported` is applied (primary Hub only; with no catalogue cache the matches are empty); a failure only logs.
- **Install upkeep, in the same transaction.** Every live library entry is brought up to date for that device: an entry linked to a catalogue game gets an install (`match_key` = `game:<id>`) when the scan found that game; an entry approved from an unmatched proposal keeps its `exe:` key and gets an install on any device whose scan finds an executable with that key; an install whose recorded path the scan still finds keeps it, one it no longer finds moves to the first path found. A path the allow-list refuses (unsigned, under a user-writable root) and any change that would push `kiosk.library` over its size limit are left out; the matches are recorded regardless. `kiosk.library` and `kiosk.process_allow` are rewritten as for `library.upsert`.
- **Proposal key.** `game:<catalogue id>` for a matched executable, else `exe:<file name>|<product name>|<publisher>` in lower case. A found item is listed unless: its key is dismissed; an entry is linked to its game; an install carries its key; or an entry's executable or install path is its path (without case). Items group by key across PCs (`pc_count` = distinct PCs, one path per PC). Name: catalogue name, else product name, else file name. Ordered by name.
- **`GET /ui/library/proposals`** (any signed-in staff, like `GET /ui/library`) returns `{proposals[{key, name, file_name, product_name, publisher, catalog_id, cover_sha256, launcher, category, age_rating, pc_count, devices[{device_id, label, path}]}], dismissed}`; a cover is named only when the cache holds that file. `GET /ui/library/cover/{sha256}` serves a stored cached image (≤ 8 MiB) to a signed-in staff member; anything else is 404.
- **`library.upsert` installs.** New field `installs[{device_id, path, match_key}]` (default empty): listed devices get that path (a missing `match_key` keeps the stored one), unlisted devices keep theirs; ≤ 200; each on a live paired PC, listed once, a full Windows path ≤ 520 chars with no `* ? { }`, key empty or a well-formed proposal key. Errors are `VALIDATION_FAILED` on `installs[i].device_id|path|match_key`. Each install path is an unsigned allow rule of its entry and goes to kiosks inside `kiosk.library` (`installs[{device_id, path}]`); a path the allow-list refuses refuses the upsert on `executable_path`. Approving sends one `library.upsert` per proposal with the catalogue id (the API still fills name, launcher and age rating from the catalogue) and the first PC's path as `executable_path`.
- **`library.dismiss {keys}`** (1–500 well-formed keys, none already dismissed, ≤ 5 000 kept) inserts `library_dismissals` rows; **`library.dismiss_clear`** deletes every row (refused when none). Both need `games.manage` (Counter by `inline-pin`; the audit row carries both actors) and take no `base_rev`.
- **Kiosk.** `library::visible(entries, device_id, exists)` lists an entry only where an install path is recorded for the device or its `executable_path` exists; an entry with neither path nor installs (launch link only) is listed everywhere.
- **Hub UI.** "Found on PCs" shows cover (fetched with the sign-in header), name, kind, launcher, category, catalogue / not-in-catalogue badge, "Found on PCs: N" and their names; select (and select all), Approve selected, Dismiss selected, "Show dismissed again" with the dismissed count. A Counter's PIN approves up to ten intents, then asks again. `library.changed` is sent for the three new tables and the list reloads.

**Never:** matching on a hash; creating a proposal for an already-approved game at a new path; a kiosk UI or launching (Story 6.23); removing an install when a later scan no longer finds the game; matching again when the catalogue changes (the next scan does); a kiosk submitting `game.matched`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Scan stored | `kiosk.scan_reported` applied | Hub submits `game.matched`; `game_matches` row rev 1 (then 2…) | refusal only logs |
| Matched game | `g.exe` / "Great" matches a signature | proposal `game:<id>`, catalogue name, cover, launcher, category, PC count | — |
| Unmatched | no signature | proposal `exe:…` named by product name or file name | — |
| Approve | `library.upsert` with `catalog_id` + installs | entry linked, installs rev 1, allow rules and `kiosk.library` updated | bad path / device → field error, nothing written |
| Counter | no PIN | `PERMISSION_DENIED`; with a ticket it applies, both actors audited | — |
| Same game, new path | later scan finds it elsewhere | install path updated (rev + 1), no new proposal | — |
| Dismiss | `library.dismiss` | key stored, hidden even after a later scan; clear shows it again | already dismissed → `VALIDATION_FAILED` `keys` |
| Stale / bogus matches | wrong `scanned_at`, unknown path | nothing written | `VALIDATION_FAILED` |
| Unsafe path | `C:\Users\…\g.exe` found | matches recorded, no install | — |
| Kiosk list | install on another PC, own path missing | entry not listed on this PC | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/matching.rs` (new) -- `GameMatched`, `LibraryDismiss(Clear)`, `ProposalView(s)`, `proposal_key`; `intent.rs` (3 kinds, ALL 102→105), `library.rs` (`LibraryInstall`, `KioskInstall`, `installs` on upsert / views / `kiosk.library`, `library_rules_with_installs`), `lib.rs`.
- `crates/arena-store/migrations/0064_library_installs.sql`, `branch.rs` (`GameMatches`, `LibraryInstalls`, `LibraryDismissals`), new `library_installs.rs`, `kiosk_scans.rs` (`list_kiosk_scans`), `reference.rs` (`signatures`, `cover_sha256`), `lib.rs`.
- `crates/arena-core/src/apply/game_match.rs`, `library_dismiss.rs` (new); `library.rs` (installs in validate / mutate / derived rules / `kiosk.library`); `mod.rs`, `validate.rs`, `mutate.rs`, `guards.rs`; `game_proposals.rs` (`match_scan`, `build_proposals`); `snapshot.rs` (`device_scan`, `proposal_inputs`, installs in `library()`); `media_cache.rs`.
- `crates/arena-api/src/lan/scan_match.rs` (new) + `session.rs`; `ui_library.rs` (`proposals`, `cover`), `ui_auth.rs` routes, `ui_events.rs` (`library.changed`).
- `crates/arena-kiosk-svc/src/library.rs` (`visible`, `load_visible`).
- `apps/hub/src/lib/FoundOnPcs.svelte` (new), `lib/library.ts`, `routes/settings/games/+page.svelte`, `packages/ui/src/messages/en.json`, `apps/hub/tests/library.spec.ts`; generated TS via `pnpm gen:types`.
- Tests: `apply/tests/game_match.rs`, `game_proposals.rs` unit tests, `proptest_apply.rs` (`Op::GameMatch`, `Op::Dismiss`, `assert_matches_hold`), `ui_library_tests.rs`, `tests/lan.rs`, kiosk `library` tests.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- intents, payloads, `installs`, views, proposal key, generated TS
- [x] `arena-store` -- migration 0064, three change-log tables, helpers, reference signatures and covers
- [x] `arena-core` -- `game.matched`, dismissals, installs in `library.upsert` and the derived settings, matcher and proposal builder, snapshot readers
- [x] `arena-api` -- background matching after a stored scan, `GET /ui/library/proposals` and `/cover/{sha256}`, SSE
- [x] `arena-kiosk-svc` -- entries listed only where an install path is known or the path exists
- [x] Hub UI -- Found on PCs
- [x] Tests for every I/O row and AC (apply, proptest, API, LAN, kiosk, Playwright)

**Acceptance Criteria:**
- Given a stored scan, when the Hub processes it, then each executable is matched to the catalogue signatures, each match is recorded with `game.matched` (catalogue id, device, path) and unmatched ones stay proposals named by product name or file name.
- Given "Found on PCs", when a Manager opens it, then proposals not in the library show the matched name and cover (or the file name), launcher, catalogue category and the number of PCs; approved and dismissed items are not listed.
- Given several proposals selected, when the Manager presses Approve, then one `library.upsert` per proposal links the catalogue id and `library_installs` stores each PC's path.
- Given a library entry, when a kiosk shows its library, then it appears only where an install path is known or its executable path exists.
- Given a later scan that finds an approved game at a new path, then the PC's install path is updated without a new proposal.
- Given dismissed proposals, when later scans report them, then they are not proposed until the dismissed list is cleared.
- Given a Counter, when they approve or dismiss, then `inline-pin` opens for manager elevation.

## Spec Change Log

## Review Triage Log

## Design Notes

Matching lives in the API layer because only it can read `reference.db`; `apply()` stays pure and replays from the change log (the matches are in the intent). The matches are stored per device so "Found on PCs" can be built without the catalogue for covered items and so a later scan can tell which paths are catalogue games. Installs carry the proposal key (`match_key`) so an approved game with no catalogue entry is still found again on other PCs and at new paths. `library.upsert.installs` is "set these devices" rather than "replace all", so the existing edit form (which sends none) never drops them.

## Verification

**Commands:**
- `cargo fmt --check` -- expected: clean
- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --all-targets -- -D warnings` and `cargo clippy -p arena-kiosk-svc --lib --bins --test scan --test runtime --test pairing -- -D warnings` -- expected: no warnings
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub` and `cargo test -p arena-kiosk-svc --lib library` -- expected: pass
- `pnpm gen:types`, `pnpm check:generated` (after commit), `pnpm check:strings`, `pnpm format:check`, `pnpm --filter ./apps/hub check`, `playwright test tests/library.spec.ts` -- expected: pass

## Implementation Notes

- Not run / pre-existing: 14 `arena-kiosk-svc` Windows lib failures (SYSTEM/Administrators ownership) and the `tests/discovery.rs` unused-import errors; `check:tokens-used` unknown tokens in `+layout.svelte` and `floor/+page.svelte`; `test:scripts` module error — all fail identically at the baseline commit. `proptest_apply` fails intermittently at the baseline commit too (the stock-in change count after `Bootstrap, Import, Threshold, Stock`; `Scan` then `WindowStart` giving `DeviceUnassigned`); the runs that pass include the new `GameMatch` / `Dismiss` operations.
- `check:generated` compares with the committed files, so it reports the new types as new until they are committed.
- Deferred: a catalogue update does not re-match stored scans (the next scan does); uninstalled games keep their install rows.
