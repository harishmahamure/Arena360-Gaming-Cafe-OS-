---
title: 'Story 2.20: Device groups with six swatches, held at the Hub'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'b8d2fd660d1c83e7536c6c20b1a7452c76baa631'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-18-kiosk-pairs-with-a-6-digit-code-from-the-hub-or-a-manager-pi.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Devices have no groups, so nothing prices or displays them by kind or colour, and Unassigned is the only state.

**Approach:**
- `device_groups` at the Hub, edited through the `group.upsert`, `group.delete` and `device.assign` intents in `apply()`.
- They're enforced by kind, limited to six colour swatches, and edited in Settings → Layout (drag, or a keyboard "Move to group" control).
- They're shown as a `group-chip` on the floor. Owner setup commands (2.15) reuse the same intents.

## Boundaries & Constraints

**Always:**
- **Hub migration** `0012_device_groups.sql` (next free number at landing; renumber on rebase if a sibling lands first):
  - `device_groups` has id, name, `kind` ∈ {pc, console, vr, other}, `colour` ∈ {teal, orange, lime, sand, coral, silver}, sort_order, rev, deleted_at, and UNIQUE(name) over rows where `deleted_at` is null (a partial unique index, case-insensitive).
  - `devices_meta.group_id` gains a FOREIGN KEY to `device_groups.id` (null means Unassigned). Use SQLite table rebuild if needed, as Epic 1's migrations do. Existing rows are kept.
- **Intents** (permission `device.manage` for Manager and Owner; a Counter gets `PIN_REQUIRED`, so the UI opens `inline-pin`):
  - **`group.upsert {id?, name, kind, colour, sort_order, base_rev?}`:**
    - creates a group, or edits one under the `base_rev` rule;
    - a duplicate name is `VALIDATION_FAILED` field `name`;
    - a colour outside the six is `VALIDATION_FAILED` field `colour`;
    - a kind change on a group with devices is refused, field `kind`.
  - **`group.delete {id, base_rev}`:** refused with `GROUP_NOT_EMPTY` (a new code) while any non-deleted device has that group. Otherwise it soft-deletes.
  - **`device.assign {device_id, group_id | null, base_rev}`:**
    - sets `devices_meta.group_id`;
    - a kind mismatch is refused with `GROUP_KIND_MISMATCH` (a new code). A `pc` device goes only to `pc` groups; `console` only to `console`; `vr` to `vr`; `sim` to `other`. Console groups never take PCs and the reverse.
    - `null` sets Unassigned.
  - **Stale edits:** an old `base_rev` from a Hub window is `STALE` with the current values. An owner setup-command actor (2.15 later; in this story an `actor{kind: owner}` intent in tests) with an old `base_rev` still applies, last write wins for owner commands, and the Hub UI toasts "Group updated by owner" on the change event.
  - The new codes `GROUP_NOT_EMPTY` and `GROUP_KIND_MISMATCH` are reserved for this story.
- **Snapshot and events:** `HubSnapshot` gains `groups: [{id, name, kind, colour, sort_order, rev}]`, and devices carry `group_id`. SSE `groups.changed` fires alongside the existing `devices.changed`.
- **Hub UI:**
  - **Settings → Layout:**
    - the group list (create and edit name, kind, swatch and order; delete, showing the inline `GROUP_NOT_EMPTY` reason);
    - each device as a draggable item;
    - a keyboard-reachable "Move to group" select per device that applies `device.assign`;
    - drag and drop as an enhancement, with the select always present.
    - A swatch picker of 6 radio buttons, each with a name, so colour isn't the only cue.
  - **Floor:** an assigned device shows a `group-chip` (a swatch dot plus the group name in muted ink), and Unassigned shows none. Add `GroupChip` to `packages/ui` if missing, with the swatch tokens from DESIGN.
  - A Counter's edit opens `inline-pin` (Epic 1).
  - Every string lives in `en.json`. axe passes in both themes and HC.
- **Tests:**
  - **Rust:** each validation; unique names after a soft delete (reuse allowed); every kind-matrix case; delete with and without devices; STALE for a staff actor but applied for the owner actor; Counter `PIN_REQUIRED`; the FK; the migration on a DB that has 2.18 rows.
  - **Playwright:** create, edit, delete-refused, "Move to group" by keyboard, the chip on the floor, the STALE message, the owner toast, a Counter getting the PIN prompt, axe.

