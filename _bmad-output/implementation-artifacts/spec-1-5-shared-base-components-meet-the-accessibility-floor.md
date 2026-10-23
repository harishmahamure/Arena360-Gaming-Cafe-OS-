---
title: 'Story 1.5: Shared base components meet the accessibility floor'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '3604755e6259094dec36a289c0b11f9ec93dedc4'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** No shared controls exist yet. Each surface would hand-roll buttons and fields, and keyboard focus, target size, non-colour cues and HC behaviour would drift or be missed.

**Approach:** Build the base components in `@arena/ui` from the generated tokens, plus a dev-only gallery app that shows every component in every theme and state. A Playwright + axe suite in CI enforces the accessibility floor: focus visibility, 44 px targets, zero serious or critical axe violations, HC behaviour, and a pseudo-locale with strings 30% longer.

## Boundaries & Constraints

**Always:**
- **Components** live in `packages/ui/src/components/` and are exported from `@arena/ui`. Svelte 5 runes. Styling uses token vars only (no hex). All text comes from `t()`.
  - `Button`: `variant: 'primary'|'secondary'|'warning'`.
    - `disabled` requires `disabledReason: MessageKey`. When disabled it renders at 40% opacity with no hover and a not-allowed cursor. The reason shows as inline text at full ink, linked through `aria-describedby`.
    - Light theme `button-warning` gets a 1px `warning-text` edge.
  - `TextField`:
    - label above, linked via `for`/`id`; `surface-2` fill, 1px `border-strong`, radius sm, ≥44px; focus shows a 2px accent border plus the focus ring.
    - `error` prop: 2px danger border, a ⚠ glyph and the message below, with `aria-invalid` and `aria-describedby`. The error text goes into a polite live region.
    - `type="password"` adds a show/hide toggle button with a catalogue label. Paste is never blocked.
  - `StatusBadge`: an uppercase `hub-micro` text tag in the state colour, with a 1px currentColor border, on `hub-tile`. The state is always written as text.
  - `AlertBadge`: a red pill with `on-danger` ink showing a count or a tag (`NEW`, `CALL`, `TIME UP`, `REQUEST` from the catalogue). Count changes are announced in a polite live region.
  - `Card`: a `surface` container with radius xl and the owner/web padding tokens.
  - `TabControl`: ARIA tabs. Arrow, Home and End keys move between tabs, with roving `tabindex`. Items are ≥44px; the selected item is inverted (`text` fill, `bg` ink). An optional `onchange` prop notifies the caller of the selected tab; URL syncing is left to the caller.
- **`focusFirstInvalid(form)`** helper, exported: focuses the first `[aria-invalid="true"]`.
- **`base.css` gains:**
  - **Focus:** a global `:focus-visible` ring. Dark is a 2px `--color-focus-ring-inner` ring plus a 2px outer ring at a 2px offset; HC is 4px `--focus-ring`; light is 2px `--focus-ring`. The ring is never removed: no `outline: none` anywhere without a replacement ring.
  - **Interaction states:** hover 6% lighter (light theme 6% darker) and pressed 6% darker with a 1px inset, via `color-mix`. Touch devices get the pressed state only, via `@media (hover: none)`.
  - **HC:** `transition: none !important`, no `box-shadow`/opacity translucency except the disabled 40%, and a 2px white outline on interactive surfaces that becomes 3px on hover.
  - **Links:** always underlined.
- **Hub-only components:** `StatusBadge` is Hub-only per Story 1.4's decision, so the gallery shows it in dark and HC only.
- **Gallery app:** `apps/gallery` (`@arena/gallery`) is a dev-only SvelteKit app with `adapter-static`. It is never deployed and not added to the deploy-style build matrix.
  - Its single route renders every component in three `[data-theme]` sections: dark, hc and light.
  - Each component appears in rest, hover, pressed, focused, disabled and error states where they apply. Hover, pressed and focused are forced through a `data-force-state` attribute that the components style identically to the real pseudo-classes.
  - It includes a demo form whose submit validates the fields and calls `focusFirstInvalid`.
  - `?pseudo=1` switches `t()` to a pseudo-locale: every message +30% length, with accented characters. Add the pseudo mode to `packages/ui/src/messages/index.js` as `setPseudo(bool)`.
  - It has landmarks (`header`/`main`) and one `h1`.
