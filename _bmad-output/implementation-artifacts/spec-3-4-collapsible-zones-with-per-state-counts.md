---
title: 'Story 3.4: Collapsible zones with per-state counts'
type: 'feature'
created: '2026-10-26'
status: 'in-review'
baseline_commit: 'a8b0f6ed785dd69c9a459d1196250b811f7b84c3'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-2-floor-view-shows-every-device-as-a-live-tile-grouped-by-zone.md'
---

<intent-contract>

## Intent

**Problem:** Staff in a large café cannot fold away zones they are not watching, so the Floor is one long scroll.

**Approach:** Each zone header gets a collapse button. A collapsed zone shrinks to one row with its name, tile count and a count per state, computed from the same pushed snapshot. The collapsed set is kept in this PC's `localStorage`; nothing goes through `apply()`.

## Boundaries & Constraints

**Always:**
- Collapse control is a real `<button>` inside the zone `h2`, with `aria-expanded` and the zone name as its accessible name; click, tap, Enter and Space toggle it; target ≥44px.
- Collapsed row: zone name, tile count, then counts like "5 in session · 2 free · 1 offline" (zero states omitted, fixed state order). Counts follow `devices.changed` without expanding.
- Collapsed zones render no tiles, and take no part in the roving tab stop or arrow navigation.
- Collapsed zone ids persist per PC in `localStorage`; a fresh install has none collapsed and nothing collapses automatically at any device count.
- All strings in `en.json`; keep every existing `data-testid`.

**Never:**
- Backend, snapshot or migration changes; any write through `apply()`.
- Collapse for group or Unassigned sections, alert badges on the collapsed row (3.25).
- Code comments (user rule).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Collapse | activate button | tiles gone, row shows "N devices" and state counts, `aria-expanded=false` | N/A |
| Live | device goes offline while collapsed | counts change, zone stays collapsed | N/A |
| Restart | reload after collapsing | same zone still collapsed | unreadable storage → all expanded |
| Fresh | no stored value | every zone expanded | N/A |

</intent-contract>

## Code Map

- `apps/hub/src/routes/floor/+page.svelte` -- zone loop (`{#each zones}`), `BADGE`, `order` (roving tab stop source), `count()`.
- `packages/ui/src/status.ts` -- `STATUS_LABELS` (export from `index.js` for the state names).
- `packages/ui/src/messages/en.json` -- `hub.floor.count_one/other` exist; add collapse and state-count strings.
- `apps/hub/tests/floor.spec.ts`, `floor-fixture.ts` -- Playwright mock Hub; `devices.changed` gate pattern.

## Tasks & Acceptance

**Execution:**
- [x] `packages/ui/src/index.js`, `en.json` -- export `STATUS_LABELS`; add `hub.floor.state_count`, `hub.floor.collapse_hint`
- [x] `apps/hub/src/routes/floor/+page.svelte` -- collapse button, counts row, persisted set, collapsed zones left out of `order`
- [x] `apps/hub/tests/floor.spec.ts` -- collapse, live counts, restart persistence, fresh install, keyboard, accessible name, axe

**Acceptance Criteria:**
- Given a zone header, when staff click or press Enter/Space on its button, then the zone is one row with name, count and per-state counts.
- Given a collapsed zone, when a device changes state, then its counts update without expanding.
- Given collapsed zones, when the Hub UI reloads, then the same zones are collapsed.
- Given a fresh install, when Floor opens, then every zone is expanded.
- Given focus on a zone button, then a screen reader hears the zone name and expanded/collapsed.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `pnpm --filter @arena/hub check` and `pnpm run check:strings` -- expected: pass
- `pnpm --filter @arena/hub test:shell` -- expected: pass
