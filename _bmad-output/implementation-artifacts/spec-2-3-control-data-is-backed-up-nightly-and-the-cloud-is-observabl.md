---
title: 'Story 2.3: Control data is backed up nightly (observability deferred to the end of Epic 2)'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: '0f8664ef8f44cdd4925cf2b9db19c00b4ca71df6'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-2-cloud-error-envelope-local-dev-mode-and-single-sqlite-linkag.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `gz_control` holds every owner account and licence, and nothing backs it up. A Turso or VPS failure could lose it.

**Approach:**
- A `jobs`-role CronJob dumps `gz_control` every night into an age-encrypted file in an off-provider R2 bucket. It keeps the latest 30 and alerts a webhook when it fails.
- A documented, tested restore puts a dump into an empty database and proves table and row counts match.

**User decision (2026-10-24):** this story's observability ACs (OpenTelemetry to Grafana Cloud, Sentry, and the telemetry scrub test) are moved to the end of Epic 2 and are **not** built here.

## Boundaries & Constraints

**Always:**
- **Dump** (`arena_store::control`, both backends):
  - `ControlDb::dump()` produces a self-contained SQL text:
    - a header line `-- arena-control-dump v1` and a JSON manifest comment `{"schema_version", "taken_at", "tables": {name: row_count}}`;
    - `CREATE` statements from `sqlite_master` (tables, then indexes and triggers), skipping `sqlite_%`, `libsql_%` and `_litestream%`;
    - `INSERT` statements with correctly quoted values (TEXT escaped, BLOB as `X'..'`, NULL, INTEGER, REAL round-trip).
  - Rows are read in pages by `rowid` (1,000 per page) so large tables never load at once.
  - `schema_migrations` is included, so a restored database needs no migration.
- **Restore:** `ControlDb::restore(sql)`:
  - It refuses a database that already has user tables (`RESTORE_TARGET_NOT_EMPTY`) and a dump whose `schema_version` is newer than the binary (`SCHEMA_TOO_NEW`).
  - It applies the script in transactional batches, then recounts every table against the manifest. Any mismatch is an error naming the table.
- **Encryption:** the `age` Rust crate (pinned; the format is compatible with the age 1.3.2 CLI):
  - The job encrypts to the X25519 recipient(s) in `ARENA_BACKUP_AGE_RECIPIENTS` (comma-separated public keys) and never holds a private key.
  - Restore decrypts with `--identity <file>`, or reads plaintext with `--plaintext` for operator pipelines.
