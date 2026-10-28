---
title: 'Story 6.4: Kiosks scan for installed games and launchers and report them to the Hub'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '10c5ab67d5b7cfe402f0f51a2a031f25371521d7'
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

**Problem:** A manager must type install paths for every game on every PC, because nothing on a kiosk looks at what is installed and the Hub has no record of it.

**Approach:** The kiosk service scans the PC (Steam, Epic, Riot and common game folders) on first connect after pairing and daily at 05:00 branch time with no session running, and reports it as the device intent `kiosk.scan_reported`. `apply()` stores it as that device's latest scan in a new branch table `kiosk_scans`; Settings → Games shows each PC's last scan time. Matching to the catalogue and approval are Story 6.5.

## Boundaries & Constraints

**Always:**

- Report fields: `scanned_at` (ms, > 0, not after the intent's `issued_at`), `entries` (≤ 500), `launchers` (≤ 3, one per `steam`|`epic`|`riot`, each with a non-empty install `path` ≤ 260 chars), `skipped` (count of unreadable folders). Entry: `path` (full path, 1–260 chars), `file_name` (1–128), `product_name` (≤ 128), `publisher` (≤ 128), `launcher` (`steam`|`epic`|`riot`|`battlenet`|`none`). Free text has no control characters. A violation is `VALIDATION_FAILED` with `field`; nothing is written. Paired kiosks only (`Requirement::Device`), no `base_rev`, no reason.
- Storage: one row per device in `kiosk_scans` (`device_id` PK, `scanned_at`, `entries` JSON, `launchers` JSON, `skipped`), inserted on the first report and replaced after. A report older than the stored one (`scanned_at` ≤ stored) is refused `VALIDATION_FAILED` on `scanned_at` and the stored scan stays. Dedupe by idempotency key applies a queued report once.
- Scan sources: Steam (install path from the default location or the registry, `libraryfolders.vdf` library paths, `appmanifest_*.acf` install dirs), Epic (`ProgramData\Epic\EpicGamesLauncher\Data\Manifests\*.item`), Riot (`ProgramData\Riot Games\RiotClientInstalls.json`), and Program Files, Program Files (x86), `C:\Games` and each other fixed drive's `Games` folder. Installer, uninstaller, crash-handler and redistributable executables are left out. Paths are de-duplicated case-insensitively, the first source wins (launcher sources before folders). Beyond 500 found, the rest are dropped.
- A folder or file that cannot be read is skipped, logged and counted in `skipped`; the scan continues.
- The scan runs off the async runtime and never while a session is on the PC: it pauses between folders while a session is active and resumes after. Runs: once after the first successful Hub connection with no recorded scan, then when the branch-time 05:00 after the last scan has passed and no session is running (at the next moment with none). The branch time zone is `branch.tz`, readable by paired kiosks and cached in `queue.db`; unknown means UTC.
- The report is queued in `queue.db` with its own idempotency key and replayed once on reconnect; a Hub that is down never loses it.
- The Hub UI stream sends `library.changed` for `kiosk_scans` changes; `GET /ui/library/scans` (same permission as `GET /ui/library`) lists every live paired PC with its last scan time, entry count and installed launchers (`null` when never scanned).

**Never:** matching scans to the catalogue, `game.matched`, "Found on PCs" or approving entries (Story 6.5); a kiosk UI; installing or launching anything; hashing executables; sending file contents; scanning during a session; adding an allow-list rule from a scan.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First scan | paired kiosk connects, never scanned | scan runs, `kiosk.scan_reported` stored, `kiosk_scans` row rev 1 | — |
| Replace | a second, newer report | row replaced (rev 2), old entries gone | — |
| Stale report | `scanned_at` ≤ stored | nothing written | `VALIDATION_FAILED` `scanned_at` |
| Bad payload | 501 entries, empty path, control character | nothing written | `VALIDATION_FAILED` with field |
| Hub down | scan completes offline | queued, applied once on reconnect | replay |
| Unreadable folder | access denied | skipped, logged, `skipped` + 1, rest scanned | — |
| Session starts mid-scan | session active | scan waits, resumes when it ends | — |
| Daily run | 05:00 branch time passed, session running | waits for the next moment with no session | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/scan.rs` (new) -- `KioskScanReported`, entry and launcher types, limits, `validate()`; `intent.rs` -- `KioskScanReported` kind/payload (model: `KioskProcessBlocked`); `scope.rs` -- paired scope and `may_read_setting` for `branch.tz`; `lib.rs` exports.
- `crates/arena-store/migrations/0063_kiosk_scans.sql`, `migrations.rs`, `branch.rs` (`Table::KioskScans`, `RowKey`), new `kiosk_scans.rs` helpers, `lib.rs` exports and reader.
- `crates/arena-core/src/apply/kiosk_scan.rs` (new) + `mod.rs`, `validate.rs`, `mutate.rs` wiring; reuses `kiosk_log::load` for the branch; `snapshot.rs` reader.
- `crates/arena-api/src/ui_library.rs` -- `GET /ui/library/scans`; `ui_events.rs` -- `library.changed` for `kiosk_scans`; `lan/session.rs` -- the device rate limit does not apply.
- `crates/arena-kiosk-svc/src/scan/` (new: `mod.rs` walker and sources, `schedule.rs`), `intents.rs`, `runtime.rs` (spawn, `Timings.scan_tick`), `hub_link.rs` (fetch `branch.tz`), `lib.rs`.
- `apps/hub/src/routes/settings/games/+page.svelte`, `lib/library.ts`, `packages/ui/src/messages/en.json`, `apps/hub/tests/library.spec.ts`; generated TS via `pnpm gen:types`.
- Tests: pinned intent lists in `intent.rs`, `scope.rs`; `apply/tests/`; `crates/arena-core/tests/proptest_apply.rs`; `crates/arena-api/tests/lan.rs`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- intent, payload, validation, scope, `branch.tz` readable, generated TS
- [x] `arena-store` -- migration 0063, table registration, row helpers, reader
- [x] `arena-core` -- apply handler, wiring, snapshot reader, proptest strategy
- [x] `arena-api` -- `GET /ui/library/scans`, SSE event, rate-limit exemption
- [x] `arena-kiosk-svc` -- scanner sources, session-aware pause, scheduler, report queueing
- [x] Hub UI -- last scan time per PC in Settings → Games
- [x] Tests for every I/O row and AC

**Acceptance Criteria:**
- Given a paired kiosk, when it first connects after pairing, and daily at 05:00 branch time when no session runs (or at the next moment with none), then it scans Steam, Epic, Riot and the common folders.
- Given a completed scan, when the service reports it, then `kiosk.scan_reported` carries each executable's path, file name, product name, publisher and launcher and which of Steam, Epic and Riot are installed with their paths.
- Given a scan report, when the Hub receives it, then it replaces that device's `kiosk_scans` row and Settings → Games shows each PC's last scan time.
- Given a scan running, when a session starts, then the scan pauses and resumes after the session.
- Given the Hub is unreachable when a scan completes, when the report is sent, then it is queued with its idempotency key and applied once on reconnect.
- Given a folder the service cannot read, when the scan reaches it, then it is skipped, logged and the scan continues.

## Spec Change Log

## Review Triage Log

## Design Notes

The report is a normal device intent, so the offline queue, idempotency key, change log and replication need nothing new. A row per device keeps "latest scan only" without a delete path. The Hub reads executables' product name and publisher is done on the kiosk (PE version info) because only the kiosk can open the files.

## Verification

**Commands:**
- `cargo fmt --check` -- expected: clean
- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub -p arena-kiosk-svc --all-targets -- -D warnings` -- expected: no warnings
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub -p arena-kiosk-svc` -- expected: pass
- `pnpm gen:types`, `pnpm check:generated`, `pnpm --filter ./apps/hub check` and the library Playwright spec -- expected: pass

## Implementation Notes

- Scanner sources live in `crates/arena-kiosk-svc/src/scan/` (`mod.rs` walker, `windows.rs` registry, fixed drives and PE version info, `schedule.rs` scheduler). Folders named `Games` are scanned before Program Files so the 500-entry cap does not drop games; Program Files skips Windows, Microsoft, Common Files and the launchers' own folders (games there are found through their launcher sources).
- A failed scan retries after an hour and queues a `scan_failed` kiosk log. An oversized report is trimmed to fit a Hub frame.
- Not run here: `crates/arena-kiosk-svc/tests/scan.rs` is unix-only (the service refuses a `config.json` not owned by SYSTEM/Administrators); it passed on Windows with the owner check bypassed temporarily. 14 pre-existing `arena-kiosk-svc` Windows lib failures and the `tests/discovery.rs` unused-import errors are unrelated.