- **Playwright tests** (`apps/gallery/tests/a11y.spec.ts`) run against `vite preview` on Chromium:
  - **Tab order:** tab through every stop and assert each has a visible indicator (a non-none outline or box-shadow differing from the unfocused state).
  - **Targets:** every `button, a, input, [role=tab], [tabindex]` is at least 44 × 44 px in each theme.
  - **axe:** `@axe-core/playwright` reports zero serious or critical violations in each theme section.
  - **HC:** computed `transition-duration` is 0s, `box-shadow` is none, and the outline is 2px (3px on hover).
  - **Pseudo-locale:** no element with `overflow: hidden` or `clip` has `scrollWidth > clientWidth`, and component boxes don't overlap.
  - **Disabled:** disabled buttons show non-empty reason text.
  - **Errors:** an error field shows ⚠, the message and `aria-invalid`, and submitting focuses the first invalid field.
  - **Badges:** a live region exists for the `AlertBadge` count and the `TextField` errors.
- **Pins:** `@playwright/test` 1.64.1 and `@axe-core/playwright` 4.14.0 go in the catalog, with README pin rows.
- **CI:** a new `a11y` job on ubuntu-latest: install, `pnpm exec playwright install --with-deps chromium`, build the gallery, run the tests, and upload the report on failure. `ci-ok` adds it to `needs`.
- **Catalogue:** new keys for component labels (show/hide password, tags, gallery headings and demo text, disabled reasons). Every existing gate stays green: strings, hex, tokens-used and contrast.

**Never:**
- Floor tiles, inline-pin, hub-rail or other later components.
- Theme switching UI.
- Honouring OS reduced motion separately (DESIGN: only HC cuts motion).
- Ship the gallery in any surface.
- `outline: none` without a replacement focus ring.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Keyboard focus | Tab onto any control, any theme | the ring is visible with the theme's style | test fails naming the element |
| Disabled | `<Button disabled disabledReason="…">` | 40% opacity, reason shown | missing reason gives a type error, and a dev throw |
| Field error | submit with an empty required field | danger border, ⚠, message, live announcement, focus on the field | N/A |
| Password | click show/hide | input type toggles; the label updates from the catalogue | N/A |
| Tabs | ArrowRight on the last tab | wraps to the first; `aria-selected` moves | N/A |
| Small target | a control under 44px | the target-size test fails naming it | N/A |
| Pseudo-locale | `?pseudo=1` | no clipped or overlapping text | the test names the element |
| HC | the `[data-theme=hc]` section | instant transitions, no shadows, 2px white outline | N/A |

</frozen-after-approval>

## Code Map

- `packages/ui/src/base.css`, `src/tokens/tokens.css` (generated; aliases `--bg`, `--text`, `--accent`, `--focus-ring`, `--color-*`, component tokens such as `--button-primary-background` and `--text-field-*` with `-hc` twins) -- consume the tokens and never edit the generated files.
- `packages/ui/src/messages/{en.json,index.js}` -- add keys and the pseudo mode. `scripts/check-strings.mjs` lints `.svelte` text and attributes.
- `packages/ui/src/index.js`, `package.json` exports -- export the components.
- `apps/hub/*` -- the template for a SvelteKit static app: `vite.config.js` with the adapter in `sveltekit({adapter})` and a fixed port (gallery: 5181), `+layout.svelte` importing base.css, tsconfig extends `$app/tsconfig`.
- `scripts/check-pins.mjs` -- catalog entries must be exact and apps must use `catalog:`. `scripts/app-styles.test.mjs` expects every app to import base.css from its layout.
- `.github/workflows/ci.yml` -- add the `a11y` job and the `ci-ok` needs. Root `package.json` `build` builds `./apps/*`, which now includes the gallery; that's fine.

## Tasks & Acceptance

**Execution:**
- [x] `packages/ui/src/components/{Button,TextField,StatusBadge,AlertBadge,Card,TabControl}.svelte`, `packages/ui/src/forms.js` -- the components and the focus helper.
- [x] `packages/ui/src/base.css`, `packages/ui/src/messages/*` -- focus, interaction and HC rules; catalogue keys; the pseudo mode (+ a unit test in `scripts/messages.test.mjs`).
- [x] `apps/gallery/**` -- the app, the gallery route and `playwright.config.ts`.
- [x] `apps/gallery/tests/a11y.spec.ts` -- one test per matrix row and AC.
- [x] `pnpm-workspace.yaml`, `README.md`, `.github/workflows/ci.yml`, lockfile -- pins and the CI job.

**Acceptance Criteria:**
- Given the gallery, when `pnpm --filter @arena/gallery test:a11y` runs, then every test passes in dark, HC and light.
- Given the repo, when `pnpm check` and `pnpm build` run, then both pass.

## Implementation Notes

