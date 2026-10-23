# User-supplied change list — "No mirror" redesign (2026-10-23)

Supplied by Harish in the architecture coaching session. Authoritative direction for the spine and for updating LLA/HLD/LLD/PRD/Journeys.

## Core
- No continuous cloud mirror of branch data. Cloud = files, mailbox (commands), licence.
- Hub computes owner reporting: `state/latest.json` (summary, every summary_interval_s) and `rollups/{yyyy-mm-dd}.json` (daily, at close) in arena-core rollup engine; schema-versioned and regenerable by the Hub.
- R2 layout per branch: `changes/` batches (unchanged), `state/latest.json`, `rollups/{date}.json`, `snapshots/{date}.db.zst` (unchanged), `audit-cache/{yyyy-mm}.db.zst` (builder output, 24 h lifecycle).
- Removed: mirror ingester, per-branch mirror DBs `gz_b_*`, `POST /uploads/manifest`, `upload_manifest` and `ingest_cursor` tables, `UPLOAD_GAP` API error (now heartbeat resend).
- Gap detection: Hub heartbeat sends `last_seq_uploaded`, `epoch`, `last_summary_at`, `last_rollup_date`; cloud lists the branch `changes/` prefix when they disagree; response may include `resend{from,to}`.
- Presign returns `{batch_url, summary_url}` in one call.
- Commands: single `commands` table in `gz_control` with `branch_id TEXT NOT NULL`, index `(branch_id, status, id)`; sweeper unchanged.
- On-demand builder (same binary, role `builder`): `builder_jobs(branch_id, month, state, built_at, key)`; downloads snapshot + month batches, replays into temp SQLite, uploads `audit-cache/{month}.db.zst`, serves queries from local LRU cache (50 GB).
- Owner read API: `/branches/{id}/live` → `state/latest.json` with `as_of`; `/branches/{id}/summary?range=` → sums rollups in memory, 30 s cache, ETag check; audit = `POST /branches/{id}/audit/build` + `GET /branches/{id}/audit?q=`.
- Tiers: `upload_interval_s` Basic 900, Premium 300 (was 300/60); `summary_interval_s` same values; `rollup_schema` version pinned per tier.
- Hub schema: `rollup_log(date TEXT PRIMARY KEY, schema INTEGER, built_at INTEGER, key TEXT)`; regenerate days where schema < current, oldest first, max 10/min on startup.
- Intents: system `summary.publish`, `rollup.publish` (write only rollup_log; appear in audit).
- Rollup trigger: closing_time + 30 min or `shift.close` of last shift.
- Migrations: rollup schema integer; cloud accepts N and N−1; adding a field needs only Hub + Owner app releases, no cloud deploy.
- Error codes new: `ROLLUP_SCHEMA_OLD`, `AUDIT_BUILDING`, `AUDIT_BUILD_FAILED`.
- Metrics: removed ingest lag; new summary age per branch, rollup missing-day count, builder queue depth.
- NFRs: owner dashboard < 2 s from files; data age ≤ cadence (Basic ≤ 15 min, Premium ≤ 5 min or live); audit first query < 10 s with progress state; Hub rollup at close < 5 s on reference hardware; summary < 50 ms; Android rollup < 5 s on 3 GB device; cost < ₹10 per paid branch.
- PRD: Backup row Basic 15-min, Premium 5-min batches; non-goal "arbitrary SQL reporting in the cloud over branch data; reports are defined fields in the rollup"; success metric "owner dashboard data age within tier target 99% of opens".
- Tests: rollup golden tests; summary < 50 ms; builder round-trip checksum equals Hub DB; Owner API sum-of-rollups equals Hub Z-report totals for 30 synthetic days; Android rollup < 5 s.
- Risks: remove Turso plan ceiling; add "rollup schema lags a report request" (Hub regenerates; schema versioned) and "audit builder load spikes" (24 h cache; add VPS). Open item: rollup schema v1 field list sign-off.
- Cost model: per paid branch ₹6 Basic / ₹12 Premium; 1k tenants ≈ ₹7,000; 1M ≈ ₹64 L; Turso ≈ $45 at 1M (control only); largest line batch + summary PUTs (60%); lever = upload cadence (30 s floor).
- Journeys: J5 owner sees Z-report via next summary + day rollup; J6 new rate appears in next `state/latest.json`.
- Design system: no change.

Full section-by-section edit list (LLA 1.1–1.12, HLD 2.1–2.13, LLD 3.1–3.11, PRD 4.1–4.5, Journeys 5.1–5.3, cost table 7) is reproduced verbatim in the session transcript and summarised above.
