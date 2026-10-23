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
| `apps/`     | Svelte / Tauri / Astro apps (`apps/<name>`, see below)         |
| `packages/` | Shared JS packages: `packages/ui` is `@arena/ui`               |
| `scripts/`  | Repo checks: `check-layers.mjs`, `check-pins.mjs`, branch rule |

Rust crates: `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-tasks`,
`arena-replay`, `arena-cloud`, `arena-kiosk-core` (libraries) and `arena-hub`, `arena-kiosk-svc`,
`arena-pi` (binaries).

### Apps

Every app renders `Placeholder` from `@arena/ui` on its root page. `pnpm build` builds them all;
`pnpm --filter @arena/<name> build` builds one.

| App               | Package             | Stack                                          | Build output                     |
| ----------------- | ------------------- | ---------------------------------------------- | -------------------------------- |
| `apps/hub`        | `@arena/hub`        | Tauri + SvelteKit, adapter-static SPA          | `build/`, crate `arena-hub-ui`   |
| `apps/kiosk`      | `@arena/kiosk`      | Tauri + SvelteKit, adapter-static SPA          | `build/`, crate `arena-kiosk-ui` |
| `apps/kitchen`    | `@arena/kitchen`    | SvelteKit, adapter-static (prerendered)        | `build/`                         |
| `apps/owner`      | `@arena/owner`      | SvelteKit, adapter-cloudflare (Workers assets) | `.svelte-kit/cloudflare/`        |
| `apps/partner`    | `@arena/partner`    | SvelteKit, adapter-cloudflare (Workers assets) | `.svelte-kit/cloudflare/`        |
| `apps/admin`      | `@arena/admin`      | SvelteKit, adapter-cloudflare (Workers assets) | `.svelte-kit/cloudflare/`        |
| `apps/onboarding` | `@arena/onboarding` | SvelteKit, adapter-cloudflare (Workers assets) | `.svelte-kit/cloudflare/`        |
| `apps/marketing`  | `@arena/marketing`  | Astro + `@astrojs/svelte`, static              | `dist/`                          |

The Tauri shells live in `apps/<app>/src-tauri` (Cargo workspace members). Build one without
installers with `pnpm --filter @arena/hub tauri build --no-bundle`. On Linux, Tauri needs
`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
build-essential` for `cargo build --workspace`.

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
| @sveltejs/vite-plugin-svelte | 7.3.1         | `pnpm-workspace.yaml` |
| vite                         | 8.3.5         | `pnpm-workspace.yaml` |
| typescript                   | 6.0.3         | `pnpm-workspace.yaml` |
| @sveltejs/adapter-static     | 4.0.0         | `pnpm-workspace.yaml` |
| @sveltejs/adapter-cloudflare | 8.0.1         | `pnpm-workspace.yaml` |
| wrangler                     | 4.152.0       | `pnpm-workspace.yaml` |
| svelte-check                 | 4.7.7         | `pnpm-workspace.yaml` |
| astro                        | 7.4.2         | `pnpm-workspace.yaml` |
| @astrojs/svelte              | 9.1.1         | `pnpm-workspace.yaml` |
| @tauri-apps/cli              | 2.12.4        | `pnpm-workspace.yaml` |
| @tauri-apps/api              | 2.12.4        | `pnpm-workspace.yaml` |
| @tauri-apps/plugin-updater   | 2.14.0        | `pnpm-workspace.yaml` |
| tauri                        | =2.12.5       | `Cargo.toml`          |
| tauri-build                  | =2.7.4        | `Cargo.toml`          |
| tauri-plugin-updater         | =2.14.0       | `Cargo.toml`          |

`pnpm-workspace.yaml` also carries two pnpm install-policy settings:

- `minimumReleaseAgeExclude` lets `wrangler@4.152.0` and the `miniflare` beta it pins install even
  though they are younger than pnpm's minimum release age. Remove or update these entries whenever
  the `wrangler` pin changes.
- `allowBuilds` allows the install scripts of `esbuild` and `workerd` (they fetch their platform
  binaries); pnpm blocks all other dependency build scripts.

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
