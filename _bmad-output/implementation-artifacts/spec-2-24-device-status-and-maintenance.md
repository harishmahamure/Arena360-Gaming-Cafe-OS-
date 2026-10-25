---
title: 'Story 2.24: Device status and maintenance'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '1776e34'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-23-floor-zones-and-grid-layout-per-branch.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-21-register-consoles-vr-rigs-and-simulators-manually.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub floor shows only a live socket flag, so staff cannot see one clear status per device or take a broken PC out of use without deleting it.

**Approach:**
- Every device carries one status (online, offline, in session, reserved, maintenance) shown as text plus its state token, never colour alone.
- A paired kiosk is online while its socket is up and for 15 s after it drops, then offline, with `last_seen_at`; a manual device not in maintenance is online.
- A Manager sets maintenance with an optional note through `device.maintenance` (permission `device.manage`, applied through `apply()`), and returns the device to service.

## Boundaries & Constraints

**Always:**
- **Hub migration** `0020_device_state.sql` (next free after 0019; renumber on rebase if a sibling lands first): `devices_meta.state` (NOT NULL, default `offline`, CHECK in online, offline, in_session, reserved, maintenance), `prior_state` (nullable, same set without maintenance) and `maintenance_note`. Backfill: manual devices (empty `token_hash`) become `online`, paired devices stay `offline` (no socket exists at migration time).
- **Stored versus shown:** the stored `state` is authoritative for the held states (in_session, reserved, maintenance), which later features and this story write. Online and offline are shown from live presence and are not written on every connect, so the stored value for them is a placeholder (`last_seen_at` is kept in memory by the LAN registry and is `null` after a Hub restart until the device reconnects).
- **`device.maintenance`:** `{device_id, on, note?}`; permission `device.manage`; `base_rev` is the device's `devices_meta` rev (required and current for staff, an owner may omit it). Unknown or revoked device is `VALIDATION_FAILED device_id`. `on` stores `state = maintenance`, remembers the previous stored state in `prior_state` (kept if already in maintenance, so a second call only edits the note) and stores the trimmed note (empty is none, over `MAINTENANCE_NOTE_MAX` = 200 characters is `VALIDATION_FAILED note`). Off on a device not in maintenance is `VALIDATION_FAILED device_id`; otherwise it restores `prior_state` (`online` when none) and clears the note. Group and floor place are untouched.
- **Snapshot:** `SnapshotDevice.status` (`online`, `offline`, `in_session`, `reserved`, `maintenance`), `last_seen_at` (epoch ms or null) and `maintenance_note`; `online` keeps meaning a live socket now. The Hub service fills the shown online/offline from the LAN registry: online while connected or disconnected for under 15 s; offline after, with `last_seen_at` at the last connect or disconnect. A manual device is online unless held.
- **Push:** the registry bumps presence on connect and disconnect and again 15 s after a disconnect, so every open view reloads through `devices.changed` within 2 s of the status changing; a maintenance write raises the existing `groups.changed`/snapshot refresh through `devices_meta`.
- **Floor:** every tile shows a status badge (text), "Last seen" for an offline device, and "Unavailable for starts" with the note in maintenance. Settings → Devices gets a Device status list of every device with Set to maintenance (a note field), Edit note and Return to service; a Counter is asked for a manager's PIN in place.

**Never:** Pricing, session start checks (Story 3.9 reads the status), persisting every connect or disconnect as a change-log row, a new error code.

## Code Map

