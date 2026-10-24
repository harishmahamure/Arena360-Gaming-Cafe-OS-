---
title: 'Story 2.2: Cloud error envelope, local dev mode and single SQLite linkage'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: '9cef6d0bdc5a1234cf8e76419314200f42ca77b2'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-1-arena-360-cloud-runs-in-india-behind-cloudflare.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The cloud has no shared error shape for `/v1`, cannot run on a developer's machine without Turso, and nobody has checked that rusqlite and libsql link only one SQLite.

**Approach:**
- **Error envelope:** one `/v1` error layer in `arena-cloud` that turns every failure (unknown path, wrong method, malformed body, internal error, panic) into a flat `AppError{code, message, field?}` body with a stable `arena-proto` code.
- **Dev mode:** `arena-cloud --role all --dev` runs `gz_control` from a local SQLite file through rusqlite, with the same embedded migrations. MinIO stands in for R2 via a compose file.
- **Linkage spike:** settled by keeping libsql remote-only. rusqlite's bundled SQLite is the only SQLite C library in the binary, and a CI test proves it at symbol level.

## Boundaries & Constraints

**Always:**
- **Codes:** add `NotFound` (`NOT_FOUND`) and `BadRequest` (`BAD_REQUEST`, "body is not valid JSON or doesn't match the request") to `arena_proto::ErrorCode`, with doc comments, and regenerate the ts-rs types. Reuse `Unsupported` for a wrong method and `Internal` for internal failures.
- **Wire:** the body is the flat `AppError` JSON (no `{"error": …}` wrapper), `content-type: application/json`. Status mapping:

  | Code | Status |
  |------|--------|
  | `NOT_FOUND` | 404 |
  | `UNSUPPORTED` (wrong method) | 405 |
  | `BAD_REQUEST` | 400, or 413 when the body is over the limit |
  | `VALIDATION_FAILED` | 422 |
  | `INTERNAL` | 500 |

  `INTERNAL` always carries the fixed message "Something went wrong" (or a similar plain sentence), and the detail is logged via tracing. No response body ever contains a backtrace, `panicked at`, a file path or a Rust type name.
- **Mechanism** (`crates/arena-cloud/src/api/`):
  - `ApiError` (wraps `AppError`) implements `IntoResponse`.
  - An `ApiJson<T>` extractor maps every axum JSON rejection to `BAD_REQUEST`, naming `field` when serde reports one.
  - The `/v1` router has a `NOT_FOUND` fallback and a method-not-allowed mapping.
  - A panic-catching layer (hand-written with `futures_util::FutureExt::catch_unwind`; no new crate) turns a panic into `INTERNAL`.
  - The body limit is 64 KiB.
  - Paths outside `/v1` other than `/healthz` also get the `NOT_FOUND` envelope.
- **Dev-only routes**, mounted only with `--dev` so the integration suite can exercise each failure: `POST /v1/dev/echo` (`{text: string}` → `{text}`), `GET /v1/dev/fail` (returns `INTERNAL`) and `GET /v1/dev/panic`. Production routing never mounts them; a test asserts a non-dev router returns `NOT_FOUND` for them.
- **Roles:** `--role all` means every role this build has (only `control` today). `/healthz` reports `role: "all"`.
- **`--dev`:**
  - Requires no env and no token. The database is `<--data-dir>/gz_control.db`, where `--data-dir` defaults to `./.arena-cloud-dev` and is created if missing.
  - The default listen address becomes `127.0.0.1:8080`, and a non-loopback `--listen` is refused (exit 64).
  - One warning log line says dev mode is on.
  - Without `--dev`, behaviour from Story 2.1 is unchanged.
- **Storage:**
  - `arena_store::control::ControlDb` gains a local backend (rusqlite, file path) next to the libsql remote one, behind the same public API (`connect`/`open_local`, `ping`, `schema_version`, `migrate`, `migrate_with`).
  - One runner drives both backends with the same `CONTROL_MIGRATIONS`, the same `SchemaTooNew` and the same per-migration transaction. Blocking rusqlite calls run off the async runtime.
  - The existing sqld tests stay. The core cases (fresh, restart, too-new untouched, a bad migration) also run against the local backend.
