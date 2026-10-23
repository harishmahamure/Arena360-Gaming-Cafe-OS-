---
title: 'Story 1.3: Generated protocol types and the message catalogue lint'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '42aad91827a04cee1eefba449cf79fafea9d67a8'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Svelte surfaces could hand-copy Rust protocol types, or hard-code UI strings. Either way the Rust types and the UI text drift apart silently.

**Approach:**
- Generate TS types from `arena-proto` with ts-rs into `packages/ui/src/generated/`, behind a CI freshness gate.
- Add a lint that forbids hand-written copies of protocol types and imports from anywhere other than `@arena/ui/generated`.
- Add one English message catalogue in `@arena/ui`, seeded with the fixed Voice lines.
- Add a string lint that fails on hard-coded user-visible strings and on missing catalogue keys.

## Boundaries & Constraints

**Always:**
- **`arena-proto`:**
  - `AppError { code: ErrorCode, message: String, field: Option<String> }`, serialized with serde (`field` skipped when it is `None`, optional in TS).
  - `ErrorCode` is a unit enum serialized `SCREAMING_SNAKE_CASE`. It is seeded with the codes Epic 1 names: `BAD_FRAME`, `UNSUPPORTED`, `FORBIDDEN`, `NOT_PRIMARY`, `SCHEMA_TOO_NEW`, `EXPIRED`, `STALE`, `TOKEN_REVOKED`, `HUB_SLOW`.
  - `Permission` is an empty catalogue enum, so its TS type is `never`.
  - All three derive `TS` with export enabled.
  - Unit tests round-trip `AppError` through serde JSON.
- **Pins:** ts-rs `=12.0.1`, serde and serde_json go in `[workspace.dependencies]` as exact pins, each with a README pin-table row. The README records "type generator: ts-rs" with one line on how to regenerate.
- **Generation:**
  - Command: `pnpm gen:types`. It runs ts-rs's export via `cargo test -p arena-proto`, with `TS_RS_EXPORT_DIR` set in `.cargo/config.toml` (`relative = true`) to `packages/ui/src/generated`.
  - It then writes a generated `index.ts` barrel re-exporting every type, then runs prettier on the folder.
  - Each generated file starts with ts-rs's "do not edit" header. The barrel gets the same header.
  - Generated files are committed.
- **Package exports:** `@arena/ui` adds the export entries `./generated` → `src/generated/index.ts`, plus `t`.
- **Freshness gate (`scripts/check-generated.mjs`):** runs generation, then fails if `git status --porcelain -- packages/ui/src/generated` is non-empty. It prints each stale, new or deleted file.
- **Protocol lint (`scripts/check-protocol-types.mjs`):**
  - It gets the protocol type names by parsing the generated barrel.
  - It scans `apps/**` and `packages/**` (`.ts`, `.js`, `.svelte`, `.astro`), skipping `generated/`, `node_modules`, `build`, `dist` and `.svelte-kit`.
  - It fails on (a) any `type|interface|enum|class` declaration whose name is a protocol type name, and (b) any import of a protocol type name from a specifier other than `@arena/ui/generated`.
  - Inside `packages/ui` itself, a relative import of `./generated` is allowed.
  - Each failure names the file and line.
- **Message catalogue:** `packages/ui/src/messages/en.json` holds flat dotted keys (`player.login.failure`, `staff.command.queued_offline`, ...).
  - It contains every fixed player line and every staff/owner line in EXPERIENCE.md "Voice and Tone", the `app.name` key ("Arena 360"), and one key per surface name.
  - Variable parts become `{name}` placeholders, for example `"Balance {balance}. Top up at counter."` and `"Setup code for {name}: {code}. Give it to them; it works once."`. The fixed copy's "her" becomes the neutral "them", because the name is a variable.
- **`t(key, params?)`:** in `packages/ui/src/messages/index.js`. It interpolates the placeholders and throws on an unknown key in dev.
- **String lint (`scripts/check-strings.mjs`):**
  - Scope: `.svelte` and `.ts`/`.js` under `apps/*/src` and `packages/ui/src`, excluding `generated/` and `messages/`.
  - Svelte: uses the `svelte/compiler` `parse` AST. It flags text nodes containing a letter, and literal values of `title`, `alt`, `placeholder`, `aria-label` and `label` attributes.
  - TS/JS: flags string literals that look like prose (start with a letter, contain a space and at least 2 words), skipping import/export specifiers.
  - A line containing `// i18n-ignore` (or `<!-- i18n-ignore -->` on the line before) is exempt.
  - It also extracts every literal `t("…")` / `t('…')` key and fails if the key is missing from `en.json`.
  - Each failure prints `file:line` and the reason.
