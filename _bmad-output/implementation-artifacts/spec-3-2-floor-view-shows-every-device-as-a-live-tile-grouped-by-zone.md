---
title: 'Story 3.2: Floor view shows every device as a live tile grouped by zone'
type: 'feature'
created: '2026-10-26'
status: 'in-review'
baseline_commit: '362e1f7098a03702e232a64383a06ef0ccc50ff6'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-1-branch-billing-rules-rounding-minimum-grace-and-time-up-mode.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Floor route lists devices as plain boxes. There is no shared `floor-tile`, no rate or time line, no tile counts, no keyboard grid navigation, and no 6/2-column layout, so staff cannot read the whole café at a glance.

**Approach:** Add a shared `floor-tile` component with all eight states, show it in the gallery (dark, HC), and rebuild the Hub floor on it. Live updates stay on the existing pushed `devices.changed` / `groups.changed` refetch; no backend change.

## Boundaries & Constraints

**Always:**
- `floor-tile` (`@arena/ui`, `data-component="floor-tile"`): states free, in-session, paused, reserved, order-pending, maintenance, offline, unassigned; 1px `border-strong`, radius md, ≥128px tall; top row name + `status-badge`; middle time slot in `type-hub-numeric`; bottom secondary line; 3px state bar; `group-chip` bottom-right; `alert-badge` slot top-right. Unassigned: 2px dashed border, no bar, no chip. The state is always written out. The focusable face has an accessible name of device, state and time.
- Hub floor: zones in snapshot order with a tile count per header, grouped-but-zoneless devices next, Unassigned section last (with its count). Tiles render from the layout-loaded snapshot with no skeleton. A free tile with a group rate reads "₹100/hr · tap to start", offline reads "Last seen <time>", maintenance shows its note.
- Layout: 6 columns at ≥1600px, 2 columns below 840px with 16px gutter and 12px gap; targets ≥44px.
- Keyboard: one roving tab stop; arrows move focus in grid order; Enter activates the focused tile. Updates never move focus or reorder tiles (keyed each).
- Keep every existing `data-testid`, the Story 1.20 Remote unlock flow and the zone `floor_x`/`floor_y` placement.
- All strings in `en.json`.

**Never:**
- Backend, snapshot or migration changes.
- Paused / order-pending / reserved session data, the tile sheet (3.5), collapsible zones (3.4), alert sounds (3.25).
- Code comments (user rule).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Free, rate | online device, group rate 10000 | badge "Online", "₹100/hr · tap to start" | N/A |
| Free, no rate | online device, group rate null | "Tap to start" | N/A |
| Offline | `last_seen_at` set | "Last seen <time>" | none shown if unset |
| Maintenance | note "fan is dead" | "Unavailable for starts · fan is dead" | N/A |
| No group | device without group | unassigned tile, dashed, no chip | N/A |
| Push | `devices.changed` while a tile is focused | state updates, focus stays | N/A |
| Narrow | width 800px | 2 columns, tiles ≥44px targets | N/A |

</frozen-after-approval>

## Code Map

- `packages/ui/src/components/FloorTile.svelte` -- new; `status.ts` -- shared state labels used by `StatusBadge.svelte` and the tile; `index.js` -- export.
- `apps/gallery/src/routes/ThemeSection.svelte` -- floor-tile section behind `hubThemes`; `apps/gallery/tests/a11y.spec.ts` -- eight states, name, focus, targets.
- `apps/hub/src/routes/floor/+page.svelte` -- rewrite tile snippet on `FloorTile`, counts, layout, roving focus; `apps/hub/src/lib/floor-nav.ts` -- pure next-tile function.
- `packages/ui/src/messages/en.json` -- `hub.floor.free_rate`, `hub.floor.tap_to_start`, `gallery.section.floor_tile`.
- `apps/hub/tests/floor.spec.ts` -- new Playwright test (mock Hub, axe dark/HC); existing `presence`, `maintenance`, `zones`, `shell`, `pair-kiosk` specs must stay green.

## Tasks & Acceptance

**Execution:**
- [x] `packages/ui/src/status.ts`, `FloorTile.svelte`, `StatusBadge.svelte`, `index.js` -- shared labels and the tile
- [x] `ThemeSection.svelte`, `a11y.spec.ts`, `en.json` -- gallery section and checks
- [x] `apps/hub/src/lib/floor-nav.ts`, `floor/+page.svelte` -- Floor rebuilt on the tile
- [x] `apps/hub/tests/floor.spec.ts` -- I/O matrix, push, keyboard, narrow layout, axe in dark and HC

**Acceptance Criteria:**
- Given the gallery, when it opens in dark and HC, then eight tiles render with text states and zero serious axe violations.
- Given a snapshot, when Floor opens, then zones show name and count and Unassigned is last.
- Given an open Floor, when `devices.changed` arrives, then the tile updates without moving focus.

## Implementation Notes

- `FloorTile` is the root element itself (`tag="li"` in the Hub) so existing test ids, the dashed border and `--x`/`--y` placement stay on the `li`; the focusable `role="group"` face inside carries the accessible name and Enter / click activation.
- A device with no group is the `unassigned` state; its online/offline text moves to the secondary line under `device-presence`. Other tiles keep `device-presence` on the state badge.
- Zone grids use `max(6, floor_x + 1)` columns; zoneless group sections use 6 columns from 1600px; below 840px every grid is 2 columns with a 12px gap.
- Roving tab stop and arrow navigation live in the Hub page (`floor-nav.ts`); Enter sets `aria-current` until the tile sheet (3.5) exists.
- Paused, order-pending and reserved session data are not derivable yet, so only the gallery shows those states.
- Verified: `pnpm -r check`, format, strings, tokens, contrast, layers checks; gallery a11y (34) and full Hub Playwright suite (611 passed, 11 skipped by project).

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `pnpm check` and `pnpm --filter @arena/hub check` -- expected: pass
- `pnpm --filter @arena/gallery test:a11y` and `pnpm --filter @arena/hub test:shell` -- expected: pass
