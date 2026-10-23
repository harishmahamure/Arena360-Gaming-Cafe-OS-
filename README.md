# Arena 360

Monorepo for the Arena 360 branch Hub, Kiosk, cloud and web surfaces.

## Bootstrap

Install the pinned toolchains (Node from `.nvmrc`; Rust is selected automatically by
`rust-toolchain.toml` via rustup), then run:

```sh
corepack enable && pnpm install && cargo build --workspace && pnpm check
```

`pnpm check` runs `cargo fmt --check`, `cargo clippy` (warnings denied), the prettier check, the
layer check, the pin check, the generated-types, protocol-type and string checks, the design-token
gates (freshness, contrast, unknown tokens, raw hex), and the check scripts' own tests. Run `cargo test --workspace` for the
Rust tests.

## Layout

| Path        | Contents                                                  |
| ----------- | --------------------------------------------------------- |
| `crates/`   | Rust workspace crates (`crates/<name>`)                   |
| `apps/`     | Svelte / Tauri / Astro apps (`apps/<name>`, see below)    |
| `packages/` | Shared JS packages: `packages/ui` is `@arena/ui`          |
| `scripts/`  | Repo checks (`check-*.mjs`), `gen-types.mjs`, branch rule |

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

## Running the Hub

```sh
cargo run -p arena-hub -- --data-dir /tmp/hub
curl 127.0.0.1:7409/healthz
```

| Flag                   | Meaning                                                    |
| ---------------------- | ---------------------------------------------------------- |
| `--data-dir <path>`    | Data directory (overrides `ARENA_HUB_DATA_DIR`)            |
| `--health-port <port>` | Loopback port for `GET /healthz` (default `7409`, not `0`) |
| `--help`, `-h`         | Print usage and exit 0                                     |
| `--version`            | Print the version and exit 0                               |

- **Data dir:** `--data-dir`, else `ARENA_HUB_DATA_DIR`, else `%PROGRAMDATA%\Arena360\Hub` on
  Windows and `$XDG_DATA_HOME/arena360/hub` (falling back to `~/.local/share/arena360/hub`)
  elsewhere. The database is `<data>/branch.db`; `<data>/branch.db.lock` is the process writer
  lock, so a second process on the same dir starts as a read-only follower.
- **Health:** `GET 127.0.0.1:<port>/healthz` returns `db_open`, `role`, `lease`, `epoch`,
  `schema_version`, `self_instance_id` and `now`. It binds loopback only.
- **Logs:** JSON lines in `<data>/logs/hub-YYYY-MM-DD.jsonl` (UTC), with `.N` part files when the
  ring fills during a day. Files older than 14 days are deleted and the total is kept at or under
  50 MB (oldest deleted first). Logs never contain PII. Set the level with `RUST_LOG` (default
  `info`, e.g. `RUST_LOG=arena_tasks=debug`).
- **Stopping:** Ctrl+C, or SIGTERM on Unix.

### Idempotency and housekeeping

Each intent carries a ULID `idempotency_key`; a missing or malformed one fails `BAD_FRAME`
before any transaction. The outcome of each key (success, or a deterministic rejection such as
`STALE`) is kept in `applied_intents`, and a retry gets that outcome back without writing
anything. Intents older than 7 days fail `EXPIRED`. While primary, the Hub prunes dedupe
records older than 30 days at start and every 24 h. That prune is a maintenance write outside
`apply()` and writes no change rows. The property suite (`crates/arena-core/tests/proptest_apply.rs`)
runs 64 cases by default; set `PROPTEST_CASES` for more.

### Seeding a dev branch

Every branch write goes through `apply()` and lands in the append-only `changes` log. To get a
branch to work on locally, build with the dev-only `dev-fixtures` feature (never enabled in
default or release builds, where `dev` is an unknown command) and run:

```sh
cargo run -p arena-hub --features dev-fixtures -- dev seed --data-dir /tmp/hub
```

`dev` must be the first argument (`arena-hub --data-dir /tmp/hub dev seed` is rejected). It
takes the lease once and submits `branch.bootstrap` as `actor{system, "dev-seed"}`: branch
"Dev Branch", code `DEV01`, INR, `Asia/Kolkata`, business day from 06:00, financial year from
April, plus the default settings. On exit it releases the lease (it expires immediately), so a
real Hub can start on the same data dir straight away.

| Exit code | Meaning                                                                        |
| --------- | ------------------------------------------------------------------------------ |
| 0         | The branch was bootstrapped                                                    |
| 1         | `ALREADY_BOOTSTRAPPED`, `NOT_PRIMARY` (another process holds the db), or other |
| 2         | `SCHEMA_TOO_NEW`: the database is newer than this binary                       |