- **Object store:**
  - An `ObjectStore` trait (`put`, `get`, `list(prefix)`, `delete`) in `arena-cloud`, with:
    - an S3 implementation on `aws-sdk-s3` (pin as close to the architecture's 1.155 as is available; record the exact version), configured for R2: region `auto`, `request_checksum_calculation = WhenRequired`, path-style for MinIO;
    - an in-memory implementation for unit tests.
  - Env: `ARENA_BACKUP_S3_ENDPOINT`, `ARENA_BACKUP_S3_BUCKET`, `ARENA_BACKUP_S3_ACCESS_KEY_ID`, `ARENA_BACKUP_S3_SECRET_ACCESS_KEY` (never logged).
- **Job:** `arena-cloud --role jobs --task control-backup` runs once and exits:
  - It dumps, encrypts and puts `gz_control/nightly/<UTC yyyy-mm-ddTHHMMSSZ>.sql.age`.
  - It then lists the prefix and deletes all but the newest 30 by key, never deleting the object it just wrote.
  - It exits 0 on success and 1 on any failure.
  - On failure it POSTs `{"text": "..."}` (Slack-compatible) to `ARENA_ALERT_WEBHOOK_URL`, if set, with a 10 s timeout. The text names the environment (`ARENA_ENV`), the task and the error code, never the secrets.
  - A failing alert POST is logged, and the exit code stays 1.
  - An unknown `--task` exits 64; `--task` without `--role jobs` exits 64.
- **Restore CLI:** `arena-cloud ops restore-control (--from-key <key> | --from-file <path>) (--identity <file> | --plaintext)`:
  - It targets the DB in `ARENA_CONTROL_DB_URL` and `ARENA_CONTROL_DB_TOKEN`, or `--dev --data-dir` (as 2.2).
  - It prints the verified table counts and exits 0, or exits 1 with the reason.
  - It never runs as part of a deploy.
- **K8s:**
  - A `CronJob` `arena-cloud-control-backup` in the base, `schedule: "30 2 * * *"`, `timeZone: Asia/Kolkata`.
  - `concurrencyPolicy: Forbid`, `startingDeadlineSeconds: 3600` (a missed slot still runs), `backoffLimit: 2`, `activeDeadlineSeconds: 1800` (a hung run can't block the next one), and success/failure history limits of 3 and 5.
  - The same image and securityContext as the Deployment, with secret env from `arena-cloud-secrets` plus `arena-cloud-backup` (the backup S3 credentials, recipients and webhook).
  - Both overlays carry it, with a separate bucket per environment.
  - Extend `check:deploy` for these invariants and the RBAC Role (cronjobs).
- **Secrets and docs:**
  - `deploy/README.md` and `deploy/secrets/README.md` add the `arena-cloud-backup` Secret.
  - The R2 backup bucket is separate from data buckets, with a bucket-scoped API token.
  - The backup age keypair: the private key is kept offline by two operators and never put in the cluster.
  - The webhook (Slack/Discord-compatible, `/slack` suffix for Discord).
- **Restore runbook** (`deploy/README.md`, "Restore gz_control"): restore into a fresh staging Turso DB, point staging at it, start the control role (healthz `migration_version` equals the dump's), and record the timings for the 4 h RTO. Running the drill once before GA is a documented operator step.

**Never:**
- OpenTelemetry, Grafana, Sentry or telemetry scrubbing (deferred by the user).
- A private age key in the cluster or repo.
- Deleting the newest 30, or the object just written.
- Restoring over a non-empty database.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Nightly ok | DB with tables and rows | an encrypted object under `gz_control/nightly/`; exit 0 | N/A |
| Retention | 32 objects exist | the 2 oldest deleted; 30 remain incl. the new one | N/A |
| Value fidelity | TEXT with quotes/newlines/unicode, BLOB, NULL, big INTEGER, REAL | identical after restore | N/A |
| Store down | put fails | exit 1, webhook POSTed with code | webhook failure logged |
| DB down | dump fails | exit 1, webhook POSTed | N/A |
| No webhook | `ARENA_ALERT_WEBHOOK_URL` unset, failure | exit 1, logged | N/A |
| Restore ok | an empty target | counts match manifest; control starts at the dump's version | N/A |
| Restore non-empty | target has user tables | `RESTORE_TARGET_NOT_EMPTY`, nothing written | exit 1 |
| Wrong identity | wrong age key | decrypt error, nothing written | exit 1 |
| Count mismatch | tampered dump | error naming the table | exit 1 |
| Newer dump | manifest version > binary | `SCHEMA_TOO_NEW` | exit 1 |

</frozen-after-approval>

## Code Map

- `crates/arena-store/src/control.rs` -- `ControlDb` with remote (libsql) and local (rusqlite) backends and the shared runner (Stories 2.1/2.2); add `dump`/`restore` over both backends. Tests: `crates/arena-store/tests/control.rs` + `support/sqld.rs` (sqld harness, `ARENA_REQUIRE_SQLD`).
- `crates/arena-cloud/src/config.rs` -- `Role` (`Control`, `All`), `parse()`; add `Jobs` + `--task`, and the `ops restore-control` subcommand. `src/main.rs` exit-code constants (1/2/64).
- `crates/arena-cloud/src/lib.rs` -- `connect_and_migrate`, `open_local_and_migrate`; reuse for the restore target (open without migrating for restore).
- `deploy/k8s/base/*`, `deploy/k8s/cluster/deployer-rbac.yaml`, `scripts/check-deploy.mjs` (+ test), `deploy/README.md`, `deploy/secrets/README.md` -- Story 2.1 deploy assets.
- `deploy/dev/compose.yaml` -- MinIO (bitnamilegacy image) for local S3; CI: add a MinIO service container on the Linux `rust` job, like the libsql-server one, with `ARENA_TEST_S3_URL` and `ARENA_REQUIRE_S3=1`; S3 integration tests skip locally with a printed reason when unset (unless required).
- `Cargo.toml` workspace deps + README pin table (`check:pins`) -- add `aws-sdk-s3`, `aws-config` (or the minimal credential/region crates), `age`, `reqwest` (0.13 per architecture, rustls) for the webhook; prefer ring where a TLS provider is selectable and record any aws-lc-rs pull-in.
- `scripts/check-sqlite-linkage.sh` -- must still pass (no new SQLite).

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store/src/control.rs`, `tests/control.rs` -- dump/restore + fidelity, non-empty, mismatch, too-new tests on both backends.
- [x] `crates/arena-cloud/src/{store.rs,backup.rs,alert.rs,config.rs,main.rs}` -- ObjectStore (S3 + memory), backup task, retention, webhook, restore CLI.
- [x] `crates/arena-cloud/tests/backup.rs` -- matrix rows via memory store and a local webhook listener; an end-to-end round trip against MinIO (gated) and sqld.
- [x] `deploy/k8s/base/cronjob-control-backup.yaml`, overlays, RBAC, `scripts/check-deploy.mjs` (+ test) -- CronJob and checks.
- [x] `.github/workflows/ci.yml` -- MinIO service + env on the Linux rust job.
- [x] `deploy/README.md`, `deploy/secrets/README.md`, `README.md` -- secrets, runbook, pins, CLI docs.

**Acceptance Criteria:**
- Given sqld + MinIO, when the backup task runs and its object is restored with the matching identity into a fresh sqld, then every table's row count equals the source's and `arena-cloud --role control` starts on it with healthz `migration_version` equal to the source's.
- Given the rendered overlays, when `check:deploy` runs, then each has the CronJob with the schedule, time zone, Forbid, deadlines and secret refs above.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` with `ARENA_REQUIRE_SQLD=1` (and `ARENA_TEST_S3_URL` pointing at local MinIO from `deploy/dev/compose.yaml` with `ARENA_REQUIRE_S3=1`) -- pass (the known `arena-hub-ui` sidecar timing failure excepted).
- clippy (workspace, `-p arena-store --features control`), `cargo fmt --check`, `scripts/check-sqlite-linkage.sh`, every `pnpm run check:*`, `pnpm run test:scripts` -- pass.

## Implementation Notes

- **Pins (exact):** `aws-sdk-s3 =1.155.0` (the architecture's version, available), `aws-smithy-http-client =1.5.1` (`rustls-ring`), `age =0.12.1` (no default features), `reqwest =0.13.5` (`rustls-no-provider`, handed a workspace `rustls` + ring `ClientConfig` with webpki-roots). `aws-sdk-s3` is built without default features (`rt-tokio` only) and given the smithy HTTPS client on rustls + ring; credentials are static (`Credentials::new`), so no `aws-config`. **No aws-lc-rs/aws-lc-sys in the tree** (`cargo tree -p arena-cloud`). reqwest still pulls `rustls-platform-verifier` (unused, as the TLS config is preconfigured). README pin rows added; `bitnamilegacy/minio` also pinned from `ci.yml` (`check-pins.mjs` reads both CI service images).
- **Dump layout:** sections `schema` (tables in `sqlite_master` rowid order), `data` (one INSERT per line), `post-data` (indexes, views, triggers in creation order), then `-- end`. Indexes/views/triggers are created after the rows so a trigger never fires on restore (the spec bullet lists them with the CREATEs; file order is ours). A TEXT with `\n`, `\r` or NUL is written as `CAST(X'…' AS TEXT)` so every INSERT is one line and its bytes are exact; `i64::MIN` as `(-9223372036854775807-1)`; REAL with Rust's shortest round-trip form. Generated columns are skipped (`pragma_table_xinfo` hidden 0 only). `WITHOUT ROWID` tables page by primary key with `LIMIT/OFFSET`. A virtual table fails the dump (`DUMP_FAILED`) rather than double-dumping shadow tables. `sqlite_sequence` is skipped per the spec (AUTOINCREMENT resumes from the restored max rowid). The dump is not a point-in-time snapshot (no long read transaction on Turso); the manifest always counts exactly the rows in the dump, so the restore check stays exact.
- **Restore:** all validation (header, manifest, markers, `-- end`, data lines are INSERTs, schema version, empty target) happens before any write. Data batches are 500 statements / ~512 KiB, each with `PRAGMA defer_foreign_keys = ON`. Afterwards every table is recounted against the manifest (a table missing on either side is a mismatch too) and the restored `schema_version` must equal the manifest's. New `ControlError` variants `Dump`, `DumpInvalid`, `TargetNotEmpty`, `CountMismatch` and `ControlError::code()`.
- **Job:** argument errors exit 64; environment errors (missing S3 vars, bad recipients, missing DB URL) are task failures (`BACKUP_CONFIG_INVALID`, exit 1, alerted) so a broken Secret is reported. A run also gives up after 25 min (`BACKUP_TIMEOUT`, alerted) inside the 1800 s deadline. The alert text carries env, task and code only. `--role jobs --task control-backup --dev [--data-dir]` backs up the local dev file (for MinIO in dev). `--role all` stays the serving roles (control).
- **Restore CLI:** `--from-file -` reads stdin (for `age -d … | … --plaintext`). Config errors (missing target or bucket env) are usage errors (64); runtime failures exit 1 with `CODE: detail`.
- **K8s:** `ARENA_ENV` is the pod namespace (downward API) and `ARENA_BACKUP_S3_BUCKET` a plain per-overlay value (`arena360-gz-control-backup-<env>`), so `check:deploy` can prove separate buckets; the endpoint, token, recipients and webhook come from `arena-cloud-backup`. The deploy workflow now also sets the CronJob image to the digest (two `image:` lines asserted). Deployer Role: `batch/cronjobs` and the `arena-cloud-backup` Secret name. `check:deploy` also fails if the deploy workflow mentions `restore-control`.
- **S3 errors** are scrubbed of both credentials (`S3Store`), tested against MinIO.
- **Verified locally:** sqld (`/opt/homebrew/bin/sqld`) and MinIO from `deploy/dev/compose.yaml` with `ARENA_REQUIRE_SQLD=1 ARENA_REQUIRE_S3=1`. The age 1.3.2 CLI is not installed here, so CLI interop was not exercised (the crate is the reference Rust implementation of the same v1 format). The Linux CI MinIO service is untested until CI runs.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | VG: R2 `WhenRequired` checksum settings unverified (MinIO accepts the defaults) | medium | P | removing the two lines passes every test; add a header-capture test |
| 2 | VG: HTTPS paths of the webhook/S3 clients never run | medium | D | needs a webpki-chained TLS test server; covered once by the staging run and drill |
| 3 | BH/ECH: WITHOUT ROWID paging by OFFSET duplicates or skips under writes | medium | P | `dump_table` OFFSET branch; keyset on the PK |
| 4 | BH/ECH: dump is not a snapshot | medium | P (local) / D (remote) | local: one read txn; remote: Turso interactive txns time out, so it's deferred |
| 5 | BH/ECH: FK deferral only per 500-statement batch | medium | P | a child before its parent across batches fails the restore; FKs off + `foreign_key_check` |
| 6 | BH/ECH/VG: `sqlite_sequence` skipped, AUTOINCREMENT high-water lost | low | P | a direct fix; latent today (ULIDs) |
| 7 | BH: rowid not preserved for non-alias tables | low | R | ids are ULIDs; no consumer references rowid |
| 8 | ECH: a row at rowid `i64::MIN` is skipped | low | P | direct fix to the first page bound |
| 9 | ECH: a user column named `rowid` | low | R | the schema is ours; no such column |
| 10 | ECH: WITHOUT ROWID text detection fooled by a trailing comment | low | R | the schema is ours; migrations have no such comments |
| 11 | BH/ECH: about 3× dump in memory, OOM kill is silent | medium | P | stream into the encryptor; limit 512Mi |
| 12 | BH/ECH/VG: no dead-man's switch; a missing webhook key blocks the pod; README says optional | medium | P | add a heartbeat URL; optional secretKeyRefs |
| 13 | BH/ECH: `backoffLimit: 2` alerts per attempt; a retry is killed by the deadline silently | medium | P | `backoffLimit: 0` |
| 14 | BH: broken manifest hidden by `unwrap_or_default` | medium | P | a direct fix |
| 15 | BH: no shrink/sanity guard before pruning | low | R | adds policy beyond the story; the heartbeat and drill catch empty dumps sooner |
| 16 | BH: the cluster token can delete every backup; no signature | medium | P (docs) | recommend a 30-day R2 bucket lock (compatible with keep-30); signing R |
| 17 | BH: `ops restore-control` env errors exit 64, not 1 | low | P | contradicts the README; move the checks to run time |
| 18 | BH: error messages repeat the code | low | P | direct Display fix |
| 19 | BH: missing tests (retention failure, second recipient) | low | P | small tests |
| 20 | BH/ECH: setup outside `RUN_TIMEOUT` | low | P | direct fix |
- **Review fixes (pass 1):**
  - `sqlite_sequence` is now dumped (a `sequences` section after the data), overriding the spec's skip, so AUTOINCREMENT resumes above a deleted max.
  - Restores run with foreign keys off: the pragma is set on the local connection, and goes ahead of `BEGIN` in each remote request, because sqld enforces FKs and opens new streams. `PRAGMA foreign_key_check` then runs after the data and fails with `RESTORE_FOREIGN_KEY_VIOLATION <table>`.
  - Paging: `WITHOUT ROWID` tables use keyset paging by PK, and the first rowid page is unbounded.
  - The local dump is one read transaction (the local mutex is now a tokio mutex held across the dump). The remote dump stays page-by-page because Turso times out interactive transactions; the module doc explains the drift.
  - `Dump` parts are written straight into the age encryptor.
  - Setup now runs inside the same 25 min limit as the run. An optional `ARENA_BACKUP_HEARTBEAT_URL` is pinged after success.
  - The webhook and heartbeat keys are `optional: true`. CronJob `backoffLimit: 0`, memory limit 512Mi.
  - `ops restore-control` reads its env at run time: a missing variable is `RESTORE_CONFIG_INVALID`, exit 1.
  - Codes appear once in messages.
  - R2 bucket lock of 30 days documented; retention is unchanged.
