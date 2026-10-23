# Arena 360

Monorepo for the Arena 360 branch Hub, Kiosk, cloud and web surfaces.

## Bootstrap

Install the pinned toolchains (Node from `.nvmrc`; Rust is selected automatically by
`rust-toolchain.toml` via rustup), then run:

```sh
corepack enable && pnpm install && cargo build --workspace && pnpm check
```

`pnpm check` runs `cargo fmt --check`, `cargo clippy` (warnings denied), the prettier check, the
layer check, the pin check and the check scripts' own tests. Run `cargo test --workspace` for the
Rust tests.

## Layout

| Path        | Contents                                                       |
| ----------- | -------------------------------------------------------------- |
| `crates/`   | Rust workspace crates (`crates/<name>`)                        |
| `apps/`     | Svelte / Tauri / Astro apps (reserved, Story 1.2)              |
| `packages/` | Shared JS packages, e.g. the shared UI package (reserved)      |
| `scripts/`  | Repo checks: `check-layers.mjs`, `check-pins.mjs`, branch rule |

Rust crates: `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-tasks`,
`arena-replay`, `arena-cloud`, `arena-kiosk-core` (libraries) and `arena-hub`, `arena-kiosk-svc`,
`arena-pi` (binaries).

### Layer rules (enforced by `pnpm check:layers`)

Normal and build dependencies between workspace crates must follow this allow-list
(dev-dependencies are ignored; crates not listed are unrestricted):

| Crate          | May depend on                                |
| -------------- | -------------------------------------------- |
| `arena-proto`  | —                                            |
| `arena-store`  | `arena-proto`                                |
| `arena-core`   | `arena-store`, `arena-proto`                 |
| `arena-api`    | `arena-core`, `arena-proto`                  |
| `arena-tasks`  | `arena-core`, `arena-store`, `arena-proto`   |
| `arena-replay` | `arena-store`, `arena-proto`                 |
| `arena-cloud`  | `arena-proto`, `arena-replay`, `arena-store` |

## Pinned versions

This table is checked against the manifests by `pnpm check:pins`. Change the manifest and this
table together. JS framework versions live in the pnpm `catalog:`; apps reference them as
`"catalog:"`.

| Item                         | Version       | Source                |
| ---------------------------- | ------------- | --------------------- |
| rust                         | 1.95.0        | `rust-toolchain.toml` |
| node                         | 22.23.4       | `.nvmrc`              |
| node engines                 | >=22.23.4 <23 | `package.json`        |
| pnpm                         | 12.15.0       | `package.json`        |
| svelte                       | 5.57.2        | `pnpm-workspace.yaml` |
| @sveltejs/kit                | 3.1.0         | `pnpm-workspace.yaml` |
| vite                         | 8.3.5         | `pnpm-workspace.yaml` |
| typescript                   | 6.0.3         | `pnpm-workspace.yaml` |
| @sveltejs/adapter-static     | 4.0.0         | `pnpm-workspace.yaml` |
| @sveltejs/adapter-cloudflare | 8.0.1         | `pnpm-workspace.yaml` |
| astro                        | 7.4.2         | `pnpm-workspace.yaml` |
| @astrojs/svelte              | 9.1.1         | `pnpm-workspace.yaml` |
| @tauri-apps/cli              | 2.12.4        | `pnpm-workspace.yaml` |
| @tauri-apps/api              | 2.12.4        | `pnpm-workspace.yaml` |
| @tauri-apps/plugin-updater   | 2.14.0        | `pnpm-workspace.yaml` |
| tauri                        | =2.12.5       | `Cargo.toml`          |
| tauri-plugin-updater         | =2.14.0       | `Cargo.toml`          |

## CI and merge blocking

`.github/workflows/ci.yml` runs on every push and pull request on `ubuntu-latest` and
`windows-latest`. The `ci-ok` job depends on all other jobs and is the single required status.

**One-time step after the GitHub remote is added:** protect the default branch so `ci-ok` (from
GitHub Actions) must pass before a merge:

```sh
scripts/protect-main.sh                   # current repo, its default branch
scripts/protect-main.sh owner/repo        # name the repo explicitly
scripts/protect-main.sh owner/repo branch # and the branch
```

This needs the GitHub CLI (`gh`) authenticated with admin rights on the repository.
