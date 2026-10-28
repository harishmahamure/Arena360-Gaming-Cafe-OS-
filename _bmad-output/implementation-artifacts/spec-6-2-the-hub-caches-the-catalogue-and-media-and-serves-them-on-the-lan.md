---
title: 'Story 6.2: The Hub caches the catalogue and media and serves them on the LAN'
type: 'feature'
created: '2026-10-29'
status: 'in-review'
review_loop_iteration: 0
followup_review_recommended: false
baseline_commit: 'cb2be672febe6d44c1c06189348872be24fe39bc'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A Hub has no copy of the global game catalogue or its art, so kiosks would have nothing to show and the WAN would be needed for every cover.

**Approach:** The primary Hub pulls the Story 6.1 manifest at start and every 6 hours into its own reference database (`<data_dir>/reference/reference.db`, never the branch store, change log or `apply()`), downloads covers first then videos into `<data_dir>/reference/media/<sha256>` verified by SHA-256, evicts videos under a cap, and serves files to paired kiosks at `GET /media/<sha256>` on `:7401`. Settings → System shows used and cap sizes and lets a Manager change the cap.

## Boundaries & Constraints

**Always:**

- Reference data (`ref_games`, `ref_signatures`, `ref_media`, `ref_files`, `ref_meta`) lives only in `reference.db`; nothing of it is written through the writer, so nothing reaches the change log, batches or snapshots.
- The pull asks `since = stored catalog_version` with the Hub device token (as the heartbeat does). 304 or an unchanged version changes nothing. A live game replaces its row, signatures and media refs; a tombstone deletes them. Files no game references any more are deleted from disk and the table. Only a `Primary` Hub that is paired and not revoked pulls. `TokenInvalid`/`TokenRevoked` marks the pairing revoked as the other tasks do.
- Covers for every game are downloaded first and are never evicted; videos follow best-effort. Each download streams to `<sha>.part`, is checked for size and SHA-256 against the manifest, then renamed; a mismatch or short read discards the part file and leaves the row pending for the next pull. No bearer token is ever sent to the media URL; redirects are refused; https is required unless the cloud endpoint itself is plain-http loopback (dev/tests).
- Cap = setting `system.media_cap_gb` (integer 1–10000, default 10 = GiB; seeded like the other defaults, edited with `settings.set` under `settings.edit`). Stored videos plus a new one may not exceed `cap − stored cover bytes`: before storing a video the least-recently-served stored videos (by `last_served_ms`, then `stored_at`) are deleted until it fits; a video that cannot fit even with no other video is skipped and marked `evicted` so it is not retried every pull; `evicted` flags are cleared when the cap grows above the cap seen at the last pull. Covers may exceed the cap.
- `GET /media/<sha256>` on `:7401`: TLS only, `Authorization: Bearer <device token>` verified as a `paired` device (shared verify permits and failure throttle with `hello`; revoked or unknown token is 401). Plain HTTP, no header or a bad token is 401 with no body detail. A non-hex or unknown hash, a row not yet downloaded, or a missing file is 404 straight away. 200 sends the file in chunks with `Cache-Control: public, max-age=31536000, immutable`, `ETag: "<sha>"`, `X-Content-Type-Options: nosniff`, a `Content-Type` from the manifest URL's extension, and `Accept-Ranges: bytes`; one `Range: bytes=a-b` is honoured with 206, an unsatisfiable one is 416. A served file updates `last_served_ms`.
- While the WAN is down or the manifest call fails, everything already cached is served; no error is shown to players; the next successful pull resumes the pending downloads. A failing pull retries with the heartbeat back-off (ceiling 30 min), a good one waits 6 h.
- `GET /ui/media-cache` (staff session) answers `{used_bytes, cover_bytes, video_bytes, cap_bytes, catalog_version, games}`; the Settings → System screen shows "Used X of Y" and edits the cap.

**Never:** writing reference data through `apply()` or the change log; evicting a cover; trusting the URL's content over the manifest hash; a Hub call that reaches the internet from a kiosk; standby/restore cache filling (Story 9.20); library or matching logic (6.3, 6.5).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First pull | empty cache, manifest with 2 games, 2 covers, 1 video | rows stored, covers then video downloaded and verified, version stored | n/a |
| Unchanged | cloud answers 304 | nothing changes; pending downloads still resumed | n/a |
| Hash mismatch | body hash differs from manifest | part file deleted, row stays pending, retried next pull | warn log, back-off |
| Tombstone | game deleted upstream | rows removed; its files deleted unless another game uses them | n/a |
| Over cap | cap 1 GiB, stored videos fill it | oldest-served videos deleted to fit the new one; covers untouched | oversized video skipped and flagged |
| WAN down | manifest unreachable | cached files still served; pending left; retry with back-off | no player-visible error |
| Serve | valid token + stored hash | 200 + immutable headers, `last_served_ms` set | — |
| Serve refused | no/bad/revoked token, plain HTTP | 401 | token failures throttled |
| Serve unknown | stored-less or malformed hash | 404 | — |

