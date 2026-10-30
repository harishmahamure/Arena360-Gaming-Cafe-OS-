---
title: 'Story 9.2: Player keys are backed up and the branch key is escrowed'
type: 'feature'
created: '2026-10-30'
status: 'review'
baseline_commit: 'd7511a48c953c8f39f9ea6171402595df660a422'
baseline_revision: 'd7511a48c953c8f39f9ea6171402595df660a422'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-1-erase-a-player-s-personal-data-by-crypto-shredding.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The keys that open players' data live only on the Hub. A dead PC loses them (the data is unreadable), and nothing off the Hub can prove an erased player's key is gone.

**Approach:** One new Hub task, `KeyBackupTask`, does two jobs. On every tier it wraps the branch KEK for the cloud escrow public key (age X25519) and sends it to `POST /v1/kek/escrow`, which stores it in `gz_control.kek_escrow` with an `admin_audit` row. On paid tiers it also seals the whole `player_keys` table with the KEK and PUTs it to `keys/player-keys.bin` (presign `kind: keys`) whenever the table changed. A new Hub table `backup_state` (outside the change log) counts key-table changes by trigger and records the last success and last error.

## Boundaries & Constraints

**Always:**
- Escrow runs for Free too (the one key-only exception to "Free uploads nothing"). It is sent when `backup_state.kek_escrowed_at` is empty: after pairing, for a Hub paired before this existed, and again after a re-pair mints a new KEK (`record_repairing` clears it).
- The escrow body is an age file (binary, base64 in JSON). The cloud refuses anything that is not (`AGE_MAGIC`, ≤ 1 KiB, `VALIDATION_FAILED` on `wrapped_kek`), so a bare key cannot be stored.
- Key backup only for paid tiers, only for a primary, only when `keys_version > keys_backed_up_version`. The object is overwritten, never versioned. An erased player's key is absent from the next object.
- Presign `kind: keys` has no date, follows the snapshot rules: paid only (`FEATURE_NOT_ENTITLED`), `FENCED` below the branch fence, own-prefix key `keys/player-keys.bin`.
- A failed PUT or a down WAN leaves the table marked changed; the retry backs off like the uploader and sends only the table as it is then. The success time and the last error code are written to `backup_state` through the Writer after the cloud answered.
- Every escrow write is one transaction with an `admin_audit` row (`kek.escrow.write`, actor `system`/the Hub, summary without the ciphertext).
- Escrow calls are rate-limited per branch (12 an hour); key backup links share the presign hourly budget.
- Logs carry no key, player id or wrapped bytes.

