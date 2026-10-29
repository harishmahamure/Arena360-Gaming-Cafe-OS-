---
title: 'Story 8.3: The heartbeat detects upload gaps and the Hub resends them'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: 'b9c1937e8c7598d6783273859d4e0adbb0e9e57f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A change-log batch can be lost between the Hub's 2xx and the bucket, or never land, and nothing notices: the cloud has no view of how far each Hub got, and the Hub never ships a range twice (AD-4, FR167).

**Approach:** The paid Hub's heartbeat reports its upload progress. The cloud compares it with a cached per-hub view and only when they disagree lists `changes/<epoch>/`, finds the first missing seq range and answers `resend{from, to}`. The uploader ships that range first (keys for exactly those seqs), then carries on from its cursor. Rows already pruned are reported as `resend_unavailable` and counted as a gap instead of being asked for again.

## Boundaries & Constraints

**Always:**
- Request adds optional `last_upload_at` (Hub clock, ms), `last_summary_at`, `last_rollup_date` (both stay absent until Stories 8.4–8.6) and `fork_seq`; `last_seq_uploaded` is the seq uploaded in the heartbeat's own epoch (0, or the fork parent's seq, while the cursor is in an older epoch). `resend_unavailable{epoch, from, to}` is sent once, until a heartbeat is answered.
- Only a paid branch (tier_at(now) not Free) is checked; a Free heartbeat is unchanged and the cloud lists nothing. The cloud caches `last_seq_seen` and `last_epoch_seen` on the `hubs` row. Equal epoch and seq to the cache: no listing at all.
- Missing range = first hole in the union of `changes/<epoch>/<first>-<last>.zst` keys within `[fork_seq+1 (else 1), last_seq_uploaded]`. A hole at the tail counts. No hole: the cache is set to the heartbeat's values and nothing is added. A hole: the cache is left alone (so it is re-checked) and `resend{from, to}` answers.
- A range the Hub reported unavailable (same epoch, covering the hole) is not requested again; the cache then moves on so the listing stops. A hole counts once in the per-branch "heartbeat gaps" metric when it is first seen (a different range than the last one issued).
- The Hub resends in batches of up to 5,000 rows, each under the key `changes/<epoch>/<first:012>-<last:012>.zst` for exactly its seqs, never moving the cursor, `uploaded_at` or `last_upload_at`. It does the resend before the normal cycle. A resend for an epoch other than the lease's is dropped.
- A listing failure (bucket down, no data bucket) never fails the heartbeat: no resend, cache untouched, a warning logged.
- No `/uploads/manifest` route, `upload_manifest` table or ingest cursor exists anywhere (a CI grep, `pnpm run check:no-manifest`).

**Never:**
- The summary, rollup or snapshot uploads (8.4–8.8), the Grafana exporter (counts stay in process and are logged, as in 8.1), a change to `apply()`, or the real epoch change of Epic 9.
- Trusting the Hub's range for a key: the range is validated (`1 <= from <= to`) and the cloud answers only ranges it found itself.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| In step | heartbeat equals cached view | no listing, no `resend` | none |
| First beat | no cache, keys 1–10 and last_seq 10 | one listing; cache set | none |
| Hole | keys 1–5000, 10001–12000; last 12000 | `resend{5001,10000}`; cache unchanged | none |
| Tail lost | keys 1–5000; last 7000 | `resend{5001,7000}` | none |
| Forked | `fork_seq` 40; keys 41–100; last 100 | no hole | none |
| Pruned | Hub has no rows 5001–10000 | next beat carries `resend_unavailable`; cloud stores it, counts the gap, no `resend` again | none |
| Free | tier Free | nothing listed or recorded | none |
| Listing fails | bucket error | heartbeat 200, no `resend` | warning |
| Resend shipped | `resend{5001,10000}` | two keys covering exactly 5001–10000; cursor unchanged | PUT failure: range kept, backoff |

</intent-contract>

## Code Map