- `crates/arena-store`: `migrations/0020_device_state.sql`, `migrations.rs`, `devices.rs` (`DeviceMetaRow` state columns, `set_device_state`), `lib.rs`.
- `crates/arena-proto`: `DeviceMaintenance`, `IntentKind::DeviceMaintenance`, `MAINTENANCE_NOTE_MAX`, `DeviceStatus`, `SnapshotDevice.{status,last_seen_at,maintenance_note}`, `apply_presence`; generated TS.
- `crates/arena-core/src/apply/maintenance.rs` (new), `mod.rs`, `guards.rs`, `validate.rs`, `mutate.rs`; `snapshot.rs`.
- `crates/arena-api`: `lan/registry.rs` (last seen, 15 s grace, delayed presence bump), `lan/mod.rs` (`Presence::status`), `health.rs` (fills the shown status); snapshot expectations in `health.rs` and `arena-hub/tests/lan.rs`.
- `apps/hub`: `floor/+page.svelte`, `settings/devices/+page.svelte`, `lib/snapshot.ts`, `tests/maintenance.spec.ts` (new), `tests/devices.spec.ts`; `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

- [x] Migration, store, proto, apply, snapshot, registry grace, generated TS, Rust tests.
- [x] Hub floor badges, Settings → Devices status list and maintenance form, strings, Playwright tests.

**Acceptance Criteria:**
- Given the migration, when it adds `state` to `devices_meta`, then existing rows are backfilled from their socket status (manual online, paired offline), the column allows exactly the five statuses, and every status shows as text plus its state token.
- Given a paired kiosk, when its socket connects or stays disconnected for 15 s, then its status becomes online or offline with `last_seen_at`, pushed to every open Hub view within 2 s.
- Given a manually registered device, when it is not in maintenance, then it shows online.
- Given a Manager sets a device to maintenance with an optional note, when `device.maintenance` applies, then the device shows "Maintenance", stays in its group and position, and is flagged unavailable for starts; "Return to service" restores its previous status.

## Verification

- `cargo test --workspace --locked`; `-p arena-hub -p arena-api --features dev-fixtures`; clippy incl. dev-fixtures; `cargo fmt --check`; every `pnpm run check:*`; `pnpm run test:scripts`; prettier; Hub `test:shell`; gallery `test:a11y`; app builds except marketing.

## Design Notes

Decisions (recommended option taken): one intent with an `on` flag rather than two, so one rev-checked write covers set, edit-note and return; online and offline stay derived from the LAN registry instead of a persisted row per socket event (they are not business state, and a write per flap would flood the change log), with the 15 s grace held in the registry and the expiry pushed by a delayed presence bump; the maintenance control lives in Settings → Devices (all devices, paired and manual) rather than on the floor tile, which already carries remote unlock; `prior_state` is kept on a repeat call so editing the note never loses the status to return to.

## Review Triage Log

Pass 1 (Blind Hunter, Verification Gap; on Sonnet). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route |
|---|---------|---------|-------|
| 1 | BH: a closing session fell through to a stale `seen` entry (online, offline, online flicker) | medium | P: any paired session entry, closing or not, counts as connected in `status()` |
| 2 | BH: `online` and `status` disagree during the grace | low | R: `online` stays a strict live socket (remote unlock needs it); documented |
| 3 | BH: a new manual device stored `offline` and restored to it | medium | P: `insert_device_meta` stores `online` for an empty token hash; test |
| 4 | BH: a stored `in_session`/`reserved` prior state can be stale on return | medium | D: the sessions and reservations stories own those states |
| 5 | BH: `state = maintenance` with no `prior_state` would violate the CHECK | low | P: fall back to `online`; never copy `maintenance` |
| 6 | BH: control characters in the note | low | P: rejected `VALIDATION_FAILED note`; test |
| 7 | BH: Return to service on another device closed an open note form | low | P: close only the form of the same device |
| 8 | BH: focus lost after Return to service; no busy disable | low | D |
| 9 | BH: maintenance offered for a device with no meta row | low | R: every paired device has a meta row since 0011; a stale `device_id` already maps to the stale message |
| 10 | BH: an unknown status string would crash the render | low | P: falls back to the offline badge |
| 11 | BH: a timer task per disconnect | low | D |
| 12 | BH: `last_seen_at` in memory only | low | D |
| 13 | VG: 2 s push end to end, `ui_snapshot` with a live registry, Counter refusal, change-log row, five tokens, STALE and PIN paths, phone width | high/medium | D (registry grace and push channel are unit tested) |
| 14 | VG: `apply_presence`, note edge cases, held states in the snapshot, removed device | medium | P: tests added |
| 15 | VG: revoked device | medium | P: soft-deleted meta row refused (revoke soft-deletes it) |