**Never:**
- Starting sessions or pricing (Epic 3 and 2.25).
- Colours outside the six.
- Hard deletes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Create | "PCs", pc, teal | group stored | N/A |
| Dup name | "pcs" again | `VALIDATION_FAILED` name | N/A |
| Bad colour | "purple" | `VALIDATION_FAILED` colour | N/A |
| Assign ok | pc device → pc group | `group_id` set | N/A |
| Mismatch | pc device → console group | `GROUP_KIND_MISMATCH` | N/A |
| Unassign | `group_id: null` | Unassigned | N/A |
| Delete busy | group with devices | `GROUP_NOT_EMPTY` | inline reason |
| Delete empty | — | soft-deleted; name reusable | N/A |
| Stale staff | old `base_rev` | `STALE` + current | N/A |
| Stale owner | owner actor, old rev | applies; toast | N/A |
| Counter | edit | `PIN_REQUIRED` | inline-pin |

</frozen-after-approval>

## Code Map

Investigate from 2.18 (`devices_meta`, `device.register`, Unassigned floor section, snapshot devices) and Epic 1 (`apply()`, permissions, `base_rev`/STALE, `inline-pin`, SSE). Hub migration **0012** (next free at landing). DESIGN.md swatch tokens (`_bmad-output/planning-artifacts/ux-designs/**/DESIGN.md`), `packages/ui/src/tokens`.

## Tasks & Acceptance

**Execution:**
- [ ] migration 0013, store, intents in `arena-core`, codes + DTOs in `arena-proto`, generated TS.
- [ ] snapshot/SSE in `arena-api`.
- [ ] `packages/ui` `GroupChip` (+ swatch picker if shared), `apps/hub` Settings → Layout + floor chip, `en.json`, tests.

**Acceptance Criteria:**
- Given a dev Hub with two paired kiosks, when a Manager creates a teal "PCs" group and moves both PCs into it with the keyboard control, then the floor shows both with the teal "PCs" chip and none under Unassigned.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast`; `-p arena-hub --features dev-fixtures`; `-p arena-api --features dev-fixtures`; clippy incl. dev-fixtures; fmt -- pass (known flaky tests recorded).
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell`; every `pnpm run check:*`; `pnpm run test:scripts`.

## Review Triage Log

Pass 1 (Verification Gap, Blind Hunter; on Sonnet). P = patch, R = reject, D = defer.

Implementation deviations: the migration is `0012_device_groups.sql` (the runner needs contiguous numbers and 2.10's 0012 has not landed; renumber on rebase). The `group.upsert` field is `group_kind`, because `kind` tags the payload. A Counter without a ticket gets `PERMISSION_DENIED` and the UI opens `inline-pin` up front. `STALE` carries no current values; the UI reloads the snapshot. The owner toast comes from `groups.changed {by_owner}`, read from the change row's actor kind.

| # | Finding | Verdict | Route |
|---|---------|---------|-------|
| 1 | BH: `groups.changed` did not refresh other windows | medium | P: layout refreshes on every event |
| 2 | BH: move select kept a refused value | medium | P: select resets after the attempt |
| 3 | BH: a device with an unknown `group_id` showed nowhere | medium | P: floor lists it as Unassigned |
| 4 | BH: toast live region toggled by display | medium | P: rendered only while shown |
| 5 | BH/VG: owner command without `base_rev` failed | medium | P: owners may omit it (last write wins); tests |
| 6 | BH: default order collided | low | P: max + 1 |
| 7 | BH: id/base_rev validation shown as a generic failure | low | P: stale message and reload |
| 8 | BH: drag of a wrong-kind device sent a doomed intent | low | P: `fits` guard |
| 9 | BH/VG: migration would fail on a non-null `group_id` | low | P: copy NULL (2.18 never set one) |
| 10 | VG: negative owner-toast test; assign/delete stale, deleted group, unknown device tests | medium | P |
| 11 | BH: unique violation race maps to a generic DB error | low | D |
| 12 | VG: no HTTP-level test of `groups.changed` / 409 mapping | high | D |
| 13 | VG: axe in HC and light only default theme; select test uses `selectOption`; drag-drop test | medium | D |
| 14 | VG: snapshot mapping test with real groups; Counter delete/assign tests | low | D |
| 15 | BH: `.claude/settings.json` in diff | n/a | R: not part of the commit |
| 16 | `DeviceRegister.kind` shares the payload tag name `kind` (2.18), so the wire form is ambiguous | medium | D (see deferred-work) |
