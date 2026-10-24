---
title: 'Story 2.1: Arena 360 cloud runs in India behind Cloudflare'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: '3ab892038fd2930d07215b65a581c68d4ac98e6f'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `arena-cloud` is still a placeholder library. There is nothing to deploy, no `gz_control` database connection, and no path that keeps owner data in India behind Cloudflare.

**Approach:**
- An `arena-cloud` binary with `--role control`: it connects to `gz_control` on Turso (Mumbai) via libsql, runs the numbered, forward-only embedded migrations (refusing `SCHEMA_TOO_NEW`), and serves `GET /healthz`.
- A container image and k3s manifests for `staging` and `production`, with an origin that admits only Cloudflare IP ranges.
- A GitHub Actions deploy workflow that decrypts SOPS + age secrets into Kubernetes secrets. It is written and statically checked here, never run.
- A CI check that fails on committed plaintext secrets or env files.

## Boundaries & Constraints

**Always:**
- **Storage layer:**
  - libsql lives in `arena-store` only, in a new `control` module behind a cargo feature `control`, so `arena-hub`'s own dependency tree (`cargo tree -p arena-hub`) never includes libsql.
  - libsql is pinned `=0.10.0` with `default-features = false` and remote-only features (`remote` plus TLS). That means no C SQLite core: the rusqlite + libsql shared-linkage spike belongs to Story 2.2.
  - If the remote TLS feature pulls a crypto provider other than ring, record it in Implementation Notes. Never add a second SQLite C library.
- **Control migrations:**
  - `crates/arena-store/control-migrations/NNNN_*.sql`, embedded with `include_str!`, versions contiguous from 1.
  - The runner creates `schema_migrations(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL)` if it's missing, then reads `MAX(version)`. A value above the binary's latest is `SCHEMA_TOO_NEW{db, binary}`.
  - Otherwise it applies each pending migration and its `schema_migrations` row in one transaction, in order.
  - A concurrent runner (rolling update) that loses the primary-key race re-reads and continues; it never fails or double-applies.
  - `0001_baseline.sql` creates **no business tables**: it only marks the baseline.
  - The pure decision `plan(db_version, latest) -> Result<pending range, SchemaTooNew>` is unit-tested.
- **Binary** (`crates/arena-cloud/src/main.rs`, bin `arena-cloud`):
  - Args are hand-parsed in the style of `arena-hub` config: `--role control` (required in 2.1; any other role exits 64 with "role not available yet") and `--listen <addr>` (default `0.0.0.0:8080`), plus `--help` and `--version`.
  - Env: `ARENA_CONTROL_DB_URL` (`libsql://` or `https://` only), `ARENA_CONTROL_DB_TOKEN` (may be empty only for `http://127.0.0.1`/`localhost` URLs, used by tests).
  - tracing JSON logs to stdout, with no token in any log line.
  - `SCHEMA_TOO_NEW` → a log line naming both versions and exit 2 (as the Hub does), with the database untouched. Unreachable at start → retry with backoff for at most 60 s, then exit 1.
- **`GET /healthz`** → 200 `{role:"control", version, gz_control:{reachable, checked_at}, migration_version}`:
  - `version` is `CARGO_PKG_VERSION`.
  - Reachability is a background `SELECT 1` every 15 s with a 3 s timeout. The handler reads the cached value and never hits Turso per request.
  - The handler always answers 200. Liveness must not restart the pod on a Turso blip; readiness uses the same path.
- **Hub probe default:** `--cloud-host` defaults to `api.arena360.cloud:443`. Update the README row and the doc comments.
- **Image:** `deploy/docker/arena-cloud.Dockerfile`:
  - A multi-stage build: the Rust toolchain from `rust-toolchain.toml`, `cargo build --locked --release -p arena-cloud`.
  - The runtime stage is distroless/cc pinned by tag, running as non-root.