**Never:**
- No endpoint returns the wrapped KEK; the only `kek_escrow` functions are the write and a count that never selects the ciphertext. The reader for key release is Story 9.10.
- No restore, key release, standby replication of keys or retention sweep of `keys/` (Stories 9.7, 9.10, 9.12, 9.15, 9.19).
- No key material in the daily snapshot or in change rows (unchanged from 8.8 and 9.1).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Free Hub, key not escrowed | cycle | one `POST /v1/kek/escrow`, `kek_escrowed_at` set; no presign, no PUT ever | cloud down: backoff, retried |
| Already escrowed | cycle | nothing sent | none |
| Re-pair with a new KEK | `record_repairing` | escrow state cleared, key backup stale; next cycle escrows again | none |
| Paid, table changed | create or erase a player | one presign `kind: keys` + PUT of the sealed table within one interval (or sooner after a `players` change, min gap 30 s) | PUT fails: `keys_error` set, table still dirty |
| Paid, nothing changed | cycle | nothing sent | none |
| Erase a player | next backup | object holds every key but theirs | none |
| Non-age body to the cloud | bare 32-byte key | `VALIDATION_FAILED`, nothing stored, no audit | none |
| Hub below the fence | `kind: keys` | `FENCED` | Hub retries with backoff |
| Release build with no escrow key | cycle | warning once, nothing sent | retried every 15 min |
| Same Hub escrows again | new KEK | its row replaced; both writes audited | none |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0083_backup_state.sql`, `src/backup_state.rs` -- `backup_state` row, insert/update/delete triggers on `player_keys` bumping `keys_version`, reads and writes; `clear_kek_escrow` on re-pair.
- `crates/arena-store/control-migrations/0027_kek_escrow.sql`, `src/control/kek_escrow.rs` -- `kek_escrow(branch_id, hub_instance_id, wrapped_kek, created_at)`; `put_kek_escrow` writes row and audit in one transaction.
- `crates/arena-proto/src/kek.rs`, `presign.rs` -- `KekEscrowRequest/Response`, `PRESIGN_KIND_KEYS`, `KEYS_OBJECT_KEY`, `KEK_ESCROW_RECIPIENTS` (empty until release), dev recipient.
- `crates/arena-core/src/key_backup.rs` -- `seal_key_table` / `open_key_table` (`A360KEYS` v1, AES-256-GCM under the KEK); `snapshot.rs` `key_backup_plan`; `writer.rs` `record_key_backup`, `record_key_backup_error`, `record_kek_escrow`.
- `crates/arena-tasks/src/keys.rs` (+ `keys/tests.rs`), `cloud.rs` -- `KeyBackupTask`, `wrap_kek`, `CloudClient::kek_escrow`; `crates/arena-hub/src/lib.rs` spawns it.
- `crates/arena-cloud/src/api/kek.rs`, `api/presign.rs`, `presign.rs` -- the route, the `keys` presign kind and its put counter.

## Tasks & Acceptance

**Execution:**
- [x] `arena-store` -- Hub migration 0083 with triggers; control migration 0027; queries
- [x] `arena-proto` -- escrow request/response, presign kind, key constants; `pnpm gen:types`
- [x] `arena-core` -- key table format, reader plan, Writer maintenance writes, re-pair clears escrow
- [x] `arena-cloud` -- `POST /v1/kek/escrow`, `kind: keys`, tests for both
- [x] `arena-tasks` / `arena-hub` -- `KeyBackupTask`, wired and aborted on shutdown
- [x] Privacy test: no name, phone or email in a batch, key backup object, request bodies or log after creating and erasing a player

**Acceptance Criteria:**
- Given a Hub of any tier with no escrow record, then it sends its KEK wrapped for the escrow key to `POST /v1/kek/escrow`, the control role stores it in `gz_control.kek_escrow`, and only the escrow private key (not in this repo) can open it.
- Given a paid Hub and a change to `player_keys`, then within one upload interval the whole table, sealed with the KEK, is PUT to `keys/player-keys.bin`, and an erased player's key is absent.
- Given a Free Hub, then nothing is uploaded under the branch prefix; the escrowed KEK is the only key material the cloud holds.
- Given a failed PUT or no WAN, then the retry backs off, only the latest table is sent afterwards, and `backup_state` holds the last success time and last error.
- Given the escrow endpoint, then no endpoint returns the wrapped KEK and every write has an `admin_audit` row.
- Given a scan after creating and erasing a player, then no plaintext name, phone or email is in a batch, a key backup, the escrow body or the Hub log.

## Spec Change Log

## Review Triage Log

## Design Notes

- The escrow check is its own cycle step rather than a heartbeat field: the Hub already knows from `backup_state` whether the cloud accepted its key, and a re-pair clears it. The AC's "next heartbeat" is met by a cycle at least every minute while unescrowed (`TIER_RECHECK`), faster than the heartbeat.
- `player_keys` changes are counted by SQL triggers, not by hooks in `apply()`: erasure, creation, import and any later writer (restore, standby apply) are all covered by the transaction that changes the table.
- Sealing the table with the KEK is on top of each DEK already being KEK-wrapped; it keeps player ids unreadable in the object and gives restore one thing to open.
- `KEK_ESCROW_RECIPIENTS` is empty in release builds like `LICENCE_VERIFY_KEYS`: the production escrow public key is added at release when its SOPS-managed private half exists. The dev recipient's private half was discarded; tests generate their own pair.

## Verification

**Commands:**
- `cargo test -p arena-store --features control -p arena-proto -p arena-core -p arena-tasks -p arena-cloud` -- expected: pass except the failures listed in the Auto Run Result
- `cargo clippy -p arena-store -p arena-proto -p arena-core -p arena-tasks -p arena-cloud -p arena-hub --all-targets -- -D warnings` -- expected: clean
- `pnpm gen:types`, `pnpm run check:layers check:pins check:protocol check:secrets check:strings` -- expected: clean (`check:generated` is clean once the generated files are committed)

## Auto Run Result

Status: in-review

Implemented per spec. Failures seen that are not from this story and fail the same on the baseline commit: `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-cloud` `backup` `a_hung_run_times_out_with_its_own_code` and `binary` `unreachable_at_start_retries_then_exits_1_without_logging_the_token`; `arena-hub` `android_fleet` test does not compile with `--features dev-fixtures` (`SessionSettle` fields); `check:deploy` needs `kubectl`.

Known gaps, left for review: release builds carry no escrow public key yet, so a production Hub escrows nothing until it is added; the Hub header and System screen do not show key backup status (Stories 9.16/9.17); the privacy test captures logs from the test thread, not from `spawn_blocking` workers; `check:generated` reports the new generated files until they are committed.
