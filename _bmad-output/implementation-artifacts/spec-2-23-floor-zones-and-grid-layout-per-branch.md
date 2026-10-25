---
title: 'Story 2.23: Floor zones and grid layout per branch'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'b736a8b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-20-device-groups-with-six-swatches-held-at-the-hub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-21-register-consoles-vr-rigs-and-simulators-manually.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub floor lists devices by group only, so staff cannot find a seat where it sits in the café.

**Approach:**
- A Manager keeps named floor zones (add, rename, reorder, delete) and places each device on a cell of a zone's grid, in Settings → Layout.
- Three new intents carry it: `zone.upsert`, `zone.delete` and `device.place`, all needing `device.manage`, applied through `apply()` like `group.upsert`, `group.delete` and `device.assign`.

## Boundaries & Constraints

**Always:**
- **Hub migration** `0016_floor_zones.sql` (0015 went to Story 2.12; renumbered on rebase): table `floor_zones` (`zone_id`, `name`, `sort_order`, `rev`, `deleted_at`, `created_at`, `updated_at`; live names unique without case) and `devices_meta.zone_id` (FK), `floor_x`, `floor_y` (both `>= 0`), with a unique index over `(zone_id, floor_x, floor_y)` for live rows.
- **Change log:** `floor_zones` joins `Table::ALL` and `RowKey`, so the replayer and `changes` cover it.
- **Grid:** `FLOOR_COLS = 12`, `FLOOR_ROWS = 8` in `arena-proto`; a cell is `floor_x` 0 to 11 and `floor_y` 0 to 7.
- **`zone.upsert`:** `id` absent creates (writer mints the id); present edits (`base_rev` required for staff, an owner may omit it). Name is a valid label, unique among live zones without case (`VALIDATION_FAILED name`); an unknown or deleted id is `VALIDATION_FAILED id`. `sort_order` reorders.
- **`zone.delete`:** a zone with live devices is refused `ZONE_NOT_EMPTY` (409); it soft-deletes (`deleted_at`) and frees the name.
- **`device.place`:** `zone_id`, `floor_x`, `floor_y` all set or all null (null makes the device Unassigned). A cell without a zone is `VALIDATION_FAILED zone_id`; a missing coordinate is `VALIDATION_FAILED floor_x` or `floor_y`; an unknown zone is `VALIDATION_FAILED zone_id`; off the grid is `VALIDATION_FAILED floor_x`/`floor_y`; a cell held by another live device in that zone is `CELL_TAKEN` (409, field `floor_x`). `base_rev` is the device's `devices_meta` rev. Any live device kind may be placed; its group is untouched.
- **Snapshot:** `HubSnapshot.zones` (live, `sort_order` then name); `SnapshotDevice.zone` now carries the zone id, plus `floor_x`, `floor_y`.
- **Events:** a change to `floor_zones` raises the existing `groups.changed` event so other Hub windows refresh.
- **Floor:** zone sections in sort order, devices placed in their cells (grid columns and rows at 840 px and wider, a plain list in cell order below); devices without a zone keep the old group sections, and the Unassigned section stays last.
- **Settings → Layout:** a zone form and list; a cell grid per zone at 840 px and wider with drag and drop and a keyboard path (Enter picks a device up, arrow keys move across cells, Enter drops, Escape cancels; a pool of unplaced devices); below 840 px a list with Zone and Position fields per device at 44 px targets. A Counter is asked for a manager's PIN in place.

**Never:** Pricing, sessions, a cell size or zone colour setting, free-form (non-grid) coordinates, hard deletes.

## Code Map