- **Linkage:**
  - libsql stays `default-features = false` and remote-only; record the spike outcome in README and Implementation Notes.
  - `scripts/check-sqlite-linkage.sh` (Linux and macOS) builds `arena-cloud` and fails unless:
    - the binary defines exactly one `sqlite3_open_v2` (via `nm`);
    - it defines no `libsql_`-prefixed C symbols;
    - `cargo tree -p arena-cloud` contains no `libsql-ffi`/`libsql-sys`.
  - It runs in the CI `rust` job on Linux.
- **MinIO:** `deploy/dev/compose.yaml` runs MinIO (image pinned by tag, added to the README pin table) on `127.0.0.1:9000`/`9001`, with dev-only credentials stated in the file and a one-shot bucket-create service for `arena-dev`. No R2 client exists yet: the first R2 story consumes it. The README dev section documents `docker compose -f deploy/dev/compose.yaml up -d` followed by `cargo run -p arena-cloud -- --role all --dev`.
- **Integration suite** (`crates/arena-cloud/tests/api_envelope.rs`): spawns the binary in `--dev` mode on a temporary data dir and port, then asserts code, status, content type and no stack trace for:
  - an unknown `/v1` path;
  - a wrong method;
  - a malformed JSON body;
  - a type-mismatched body (with `field`);
  - an oversize body;
  - `/v1/dev/fail`;
  - `/v1/dev/panic`, after which the server is still serving.

  It also asserts `/healthz` reports `role: "all"` and `migration_version: 1`, and that a restart on the same data dir applies nothing. It needs no sqld, so it runs on both CI OSes.

**Never:**
- A C SQLite core in libsql, or a second SQLite library.
- An S3/R2 client in this story.
- Dev routes reachable without `--dev`.
- Changing the Hub or LAN error wire shapes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Unknown path | `GET /v1/nope` | 404 `{code:"NOT_FOUND",message}` | N/A |
| Wrong method | `DELETE /v1/dev/echo` | 405 `UNSUPPORTED` | N/A |
| Malformed body | `POST /v1/dev/echo` with `{"text":` | 400 `BAD_REQUEST` | N/A |
| Type mismatch | `{"text": 5}` | 400 `BAD_REQUEST`, `field:"text"` | N/A |
| Too large | 65 KiB body | 413 `BAD_REQUEST` | N/A |
| Internal | `GET /v1/dev/fail` | 500 `INTERNAL`, generic message; detail only in logs | N/A |
| Panic | `GET /v1/dev/panic` | 500 `INTERNAL`, no "panicked"; the next request still served | N/A |
| Dev start | `--role all --dev`, empty dir | DB file created, migration 1, healthz `role:"all"` | N/A |
| Dev restart | same dir | nothing applied | N/A |
| Dev too new | local DB at latest+1 | exit 2 | N/A |
| Dev non-loopback | `--dev --listen 0.0.0.0:8080` | exit 64 | N/A |
| Prod router | no `--dev`, `GET /v1/dev/fail` | 404 `NOT_FOUND` | N/A |
| Linkage | built binary | exactly one `sqlite3_open_v2`, no `libsql_` symbols | script fails otherwise |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/error.rs` -- `ErrorCode` enum + `as_str` + tests; add the two variants. Regenerate with the repo's generator (`scripts/gen-types.mjs`; `pnpm check:generated` compares) into `packages/ui/src/generated`. `scripts/check-protocol-types.mjs`/`check-strings.mjs` may need the new codes registered — run them.
- `crates/arena-api/src/ui_auth.rs:661` `ui_error`, `status_of` -- Hub pattern for hiding internal detail; mirror the idea, don't share code (layers: arena-cloud may not depend on arena-api).
- `crates/arena-cloud/src/{lib.rs,config.rs,health.rs,main.rs}` (Story 2.1) -- `Role` enum (add `All`), `parse()` (add `--dev`, `--data-dir`), `connect_and_migrate`, `serve_control(listener, db, version, interval, shutdown)`; build the full app router (healthz + `/v1`) in one place.
- `crates/arena-store/src/control.rs` (Story 2.1) -- remote-only `ControlDb`; add the local backend. rusqlite is already a non-optional dep of arena-store (bundled).
- `crates/arena-store/tests/{control.rs,support/sqld.rs}` -- the sqld harness; local-backend tests need only a tempdir.
- `crates/arena-cloud/tests/binary.rs` -- spawn/poll helpers to reuse for `api_envelope.rs`.
- `.github/workflows/ci.yml` `rust` job -- add the linkage script step (Linux).
- `README.md` -- cloud usage section (Story 2.1), pin table (check:pins), dev section.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/error.rs`, `packages/ui/src/generated/*` -- new codes + regen.
- [x] `crates/arena-store/src/control.rs`, `crates/arena-store/tests/control.rs` -- local backend, shared runner, local tests.
- [x] `crates/arena-cloud/src/api/{mod.rs,error.rs,json.rs,panic.rs,dev.rs}`, `src/lib.rs`, `src/config.rs`, `src/main.rs` -- envelope layer, dev routes, `--role all`, `--dev`, `--data-dir`.
- [x] `crates/arena-cloud/tests/api_envelope.rs`, `tests/config.rs` -- matrix rows.
- [x] `scripts/check-sqlite-linkage.sh`, `.github/workflows/ci.yml` -- linkage check.
- [x] `deploy/dev/compose.yaml`, `README.md` -- MinIO and dev docs, pins, spike outcome.

