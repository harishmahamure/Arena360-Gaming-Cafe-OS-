# Change-list verification — 2026-10-23

Source: `user-change-list-verbatim.md`. Targets: LLD = `docs/Arena360 — Low-Level Design.md`, HLD = `docs/Arena360 — High-Level Design.md`, PRD = `docs/Arena 360 - Product.md`, UJ = `docs/Arena360 — User Journeys.md`. Spine revisions (Tauri, SvelteKit, no mirror, etc.) left intact.

**Totals:** 51 items — Present 31 · Applied now 16 · Not applicable 4.

## 1. Low-Level Architecture → LLD

| # | Result | Where / note |
| --- | --- | --- |
| 1.1 | Present | Cloud internals: roles `control|command|builder|jobs|all`, no ingester; `commands` in `gz_control`; Audit builder (role `builder`) |
| 1.2 | Present | Uploader PUTs `state/latest.json`; publisher writes `rollups/{date}.json` at close; rollup engine in `arena-core` |
| 1.3 | Applied now | New **R2 layout** paragraph after Presign: `state/latest.json`, `rollups/{yyyy-mm-dd}.json`, `snapshots/{yyyy-mm-dd}.db.zst`, `audit-cache/{yyyy-mm}.db.zst` (24 h lifecycle) |
| 1.4 | Present | Uploader: "There is no manifest call"; heartbeat `last_seq_uploaded`, epoch; list `changes/` prefix; presign unchanged |
| 1.5 | Present | `commands` has `branch_id TEXT NOT NULL`, index `commands_poll(branch_id, status, id)` in `gz_control`; `builder_jobs`; no `gz_b_*`, `upload_manifest`, `ingest_cursor` |
| 1.6 | Present | Owner API: `/live` from `state/latest.json`, `/summary` sums rollups, 30 s cache; audit via builder |
| 1.7 | Present | Heartbeat body `last_seq_uploaded, last_summary_at, last_rollup_date`; `resend{from,to}`; no manifest endpoint |
| 1.8 | Present | `GET /v1/branches/{id}/live` (with `as_of`), `/summary?range=` from rollups, `POST …/audit/build` + `GET …/audit?q=`; no `/changes` endpoint |
| 1.9 | Applied now | Heartbeat paragraph: `upload_interval_s` and `summary_interval_s` both Basic 900 / Premium 300; `rollup_schema` pinned per tier |
| 1.10 | Applied now | New "Build order, cost and explicitly absent" section: cost table (₹6/₹12, ₹7,000 at 1k, ₹64 L at 1M, Turso ≈ $45, PUTs ≈ 60%, cadence lever) |
| 1.11 | Applied now | Build order: step 3 "Uploader + summaries + rollups → Owner dashboards from files"; no ingester step; step 5 on-demand builder after commands (step 4) |
| 1.12 | Applied now | "Explicitly absent: continuous cloud mirror of branch data; …" |

## 2. HLD

| # | Result | Where / note |
| --- | --- | --- |
| 2.1 | Present | Decisions: "Files, mailbox, licence"; Owner reporting row |
| 2.2 | Present | Owner ↔ Arena 360 row: reads Hub-computed files; diagram unchanged |
| 2.3 | Present | No ingester row; Command relay owns `commands` in `gz_control`; Audit builder row; Hub → R2 carries `state/latest.json` + rollups, no manifest |
| 2.4 | Applied now | VPS subgraph "control plane, command API, builder"; Turso box "control DB only"; R2 box "batches, summaries, rollups"; Turso row scales "Developer → Scaler, control only" (spine AD says Developer plan, so kept instead of "Free"); libSQL self-hosting removed |
| 2.5 | Applied now | Diagram and stores already file-based (R2 prefix row lists `state/latest.json`, `rollups/`, `audit-cache/`); rule 3 reworded to "summaries carry `as_of`; rollups are schema-versioned and regenerable by the Hub" |
| 2.6 | Not applicable | "Unchanged" in the change list |
| 2.7 | Present | Cloud row "control, command, builder, jobs"; libsql only for `gz_control`; Data row SQLite + Turso control + R2 files |
| 2.8 | Present | Latency (owner) under 2 s from files, data age ≤ cadence, audit < 10 s; Cost under ₹10 per paid branch |
| 2.9 | Applied now | Isolation: "Per-branch R2 prefix; presigned URLs scoped …; owner API enforces the tenant on every object key"; per-branch DB wording removed |
| 2.10 | Present | No libSQL bullet; R2 bullet: largest line PUTs, cadence is the lever |
| 2.11 | Applied now | Staging: "no ingester; builder tested with a synthetic month"; rollout: "a Hub release carries any rollup schema bump; cloud must accept N and N−1" |
| 2.12 | Present | Column "R2 files + builder"; J5/J6 updated; J6 must-not-fail unchanged |
| 2.13 | Applied now | Turso ceiling risk already absent; two new risks present; libSQL runbook open item removed; rollup schema v1 sign-off present |

