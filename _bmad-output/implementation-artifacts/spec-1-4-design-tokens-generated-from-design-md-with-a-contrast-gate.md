---
title: 'Story 1.4: Design tokens generated from DESIGN.md with a contrast gate'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '889d99bba9e073a4a2f84daed5a3c63c8cc05362'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** No surface has design tokens yet. Colours, type and spacing would be copied by hand, which leads to drift, incomplete HC and light themes, and colour pairs that fail contrast.

**Approach:** Generate CSS custom properties and TS constants from DESIGN.md's frontmatter into `@arena/ui`, behind a freshness gate. Add CI gates for:
- contrast, using pairs derived automatically
- missing token references
- raw hex colours
- remote fonts or CDNs in the Hub, Kiosk and Kitchen builds

Bundle Sora.

## Boundaries & Constraints

**Always:**
- **Source and parsing:**
  - The generator reads `_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md`, a path constant shared by every script.
  - It parses the frontmatter with the `yaml` npm package, an exact pin in root devDependencies with a README pin row.
- **DESIGN.md edit (decided by the human):** add an `elevation:` frontmatter key taken from the "Elevation & Depth" prose: `game-tile: 0 10px 30px rgba(0,0,0,.35)`, `menu: 0 16px 40px rgba(0,0,0,.35)`, `toast: 0 20px 60px rgba(0,0,0,.5)`, `none: none`. Add one sentence in that prose section pointing to the key. Make no other DESIGN.md changes.
- **Generated output:** `packages/ui/src/tokens/` is committed and carries a "generated, do not edit" header. Regenerate with `pnpm gen:tokens`.
  - **`tokens.css`:**
    - `:root` declares `--color-<name>` for every colour, including every `-hc` and `-light` value.
    - Theme mapping: `:root, [data-theme="dark"]` sets the semantic aliases (`--bg`, `--text`, ... one per base colour name) to the dark values. `[data-theme="hc"]` maps them to `-hc` and `[data-theme="light"]` maps them to `-light`, falling back to the dark value only where DESIGN.md has no light twin.
    - Component tokens: `--<component>-<prop>` for each `components.*` property whose value is a single `{colors.X}` reference, with an `-hc` twin pointing at `X-hc`.
    - Type ramps: `--type-<ramp>-{family,size,weight,line-height,letter-spacing}` plus a `.type-<ramp>` utility class for every `typography.*` entry (Kiosk, Hub and Web ramps).
    - Also `--space-<key>` (including `touch-min` 44px), `--radius-<key>`, `--elevation-<key>`, and a `.tabular-nums` utility.
  - **`tokens.ts`:** typed `as const` objects (`colors`, `typography`, `spacing`, `radius`, `elevation`, `components`) and a `TokenName` union of every CSS variable name.
- **Generator failures:** generation fails with a clear message if any component colour reference has no `-hc` twin, or if any `{colors.X}` reference is unresolved.
- **Freshness gate:** `check:tokens-fresh` regenerates and fails when `git status --porcelain -- packages/ui/src/tokens` is non-empty. Reuse `parsePorcelain`/`formatReport`/`gitStatus` from `scripts/check-generated.mjs`.
- **Contrast gate (`check:contrast`):** uses WCAG 2.x relative luminance. Pairs are derived automatically per theme (dark, hc, light), with no hand list:
  1. Every component with both a `foreground` and a `background` colour reference is a text pair with a floor of 4.5. The floor is 3.0 when the component's typography is ≥24px, or ≥18.66px at weight ≥700.
  2. Every ink token (`text`, `text-muted`, `ok`, `danger`, `info`, `state-*`, plus `accent-text` and `warning-text` where they exist) is checked against every surface in its theme (`bg`, `surface`, `surface-2`, and `hub-bg`/`hub-header`/`hub-tile` in dark and hc), with a floor of 4.5.
  3. Every `on-X` is checked on `X`, and every `chip-*-text` on its `chip-*-bg`, with a floor of 4.5.
  4. Non-text, with a floor of 3.0:
     - `border-strong` and the focus-ring colours against each surface in their theme;
     - `accent` against each surface;
     - the group swatches against `hub-tile` and `hub-tile-hc`.
  - Theme twins are resolved by the `-hc`/`-light` suffix.
  - **Decision (human, after the first run):** in dark and hc, `state-*` inks are checked only against `hub-tile`, `hub-bg` and `surface`, where floor states are drawn. In light they stay checked on `bg`, `surface` and `surface-2`. This reflects that `state-paused` is 4.22 on `surface-2`, a surface it never sits on.
  - **Decision (human):** the light theme isn't checked for Hub-only tokens: `hub-*` colours, and components whose colour refs are all `hub-*`/`state-*` (for example `tile-order-badge`). DESIGN.md makes the Hub dark + HC only. Do not emit light component twins for them either.
  - Tokens documented as fill-only or decorative are excluded and named in a constant with the DESIGN.md reason: `line*`, `hub-line*`, `warning-light`, `scrim-base` and the `focus-ring-outer`-over-media case.
  - Output is one line per failing pair (theme, fg, bg, ratio, floor). The gate exits non-zero on any failure.