- **Manifests:** `deploy/k8s/` kustomize, with `base/` and `overlays/{staging,production}`:
  - **Namespaces:** `staging` / `production`.
  - **Deployment** `arena-cloud-control`:
    - args `--role control`;
    - secret env from `arena-cloud-secrets` (`ARENA_CONTROL_DB_URL`, `ARENA_CONTROL_DB_TOKEN`) via `secretKeyRef`;
    - liveness and readiness on `/healthz`;
    - resources set;
    - `runAsNonRoot`, `readOnlyRootFilesystem`.
  - **Service** and **Ingress** (Traefik, bundled with k3s): hosts `api.staging.arena360.cloud` / `api.arena360.cloud`.
  - **TLS to the origin:** a Cloudflare Origin CA certificate in secret `origin-tls` (SOPS), for Full (strict).
  - **Origin lock:**
    - A Traefik `Middleware` `ipAllowList` holds exactly the Cloudflare ranges committed in `deploy/cloudflare/ips-v4.txt` and `ips-v6.txt`, attached to every router of the Ingress.
    - `deploy/k8s/cluster/traefik-config.yaml` is a `HelmChartConfig` setting `externalTrafficPolicy: Local`, so the source IP is real.
  - No `kind: Secret` is ever committed under `deploy/k8s`.
- **Secrets:**
  - `.sops.yaml` creation rules cover `deploy/secrets/*.enc.yaml` (age recipients).
  - The encrypted files hold the `arena-cloud-secrets` and `origin-tls` Secret manifests per environment.
  - Generating the age key and encrypting the real Turso token are documented human steps (`deploy/README.md`). The deploy job fails with a clear message when `deploy/secrets/<env>.enc.yaml` is missing.
- **Deploy workflow** (`.github/workflows/deploy-cloud.yml`):
  - `push: main` → staging, `push: tags v*` → production, using GitHub environments of the same names.
  - Jobs:
    - build and push the image to GHCR, tagged with the commit SHA;
    - install sops `3.13.3` and age `1.3.2` by pinned URL plus sha256;
    - `SOPS_AGE_KEY` comes from the environment secret;
    - decrypt and apply the secrets, set the image, apply the overlay and wait on `rollout status`. All over SSH to the VPS, with a pinned `known_hosts` from a secret; no public kube-apiserver.
  - **Post-deploy smoke:**
    - `/healthz` through the edge reports `migration_version` and `reachable: true`;
    - `http://` → a 301/308 to `https://`;
    - a direct origin request from the runner (outside Cloudflare) gets a 403 or a refused connection.
- **Edge settings** (Always Use HTTPS, SSL Full (strict), proxied DNS records) and the Turso database creation in `aws-ap-south-1` are documented in `deploy/README.md` as operator steps, alongside a ufw rule limiting 80/443 to the Cloudflare ranges.
- **`check:secrets`** (`scripts/check-secrets.mjs` plus a test), added to `check` and the CI `checks` job. It fails when:
  - a tracked file under `deploy/secrets/` lacks SOPS metadata (`sops.mac`);
  - any `.env`/`*.env` file is tracked (`.env.example` excepted);
  - `kind: Secret` appears under `deploy/k8s`;
  - tracked text matches `AGE-SECRET-KEY-1`, a PEM private key, or a JWT-shaped token over 100 characters outside test fixtures.
- **`check:deploy`** (`scripts/check-deploy.mjs` plus a test): it renders both overlays with `kubectl kustomize` and asserts the invariants above (namespace, host, the args, secretKeyRef only, probes, non-root, the allowlist equal to the committed ranges and attached to the Ingress). It also parses `deploy-cloud.yml`: the triggers per environment, the sops/age versions, and that the smoke steps are present.