- `crates/arena-proto/src/pairing.rs` -- `HeartbeatRequest`, `HeartbeatResponse`, `HeartbeatFence`; new `SeqRange`, `ResendUnavailable`; TS is generated (`pnpm gen:types`).
- `crates/arena-store/control-migrations/0018_*.sql`, `src/control.rs`, `src/control/hubs.rs` -- cached view columns on `hubs`, `Beat`/`HubRow` pattern, `branch_fence_epoch`.
- `crates/arena-cloud/src/api/heartbeat.rs` -- where the check goes (after the licence claims); `presign.rs` -- `Presigner` client/bucket (add listing, a memory lister for tests), `PutMetrics` pattern for the gap metric, `auth.rs` `AuthState`.
- `crates/arena-store/src/upload.rs`, `crates/arena-core/src/{snapshot,status}.rs` -- cursor, fork, `read_pending_batch`; add the range read, `UploadStatus.last_seq_uploaded`/`fork_seq`.
- `crates/arena-tasks/src/heartbeat.rs`, `upload.rs`, `lib.rs`, `crates/arena-hub/src/lib.rs` -- the request, the answer, a shared `ResendInbox`, the resend pass of the uploader.
- `scripts/check-no-remote.mjs` (+ test) -- pattern for a repo grep gate; `package.json` `check` chain.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/pairing.rs`, `lib.rs`, generated TS -- new request/response fields and range types.
- [x] `crates/arena-store/control-migrations/0018_hub_upload_view.sql`, `control.rs`, `control/hubs.rs` -- columns, `hub_upload_view`, `record_upload_view`, `record_resend_issued`, `record_resend_unavailable`, `clear_resend`.
- [x] `crates/arena-cloud/src/presign.rs`, `api/heartbeat.rs`, `auth.rs` -- listing, gap finder (`missing_range`), handler wiring, `GapMetrics`.
- [x] `crates/arena-store/src/upload.rs`, `crates/arena-core/src/{snapshot,status}.rs` -- `read_range_batch`, upload progress fields.
- [x] `crates/arena-tasks/src/{heartbeat,upload,lib}.rs`, `crates/arena-hub/src/lib.rs` -- fields in the request, the inbox, the resend pass.
- [x] `scripts/check-no-manifest.mjs`, test, `package.json` -- the grep gate.
- [x] Tests -- gap finder units, cloud matrix (`tests/gaps.rs`), store range read, task resend and unavailable paths.

**Acceptance Criteria:**
- Given a paid Hub whose heartbeat agrees with the cloud's cached view, when it beats, then no bucket listing is made.
- Given a hole in `changes/<epoch>/`, when the Hub beats, then the answer carries `resend{from, to}` for exactly the first hole, and after the Hub resends it the next beat answers none and the view is updated.
- Given pruned rows in the range, when the Hub resends, then the next heartbeat carries `resend_unavailable` and the cloud records it, counts the gap and stops asking.
- Given the repository, when `pnpm run check:no-manifest` runs, then it finds no manifest route, table or ingest cursor.

## Spec Change Log

## Review Triage Log

## Design Notes

The cloud never learns the fork point on its own, so the Hub sends `fork_seq`. The unchanged cache after a hole is deliberate: it keeps the listing going each beat until the range is shipped, and the Hub's resend inbox is idempotent (the same range offered twice is one pass).

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-cloud -p arena-hub`; `cargo clippy` on those crates with `-D warnings`; `cargo fmt --check`; `pnpm run check:generated`; `pnpm run check:protocol`; `pnpm run check:no-manifest`.

## Auto Run Result

Status: in-review (implemented and verified; the review pass is the next iteration's).

- Summary: the paid Hub's heartbeat now carries its upload position (`last_seq_uploaded` of its own epoch, `last_upload_at`, `fork_seq`, and `resend_unavailable`). The cloud keeps a per-Hub view (`hubs.last_epoch_seen`/`last_seq_seen`, migration 0018). When the heartbeat agrees with it nothing is listed; when it differs the cloud lists `changes/<epoch>/`, finds the first hole and answers `resend{from, to}`. The uploader ships that range first, in batches under exact keys, without moving the cursor. Pruned rows are reported as `resend_unavailable`, recorded on the Hub row, counted and not asked for again. `pnpm run check:no-manifest` guards against a manifest route, table or ingest cursor.
- Files: `crates/arena-proto/src/pairing.rs` (+ generated TS); `crates/arena-store/control-migrations/0018_hub_upload_view.sql`, `src/control.rs`, `src/control/hubs.rs`, `src/upload.rs`; `crates/arena-cloud/src/{gaps,presign,auth,lib}.rs`, `src/api/heartbeat.rs`, `tests/gaps.rs`; `crates/arena-core/src/{snapshot,status}.rs`; `crates/arena-tasks/src/{heartbeat,upload,lib}.rs`; `crates/arena-hub/src/lib.rs`; `scripts/check-no-manifest.mjs` (+ test), `package.json`.
- Verification: `cargo fmt --check` and `cargo clippy -D warnings` clean on arena-proto/store/core/tasks/cloud/hub/api; new tests green (cloud `gaps` 10, gap finder units, store range/progress 5, tasks resend 6 and heartbeat 3); `check:protocol` and `check:no-manifest` ok. `check:generated` reads the git tree, so it is clean once the regenerated TS is committed.
- Failing before and after (not touched): `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-cloud` `binary::unreachable_at_start_…` and `backup::a_hung_run_times_out_with_its_own_code`; `arena-kiosk-svc` and `arena-e2e` do not compile on this Windows machine.
- Decisions: the epoch of a resend is the heartbeat's own (the answer is `{from, to}` as the story says); the Hub sends `fork_seq` so the cloud knows where an epoch starts; a run the Hub reported unavailable counts as held when looking for the next hole; the gap metric counts a hole once per distinct run, with a separate `unavailable` count, kept in process and logged (the Grafana exporter stays deferred with Story 2.3).
- Residual risks: each upload that moves the Hub's position costs one bucket listing on the next heartbeat (by design, the cloud has no other view); a listing failure or a missing data bucket is logged and treated as no gap; the real R2 listing path (`list_objects_v2`) is not exercised here (tests use an in-memory listing); the resend is picked up at the uploader's next cycle (up to one `upload_interval_s`), not immediately.

## Code Review (2026-10-30)

Fixed: two separate pruned runs overwrote each other in the single stored `resend_unavailable` (cloud view and Hub inbox), so the cloud kept asking; they are now merged (`ResendUnavailable::merged`, holes are handled in seq order). A partly filled resend (a shorter hole inside the last issued range) no longer counts as a new gap. Accepted as is: the unavailable report is acknowledged on any 200; one listing per position change; the Hub's range is trusted for the held set (authenticated Hub).
