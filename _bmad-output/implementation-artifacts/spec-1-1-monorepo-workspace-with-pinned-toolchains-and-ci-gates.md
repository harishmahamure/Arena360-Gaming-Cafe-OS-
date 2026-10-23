---
title: 'Story 1.1: Monorepo workspace with pinned toolchains and CI gates'
type: 'chore'
created: '2026-10-23'
status: 'draft'
route: 'dispatch'
review_loop_iteration: 0
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

## Open Questions

1. **Merge blocking on `main`.** The repo has no Git remote, so GitHub branch protection can't be set from this story. Options:
   - **(a)** Commit `scripts/protect-main.sh`, which uses `gh api` to make `ci-ok` the required check, and document it in the README for you to run once a remote exists. Nothing is enforced until you run it.
   - **(b)** Document the required setting in the README only.
   - **(c)** Skip it for now.

## Code Map

- Greenfield: only `_bmad/`, `_bmad-output/` and `docs/` exist. Do not modify these.
- `_bmad-output/planning-artifacts/architecture/architecture-arena-360-final-2026-10-23/ARCHITECTURE-SPINE.md` -- layer table (§Design Paradigm) and Stack versions. These are the source of the allow-list and the pins.
- Local tools: cargo 1.95.0, pnpm 12.10.1 and Node 22.22.3 (below the pin; the user will upgrade). `jq` is not installed, so the checks use Node only.

## Tasks & Acceptance

**Execution:**
- [ ] `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore` -- workspace with `members = ["crates/*"]`, resolver 3, edition 2024, shared `[workspace.package]` and `[workspace.dependencies]` pins.
- [ ] `crates/<11 crates>/{Cargo.toml,src/lib.rs|main.rs}` -- placeholder plus one test. Inter-crate deps follow the allow-list (for example core → store, proto) so the check has real edges.
- [ ] `package.json`, `pnpm-workspace.yaml`, `.nvmrc`, `.prettierrc`, `pnpm-lock.yaml`:
  - private root with `packageManager`, `engines` and the catalog
  - prettier as a devDependency
  - scripts `check:layers`, `check:pins`, `test:scripts`, `format:check`, `check`
- [ ] `scripts/check-layers.mjs` plus `scripts/check-layers.test.mjs` -- a pure `findViolations(metadata, rules)` and a CLI wrapper. Tests cover the matrix rows.
- [ ] `scripts/check-pins.mjs` plus `scripts/check-pins.test.mjs` -- a pure `comparePins(table, sources)`, plus readers for rust-toolchain.toml, .nvmrc, package.json, the pnpm catalog and Cargo.toml. Tests cover the matrix rows.
- [ ] `README.md` -- the bootstrap command (`corepack enable && pnpm install && cargo build --workspace && pnpm check`), the layout, and the pin table (columns: Item | Version | Source).
- [ ] `.github/workflows/ci.yml` -- the pipeline described above.

**Acceptance Criteria:**
- Given a fresh clone with the pinned toolchains, when the bootstrap command runs, then pnpm installs the root, `cargo build --workspace` succeeds and `cargo test --workspace` reports 11 passing tests.
- Given the repo, when `pnpm check` runs locally, then fmt, clippy, the layer check, the pin check and the script tests all pass.
- Given the workflow file, when it is inspected, then it triggers on push to any branch and on PR, runs on both OSes, and ends in a `ci-ok` aggregator job.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` -- expected: clean, 11 tests pass
- `pnpm install --frozen-lockfile && pnpm check` -- expected: exit 0
- Temporarily add `arena-store` to `arena-proto`'s deps and run `pnpm check:layers` -- expected: non-zero exit naming the edge. Revert afterwards.
