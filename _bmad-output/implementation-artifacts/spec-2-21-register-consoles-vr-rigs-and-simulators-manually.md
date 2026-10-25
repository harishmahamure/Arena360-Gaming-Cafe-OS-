---
title: 'Story 2.21: Register consoles, VR rigs and simulators manually'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '0a1b692'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-20-device-groups-with-six-swatches-held-at-the-hub.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Only PCs with a kiosk can reach the Hub's floor, so consoles, VR rigs and simulators cannot be grouped or timed.

**Approach:**
- A manager adds them by hand through `device.register` with an empty `token_hash` (no device token), a kind, a label, an optional group and, for consoles, a TV number.
- Edits go through `device.register` with `base_rev`; removal is `device.revoke`, which soft-deletes the `devices_meta` row and frees the cap slot and TV number.

## Boundaries & Constraints

**Always:**
- **Hub migration** `0014_manual_devices.sql` (next free number on master; renumber on rebase): `devices_meta.tv_number INTEGER NULL` and a unique index over `tv_number` where `kind = 'console'`, `tv_number IS NOT NULL` and `deleted_at IS NULL`.
- **Manual device:** `device.register` with `class = paired`, `device_kind` in console, vr, sim and an empty `token_hash`. It writes a `devices` row (empty hash, which never verifies) and a `devices_meta` row (kind, label, `group_id`, `tv_number`), and no `display.hc` setting. A PC is refused (`VALIDATION_FAILED kind`); the pairing system actor cannot register a manual device.
- **Wire fix (deferred from 2.18):** `DeviceRegister.kind` is renamed `device_kind`, since `kind` is the payload tag. `DeviceRegister` gains optional `tv_number` and `group_id`.
- **TV number:** a console needs one (1 to 999) unless it has a token; only a console may carry one; a number held by a live console is `VALIDATION_FAILED tv_number`. Edit excludes the device itself.
- **Group on create:** must exist and fit the kind (`GROUP_KIND_MISMATCH` field `group_id`, `VALIDATION_FAILED group_id`). An edit refuses `group_id` (move with `device.assign`) and refuses a kind change while grouped (`GROUP_KIND_MISMATCH`, field `kind`).
- **Cap:** new code `DEVICE_LIMIT`, refused when the integer setting `licence.device_cap` is set and the count of live `devices_meta` rows has reached it. The setting is seeded by Story 2.19; absent means no cap. HTTP 403; the Hub UI shows the `tier-lock`.
- **Owner:** an `actor{kind: owner}` `device.register` has the same result (permission `device.manage` is already satisfied by the owner).
- **Removal:** `device.revoke` (needs `device.remote` and a reason; the UI sends `other`) also sets `devices_meta.deleted_at` when a live row exists, with a change row.
- **Snapshot:** `SnapshotDevice` gains `tv_number` and `manual` (omitted when false).
- **Hub UI:** Settings → Devices (add form, list with Edit and Remove, PIN in place for a Counter); the floor tile shows "TV n" and hides presence and Remote unlock for manual devices; manual devices join Settings → Layout's "Move to group".

**Never:** Pricing, sessions, device tokens for manual devices, hard deletes.

## Code Map

- `crates/arena-store`: migration, `devices.rs` (`tv_number` column, `tv_number_taken`, `count_live_devices`, `set_device_meta_deleted`).
- `crates/arena-core/src/apply/devices.rs`: `NewDev` target, `validate_register`/`validate_edit`/`validate_tv`, `mutate_register`, `mutate_revoke`; `snapshot.rs`.
- `crates/arena-proto`: `DeviceRegister`, `ErrorCode::DeviceLimit`, `KEY_DEVICE_CAP`, `SnapshotDevice`; generated TS.
- `apps/hub`: `settings/devices/+page.svelte`, floor tile, `en.json`, `tests/devices.spec.ts`.

## Tasks & Acceptance

- [x] Migration, store helpers, proto, apply, snapshot, generated TS.
- [x] Hub page, floor tile, strings, Rust and Playwright tests.

**Acceptance Criteria:**
- Given the migration, when the schema is inspected, then `devices_meta.tv_number` is nullable and uniquely indexed over live console rows.
- Given a Manager picks Console, VR rig or Simulator with a label, a matching group and (console) a TV number, when `device.register` applies, then a tokenless `devices_meta` row exists and the floor shows it with its label and "TV n".
- Given the same intent from the owner, then the result is identical with `actor{kind: owner}` on the change.
- Given a TV number already used by a live console, then it is refused with a `tv_number` field error.
- Given the cap is reached, when a manual device is added, then it is refused `DEVICE_LIMIT` and the UI shows the tier-lock.
- Given a manual device, when a Manager edits it, then it goes through `device.register` with `base_rev` (`STALE` if old); when removed, it is soft-deleted and its slot and TV number are free.

## Verification

- `cargo test --workspace --locked`; `-p arena-hub -p arena-api --features dev-fixtures`; clippy incl. dev-fixtures; every `pnpm run check:*`; `pnpm run test:scripts`; Hub `test:shell`; gallery `test:a11y`.

## Design Notes

Decisions (recommended option taken): reuse `device.register` and `device.revoke` rather than new intents; the cap reads `licence.device_cap` because Story 2.19 (cap enforcement and its licence plumbing) has not landed; removal sends reason `other`; the migration is 0014 (0013 is on master).

## Review Triage Log

Pass 1 (Blind Hunter, Verification Gap; on Sonnet). P = patch, R = reject, D = defer.

Implementation deviations: migration is `0014` (0013 is on master). Story 2.19 has not landed, so there is no cap setting yet; the cap reads `licence.device_cap` and applies to manual devices only.

| # | Finding | Verdict | Route |
|---|---------|---------|-------|
| 1 | BH: a manual device could be edited into a PC | medium | P: `validate_edit` refuses `kind` pc for a tokenless device; test |
| 2 | BH/VG: cap counted revoked-before-migration rows; applied to PC pairing | medium | P: migration soft-deletes meta of revoked devices; `full` only for manual devices; test |
| 3 | BH/VG: unparseable or unseeded `licence.device_cap` fails open | medium | D: Story 2.19 seeds the key with a rule |
| 4 | BH: changing kind kept a stale group id | medium | P: kind change clears the group |
| 5 | BH: double submit registered twice | medium | P: in-flight guard in `run` |
| 6 | BH: TV field accepted "3abc" | low | P: strict 1 to 3 digit parse |
| 7 | BH: Edit did not move focus to the form | low | P: focus the first input |
| 8 | BH/VG: Manager positive path, system actor, label-only edit, edit with `group_id`, deleted group, TV 1000/999, hc absent, revoke change row, cap vs tokened register | medium | P: tests added |
| 9 | VG: floor test vacuous; Layout lists manual devices | medium | P: Playwright asserts presence and unlock are absent and Layout lists it |
| 10 | BH: `kind` to `device_kind` rename breaks old serialised payloads | low | R: the dual `kind` key was never serialisable; nothing stored |
| 11 | VG: 403 mapping test for `DEVICE_LIMIT`; Counter removal Playwright; tier hard-coded Basic | low | D |
| 12 | `kitchen.spec.ts` snapshot lacked `groups` (svelte-check failure from 2.20/7.1) | low | P: `groups: []` added |

