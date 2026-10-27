---
title: 'Story 4.14: Ad requests and impressions are recorded without ids and sent with the heartbeat only'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_revision: '08d33084290ab4817a522f6cbff3ece956d72dd2'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-13-a-standard-interstitial-only-when-staff-come-back-from-the-i.md'
warnings:
  - 'oversized'
deferred: []
---

<intent-contract>

## Intent

**Problem:** The team cannot see ad revenue health or fill rate for Free branches, and Free branches upload nothing else.

**Approach:** The plugin's `ad.requested` / `ad.shown` events are forwarded by the UI to the Hub, which records one row each through `apply()` as a system intent (`ad.request_recorded`, `ad.impression_recorded`) with an idempotency key from the event. The heartbeat of a Free branch carries day-and-slot totals of the rows after a cursor; the cloud adds them to `ad_request_daily` / `ad_impression_daily` and stores the cursor in the same transaction; the acknowledged `through_id` in the response moves the Hub's cursor.

## Boundaries & Constraints

**Always:**
- Rows hold `id`, `slot`, `at` only: no device, creative, advertiser or advertising id, no tenant name in any metric.
- Totals are per business day and slot; a cursor moves only on the cloud's acknowledgement; counts are never lost or doubled.
- Free uploads no batch, summary or rollup and makes no presign call; `last_seq_uploaded` stays 0.

**Never:**
- No ad symbol in Rust (the Hub only knows the slot names `banner` / `interstitial`).
- No upgrade or lapse handling (Story 4.15). No OpenTelemetry/Grafana export (the exporter is Story 2.3's deferred work); the cloud only holds the totals and a query for fill rate.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| `ad.shown` / `ad.requested` | gate on | UI posts it once; one row |
| Same event id again | any | no second row |
| Heartbeat, Free | rows after cursor | `ad_impressions` / `ad_requests` `{through_id, counts}` |
| Heartbeat, paid or no rows | any | neither block sent |
| `through_id` ≤ stored cursor | cloud | nothing added; stored cursor returned |
| WAN down 3 days | rows over 3 days | next heartbeat sends each day once |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0043_ad_metrics.sql`, `src/ad.rs`, `lib.rs`, `migrations.rs` -- Hub tables (`ad_impressions`, `ad_requests`, two single-row cursors) and queries. Local bookkeeping outside the change log, like the licence cache.
- `crates/arena-proto` -- `AdSlot`, `AdImpressionRecorded`, `AdRequestRecorded`, two `IntentKind`s (system), `AdCounts` / `AdDayCount`, heartbeat request and response fields; `packages/ui/src/generated` via `pnpm gen:types`.
- `crates/arena-core/src/apply/ad.rs` + `mod.rs` / `mutate.rs` / `validate.rs` / `guards.rs`; `writer.rs` -- `ad_pending`, `ad_ack`.
- `crates/arena-api/src/ui_auth.rs` -- `POST /ui/ads/event`.
- `crates/arena-tasks/src/heartbeat.rs` -- build, send, acknowledge.
- `crates/arena-store/control-migrations/0015_ad_metrics.sql`, `src/control/ads.rs`, `control.rs`; `crates/arena-cloud/src/api/heartbeat.rs`.
- `apps/hub/src/lib/ads.ts`, `ad-metrics.ts` (new), `routes/+layout.svelte`; Kotlin `AdsConfig.kt` / `ArenaAds.kt` (event `id`).
- Tests: core apply, writer, heartbeat, store control, `ad-metrics.unit.spec.ts`, `ad-metrics.spec.ts`.

## Design Notes

- Ad rows are not in the change log: the intent is applied through `apply()` (permission, epoch and dedupe guards, one transaction) and writes no change record; replay need not rebuild them.
- The event id is a ULID minted by the plugin (the page makes one when absent) and is the intent's idempotency key.
- Business day comes from the branch locale when the heartbeat is built; at most 5 000 rows go in one heartbeat, the rest follow.
- Cursor is keyed by branch in the cloud; a replacement Hub restarting its ids is a known gap (deferred).

## Tasks & Acceptance

**Execution:**
- [x] Hub store -- migration, queries, schema test (no id columns)
- [x] Proto -- intents, counts, heartbeat fields, generated types
- [x] Core -- apply, writer `ad_pending` / `ad_ack`, tests (duplicate, 3-day outage)
- [x] API -- `POST /ui/ads/event`
- [x] Heartbeat -- Free-only blocks, acknowledgement, no presign
- [x] Cloud -- control migration, cursor-guarded totals, handler, fill-rate query
- [x] UI + plugin -- forward events with ids
- [x] README, deferred work

**Acceptance Criteria:** those of Story 4.14 in `epics.md`; the Grafana export is deferred with Story 2.3's.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test -p arena-core -p arena-store -p arena-proto`, `pnpm gen:types`, `pnpm --filter @arena/hub check`, hub Playwright, `check:strings`, `check:ads`, prettier.