- **Sora:** `@fontsource-variable/sora` (pinned through the catalog) is imported by `@arena/ui`'s base stylesheet `packages/ui/src/base.css`. That file imports `tokens.css`, sets `font-family: var(--type-…-family), system-ui`, and is imported by every app's root layout or page. Vite bundles the woff2, so nothing loads remotely.
- **Remote-reference gate (`check:no-remote`):** runs after the build over `apps/{hub,kiosk,kitchen}/build`. It fails on any `url(http…)`, `@import` of http(s), `<link href="http…">`, `<script src="http…">`, or any `fonts.googleapis`/`fonts.gstatic`/`cdn` host, naming the file and URL. It runs in the CI `build` job for those three apps.
- **Unknown-token gate (`check:tokens-used`):** scans `.svelte`/`.css`/`.ts`/`.js`/`.astro` under `apps/*/src` and `packages/ui/src` (excluding `src/tokens`) for `var(--name)` and fails on names not defined in `tokens.css` or declared in the same file. It names the file:line and the token.
- **Hex lint (`check:hex`):** the same scan scope. It fails on `#[0-9a-fA-F]{3,4,6,8}` colour literals outside `packages/ui/src/tokens/`, except HTML entities and lines marked `hex-ignore`.
- **Wiring:** every gate is a `scripts/*.mjs` file with its pure core exported, `node --test` fixtures for each matrix row, root scripts, `pnpm check` (the build-dependent gates excluded) and the CI `checks` job.

**Never:**
- Change any colour value or contrast floor in DESIGN.md to make the gate pass. If a real DESIGN.md pair fails, stop and report it.
- Theme switching UI or per-device HC setting (Story 1.21).
- Restyle apps beyond importing `base.css`.
- `backdrop-filter`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh | tokens committed | `check:tokens-fresh` exits 0 | N/A |
| Stale | a DESIGN.md colour is edited, not regenerated | fails | lists `packages/ui/src/tokens/tokens.css` |
| Low contrast | fixture: text `#777777` on bg `#666666` | `check:contrast` fails | `dark text on bg 1.3 < 4.5` |
| Large text | fixture: component at 24px with a 3.2 ratio | passes (floor 3.0) | N/A |
| Missing twin | fixture: component refs `{colors.foo}`, no `foo-hc` | generation fails | names `foo` |
| Removed token | `var(--color-gone)` in an app | `check:tokens-used` fails | `file:line unknown token --color-gone` |
| Raw hex | `color: #FF00AA` in a .svelte file | `check:hex` fails | `file:line raw hex colour #FF00AA` |
| Remote font | build CSS has `url(https://fonts.gstatic.com/x.woff2)` | `check:no-remote` fails | names the file and URL |

</frozen-after-approval>

## Code Map