</intent-contract>

## Code Map

- `crates/arena-store/src/reference.rs` -- NEW `ReferenceDb` (rusqlite, own file): schema, `apply_manifest`, pending/lookup/usage/eviction queries. Export from `lib.rs`.
- `crates/arena-core/src/media_cache.rs` -- NEW `MediaCache` (db + `media/` dir + cap callback): apply a manifest and delete orphan files, `serve_lookup`, `commit_download`, `make_room`, `status`.
- `crates/arena-tasks/src/catalog.rs` -- NEW `CatalogPullTask` (model: `template.rs` gating, token via `Keystore`, back-off from `heartbeat.rs`).
- `crates/arena-tasks/src/cloud.rs` -- `CloudClient::catalog_manifest` (GET, `Option` for 304) and `fetch_media` (streamed to a file, SHA-256 on the fly, no bearer). `lib.rs` exports.
- `crates/arena-api/src/lan/listener.rs`, `mod.rs` -- `Route::Media`, `answer_media` (TLS web path; plain gets 401); `LanHub::with_media`. Model: `answer_hub_info`, `session.rs` hello token verify (~l.179–215).
- `crates/arena-api/src/ui_auth.rs` -- `UiAuth::with_media_cache`, `GET /ui/media-cache` next to `settings`.
- `crates/arena-hub/src/lib.rs`, `config.rs` -- build `MediaCache` under `data_dir/reference`, spawn the task, pass to `LanHub` and `UiAuth`, abort on shutdown.
- `crates/arena-proto/src/operating.rs` (+ `lib.rs`) -- `KEY_MEDIA_CAP_GB`, `DEFAULT_MEDIA_CAP_GB`, `MediaCacheStatus` (ts-rs).
- `crates/arena-core/src/settings_defaults.rs` -- default, `Integer{1,10000}` rule, `KEYED_FIELD_KEYS` entry.
- `apps/hub/src/routes/settings/system/` (+ `lib/`, `packages/ui/src/messages/en.json`, generated types) -- "Game media cache" section.
- Tests: unit tests beside each new module; `crates/arena-hub/tests/` for the end-to-end serve + isolation; existing counts in `settings_defaults` tests.

## Tasks & Acceptance

**Execution:**
- [x] `arena-store/src/reference.rs` -- reference DB and queries
- [x] `arena-core/src/media_cache.rs` -- cache logic, eviction, serving lookup, status
- [x] `arena-proto` + `settings_defaults.rs` -- cap setting, status type
- [x] `arena-tasks/src/{cloud,catalog,lib}.rs` -- manifest call, streamed verified download, 6-hourly pull task
- [x] `arena-api` -- `/media/<sha256>` route, `/ui/media-cache`
- [x] `arena-hub` -- wire up and shut down
- [x] Hub UI -- Settings → System "Game media cache" (used of cap, cap field)
- [x] Tests for every AC and I/O row

**Acceptance Criteria:**
- Given the Hub with the WAN up, when it starts and every 6 hours after, then it pulls the manifest from its last `catalog_version` and stores games, signatures and media refs in the separate reference database, never through `apply()`.
- Given manifest entries, when the cache fills, then covers are downloaded first and always kept, videos best-effort afterwards, each verified against its SHA-256, and a mismatch is discarded and retried on the next pull.
- Given the media cap, when stored media would exceed it, then videos are evicted least-recently-served first, covers never, and the Settings screen shows used and cap sizes.
- Given a paired kiosk, when it requests `GET /media/<sha256>` on `:7401` with its device token, then the Hub serves the file with immutable cache headers; an unknown or not-yet-downloaded hash is 404 quickly; an unauthenticated request is refused.
- Given the WAN is down or the manifest call fails, when kiosks request media, then everything cached is served, no player-visible error appears, and the next successful pull resumes where it stopped.
- Given the reference cache, when the change log, branch batches and snapshots are inspected in a test, then nothing of the cache appears in them.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check` -- expected: clean
- `cargo clippy -p arena-store -p arena-core -p arena-tasks -p arena-api -p arena-hub -p arena-proto --all-targets -- -D warnings` -- expected: no warnings
- `cargo test -p arena-store -p arena-core -p arena-tasks -p arena-api -p arena-hub -p arena-proto` -- expected: pass
- `pnpm --filter ./apps/hub check` and the hub tests -- expected: pass