- `crates/arena-store`: `migrations/0016_floor_zones.sql`, `migrations.rs`, `branch.rs` (`Table::FloorZones`, `RowKey::FloorZones`), `devices.rs` (`ZoneRow`, zone helpers, `cell_taken`, `set_device_place`, `DeviceMetaRow` place columns), `lib.rs` (`Store::zones`).
- `crates/arena-proto`: `ZoneUpsert`, `ZoneDelete`, `DevicePlace`, kinds and payloads, `FLOOR_COLS`/`FLOOR_ROWS`, `ErrorCode::{ZoneNotEmpty, CellTaken}`, `SnapshotZone`, `SnapshotDevice.floor_x/floor_y`; generated TS.
- `crates/arena-core/src/apply/zones.rs` (new), `mod.rs`, `guards.rs`, `validate.rs`, `mutate.rs`; `snapshot.rs`.
- `crates/arena-api`: HTTP mapping of the two codes, `ui_events.rs` table match; `arena-kiosk-core/queue.rs`, `arena-cloud/api/error.rs` error lists; `arena-replay` fixture.
- `apps/hub`: `settings/layout/+page.svelte`, `settings/layout/ZoneGrid.svelte` (new), `floor/+page.svelte`, `lib/snapshot.ts`, `tests/zones.spec.ts` (new), snapshot fixtures; `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

- [x] Migration, store, proto, apply, snapshot, generated TS, Rust tests.
- [x] Hub zone form, grid editor, phone list, floor, strings, Playwright tests.

**Acceptance Criteria:**
- Given Settings → Layout, when a Manager adds, renames, reorders or deletes a zone, then `floor_zones` stores name and sort, and a zone with devices cannot be deleted until they move (`ZONE_NOT_EMPTY`).
- Given the layout grid, when a device tile is dragged to a cell in a zone, or moved by keyboard (Enter picks up, arrow keys move, Enter drops), then `device.place` stores `zone_id`, `floor_x`, `floor_y` on `devices_meta`, and two devices cannot occupy one cell (`CELL_TAKEN`).
- Given a saved layout, when the Hub restarts or another Hub window opens, then the floor shows zones in sort order with devices in their cells and the Unassigned section stays at the end.
- Given a phone-width Hub (under 840 px), when the layout editor opens, then it falls back to a list with "Zone" and "Position" fields per device, with 44 px targets.

## Verification

- `cargo test --workspace --locked`; `-p arena-hub -p arena-api --features dev-fixtures`; clippy incl. dev-fixtures; `cargo fmt --check`; every `pnpm run check:*`; `pnpm run test:scripts`; prettier; Hub `test:shell`; gallery `test:a11y`; app builds except marketing.

## Design Notes

Decisions (recommended option taken): three intents mirroring groups rather than one layout blob, so each write is a small versioned row with a `base_rev`; the grid is a fixed 12 by 8 per zone; zone changes reuse the `groups.changed` event; placing never touches the device's group; keyboard move is a pick-up and drop with focus moving across cells, and the pool of unplaced devices doubles as the keyboard route into a zone; below 840 px the Position field lists the free cells of the zone; an empty zone order on add means last.

## Review Triage Log

Pass 1 (Blind Hunter, Verification Gap; on Sonnet). P = patch, R = reject, D = defer.

Implementation deviation: migration is `0016` (Story 2.12 took 0015 while this was in flight).

| # | Finding | Verdict | Route |
|---|---------|---------|-------|
| 1 | VG: staff `base_rev` required/stale and owner bypass untested | medium | P: test with a Manager for all three intents |
| 2 | VG: soft-deleted zone/device, name padding, tie order, group untouched by place untested | medium | P: tests added |
| 3 | BH: picking a tile focused zone 1 instead of its own cell; focus lost after a pool device is placed | medium | P: focus own cell on pick and the placed cell after a place |
| 4 | BH: phone selects kept a value the Hub never applied | medium | P: select reset to the model after the attempt |
| 5 | BH: deleting the zone being edited left a dead edit; a blank order moved the zone; order accepted junk | medium | P: reset on delete, blank keeps the old order, 1 to 9 digit parse |
| 6 | BH: floor grid collapsed empty rows | medium | P: `--rows` and a row minimum |
| 7 | BH: carried state only an outline; Escape only inside a cell; same-cell click sent a no-op | low | P: `aria-pressed`, window Escape, same-cell click puts it down |
| 8 | VG: drag and drop and Escape untested | medium | P: Playwright test |
| 9 | BH: h4 under h2 | low | P: h3 |
| 10 | VG: `groups.changed` for `floor_zones`, 409 mapping, store-level index tests, Counter PIN prompt, phone floor list | low | D |
| 11 | BH: no all-or-none CHECK on the place columns (SQLite cannot add one by ALTER); revoke keeps zone and cell; no upper bound on `sort_order`; `free()` quadratic; no busy flag on rapid places; approval block sits above the group form | low | D |
| 12 | BH: `device.place` accepts any live device kind | low | R: any device may sit on the floor (spec) |
