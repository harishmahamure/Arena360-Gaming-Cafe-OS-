# User change list — verbatim (supplied 2026-10-23)

1. Low-Level Architecture (the original Gamezone doc) — applies to docs/Arena360 — Low-Level Design.md
| # | Section | Change |
|---|---|---|
| 1.1 | Component inventory | removed Mirror ingester. changed Command API: commands table lives in gz_control, not per-branch DBs. new On-demand builder (same binary, role builder). |
| 1.2 | Hub internals, module table | changed uploader: also writes state/latest.json each cycle and rollups/{date}.json at close. new rollup engine in arena-core computing both from SQLite. |
| 1.3 | Change log and replication, R2 layout | new keys: tenants/{t}/branches/{b}/state/latest.json, .../rollups/{yyyy-mm-dd}.json, .../snapshots/{yyyy-mm-dd}.db.zst (unchanged), .../audit-cache/{yyyy-mm}.db.zst (builder output, 24 h lifecycle). |
| 1.4 | Upload protocol | removed POST /uploads/manifest. changed gap detection: Hub sends last_seq_uploaded and epoch in heartbeat; cloud lists the branch's changes/ prefix when the two disagree. Presign unchanged. |
| 1.5 | Cloud services | removed per-branch mirror DBs gz_b_* and the Ingestion paragraph. changed commands schema gains branch_id TEXT NOT NULL and index (branch_id, status, id); lives in gz_control. removed upload_manifest and ingest_cursor tables. new builder_jobs(branch_id, month, state, built_at, key). |
| 1.6 | Mirror read API | changed to "Owner read API": fetches state/latest.json per branch and rollups/*.json per range from R2, sums in memory, caches 30 s. Audit endpoints call the builder. |
| 1.7 | APIs, Hub → cloud table | removed POST /uploads/manifest. changed POST /heartbeat body adds last_seq_uploaded, last_summary_at, last_rollup_date; response may add resend{from,to}. |
| 1.8 | APIs, Owner table | changed GET /branches/{id}/summary reads rollups; new GET /branches/{id}/live returns state/latest.json with as_of; changed GET /branches/{id}/changes becomes POST /branches/{id}/audit/build + GET /branches/{id}/audit?q= served from the built DB. |
| 1.9 | Tiers and flags | changed upload_interval_s: Basic 900, Premium 300 (was 300 / 60). new summary_interval_s (same values). new rollup_schema version pinned per tier so a Hub knows which fields to emit. |
| 1.10 | Cost | replaced with the C cost model (≈₹7k at 1k tenants, ≈₹64 L at 1M; PUTs 60% of bill; Turso ≈ $0). |
| 1.11 | Build order | step 3 changed to "uploader + summaries + rollups → Owner dashboards from files"; step "ingester" removed; new step "on-demand builder for audit" after commands. |
| 1.12 | Explicitly absent list | add "continuous cloud mirror of branch data". |

2. High-Level Design
| # | Section | Change |
|---|---|---|
| 2.1 | Decisions table | changed "What the cloud does": mirror → "files, mailbox, licence". new row: "Owner reporting: Hub-computed rollups on R2, no cloud SQL over branch rows". |
| 2.2 | System context | text only: owner reads files. Diagram unchanged. |
| 2.3 | Component view | removed Mirror ingester row. changed Command API "owns" → commands in gz_control. new On-demand builder row. Interfaces table: removed "Hub → R2 manifest"; new "Hub → R2 summaries and rollups". |
| 2.4 | Deployment view | changed VPS subtitle to "control plane, command API, builder"; Turso box to "control DB only"; R2 box to "batches, summaries, rollups". Table: VPS scales by "a second box for builder load", Turso row "Free → Scaler, control only". |
| 2.5 | Data architecture | changed diagram: "Ingester → Turso mirror" boxes become "R2 summaries/rollups → Owner API". Stores table: removed branch mirrors row; new rows for state/latest.json, rollups/, audit-cache/. Rules: rule 3 reworded "summaries carry as_of; rollups are schema-versioned and regenerable by the Hub". |
| 2.6 | Integrations | unchanged. |
| 2.7 | Technology stack | changed cloud row: "control plane, command API, builder"; removed libsql-for-mirrors mention; Data row: "SQLite (branch), Turso control DB, R2 files". |
| 2.8 | NFRs | changed "Latency (owner)": dashboard under 2 s from files; data age ≤ cadence; audit first query under 10 s. changed Cost: under ₹10 per paid branch. |
| 2.9 | Security | changed Isolation row: "per-branch R2 prefix; presigned URLs scoped; owner API enforces tenant on every object key". Remove "per-branch Turso DB". |
| 2.10 | Scalability | already revised; remove the libSQL self-hosting bullet; add "cost is linear in PUTs; cadence is the lever". |
| 2.11 | Environments | changed staging: no ingester; builder tested with a synthetic month. Rollout: Hub release carries rollup schema bumps; cloud must accept schema N and N−1 rollups. |
| 2.12 | Traceability | journeys 5, 6, 7: "Ingester + mirror" column → "R2 files"; journey 6 "must not fail" unchanged. |
| 2.13 | Risks | removed Turso plan ceiling. new "Rollup schema lags a report request" (mitigation: Hub regenerates history; schema versioned). new "Audit builder load spikes" (cache 24 h; add VPS). Open items: remove libSQL runbook; add rollup schema v1 field list sign-off. |

3. Low-Level Design
| # | Section | Change |
|---|---|---|
| 3.1 | Hub module map | diagram: uploader subtitle → "batches, summary, rollups". Crates: arena-core gains domain::rollup; arena-tasks gains summary (timer) and rollup (daily at close). |
| 3.2 | Domain schema | new table rollup_log(date TEXT PRIMARY KEY, schema INTEGER, built_at INTEGER, key TEXT) so the Hub knows which days to regenerate after a schema bump. No other change. |
| 3.3 | Intent catalogue | new System intents summary.publish and rollup.publish (no writes except rollup_log; exist so they appear in the audit trail). command.apply unchanged. |
| 3.4 | Hub engines | new Summary engine: every summary_interval_s, build state/latest.json from in-memory floor state + 6 queries; PUT to presigned URL; never blocks apply. new Rollup engine: at closing_time + 30 min or on shift.close of the last shift, run ~20 queries, emit rollups/{date}.json with schema; on startup, regenerate any date where rollup_log.schema < current oldest-first, rate-limited to 10/min. Uploader: changed cadence floor stays 30 s; summary and batch share one presign call returning two URLs. |
| 3.5 | Kiosk | unchanged. |
| 3.6 | Pi / Android | Android Hub: rollup runs as part of the foreground service; confirm it completes under 5 s on 3 GB device (test plan). |
| 3.7 | Cloud internals | removed Uploads and ingester block. new uploads::presign returns {batch_url, summary_url}; heartbeat compares last_seq_uploaded to R2 listing, returns resend on gap. changed Commands: single table in gz_control; sweeper unchanged. new builder: job queue in builder_jobs; worker downloads snapshot + batches for the month, replays into a temp SQLite, uploads audit-cache/{month}.db.zst, serves queries by downloading the cache to local disk (LRU 50 GB); 24 h lifecycle rule on the prefix. changed Owner API: reports module reads rollups with a per-(branch, month) in-memory cache and an ETag check on R2; live module reads state/latest.json. removed Metrics: ingest lag; new summary age per branch, rollup missing-day count, builder queue depth. |
| 3.8 | Owner app | changed BranchSummaryProvider reads /live (summary) and /summary?range= (rollups); as_of shown from the summary file. Audit screen gains a "Preparing audit…" state for the first build. |
| 3.9 | Error codes | removed UPLOAD_GAP as an API error (now a heartbeat resend). new ROLLUP_SCHEMA_OLD, AUDIT_BUILDING, AUDIT_BUILD_FAILED. Events: unchanged. Metrics: as 3.7. |
| 3.10 | Migrations | new paragraph: rollup schema integer; cloud accepts N and N−1; Hub regenerates on bump; adding a field never requires a cloud deploy, only a Hub release and an Owner-app release to display it. |
| 3.11 | Test plan | removed replay-on-libSQL row. new rows: rollup golden tests (fixed SQLite fixture → expected JSON); summary under 50 ms on reference PC; builder round-trip (snapshot + batches → DB → checksum equals Hub DB); Owner API sum-of-rollups equals Hub Z-report totals for 30 synthetic days; Android rollup under 5 s. |

4. PRD — 4.1 dashboards data age (Basic ≤ 15 min, Premium ≤ 5 min or live; audit first query up to 10 s with progress); 4.2 Backup row Basic 15-min, Premium 5-min; 4.3 Performance: Hub rollup at close < 5 s, summary < 50 ms; 4.4 Non-goal: arbitrary SQL reporting in the cloud over branch data; 4.5 Success metric: dashboard data age within tier target 99% of opens.

5. User Journeys — 5.1 J5 Z-report reaches owner via next summary + day rollup; 5.2 J6 new rate appears in next state/latest.json; 5.3 index unchanged.

6. Design system — no change.

7. Cost model
| Figure | Old | New |
|---|---|---|
| Per paid branch, cloud | ₹5–7 | ₹6 Basic, ₹12 Premium |
| 1,000 tenants, cloud | ₹14,700 | ₹7,000 |
| 1M tenants, cloud | ₹1.7 Cr | ₹64 L |
| Turso at 1M | $32k | ~$45 |
| Largest line | Turso writes / fast PUTs | Batch + summary PUTs, 60% |
| Lever | — | Upload cadence; 30 s floor |
