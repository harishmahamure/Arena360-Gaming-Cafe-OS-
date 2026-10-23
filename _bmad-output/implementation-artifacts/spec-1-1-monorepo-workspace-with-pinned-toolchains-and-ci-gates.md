---
title: 'Story 1.1: Monorepo workspace with pinned toolchains and CI gates'
type: 'chore'
created: '2026-10-23'
status: 'done'
baseline_commit: '72f3f8a86a903f4fa5cb42216ffbf3c2b089136a'
route: 'dispatch'
review_loop_iteration: 1
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The repo has only planning docs. Every later story needs the agreed monorepo shape, pinned toolchains and automated drift gates, so that layering or version drift is caught in CI instead of in review.

**Approach:** Scaffold a Cargo workspace with the 11 spine crates and a pnpm JS workspace root. Pin every toolchain and framework version in manifests, with a README pin table. Add two dependency-free Node check scripts (a layer direction check and a pin-table check) and a GitHub Actions pipeline running fmt, lint, tests and both checks on Linux and Windows.

## Boundaries & Constraints

**Always:**
- Layout: `crates/<name>` for Rust, `apps/` and `packages/` reserved for JS (created empty with `.gitkeep`), `scripts/` for the checks.
- One package manager is used: pnpm, pinned via `packageManager: "pnpm@12.15.0"`.
- Node pins: `.nvmrc` = `22.23.4` and `engines.node` = `>=22.23.4 <23`.
- Rust pin: `rust-toolchain.toml` sets channel `1.95.0` with components rustfmt and clippy.
- JS framework pins live in the pnpm `catalog:` in `pnpm-workspace.yaml` (exact versions), so Story 1.2 apps reference `catalog:`:
  - svelte `5.57.2`, @sveltejs/kit `3.1.0`, vite `8.3.5`, typescript `6.0.3`
  - adapter-static `4.0.0`, adapter-cloudflare `8.0.1`
  - astro `7.4.2`, @astrojs/svelte `9.1.1`
  - @tauri-apps/cli `2.12.4`, @tauri-apps/api `2.12.4`, @tauri-apps/plugin-updater `2.14.0`
- Rust pins live in `[workspace.dependencies]` with exact `=` versions: tauri `=2.12.5`, tauri-plugin-updater `=2.14.0`. Crates do not use them yet.
- Each crate has a placeholder module and exactly one passing `#[test]`. `arena-hub`, `arena-kiosk-svc` and `arena-pi` are binaries; the rest are libs.
- Layer check: reads `cargo metadata`, considers normal and build dependency edges between workspace crates (dev-deps are ignored), and enforces an allow-list taken from the spine layer table:
  - proto → none
  - store → proto
  - core → store, proto
  - api → core, proto
  - tasks → core, store, proto
  - replay → store, proto
  - cloud → proto, replay, store
  - Other crates are unrestricted.
  - On violation it exits non-zero and prints `forbidden edge: <from> -> <to>` for every offending edge.
- Pin check: parses the README pin table and compares every row against its source file. Any mismatch, missing row or extra row exits non-zero and names the item, the table value and the manifest value.
- CI runs on every push and PR with a matrix of ubuntu-latest and windows-latest. It runs:
  - `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` and `cargo test --workspace`
  - `pnpm install --frozen-lockfile`, a prettier check, both check scripts and their own `node --test` suites
  - a final `ci-ok` job that needs all others and is the single required status.

**Never:** No Svelte/Tauri app scaffolding (that is Story 1.2), no ts-rs or generated types (Story 1.3), no domain code. No third-party actions beyond actions/checkout, actions/setup-node, pnpm/action-setup, dtolnay/rust-toolchain and Swatinem/rust-cache, pinned to major tags. No network calls in the check scripts.

**Decision (merge blocking):** Commit `scripts/protect-main.sh`, which uses `gh api` to protect the repository's **default branch** (looked up through `gh repo view`; `master` today, with an optional argument to override it) with `ci-ok` as the required status check. Document it in the README as a one-time step. (The human answered this in review loop 1: the trunk is the repo default branch, not a hardcoded `main`.)

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Clean tree | repo as scaffolded | both checks exit 0 | N/A |
| Proto depends on store | `arena-proto` gains a dep on `arena-store` | layer check fails | prints `forbidden edge: arena-proto -> arena-store` |
| Core depends on api | `arena-core` → `arena-api` | layer check fails | names that edge; lists all edges if several |
| Pin bumped silently | `vite` catalog changed to 8.4.0, README unchanged | pin check fails | prints item, README value, manifest value |
| Row missing | pin row deleted from README | pin check fails | `missing pin row: <item>` |
| Unparseable table | README pin table header missing | pin check fails | `pin table not found in README.md` |

</frozen-after-approval>

## Code Map

- Greenfield: only `_bmad/`, `_bmad-output/` and `docs/` exist. Do not modify these.
- `_bmad-output/planning-artifacts/architecture/architecture-arena-360-final-2026-10-23/ARCHITECTURE-SPINE.md` -- layer table (§Design Paradigm) and Stack versions. These are the source of the allow-list and the pins.
- Local tools: cargo 1.95.0, pnpm 12.10.1 and Node 22.22.3 (below the pin; the user will upgrade). `jq` is not installed, so the checks use Node only.