## 3. LLD

| # | Result | Where / note |
| --- | --- | --- |
| 3.1 | Applied now | Diagram uploader "(batches, summary, rollups)"; `arena-core` `domain::rollup`; `arena-tasks` publisher `summary` (timer) and `rollup` (daily at close) |
| 3.2 | Present | `rollup_log(date, schema, built_at, key)` |
| 3.3 | Present | `summary.publish` / `rollup.publish`, System, writes `rollup_log` only; `command.apply` unchanged |
| 3.4 | Applied now | Summary: in-memory floor state + 6 queries, never blocks `apply()`; Rollup: ~20 queries, emits with `schema`; regeneration oldest-first 10/min and shared presign already present |
| 3.5 | Not applicable | "Unchanged" |
| 3.6 | Applied now | Android Process row: rollup runs in the foreground service, under 5 s on 3 GB (test row already present) |
| 3.7 | Applied now | Owner API cache now per-(branch, month) with ETag on R2; presign `{batch_url, summary_url}`, resend, single commands table, builder (LRU 50 GB, 24 h) and metrics already present |
| 3.8 | Applied now | Owner app `state` row: summary store reads `/live` and `/summary?range=`, `as_of` from summary file; audit "Preparing audit…" state. `BranchSummaryProvider` (Flutter name) expressed as a Svelte store per the SvelteKit revision |
| 3.9 | Present | No `UPLOAD_GAP`; `ROLLUP_SCHEMA_OLD`, `AUDIT_BUILDING`, `AUDIT_BUILD_FAILED` present |
| 3.10 | Present | Migrations: rollup schema integer, N and N−1, Hub regenerates, no cloud deploy for new field |
| 3.11 | Applied now | Rollup row: fixed SQLite fixture → expected JSON, summary < 50 ms on reference PC; builder round-trip, 30-day sum-of-rollups, Android < 5 s already present; no libSQL replay row |

## 4. PRD

| # | Result | Where / note |
| --- | --- | --- |
| 4.1 | Present | Today row (Basic ≤ 15 min, Premium ≤ 5 min or live); Audit row "Preparing audit…", first results under 10 s |
| 4.2 | Present | Backup row: Basic 15-min, Premium 5-min batches |
| 4.3 | Present | Performance NFR: rollup at close under 5 s, summary under 50 ms |
| 4.4 | Present | Non-goals: arbitrary SQL reporting in the cloud over branch data |
| 4.5 | Present | Success metric: data age within tier target on 99% of opens |

## 5. User Journeys

| # | Result | Where / note |
| --- | --- | --- |
| 5.1 | Present | J5 Z-report row: next summary upload + day's rollup |
| 5.2 | Present | J6 change-logged row: next `state/latest.json` |
| 5.3 | Not applicable | "Index unchanged" |

## 6. Design system

| # | Result | Note |
| --- | --- | --- |
| 6 | Not applicable | "No change" (N/A items: 2.6, 3.5, 5.3, 6) |

## 7. Cost model (any doc citing cost)

| Figure | Result | Where |
| --- | --- | --- |
| Per paid branch ₹6 Basic / ₹12 Premium | Present | HLD NFR + scalability; PRD goal 5 and metric; LLD cost table (added under 1.10) |
| 1,000 tenants ≈ ₹7,000 | Present | HLD scalability; LLD cost table |
| 1M ≈ ₹64 L | Present | HLD scalability; LLD cost table |
| Turso at 1M ≈ $45 | Present | HLD scalability; LLD cost table |
| Largest line: batch + summary PUTs, 60% | Present | HLD scalability; LLD cost table |
| Lever: upload cadence, 30 s floor | Present | HLD NFR, scalability, risks; LLD cost table |

Old figures (₹5–7, ₹14,700, ₹1.7 Cr, $32k) appear in no doc.
