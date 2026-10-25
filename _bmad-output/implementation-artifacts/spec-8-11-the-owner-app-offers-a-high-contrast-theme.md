---
title: 'Story 8.11: The Owner app offers a high-contrast theme'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '0a1b692'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-10-the-owner-app-shell-navigation-themes-install-and-free-limit.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-21-high-contrast-theme-as-a-per-device-branch-setting.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The shared `ThemeToggle` offers System, Light and Dark only. An owner with low vision, or in bright light, cannot switch the Owner app to the high-contrast token set the branch apps already use.

**Approach:**
- Add a fourth choice, `hc`, to the shared theme helpers and `ThemeToggle` ("High contrast"). It is stored in the same per-device `localStorage` key, so it is never synced and survives sign-out (sign-out clears only the IndexedDB cache).
- `hc` resolves to `data-theme="hc"` regardless of `prefers-color-scheme`; the pre-paint scripts of the Owner and onboarding apps resolve it the same way.
- The existing `[data-theme="hc"]` tokens and `base.css` rules (solid surfaces, no shadows, 2px outline, yellow focus ring) apply unchanged.
- Under System with `forced-colors: active`, no theme override is added: the browser's system colours apply and every control stays visible and focusable.

## Boundaries & Constraints

**Always:**
- Reuse the 1.21 HC tokens and rules. No new colours, no `-hc` token additions.
- State is never colour-only: badges, tier-locks and offline notices keep their text labels (asserted in HC).
- All strings in `en.json`.

**Never:**
- Syncing the choice to the cloud or other devices.
- An `hc` theme or `ThemeToggle` on the branch apps (`no-theme-toggle` test stays green).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Choose HC | Account, tap High contrast | `data-theme="hc"`, tab selected, remembered after reload |
| HC over OS | HC chosen, OS light or dark | stays `hc` |
| Sign out | HC chosen, sign out | choice kept |
| Forced colours | System, `forced-colors: active` | no data-theme override beyond light/dark; controls visible and focusable |
| Stored garbage | unknown value | System |

</frozen-after-approval>

## Code Map

`packages/ui/src/theme.js`, `packages/ui/src/components/ThemeToggle.svelte`, `packages/ui/src/messages/en.json`, `apps/owner/src/app.html`, `apps/onboarding/src/app.html`, `apps/owner/tests/{theme,a11y,shell}.spec.ts`.

## Tasks & Acceptance

- [ ] Tests first: HC choice, remembered, beats OS, kept across sign-out, HC computed styles, labels present, forced colours, axe in HC on all routes at both widths.
- [ ] `theme.js`, `ThemeToggle`, `en.json`, both `app.html` scripts.

**Acceptance Criteria:**
- Given the Story 8.11 criteria in `epics.md`, when the Owner app runs against a stubbed `/v1`, then each is met, the contrast check passes and axe reports no serious or critical violation in light, dark and HC.

## Verification

`ARENA_PW_PORT=5491 pnpm --filter @arena/owner test`; onboarding tests; every `pnpm run check:*`; `test:scripts`; gallery `test:a11y`; builds excluding marketing.

## Review Triage Log

Pass 1 (Verification Gap and Blind Hunter; on Sonnet). P = patch, D = defer, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH: no HC CSS in the diff | n/a | R | the 1.21 `[data-theme="hc"]` tokens and `base.css` rules already exist; the HC computed-style test proves they apply |
| 2 | VG: forced-colours test never checks `data-theme` | medium | P | asserts the theme is not `hc` |
| 3 | VG: onboarding pre-paint HC untested | medium | P | onboarding test: stored `hc` wins over the OS and selects the tab |
| 4 | VG: solid surfaces and other interactive surfaces unchecked | medium | P | nav background opacity and a button outline asserted |
| 5 | BH: other `data-theme` consumers / `color-scheme` | low | R | no `light`/`dark` assumptions elsewhere; tokens set per theme |
| 6 | BH: four tabs overflow at 360 px | low | R | a11y suite checks targets and no horizontal scroll at both widths |
| 7 | VG: variance, data-age, command-row labels not asserted in HC | medium | D | those components do not exist in the Owner app yet; asserted when stories 8.x add them |
| 8 | VG/BH: ring test brittle (Tab round trip, literal colour) | low | D | |
| 9 | VG: no `theme.js` unit test; no "not synced" network assertion; sign-out stub | low | D | |
| 10 | BH: no `prefers-contrast` auto-select | low | R | HC is an explicit per-device choice |