## Tasks & Acceptance

**Execution:**
- [x] `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore` -- workspace with `members = ["crates/*"]`, resolver 3, edition 2024, shared `[workspace.package]` and `[workspace.dependencies]` pins.
- [x] `crates/<11 crates>/{Cargo.toml,src/lib.rs|main.rs}` -- placeholder plus one test. Inter-crate deps follow the allow-list (for example core → store, proto) so the check has real edges.
- [x] `package.json`, `pnpm-workspace.yaml`, `.nvmrc`, `.prettierrc`, `pnpm-lock.yaml`:
  - private root with `packageManager`, `engines` and the catalog
  - prettier as a devDependency
  - scripts `check:layers`, `check:pins`, `test:scripts`, `format:check`, `check`
- [x] `scripts/check-layers.mjs` plus `scripts/check-layers.test.mjs` -- a pure `findViolations(metadata, rules)` and a CLI wrapper. Tests cover the matrix rows.
- [x] `scripts/check-pins.mjs` plus `scripts/check-pins.test.mjs` -- a pure `comparePins(table, sources)`, plus readers for rust-toolchain.toml, .nvmrc, package.json, the pnpm catalog and Cargo.toml. Tests cover the matrix rows.
- [x] `scripts/protect-main.sh` -- gh api branch protection for main requiring `ci-ok`.
- [x] `README.md` -- the bootstrap command (`corepack enable && pnpm install && cargo build --workspace && pnpm check`), the layout, and the pin table (columns: Item | Version | Source).
- [x] `.github/workflows/ci.yml` -- the pipeline described above.

**Acceptance Criteria:**
- Given a fresh clone with the pinned toolchains, when the bootstrap command runs, then pnpm installs the root, `cargo build --workspace` succeeds and `cargo test --workspace` reports 11 passing tests.
- Given the repo, when `pnpm check` runs locally, then fmt, clippy, the layer check, the pin check and the script tests all pass.
- Given the workflow file, when it is inspected, then it triggers on push to any branch and on PR, runs on both OSes, and ends in a `ci-ok` aggregator job.

## Implementation Notes

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | protect-main.sh hardcodes `main` (blind, verify-gap, edge) | medium | `git ls-remote --symref origin HEAD` → `refs/heads/master`; local `main` is stale at ebb99d8. The script would protect an unused branch. | intent_gap |
| 2 | `contexts` form of required checks accepts a status from any token with write access | low | GitHub's docs prefer `checks` with `app_id`; spoofing needs a repo-write token. Fix is a direct correction. | patch |
| 3 | check-pins doesn't require exact versions (`^5.57.2` would pass if the README matches) | medium | `comparePins` compares strings only; nothing asserts an exact semver form. | patch |
| 4 | prettier, `rust-version` and the dtolnay ref are outside the pin table | low | Real, but prettier isn't a listed pin; rustup honours rust-toolchain.toml and a `rust-version` mismatch fails loudly. The fix adds readers. | reject |
| 5 | readCargoWorkspaceDeps misses multi-line, subtable and crate-level forms | low | No such form exists in the repo today; the fix adds parsing branches. | reject |
| 6 | check-layers leaves unknown crates unrestricted | false | The frozen spec says "Other crates are unrestricted". | reject |
| 7 | Layer table duplicated between README and LAYER_RULES | low | Real duplication; the fix would add README parsing. | reject |
| 8 | CI cargo commands missing `--locked` | low | Without it, cargo can re-resolve Cargo.lock in CI. Fix: one flag. | patch |
| 9 | Actions pinned to major tags, not SHAs | false | The frozen spec says "pinned to major tags". | reject |
| 10 | push + pull_request both run on PRs | false | The frozen spec requires both triggers. | reject |
| 11 | Rust tauri 2.12.5 vs JS 2.12.4 | false | These are the spec-mandated pins; the crate and the npm packages version independently. | reject |
| 12 | parsePinTable accepts rows with fewer than 3 cells | low | Gives a confusing `README undefined` message. Fix: a direct cell-count check. | patch |
| 13 | .gitignore is missing app build outputs and `.env*` | low | Story 1.2 apps would produce these; the fix adds a few lines. | patch |
| 14 | Duplicate pin row path is untested | low | Pre-verified gap: no test feeds a repeated row. | patch |
| 15 | `argv[1]` equality gate skips main() under a symlinked path, so the check exits 0 silently | low | `pathToFileURL(argv[1])` differs from the realpath'd `import.meta.url` under a symlink, giving a silent pass. Fix: compare realpaths. | patch |
| 16 | Prototype-key crate or item names | low | Unreachable: no crate named `constructor`. The fix adds guards. | reject |
| 17 | Missing manifest throws an uncaught exception | false | A loud failure on a broken checkout is correct behaviour. | reject |
| 18 | pnpm named `catalogs:` not read | low | Not used; the fix adds parsing. | reject |

## Verification

**Commands:**
- `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` -- expected: clean, 11 tests pass
- `pnpm install --frozen-lockfile && pnpm check` -- expected: exit 0
- Temporarily add `arena-store` to `arena-proto`'s deps and run `pnpm check:layers` -- expected: non-zero exit naming the edge. Revert afterwards.
