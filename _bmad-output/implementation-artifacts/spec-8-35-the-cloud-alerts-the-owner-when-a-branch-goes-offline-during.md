---
title: "Story 8.35: The cloud alerts the owner when a branch goes offline during opening hours"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: 'e2abd132cf436c49fddeb175980eb60be638d7b3'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The cloud knows when a paid Hub last sent a heartbeat or an upload, but nobody tells the owner when a branch has been unreachable, or has stopped backing up, while it should be open.

**Approach:** The paid heartbeat carries the branch's opening hours and timezone, which the cloud caches. A new jobs task `offline-watch` (every 5 minutes) raises the 8.33 owner alert `branch_offline` or `branch_not_uploading` once per episode, adds a push-only `branch_online` entry when the heartbeat returns, and holds all alerts and pages the on-call when too many branches would alert at once.

## Boundaries & Constraints

**Always:**
- `arena-proto`: `BranchOpening {opening_hours: String (the `branch.opening_hours` JSON), timezone: String}`; `HeartbeatRequest.opening: Option<BranchOpening>` (omitted when `None`); `OwnerAlertKind::BranchNotUploading` (param `minutes`) and `BranchOnline` (param `minutes` = how long it was offline), each with its plain-text line and the `#today-<branch>` link; neither is `hub_sourced`. `opening_window_start(week, tz, now_ms) -> Option<i64>`: the start (ms) of the opening window that contains `now_ms`, `None` when closed (windows that wrap midnight and the previous day's window count).
- The Hub sends `opening` on every heartbeat of a paid tier (the stored opening hours, else the default, and the effective timezone); never on Free. The cloud stores it only when it parses (`parse_opening_hours`, an IANA zone, JSON at most 1 KiB), only on a paid tier, and writes nothing when it is unchanged. A bad block is ignored and never fails the heartbeat.
- Control migration 24: `branch_opening (branch_id, opening_hours, timezone, updated_at)` and `branch_watch (branch_id, offline_key, upload_key)` where a key is the `last_seen_at` / newest-upload value of the episode that was alerted.
- `arena-cloud --role jobs --task offline-watch` runs in k8s every 5 minutes and uses the same `AuthState` as the web role (`auth_state` over `auth_from_env`) so alerts push and send exactly as in 8.33. For each branch with a live (not revoked) Hub on a paid tier, with `now` = cloud time:
  - **Offline**: `last_seen_at` is at least 30 min old, hours are cached, `now` is inside an opening window starting at `w`, and `now >= max(last_seen_at, w) + 30 min`; raise `branch_offline` (`minutes` = `now - max(last_seen_at, w)` in minutes) under id `off-<branch>-<last_seen_at>`, then set `offline_key = last_seen_at`. A branch whose `offline_key` already equals `last_seen_at` is not alerted again.
  - **Not uploading**: heartbeat is younger than 30 min, `newest = max(last_upload_at, last_summary_at)` exists, and the same window rule holds for `newest`; raise `branch_not_uploading` under `up-<branch>-<newest>` and set `upload_key = newest`. A later upload changes `newest`, so a later episode is a new alert; the key is cleared when uploads are fresh.
  - **Back online**: `offline_key` is set and `last_seen_at > offline_key`; add `branch_online` under `on-<branch>-<offline_key>` with push only (no WhatsApp) and clear `offline_key`, then evaluate the branch again.
- **Mass hold**: if the branches that would alert (offline or not uploading) number at least 5 and are more than 20% of the paid branches with a live Hub, none is raised or recorded; the on-call is paged through `ARENA_ALERT_WEBHOOK_URL` (text names env, task and count) and the job still succeeds. The next run re-evaluates.
- `owner_alerts::Raise` gains `whatsapp: bool`; the dev fixture and `/v1/notify` pass `true`. A failed raise of one branch is logged and does not stop the others or record its key.

**Never:** A Free branch alert, an alert for a branch with no cached hours, quiet hours, digests, per-branch thresholds, a change to how the Hub or its heartbeat cadence work for Free, personal data in an alert.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Offline in hours | No heartbeat for 35 min, open since 2 h | One `branch_offline`; a rerun adds none | No error |
| Offline outside hours | Silent all night, opens 09:00 | Alert at 09:30 | No error |
| Not uploading | Heartbeat fresh, upload and summary 35 min old in hours | One `branch_not_uploading`; an upload then a new stall alerts again | No error |
| Back online | Alerted, then a heartbeat | One push-only `branch_online`; a new outage alerts again | No error |
| Free or no hours | Free tier, or hours never sent | Nothing | No error |
| Mass outage | 5 of 10 paid branches silent | Nothing raised, on-call paged | Job exits 0 |
| Bad hours block | Unparseable hours or zone | Heartbeat succeeds, cache unchanged | Ignored |

</intent-contract>

## Code Map

- `crates/arena-proto/src/pairing.rs` (`HeartbeatRequest`), `operating.rs` (window helper), `owner_alert.rs` (two kinds) -- shared types; TS in `packages/ui/src/generated/`.
- `crates/arena-core/src/writer.rs` -- `branch_opening()` read (settings, branch locale, `locale::resolve` as in `session_screen::closing_in`); `crates/arena-tasks/src/heartbeat.rs` (`exchange`) sends it on paid tiers.
- `crates/arena-store/control-migrations/0024_branch_watch.sql`, `src/control.rs` (migration list and version), new `src/control/branch_watch.rs` -- cache, watch rows, candidate query (branches ⨝ live hub ⨝ `hub_upload_view` ⨝ entitlement ⨝ opening ⨝ watch).
- `crates/arena-cloud/src/api/heartbeat.rs` -- store `opening`; new `src/offline_watch.rs`; `config.rs` (`Task::OfflineWatch`, usage), `backup.rs` (`run_job`, codes), `owner_alerts.rs` (`Raise.whatsapp`), `api/dev.rs`, `api/notify.rs`; `deploy/k8s/base/cronjob-offline-watch.yaml` and `kustomization.yaml`.
- Tests: `crates/arena-cloud/tests/owner_alerts.rs` (harness: `paid`, `pair_hub`, `alerts`), `config.rs`, proto unit tests, `arena-tasks` heartbeat test.

## Tasks & Acceptance

**Execution:**
- `crates/arena-proto/...` -- types, kinds, window helper -- one definition for Hub, cloud and app
- `crates/arena-core/src/writer.rs`, `arena-tasks/src/heartbeat.rs` -- Hub sends opening hours and zone when paid
- `crates/arena-store/...` -- migration 24, cache and watch queries
- `crates/arena-cloud/...` -- heartbeat cache, `offline-watch` task, `Raise.whatsapp`, config, cronjob manifest
- Tests -- matrix rows (cloud), window helper (proto), heartbeat block (tasks)

**Acceptance Criteria:**
- Given a paid Hub, when it sends a heartbeat, then the cloud holds its opening hours and timezone for the branch and a Free Hub's heartbeat stores none.
- Given the matrix cases, when `offline-watch` runs, then each raises exactly the alerts shown and a repeat run raises none.
- Given typecheck, lint, Rust tests and `pnpm check:generated`, `check:strings`, they pass.

## Spec Change Log

- Matrix row "Mass outage" said 3 of 10; the Always rule (at least 5 and more than 20%) is the design, so the row now reads 5 of 10 (planning error, found at implementation).

## Review Triage Log

## Design Notes

An episode key is the timestamp that started the silence (`last_seen_at` or the newest upload), so the alert id is deterministic and the whole raise is idempotent even if the state write fails. Upload times are Hub clock readings compared with cloud time; the Hub keeps trusted time, so skew is small against a 30-minute limit. The 5-branch floor on the mass hold keeps a single outage in a small estate from paging the on-call instead of telling the owner.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-cloud` -- expected: pass except known unrelated failures
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` -- expected: clean
- `pnpm check:generated && pnpm check:strings && pnpm check:pins` -- expected: ok

## Auto Run Result

Status: done
Implemented by subagent; verified with cargo test (proto, store, core, tasks, cloud), clippy and fmt on the five crates, check:strings, check:pins, owner svelte-check and `pnpm gen:types`. Pre-existing unrelated failures: cloud backup `a_hung_run_times_out_with_its_own_code`, cloud binary `unreachable_at_start_retries_then_exits_1_without_logging_the_token`, tasks `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `a_printer_that_stops_reading_times_out`. `arena-core` `proptest_apply` failed once (stock-in change count 168 vs 167 after an owner command, threshold and stock case) and passed on four reruns including a clean baseline: a flake that comes from the Story 8.34 outbox rows, not from this story. `check:deploy` could not run (no kubectl).
Open notes for review: branches already alerted do not count toward the mass hold; `branch_online` minutes run from the last alerted `last_seen_at`; the job needs the control role's secrets and migration 24 applied first.

## Code Review Result

Reviewed against the spec: offline, not-uploading, back-online, mass-hold and `Raise.whatsapp` logic match the Always rules. No blocking findings. Clippy and fmt are clean; the proto, store and cloud tests pass except the two pre-existing unrelated cloud failures listed above. Status: done.