**Never:**
- Running the deploy, pushing images, or calling Cloudflare, Turso or GitHub from this work.
- Business tables in `gz_control`.
- libsql in the Hub, a C SQLite core in libsql, or plaintext secrets anywhere.
- The `AppError` envelope or `--role all --dev` (both Story 2.2).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh DB | empty `gz_control` | `schema_migrations` created, baseline applied, healthz `migration_version: 1` | N/A |
| Re-start | DB at latest | nothing applied; starts | N/A |
| Too new | DB `MAX(version)` = latest+1 | exit 2, log names both versions, no write | N/A |
| Race | two runners on a fresh DB | both start; one row per version | loser re-reads |
| DB down at start | unreachable URL | retries ≤60 s then exit 1 | logged without token |
| DB blip later | `SELECT 1` fails | healthz 200, `reachable:false` | recovers next tick |
| Bad role | `--role builder` | exit 64 "role not available yet" | N/A |
| Bad URL | `file:` or missing env | exit 64 naming the variable | N/A |
| Plaintext secret | unencrypted `deploy/secrets/x.yaml` | `check:secrets` fails naming the file | N/A |
| Allowlist drift | middleware CIDRs ≠ committed list | `check:deploy` fails | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-store/src/migrations.rs` -- Hub migration list pattern (`Migration{version, sql}`, contiguity test); mirror it for `control` but keep it separate; do not touch the Hub list.
- `crates/arena-store/Cargo.toml` -- add optional `libsql` and feature `control = ["dep:libsql"]`; tokio may be needed as optional too.
- `crates/arena-store/src/lib.rs` -- `#[cfg(feature = "control")] pub mod control;`; existing `SchemaTooNew` error style lives here, reuse its naming.
- `crates/arena-cloud/{Cargo.toml,src/lib.rs}` -- placeholder; becomes lib (config parse, health router, runner wiring) + `src/main.rs`. Depends on `arena-store` with `features = ["control"]`, `axum`, `tokio` (`net`), `tracing`, `tracing-subscriber`, `serde`, `serde_json`. Layer rule already allows proto/replay/store (`scripts/check-layers.mjs:17`).
- `crates/arena-hub/src/config.rs` + `crates/arena-hub/src/main.rs` -- arg-parsing style, `USAGE`, exit-2-on-`SCHEMA_TOO_NEW` to mirror.
- `crates/arena-api/src/health.rs` -- `HealthBody` + whole-JSON test pattern to mirror for the cloud health body.
- `crates/arena-tasks/src/connectivity.rs` -- `DEFAULT_CLOUD_HOST` (`api.arena360.in:443`) → `api.arena360.cloud:443`; `crates/arena-hub/src/config.rs:29` doc comment; `README.md:84`.
- `Cargo.toml` `[workspace.dependencies]` -- add `libsql = { version = "=0.10.0", default-features = false, ... }`; `scripts/check-pins.mjs` requires every third-party entry in the README pin table (`README.md` ~l.1000) — add libsql, sops, age, k3s note, distroless tag, libsql-server image.
- `.github/workflows/ci.yml` -- `checks` job (l.41-66): add `check:secrets`, `check:deploy`; `rust` job: control integration tests need a libsql server → service container `ghcr.io/tursodatabase/libsql-server:<pinned>` (ubuntu only) with `ARENA_TEST_SQLD_URL`; `ci-ok` needs unchanged unless a new job is added.
- `package.json` -- `check:secrets`, `check:deploy`, add both to `check`.
- Local test DB: `sqld` 0.24.31 is installed (`/opt/homebrew/bin/sqld`). Tests use `ARENA_TEST_SQLD_URL` if set, else spawn `sqld --http-listen-addr 127.0.0.1:<free port> --db-path <tempdir>`, else skip with a printed reason — unless `ARENA_REQUIRE_SQLD=1` (set in CI), which makes absence fail.
- `kubectl` is installed locally (for `kubectl kustomize`); ubuntu runners ship it. `sops`/`age` are not installed locally — nothing here needs them.

## Tasks & Acceptance