- `scripts/check-generated.mjs` -- reuse `parsePorcelain`, `formatReport` and `gitStatus` (it takes the root; generalize it to a dir argument). `scripts/gen-types.mjs` shows the temp-dir, then format, then swap pattern; follow it for `gen-tokens.mjs`.
- `scripts/check-strings.mjs` -- `listFiles`/`scanRoots` style, the exact-path skip and the `isMain` realpath guard. Follow them. `check:strings` scans `.svelte`, so the new `base.css` imports must not add text.
- `packages/ui/{package.json,src/index.js,src/Placeholder.svelte}` -- add exports for `./tokens.css`, `./base.css` and `./tokens` (TS). The Placeholder may use `var(--text)` and so on.
- `apps/*/src/routes/+page.svelte` (7 SvelteKit apps) and `apps/marketing/src/pages/index.astro` -- import `@arena/ui/base.css`.
- `package.json`, `.github/workflows/ci.yml` (`checks` job; the `build` matrix with its Placeholder grep step) -- wire the gates.
- `README.md` -- pin table and the gates section.

## Tasks & Acceptance

**Execution:**
- [x] DESIGN.md -- `elevation:` key and the one-line prose pointer.
- [x] `scripts/gen-tokens.mjs` (+ test), `packages/ui/src/tokens/{tokens.css,tokens.ts}` -- the generator and its output.
- [x] `scripts/check-contrast.mjs` (+ test with fixtures), `scripts/check-tokens-used.mjs` (+ test), `scripts/check-hex.mjs` (+ test), `scripts/check-no-remote.mjs` (+ test), `scripts/check-tokens-fresh.mjs` -- the gates.
- [x] `packages/ui/src/base.css`, `packages/ui/package.json`, Sora dependency, app imports -- styling entry and font.
- [x] `package.json`, `pnpm-workspace.yaml`, `README.md`, `.github/workflows/ci.yml`, lockfile -- wiring and pins.

**Acceptance Criteria:**
- Given the repo, when `pnpm gen:tokens && git status --porcelain` runs after committing, then the tree is clean.
- Given the real DESIGN.md, when `pnpm check:contrast` runs, then it passes, or the implementer reports the exact failing pairs.
- Given `pnpm build`, when the Hub output is inspected, then the bundled CSS contains a local Sora `@font-face` woff2 and `check:no-remote` passes on Hub, Kiosk and Kitchen.
- Given the repo, when `pnpm check` runs, then every gate passes.

## Implementation Notes