**Acceptance Criteria:**
- Given a clean checkout with no Turso/Cloudflare credentials, when a developer follows the README dev section, then the cloud serves `/healthz` locally with `migration_version: 1`.
- Given `scripts/check-sqlite-linkage.sh`, when run locally (macOS) and in CI (Linux), then it passes, and it fails when pointed at a binary with two SQLite copies (proved by a script self-test against a crafted symbol list).

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`), `cargo test --locked -p arena-store --features control` -- pass (the known `arena-hub-ui` sidecar timing failure excepted).
- `cargo clippy --locked --workspace --all-targets -- -D warnings`, plus `-p arena-store --features control` -- clean; `cargo fmt --check`.
- `scripts/check-sqlite-linkage.sh` -- pass.
- every `pnpm run check:*` (incl. `check:generated` after commit), `pnpm run test:scripts` -- pass.

## Implementation Notes

- **Linkage spike outcome:** settled by keeping libsql remote-only (`=0.10.0`, `default-features = false`, `remote` + `tls`). `cargo tree -p arena-cloud` has no `libsql-ffi`/`libsql-sys`; the macOS debug binary defines exactly one `_sqlite3_open_v2` (rusqlite's bundled `libsqlite3-sys`) and no `libsql_`-prefixed C symbols (only mangled Rust symbols of `libsql_sqlite3_parser`, which the script ignores). `scripts/check-sqlite-linkage.sh` passes locally on macOS; its `--self-test` (also run by `test:scripts`, skipped on Windows) proves the rules fail on a two-copy listing, a libsql C core listing and an empty listing. The Linux run happens in the CI `rust` job and was not executed locally.
- **Local backend:** `ControlDb` is now an enum backend (libsql remote | `Arc<Mutex<rusqlite::Connection>>`), with every rusqlite call on `spawn_blocking`. `open_local` sets WAL and a 5 s busy timeout. One runner (`migrate_with` → `apply_one`) drives both; a failed step is classified per backend (remote: SQL-and-not-lock text; local: anything but `SQLITE_BUSY`/`SQLITE_LOCKED`) as deterministic (fails at once as `ControlError::Migration`) or retried.
- **Envelope:** `api::with_envelope` wraps `/healthz` and `/v1` with `method_not_allowed_fallback` (405 `UNSUPPORTED`), a router `fallback` (404 `NOT_FOUND`, also for paths outside `/v1`), `DefaultBodyLimit::max(64 KiB)` and a `from_fn` middleware using `futures_util::FutureExt::catch_unwind`. A JSON rejection's message is fixed text (serde's own message can name Rust types); `field` comes from `serde_path_to_error`'s path, or from "missing field `x`" at the top level. This adds a direct `serde_path_to_error =0.1.20` dependency to arena-cloud (already in the tree via axum's `json` feature; README pin row added). `main` installs a panic hook that logs the panic as one JSON line instead of default stderr text.
- **Hub code:** `NOT_FOUND`/`BAD_REQUEST` got arms in the Hub's exhaustive matches (`arena-api::ui_auth::status_of` → 404/400, `arena-kiosk-core::queue::is_deterministic` → true). The Hub never produces either code, so its wire shapes are unchanged.
- **`--dev` details:** `--data-dir` without `--dev` is a usage error (64); `--dev` ignores `ARENA_CONTROL_DB_*`; a local open failure is `StartError::Local` (exit 1, not retried). `--role control --dev` is also accepted. `.arena-cloud-dev/` is gitignored and dockerignored.
- **MinIO images:** the upstream `minio/minio` and `minio/mc` images are no longer on Docker Hub (and quay.io needs auth), so the compose file pins Bitnami's frozen legacy builds `bitnamilegacy/minio:2025.7.23-debian-12-r5` and `bitnamilegacy/minio-client:2025.7.21-debian-12-r3`. Verified locally with `docker-compose up -d`: MinIO healthy on 127.0.0.1:9000 and the one-shot service created `arena-dev`. This build has no web console, so 9001 (in the frozen spec) is not published (review finding). `check-pins.mjs` gained a `deploy/dev/compose.yaml` reader (+ test).
- **check:generated** reports `ErrorCode.ts` stale until the regenerated file is committed (it compares against git); the regenerated content only adds the two codes.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH/VG: `ARENA_CONTROL_DB_RETRY_SECS` validated in `--dev`, though the README says it's ignored | low | P | `config.rs` parses it before the `if dev`; a stray value exits 64 |
| 2 | VG: a top-level missing field (`{}`) naming `field` is never tested end-to-end | medium | P | only a hand-made string reaches `missing_field` |
| 3 | BH/ECH/VG: a nested missing field names the parent (`items[0]`) | low | P | direct fix: append the name to the path |
| 4 | BH: `status_of(Unsupported)` = 405 diverges from the Hub's 400 | medium | P | a future handler returning `UNSUPPORTED` for a version problem would say "Method Not Allowed"; keep 405 only on the method fallback |
| 5 | BH: non-JSON rejections (Path/Query/String/415/413 on Bytes) answer plain text | medium | P | `ApiJson` covers only `Json`; add a response-mapping layer for any non-JSON ≥400 |
| 6 | BH: `Role::runs_control()` is unused | low | P | direct: have `main` use it |
| 7 | BH: a panic is logged twice, the hook has no backtrace | low | R | cosmetic; one extra log line |
| 8 | BH: no e2e test for 422 / Allow header | low | R | no route produces 422 yet; missing content type is covered by #5's test |
| 9 | BH/ECH: the 413 test can see a reset before the response | medium | P | the server closes with unread bytes → RST on Linux; send the headers with an oversize `Content-Length` only |
| 10 | BH/ECH: the linkage check runs on the debug binary; a stripped binary says "found 0" | low | P (message only) | direct: report "no symbols (stripped?)"; R for the release-artifact part |
| 11 | ECH: `check_tree` under pipefail: `grep -q` → SIGPIPE → false pass | high | P | `cargo tree \| grep -q` returns 141, the `if` is false and a libsql C core goes unreported |
| 12 | ECH: `CARGO_TARGET_DIR` is ignored by the linkage script | low | P | direct `${CARGO_TARGET_DIR:-…}` |
| 13 | BH/ECH: `readDevCompose` silently skips unparsable images | medium | P | an unpinned compose image passes `check:pins` |
| 14 | BH: compose publishes 9001 with no console | low | P | deletion |
| 15 | BH: the diff omits the generated TS and the lockfile | false | R | excluded on purpose; `check:generated` and `--locked` cover them |
| 16 | BH: local non-lock I/O errors are treated as deterministic | low | R | dev-only; failing at once on a full disk is acceptable |
| 17 | BH: no SIGTERM/WAL checkpoint test | low | R | SQLite recovers the WAL; dev-only |
| 18 | ECH: other 5xx codes are not sanitised | low | R | their messages are fixed text written in our code |
| 19 | ECH: WAL mode not verified | low | R | dev-only, behaviour still correct |
| 20 | ECH: healthz `reachable==true` asserted before the first check | medium | P | the 2.1 fix seeds `false`; poll until true |
| 21 | ECH: `free_port` race | low | R | the repo's established test pattern |
| 22 | ECH: a dynamically linked libsqlite3 isn't checked | low | P | direct: `otool -L`/`ldd` check for libsqlite3 |
| 23 | VG: no test for `--dev` failing to open its data dir | low | D | VG disposition defer; dev-only |
| 24 | VG: local busy/locked retry not exercised | low | D | VG disposition defer; the 5 s busy_timeout absorbs it |