**Execution:**
- [x] `Cargo.toml`, `crates/arena-store/{Cargo.toml,src/lib.rs,src/control.rs,control-migrations/0001_baseline.sql}` -- libsql pin, `control` feature, connect, `plan()`, runner, `SchemaTooNew` -- storage layer owns all SQL.
- [x] `crates/arena-store/tests/control.rs` -- sqld-backed tests: fresh, re-start, too-new untouched, concurrent race; `plan()` unit tests in-module -- matrix rows 1-4.
- [x] `crates/arena-cloud/{Cargo.toml,src/lib.rs,src/config.rs,src/health.rs,src/main.rs}` -- args/env parse, startup retry, reachability task, `/healthz` -- the control role.
- [x] `crates/arena-cloud/tests/*.rs` -- config parse (bad role, bad URL, missing env), healthz whole-JSON via `tower::oneshot` with fake reachability, binary exit 2 on too-new and exit 1 on unreachable (short retry window via a test-only env such as `ARENA_CONTROL_DB_RETRY_SECS`) -- matrix rows 5-8.
- [x] `crates/arena-tasks/src/connectivity.rs`, `crates/arena-hub/src/config.rs`, `README.md` -- Hub probe default `api.arena360.cloud:443`, update affected tests.
- [x] `deploy/docker/arena-cloud.Dockerfile`, `.dockerignore` -- image.
- [x] `deploy/k8s/{base,overlays/staging,overlays/production,cluster}/**`, `deploy/cloudflare/ips-v4.txt`, `ips-v6.txt` -- manifests + committed ranges (current published Cloudflare list).
- [x] `.sops.yaml`, `deploy/secrets/README.md`, `deploy/README.md` -- creation rules; operator runbook (k3s install + pinned version record, Turso `aws-ap-south-1`, age key, Cloudflare settings, ufw, GitHub environment secrets).
- [x] `.github/workflows/deploy-cloud.yml` -- build/push, decrypt, apply over SSH, smoke.
- [x] `scripts/check-secrets.mjs`, `scripts/check-secrets.test.mjs`, `scripts/check-deploy.mjs`, `scripts/check-deploy.test.mjs`, `package.json`, `.github/workflows/ci.yml` -- the two checks with fixture tests, wired into `check` and CI; libsql-server service for rust tests.
- [x] `README.md` -- crate/binary row, pin table, `arena-cloud` usage section.

**Acceptance Criteria:**
- Given a fresh sqld, when `arena-cloud --role control` starts, then `GET /healthz` returns 200 with `role:"control"`, the package version, `gz_control.reachable:true` and `migration_version:1`, and `schema_migrations` holds exactly one row.
- Given `cargo tree -p arena-hub`, when inspected, then `libsql` does not appear; given `cargo tree -p arena-cloud`, then no `libsql-ffi`/`libsql-sys` C core appears.
- Given the rendered overlays, when `check:deploy` runs, then staging and production differ only in namespace, host, replicas/resources and image tag, and every Ingress router carries the Cloudflare allowlist.
- Given the repo, when `check:secrets` runs, then it passes; its tests prove each failure rule fires on a fixture.

## Design Notes

Origin lock is two layers because neither alone suffices: Traefik's `ipAllowList` (needs `externalTrafficPolicy: Local` so the source IP isn't the node's) answers 403 inside the cluster, and the documented ufw rule drops non-Cloudflare traffic before it reaches the node. Edge HTTP→HTTPS is a Cloudflare zone setting, so the repo proves it only through the post-deploy smoke.

Healthz caches reachability so kubelet probes (every 10 s, per pod) never become Turso load, and a Turso blip never restarts pods.

## Verification