- The first run of `check:contrast` failed on `dark state-paused on surface-2 4.2 < 4.5` and `light hub-tile on state-order-pending 3.0 < 4.5` (`tile-order-badge`). Both are resolved by the human decisions in the frozen block, without changing DESIGN.md: `STATE_SURFACES` limits dark/hc state inks to `hub-tile`, `hub-bg` and `surface`; light skips `hub-*` colours and Hub-only components (`isHubOnlyComponent` in `gen-tokens.mjs`: every colour ref is `hub-*`/`state-*`). Hub-only component tokens are emitted under `:root, [data-theme="dark"], [data-theme="hc"]` only, and `tokens.ts` exports them as `hubOnlyComponents`. Result: `contrast check: ok (237 pairs)`.
- In hc and light, a base colour with no twin falls back to its dark value (as the generated CSS does). A pair that resolves to the same colours as an earlier theme is reported once.
- `Sora` maps to the stack `"Sora Variable", "Sora"` (the fontsource variable family name). A ramp `note` that starts with "Uppercase" adds `text-transform: uppercase` to its `.type-*` class; "Tabular numerals" adds `tabular-nums`.
- The HC-twin check now covers every `{colors.X}` in a component value. Refs that don't follow `X-hc` are resolved by `hcTwin()`: an `-hc` ref is its own HC value; a `-light` ref is light-only (no `-hc` token is emitted for that prop); `focus-ring-inner`/`focus-ring-outer` map to `focus-ring-hc` (`HC_TWINS`, "In HC it is 4px yellow"); `group-*` swatches are used unchanged in HC (`HC_SHARED`; DESIGN.md gives their ratios on black).
- Compound component values replace `{colors.X}` with `var(--color-X)`, `{spacing|rounded|elevation.X}` with their variables, and `{typography.X}`/`{components.X}` with a plain identifier. A component token whose name equals a semantic colour alias (`focus-ring.inner` vs the `focus-ring-inner` colour) is not emitted; the alias keeps the name.
- `check:pins` now also reads the `yaml` root devDependency (`PINNED_DEV_DEPENDENCIES`) and requires it to be exact. `check:strings` skips `packages/ui/src/tokens`.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Nothing checks that apps import base.css or that builds contain Sora and the tokens; non-root routes would miss them (verify-gap, blind, edge) | medium | Deleting the import keeps every gate green. The import lives on `+page.svelte`, not the layout. | patch |
| 2 | tokens-fresh entry point never exercised end to end (verify-gap, blind) | low | The test wires `gitStatus(root, TOKENS_DIR)` itself. | patch |
| 3 | Light-only and HC-only colours get no semantic alias; focus ring stays pink in HC and light | medium | `aliases()` iterates the dark base names only; `--accent-text`, `--warning-text` and a themed focus ring are absent. | patch |
| 4 | Compound component values (`2px solid {colors.x}`) are dropped and skip the `-hc` twin check | medium | `SINGLE_COLOR_REF` gates both emission and validation; the focus-ring and border tokens are missing. | patch |
| 5 | Contrast pairs only use foreground/background; other ink keys ignored | low | The frozen rule defines component pairs as foreground + background. | reject |
| 6 | `base.css` forces `color-scheme: dark` | low | Light subtrees get dark native controls. Direct fix. | patch |
| 7 | `data-theme` never set; no theme toggle; web apps use the Hub body family | false | Theme switching is out of scope (Stories 1.21 and later); every ramp's family is Sora. | reject |
| 8 | Hub-only tokens inherit dark values under light | false | This is the human decision: the Hub is dark + HC only. | reject |
| 9 | check-hex flags `href="#add"` and `#cafe {` id selectors, and skips `app.html` | low | Verified by the edge reviewer. | patch |
| 10 | check-no-remote misses protocol-relative `@import`, `img`/`srcset`/`import()`/`ws`, and passes on an empty build dir (blind, edge) | medium | Verified: `import("https://esm.sh/x")` returns `[]`. An empty build passes vacuously. | patch |
| 11 | `generate()` can leave the tokens folder empty if the copy fails after `rmSync` | low | Contradicts its own comment. Fix: backup, rename, restore. | patch |
| 12 | Excluding `warning-light` also drops the light `button-warning` and `on-warning` pairs | medium | `isExcluded` runs on resolved twin names, so `on-warning-light` on `warning-light` is never measured. | patch |
| 13 | Group-swatch twin pairing / a non-hex colour throws | low | No group twins or non-hex colours exist. | reject |
| 14 | `isLargeText` assumes px | low | Every ramp is px. | reject |
| 15 | Typography ramp missing a field emits `undefined` | low | Direct validation. | patch |
| 16 | Generated name collisions silently override | low | Direct duplicate check. | patch |
| 17 | A component ref to a twin asks for a `-hc-hc` twin | low | No such refs exist. | reject |
| 18 | Uncaught stack traces in gate mains | false | A loud failure is correct. | reject |
| 19 | `var(--color-${x})` template flagged as an unknown token | low | Direct skip. | patch |
| 20 | Quoted object-key custom properties not seen as local declarations | low | No such usage exists. | reject |
| 21 | Missing tokens.css gives ENOENT | false | Loud failure. | reject |
| 22 | Placeholder lost its font fallback | low | base.css is always loaded; covered by #1. | reject |
| 23 | Dead `over: "media"` field, duplicate section numbering | low | Cosmetic. | reject |

## Verification

**Commands:**
- `pnpm gen:tokens && pnpm check` -- expected: exit 0
- `pnpm -r --workspace-concurrency=1 --filter "./apps/*" build && node scripts/check-no-remote.mjs` -- expected: exit 0 (use Node 22.23.4)
- `cargo test --workspace --locked` -- expected: pass