- Labels are props of type `MessageKey` (`label`, `disabledReason`, `error`, tab `label`), so every component string goes through `t()`. `check-strings` now treats a text attribute whose whole value is a key-shaped string as a catalogue reference: known keys pass and unknown keys fail as `unknown message key`.
- The dark focus ring is `outline` (inner, 2px offset) plus `box-shadow` (a 2px `--ring-gap` gap, then the 6px outer ring). `--ring-gap` defaults to `--bg`; Card and the tab track set it to their own fill.
- In HC the 2px white outline is `outline`, so the focused element swaps it for the 4px `--focus-ring`. Hover makes it 3px, pressed insets it with `outline-offset: -2px`, and every `box-shadow` and transition is forced off.
- Interaction tints use `--ui-bg` on `.ui-interactive` in base.css, so components set `--ui-bg` and never `background-color`. Hover sits inside `@media (hover: hover)`.
- Disabled buttons use `aria-disabled="true"`, so they stay focusable and the reason can be reached. Clicks, Enter, Space and implicit submission are cancelled in the click handler. The reason is a sibling span outside the 40% opacity, linked by `aria-describedby`. The dev throw is `assertDisabledReason` in `forms.js` (unit-tested), and the type error is pinned by `apps/gallery/src/lib/type-checks.ts` through `svelte-check`.
- The pseudo-locale wraps each template as `[accented ~~~]`. Placeholders stay intact, and the result is at least 130% of the original length.
- The Playwright overlap check compares `[data-component]` boxes that don't contain each other. The clipping check skips 1px visually-hidden live regions.
- The CI `a11y` job also runs `svelte-check` on the gallery, which enforces the type check above.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Button Props reject `disabled={boolean}` | medium | The union accepts only literal `false`/`true`, so real callers fail svelte-check. | patch |
| 2 | Native `disabled` drops the button from tab order, so the reason is only reachable in browse mode | medium | `aria-describedby` on an unfocusable button is not read on focus. Fix: `aria-disabled` plus blocking activation. | patch |
| 3 | String lint accepts key-shaped values on native attributes, which then render raw keys (all three layers) | medium | `KEY_SHAPE` exemption isn't restricted to component tags. | patch |
| 4 | "1 alerts"; zero count shown; no plural | low | `ui.alert.count` = "{count} alerts". | patch |
| 5 | TabControl `selected` not in `items` leaves no tabbable tab (blind, edge) | low | Every tab gets `tabindex=-1`. | patch |
| 6 | `prefers-reduced-motion` ignored | false | The frozen spec says "Never honour OS reduced motion separately"; DESIGN says only HC cuts motion. | reject |
| 7 | No phone-width a11y run | low | The Card has an 839px breakpoint, and target/clipping checks matter most on a phone. | patch |
| 8 | Gallery tsconfig includes `test`, not `tests` | low | The spec file isn't type-checked. | patch |
| 9 | Playwright webServer previews a possibly stale build | low | `reuseExistingServer` serves old output. | patch |
| 10 | HC focus-ring colour not distinguished from the outline in tests | low | Width is tested; the tokens differ by DESIGN. | reject |
| 11 | HC `opacity: 1` rule has zero specificity (blind, edge) | low | `:where()` loses to any class rule. | patch |
| 12 | StatusBadge Hub-only rule enforced only by a comment | low | The only consumer is the gallery. | reject |
| 13 | Pseudo padding computed on the template | low | Approximate by design. | reject |
| 14 | Demo form "All fields look good." never clears | low | `valid` is never reset. | patch |
| 15 | Hover/pressed tints and their forced copies untested | low | Pre-verified gap. | patch |
| 16 | `assertDisabledReason` in Button never exercised | low | Types already block the case; it would need a component harness. | reject |
| 17 | Tab keys swallow Alt/Ctrl/Meta+Arrow | low | `preventDefault` on modified keys. | patch |
| 18 | RTL arrow direction | low | English-only product. | reject |
| 19 | `items` shrinking leaves stale tab refs | low | No dynamic items yet. | reject |
| 20 | A repeated identical error isn't re-announced | low | Live region content unchanged; edge case. | reject |
| 21 | `focusFirstInvalid` reports success when focus didn't move | low | Direct fix: compare `activeElement`. | patch |
| 22 | Nested theme subtrees inherit the light ring | low | No nested themes anywhere. | reject |
| 23 | Disabled guard skipped under SSR | low | Direct fix: call it in the script body. | patch |
| 24 | Gallery lacks some states (TextField pressed, Tab hover) | low | The spec says "where applicable". | reject |

## Verification

**Commands:**
- `pnpm check` -- expected: exit 0
- `pnpm --filter @arena/gallery build && pnpm --filter @arena/gallery test:a11y` -- expected: all pass (use Node 22.23.4; run `pnpm exec playwright install chromium` first)
- `pnpm -r --workspace-concurrency=1 --filter "./apps/*" build` -- expected: exit 0