**Commands:**
- `cargo test --workspace --locked` and `cargo test --locked -p arena-store --features control` with `ARENA_REQUIRE_SQLD=1` -- pass, control tests run against local sqld.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo clippy --locked -p arena-store --features control --all-targets -- -D warnings` -- clean; `cargo fmt --check` -- clean.
- `cargo tree -p arena-hub -e normal | grep -c libsql` -- 0.
- `pnpm run check:secrets`, `pnpm run check:deploy`, every other `check:*`, `pnpm run test:scripts` -- pass.
- `docker build -f deploy/docker/arena-cloud.Dockerfile .` -- builds (record if skipped for time).

**Manual checks:**
- `deploy/README.md` lists every operator step that this story cannot perform (age key, encrypted secrets, Turso DB, Cloudflare zone settings, k3s install, ufw, GitHub environments).

## Implementation Notes

- **Crypto provider:** libsql `remote` + `tls` pulls hyper-rustls 0.25 / rustls 0.22 on **ring** (no aws-lc, no OpenSSL). `cargo tree -p arena-cloud` has no `libsql-ffi`/`libsql-sys`; only rusqlite's bundled `libsqlite3-sys` is a C SQLite.
- **Connector:** libsql's default connector loads the platform certificate store and *panics* when it cannot (seen on macOS). `ControlDb::connect` passes its own hyper-rustls connector with Mozilla roots (webpki-roots). This adds pinned `hyper-rustls =0.25.0` and `hyper =0.14.32` (same majors libsql uses, so no duplicate versions).
- **Race handling:** each migration runs as one libsql transactional batch with the `schema_migrations` INSERT first; on failure the runner re-reads `MAX(version)` and continues if another runner applied it (bounded retries for a lock held by an uncommitted peer).
- **`cargo tree -p arena-hub -e normal | grep -c libsql`** literally returns 1 because of rusqlite's `libsqlite3-sys`; CI greps `' libsql(-[a-z0-9]+)? v'` instead, which is 0.
- **sops/age sha256:** not fetched (no GitHub calls from this work). `deploy-cloud.yml` carries `PIN-ME` placeholders; the install step refuses non-sha256 values and `check:deploy` warns. Operator step 8 in `deploy/README.md`.
- **Origin smoke:** "refused" also accepts curl exits 28 (dropped), 35 (TLS), 52 (empty reply), 56 (reset).
- **Review follow-ups:** trigger is `push: master` (the repo's default branch); deploy calls `ci.yml` (`workflow_call`) first; tags promote the staged `:<sha>` image (never rebuild) and both environments deploy by digest; namespaces, deployer RBAC (`deploy/k8s/cluster/deployer-rbac.yaml`) and the Traefik config are one-time operator steps with one SSH user and SA kubeconfig per environment; the origin lock's outer layer is the Hostinger provider firewall (host ufw is bypassed by ServiceLB DNAT); proxied A records only; `ghcr-pull` mandatory; `preStop` sleep 5 s; deterministic migration SQL errors fail at once as `ControlError::Migration`.
- **k3s version** recorded as `v1.33.4+k3s1` in `deploy/README.md` and the pin table; confirm at install time.
- **Docker:** `docker build -f deploy/docker/arena-cloud.Dockerfile .` built locally (12 MB image, user 65532, runs read-only).

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap). Routes: P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | VG: nothing proves `serve_control` wires the ping loop into `/healthz`; the seed `reachable:true, checked_at:now` satisfies the only binary test | medium | P | `binary.rs` asserts only the seeded values; deleting the spawn passes every test. Fix together with #2 |
| 2 | BH: seed contradicts `Reachability` doc (`None` before the first check); smoke can pass on the seed | medium | P | `lib.rs` seeds `checked_at: Some(now_ms())`; the doc says `None`. Same root as #1 |
| 3 | BH/VG: deploy triggers on `main`, repo default is `master`; `check-deploy` hard-codes `[main]` | medium | P | `git branch -a` shows only master; staging never deploys |
| 4 | BH: deploy doesn't wait for CI | medium | P | `deploy-cloud.yml` has no link to `ci.yml`; a red commit ships |
| 5 | BH: tag rebuilds and overwrites the staging-tested `:<sha>` image | medium | P | the build job runs on tags too; production should promote the staging image |
| 6 | BH: staging SSH key is effectively cluster-admin, so it can change production; every run re-applies cluster-wide Traefik config | medium | P | README tells the operator to copy `k3s.yaml`. Fix: per-environment namespace-scoped ServiceAccount + kubeconfig, cluster config and namespaces made operator steps |
| 7 | BH: ufw doesn't filter k3s ServiceLB DNAT'd traffic (FORWARD chain), but the README claims it drops it | medium | P | known k3s/Docker ufw bypass; move that layer to the Hostinger VPS firewall in the runbook |
| 8 | BH: IPv6 half set up (AAAA records, no dual-stack) | low | P | README l.82 says A/AAAA; fix the docs to A only |
| 9 | BH: real client IP is lost (no `CF-Connecting-IP`/trustedIPs) | low | R | no consumer in 2.1; rate-limit stories own it |
| 10 | BH: rollback after a migration crash-loops on `SCHEMA_TOO_NEW` | low | P | by design (forward-only, roll-forward fixes, as on the Hub); add a runbook note |
| 11 | BH: no migration checksum | low | R | adds a column and a startup rule not in the architecture; the Hub list has none either |
| 12 | BH/ECH: `check-deploy` crashes at module load without `kubectl` | medium | P | top-level `render()`; give a clear "needs kubectl" error (not a skip: CI must run it) |
| 13 | BH: kubectl/kustomize version not pinned in CI | low | R | the rendered invariants checked are version-insensitive |
| 14 | BH: `check:secrets` passes plaintext values when a SOPS block exists | medium | P | `hasSopsMac` only checks `sops.mac`; require every `data`/`stringData` leaf to be `ENC[...]` |
| 15 | BH: base images by tag, not digest | low | R | the frozen spec says "pinned by tag" |
| 16 | BH: `ghcr-pull` described as optional but always referenced | medium | P | first deploy hangs in ImagePullBackOff; make it mandatory in the runbook, inside `<env>.enc.yaml` |
| 17 | BH: no preStop, no PDB | low | P (preStop only) | add `lifecycle.preStop.sleep` 5 s; a PDB is useless on one node, R |
| 18 | BH: public healthz shows version; test-only retry env ships | low | R | version is required by the AC; the env can only shorten the window to ≤60 s |
| 19 | ECH: multi-document `<env>.enc.yaml` fails `check:secrets` | high | P | `parseYaml` throws on multi-doc and the regex fallback misses (confirmed by reviewer); use `parseAllDocuments` |
| 20 | ECH: a deterministic migration SQL error retries 60 s and is logged "unreachable" | medium | P | `apply_one` maps all errors to `Db`; report a migration error distinctly and don't retry it at startup |
| 21 | ECH: trailing whitespace/newline in token or URL | low | P | a direct `trim()`; common with YAML block scalars |
| 22 | ECH: non-UTF-8 env reads as "not set" | low | R | unlikely; the message is still actionable |
| 23 | ECH: repeated `--role`/`--listen`, last one wins | low | R | the same as `arena-hub` config parsing |
| 24 | ECH: `RUST_LOG=debug` may log the Authorization header | maybe-false | R | operator-chosen verbosity; would need hyper 0.14/libsql debug output inspected; low if true |
| 25 | ECH: `ControlDb::connect` accepts non-loopback `http://` | low | R | the only caller (`config::parse`) refuses it |
| 26 | ECH: a migration containing BEGIN/COMMIT breaks the wrapping transaction | low | P | a direct test assertion over `CONTROL_MIGRATIONS` |
| 27 | ECH: non-loopback `ARENA_TEST_SQLD_URL` gives misleading test failures | low | R | CI uses `127.0.0.1` |
| 28 | ECH: origin smoke rejects TLS error/reset (curl 35/52/56) from a correctly locked origin | medium | P | a firewall reset or Traefik TLS refusal gives those codes; accept them |