- **Existing UI moves to the catalogue:**
  - `Placeholder` renders `t('app.name')` and a `surface` prop.
  - App pages pass `surface={t('surface.<app>')}`.
  - `<title>`s use `t()`.
  - Marketing's `.astro` page passes a `t()` value.
- **Wiring:**
  - Root scripts: `gen:types`, `check:generated`, `check:protocol`, `check:strings`, all added to `check`.
  - The CI `checks` job runs all three.
  - Every checker has its pure core in exported functions with `node --test` suites (`scripts/*.test.mjs`).

**Never:**
- Locale formatting (Story 1.12).
- Design tokens (Story 1.4).
- Translation into other languages.
- Network calls in the scripts.
- Hand-written protocol types anywhere, including tests outside `scripts/` fixtures.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh | generated files committed and current | `check:generated` exits 0 | N/A |
| Stale | a new `ErrorCode` variant is added in Rust, files not regenerated | fails | lists `packages/ui/src/generated/ErrorCode.ts` |
| Hand copy | `apps/hub/src/x.ts` declares `type AppError = …` | `check:protocol` fails | `apps/hub/src/x.ts:1 hand-written protocol type AppError` |
| Wrong import | `import type { AppError } from "../../packages/ui/src/generated/AppError"` in an app | fails | names the file:line and the required specifier |
| Hard-coded text | `<p>Printer offline</p>` in a `.svelte` file | `check:strings` fails | `file:line hard-coded string "Printer offline"` |
| Ignored | the same, with `<!-- i18n-ignore -->` above it | passes | N/A |
| Missing key | `t('staff.nope')` | fails | `file:line unknown message key staff.nope` |
| Interpolation | `t('player.balance.insufficient', {balance: '₹40'})` | `"Balance ₹40. Top up at counter."` | an unknown key throws |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/{Cargo.toml,src/lib.rs}` -- placeholder lib. Add modules `error.rs` and `permission.rs` and keep the existing test. `arena-proto` must stay free of workspace deps; external deps are fine.
- `Cargo.toml` -- exact `=` pins are enforced by `scripts/check-pins.mjs` (`readCargoWorkspaceDeps` handles `name = "=x"` and `{ version = "=x", features = [...] }`). Add rows to the README pin table.
- `packages/ui/{package.json,src/index.js,src/Placeholder.svelte}` -- extend the exports; `Placeholder` currently renders a literal "Arena 360".
- `apps/*/src/routes/+page.svelte` and `apps/marketing/src/pages/index.astro` -- currently pass literal `surface="Hub"` and so on, plus literal `<title>`s. Move these to `t()`.
- `scripts/check-pins.mjs` -- `checkManifestDeps` already enforces `catalog:` and `workspace:*`. Follow its style: pure exported core, a realpath-guarded `main`, and tests in `*.test.mjs`.
- `package.json` (root) `check` script and `.github/workflows/ci.yml` `checks` job (it already has the Rust toolchain and pnpm) -- add the new checks there.
- `.prettierignore` -- make sure prettier formats `generated/` (do not ignore it) so the output is stable.

## Tasks & Acceptance

**Execution:**
- [x] `Cargo.toml`, `.cargo/config.toml`, `crates/arena-proto/**` -- the types, the ts-rs export and the serde tests.
- [x] `scripts/gen-types.mjs`, `packages/ui/src/generated/**` -- generate, build the barrel, format, and commit the output.
- [x] `packages/ui/src/messages/{en.json,index.js}`, `packages/ui/package.json`, `packages/ui/src/index.js` -- the catalogue, `t` and the exports.
- [x] `scripts/check-generated.mjs` (+ test of the porcelain parser and report), `scripts/check-protocol-types.mjs` (+ test), `scripts/check-strings.mjs` (+ test) -- cover every matrix row with fixtures.
- [x] `packages/ui/src/Placeholder.svelte`, `apps/*/src/routes/+page.svelte`, `apps/marketing/src/pages/index.astro` -- move to the catalogue.
- [x] `package.json`, `.github/workflows/ci.yml`, `README.md` -- scripts, CI steps, the generator note and pin rows.

**Acceptance Criteria:**
- Given the repo, when `pnpm gen:types && git status --porcelain` runs, then the tree is clean.
- Given the repo, when `pnpm check` and `pnpm build` run, then both pass with all apps still rendering "Arena 360" plus their surface name.
- Given the catalogue, when it is compared with EXPERIENCE.md "Voice and Tone", then every fixed player and staff/owner line has a key.

## Implementation Notes

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | `gen:types` deletes `generated/` before cargo succeeds (blind, verify-gap, edge) | medium | `generate()` calls `rmSync` before `cargo test`; a compile failure leaves the tracked folder deleted. | patch |
| 2 | Plain `cargo test` rewrites the generated files unformatted (blind; also seen in verification) | medium | `TS_RS_EXPORT_DIR` is global in `.cargo/config.toml`. After `cargo test --workspace` the files differ, which breaks `format:check` locally. | patch |
| 3 | Cargo stdout swallowed on failure | low | CI shows only the exit code. Fix: inherit stdio. | patch |
| 4 | ENOENT when ts-rs exports nothing | low | `readdirSync` on a missing dir; folded into the rework for #1. | patch |
| 5 | `.astro` not string-linted (all three layers) | low | Prose scanning in `.astro` is outside the frozen scope (Svelte/TS), but an unknown `t()` key in `index.astro` would ship as a raw key. Fix: key check only. | patch (keys only) |
| 6 | Single-word literals in scripts not flagged | false | The frozen spec defines prose as at least 2 words, to limit false positives on identifiers. | reject |
| 7 | No catalogue keys for ErrorCode values | low | No UI shows errors yet; that belongs to later stories. | reject |
| 8 | `ErrorCode` has no unknown-code fallback, so an older client fails to deserialize a newer code | medium | Real against the "kiosks from the previous two releases" rule. Today nothing outside tests deserializes `AppError`; the first real reader arrives with the protocol work in Story 1.9. The fix adds a public variant the spec doesn't settle. | defer (Story 1.9) |
| 9 | `check:generated` fails locally on correct but uncommitted output | low | By design, it compares against HEAD. Fix: one README line. | patch |
| 10 | `export *` and `import * as` bypass the protocol lint (blind, edge) | low | Verified: returns `[]`. Fix: flag star forms whose specifier points into `generated` but isn't allowed. | patch |
| 11 | Structural copies under another name are not detected | low | Heuristic beyond the spec's name-based rule; adds complexity. | reject |
| 12 | `interpolate` renders a null or undefined param as "undefined" | low | `String(undefined)`. Direct correction: treat null as missing. | patch |
| 13 | Unused keys, placeholder/param mismatch, dynamic keys, prod `t()` untested | low | Not reachable today; each fix adds a new check. | reject |
| 14 | `./messages` export has no `types` | low | The JSDoc types resolve through `index.js`; no consumer is affected. | reject |
| 15 | Regex after keyword + space read as a string | low | Verified (`return /"a b"/`). Fix: `trimEnd()`, a direct correction. | patch |
| 16 | `${}` containing a quoted brace mis-tokenizes | low | Rare pattern; the fix means rewriting the scanner. | reject |
| 17 | `{@html}` and directive expressions not linted | low | Rare; adds walker branches. | reject |
| 18 | Svelte parse error reports a character offset as the line | low | Verified (line 7 in a 4-line file). Direct correction. | patch |
| 19 | Both linters skip any directory named `generated` or `messages` | low | Hand-written `apps/x/src/generated/` would escape. Fix: skip the exact package paths. | patch |
| 20 | Relative `generated` specifier inside `packages/ui` is not resolved | low | Only `packages/ui` is affected and it has a single generated dir. | reject |
| 21 | Porcelain quoting and rename old path | low | Paths under `generated/` are ASCII type names. | reject |
| 22 | Freshness gate failure path never exercised end to end | low | Pre-verified gap: no test calls `gitStatus`. Fix: a temp git repo test. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked` -- expected: pass
- `pnpm gen:types && git status --porcelain -- packages/ui/src/generated` -- expected: empty
- `pnpm check && pnpm build` -- expected: exit 0
- Temporarily add `<p>Printer offline</p>` to `apps/hub/src/routes/+page.svelte` and run `pnpm check:strings` -- expected: fails naming the file:line. Revert afterwards.