| Exit code | Meaning                                                              |
| --------- | -------------------------------------------------------------------- |
| 0         | Clean shutdown, `--help` or `--version`                              |
| 1         | Runtime failure (logs, store, port bind, or the healthz server died) |
| 2         | `SCHEMA_TOO_NEW`: the database is newer than this binary             |
| 64        | Bad command-line arguments                                           |

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
| @fontsource-variable/sora    | 5.3.0         | `pnpm-workspace.yaml` |
| @playwright/test             | 1.64.1        | `pnpm-workspace.yaml` |
| @axe-core/playwright         | 4.14.0        | `pnpm-workspace.yaml` |
| yaml                         | 2.9.1         | `package.json`        |
| tauri                        | =2.12.5       | `Cargo.toml`          |
| tauri-build                  | =2.7.4        | `Cargo.toml`          |
| tauri-plugin-updater         | =2.14.0       | `Cargo.toml`          |
| serde                        | =1.0.229      | `Cargo.toml`          |
| serde_json                   | =1.0.154      | `Cargo.toml`          |
| ts-rs                        | =12.0.1       | `Cargo.toml`          |
| tokio                        | =1.54.1       | `Cargo.toml`          |
| axum                         | =0.8.9        | `Cargo.toml`          |
| rusqlite                     | =0.40.2       | `Cargo.toml`          |
| tracing                      | =0.1.44       | `Cargo.toml`          |
| tracing-subscriber           | =0.3.23       | `Cargo.toml`          |
| ulid                         | =3.0.0        | `Cargo.toml`          |
| rmp-serde                    | =1.3.1        | `Cargo.toml`          |
| chrono-tz                    | =0.10.4       | `Cargo.toml`          |
| sha2                         | =0.10.9       | `Cargo.toml`          |
| tempfile                     | =3.27.0       | `Cargo.toml`          |
| tower                        | =0.5.3        | `Cargo.toml`          |
| proptest                     | =1.12.0       | `Cargo.toml`          |

## Generated protocol types and the message catalogue

- **Type generator: ts-rs.** Protocol types live in `crates/arena-proto` and derive `TS`. Run
  `pnpm gen:types` after changing them: it runs `cargo test -p arena-proto --features export-ts`
  with `TS_RS_EXPORT_DIR` pointing at a temp folder, writes the `index.ts` barrel, formats the
  output and only then replaces `packages/ui/src/generated`. Plain `cargo test` exports nothing.
  Commit the output. Import protocol types only from `@arena/ui/generated`.
- **Messages.** All user-visible text lives in `packages/ui/src/messages/en.json` (flat dotted
  keys, `{name}` placeholders) and is read with `t(key, params?)` from `@arena/ui`. Exempt a line
  from the string lint with `// i18n-ignore`, or put `<!-- i18n-ignore -->` on the line before.
  `.astro` files are checked for unknown `t()` keys only.
- **Gates.** `pnpm check:generated` regenerates and fails when the result differs from `HEAD`
  (so commit regenerated output before running it locally),
  `pnpm check:protocol` fails on hand-written protocol types or imports from anywhere but
  `@arena/ui/generated`, and `pnpm check:strings` fails on hard-coded user-visible strings and
  unknown message keys.

## Design tokens

- **Source.** Colours, type ramps, spacing, radii and elevation come from the frontmatter of
  `_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md` (the
  `DESIGN_MD` constant in `scripts/gen-tokens.mjs`, shared by every token script). Run
  `pnpm gen:tokens` after editing it and commit `packages/ui/src/tokens/` (`tokens.css`,
  `tokens.ts`; generated, do not edit). Generation fails on an unresolved `{group.key}` reference
  or a component colour without an `-hc` twin.
- **Themes.** `--color-<name>` holds every raw colour. Component values with colour refs inside a
  longer value (`2px solid {colors.x}`) are emitted with `var(--color-x)` and an `-hc` twin. Use the semantic aliases (`--bg`, `--text`,
  ...) and component tokens (`--<component>-<prop>`): dark is the default (`:root`,
  `[data-theme="dark"]`), `[data-theme="hc"]` maps them to the `-hc` values and
  `[data-theme="light"]` to the `-light` values (falling back to dark where there is no light twin).
- **Styling entry.** Every SvelteKit app imports `@arena/ui/base.css` once from
  `src/routes/+layout.svelte` (marketing: `src/pages/index.astro`). It bundles
  the Sora variable font (`@fontsource-variable/sora`) and imports `tokens.css`; nothing loads
  remotely. TS constants and the `TokenName` union are in `@arena/ui/tokens`.
- **Gates.** `pnpm check:tokens-fresh` regenerates and fails when `packages/ui/src/tokens` differs
  from `HEAD`. `pnpm check:contrast` checks WCAG 2.x ratios for pairs derived from DESIGN.md in the
  dark, hc and light themes (4.5 for text, 3.0 for large text and non-text); fill-only and
  decorative tokens are listed with their reason in `EXCLUDED`. `pnpm check:tokens-used` fails on
  `var(--name)` that neither `tokens.css` nor the same file declares. `pnpm check:hex` fails on raw
  hex colours outside the generated tokens (exempt a line with `hex-ignore`). After a build,
  `pnpm check:no-remote [app ...]` fails on remote URLs, fonts or CDN hosts in the Hub, Kiosk and
  Kitchen output, and on a build without a bundled `sora-*.woff2` or a CSS file declaring
  `--color-bg` (CI runs it in each of those build jobs; it is not part of `pnpm check`).

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
