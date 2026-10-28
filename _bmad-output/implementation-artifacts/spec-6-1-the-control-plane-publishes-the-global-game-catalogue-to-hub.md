---
title: 'Story 6.1: The control plane publishes the global game catalogue to Hubs'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: 'd0e59dc'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
warnings: []
deferred:
  - 'The manifest is one response with no paging; a catalogue of a few thousand games is a few hundred KB. Paging arrives if the catalogue outgrows that.'
  - 'Two `catalog import` runs at the same moment can both read the old rows; the version bump and each row write are one transaction, but the diff is read before it. Imports are an operator action, one at a time.'
  - 'The S3/R2 upload and the CDN base URL were exercised against the in-memory store only; sqld-backed migration and dump tests skip when no sqld is installed.'
---

<intent-contract>

## Intent

**Problem:** Hubs have no source of game art: there is no catalogue in `gz_control`, no way to load one, and no endpoint a Hub can read it from.

**Approach:** Migration 17 adds `catalog_games`, `catalog_exe_signatures`, `catalog_media`, a single-row `catalog_state` version counter and the append-only `admin_audit`. `arena-cloud catalog import <file>` loads a JSON or CSV list and its media idempotently. `GET /v1/catalog/manifest?since=<version>` serves a paired Hub the published changes since that version.

## Boundaries & Constraints

**Always:**

- `catalog_version` is `catalog_state.version`. It moves by one in the same transaction as any change to a game that is, or becomes, published (publish, unpublish, delete, revive, edit, signature or media change). A re-import of an identical file changes nothing and bumps nothing. Each changed row stores that version as `changed_version`; a game's `rev` goes up by one per change.
- Games are keyed by trimmed, case-insensitive name + launcher (unique index). Signatures in the file replace the game's set; an omitted cover or video leaves the stored one.
- Media are immutable objects `catalog/<sha256>.<ext>` in the object store, uploaded once by hash (skipped when the key is present). Covers: jpg, jpeg, png, webp, up to 5 MiB; videos: mp4, webm, up to 100 MiB.
- Manifest: `since` omitted = 0. A game with `changed_version > since` appears live (published, not deleted) with its signatures and media, or as a tombstone (`deleted: true`, id and rev only; not sent when `since` is 0). A video is listed only when `flash_screened`; the cover always. `since >= version`, or a matching `If-None-Match`, answers 304 with the `ETag` (`"<since>-<version>"`).
- Auth is the Hub device token (`Authorization: Bearer hub_…`): none or unknown is 401 `TOKEN_INVALID`, a revoked Hub is 403 `FORBIDDEN`, over 30 calls per tenant per minute is 429 `RATE_LIMITED`. Auth and the limit run before any catalogue read.
- `admin_audit` rows (ULID id, `at`, `actor_kind` admin|system, `actor_id`, `action`, `target_kind`, `target_id`, `reason`, `summary`, `request_id`) can only be appended: triggers abort every UPDATE and DELETE.
- `catalog import` writes its games, version bump and one `admin_audit` row (`actor_kind = admin`, action `catalog.import`, the file name and counts in `summary`) in one transaction.

**Never:** an unscreened video in a manifest; a Hub call that needs a cookie; secrets or player data in `admin_audit.summary`; any change to Hub schemas.

## Code Map

- `crates/arena-store/control-migrations/0017_catalog.sql`, `src/control.rs` -- the tables, the triggers, the migration entry.
- `crates/arena-store/src/control/admin_audit.rs` -- `NewAdminAudit`, `append_admin_audit`, `admin_audit` reads for tests.
- `crates/arena-store/src/control/catalog.rs` -- `catalog_version`, `catalog_import_state`, `apply_catalog_import`, `catalog_manifest_rows`.
- `crates/arena-proto/src/catalog.rs` -- `CatalogManifest`, `CatalogGame`, `CatalogSignature`, `CatalogMedia` (shared with the Hub's cache in Story 6.2).
- `crates/arena-cloud/src/catalog.rs` -- import file parsing (JSON and CSV), the import plan, media upload and the manifest builder.
- `crates/arena-cloud/src/api/catalog.rs` -- the manifest route.
- `crates/arena-cloud/src/config.rs`, `main.rs`, `ops.rs` -- `catalog import <file> --operator <name> [--media-dir <dir>] [--dev [--data-dir <dir>]]`.
- Tests: `arena-store/tests/control.rs` and `arena-cloud/tests/backup.rs` (table lists), `arena-cloud/tests/catalog.rs` (API, import, audit).

## Tasks & Acceptance

- [x] Migration 17 with the catalogue tables, `catalog_state`, `admin_audit` and its append-only triggers
- [x] Store module for import, manifest rows and audit append
- [x] Shared manifest types in `arena-proto`
- [x] Import parsing (JSON, CSV), plan and idempotent apply with media uploaded once by hash
- [x] `GET /v1/catalog/manifest` with ETag, 304, auth and the per-tenant limit
- [x] `arena-cloud catalog import` command
- [x] Tests for every acceptance criterion below

**Acceptance Criteria:**
- Given `gz_control`, when migration 17 runs, then the three catalogue tables, the version counter and `admin_audit` exist, and an UPDATE and a DELETE on `admin_audit` both fail and leave the row unchanged.
- Given a paired Hub, when it calls the manifest with `since`, then it gets only the published games changed since then (tombstones for unpublished or deleted ones, none when `since` is 0) with signatures, media refs and the current version, an ETag, and 304 when nothing changed.
- Given a video that is not flash-screened, then it is absent from the manifest while the game's cover is present.
- Given stored media, then it sits at `catalog/<sha256>.<ext>`, the manifest gives its CDN URL, hash and size, and a second import uploads nothing again.
- Given `catalog import`, then games, signatures and media are upserted by name + launcher, the version moves once, and one `admin_audit` row with the file name and counts is written in the same transaction; a second identical run changes nothing.
- Given no token, a revoked token or too many calls, then the manifest answers 401, 403 or `RATE_LIMITED` and returns no catalogue data.

## Verification

- `cargo fmt --check`, `cargo clippy -p arena-cloud -p arena-store -p arena-proto --all-targets -- -D warnings`, `cargo test -p arena-cloud -p arena-store -p arena-proto`.

</intent-contract>
