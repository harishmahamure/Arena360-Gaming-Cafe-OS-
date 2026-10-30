---
title: 'Story 8.39: A Cloudflare Tunnel for entitled branches, run by the Windows Hub'
type: 'feature'
created: '2026-10-30'
status: 'review'
baseline_commit: '6492f9d22183b07c12eec7391481bb5e0bb2eeb6'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: ['oversized']
deferred:
  - summary: >-
      A revoked or abandoned Hub never heartbeats again, so its tunnel, DNS name and Access app stay in Cloudflare.
    evidence: |-
      Cleanup runs only inside the heartbeat of a non-revoked Hub (`tunnel_answer`); `authenticate` refuses a revoked Hub first. Needs an operator sweep over `branch_tunnels` joined to revoked Hubs (like the `command-sweep` revoked-Hub pass).
    location: >-
      crates/arena-cloud/src/api/heartbeat.rs
    severity: medium
  - summary: >-
      The connector line reads "On" once the child has run 5 s; nothing checks that cloudflared is connected to the edge.
    evidence: |-
      `cloudflared` keeps running while retrying with a bad token or no WAN, and its output is discarded. Settle on its `--metrics` /ready probe before 8.40 relies on "On".
    location: >-
      crates/arena-tasks/src/tunnel.rs
    severity: medium
  - summary: >-
      A lost-credential re-grant returns the same Cloudflare connector token, so a replaced PC's old token stays valid until the tunnel is deleted.
    evidence: |-
      Cloudflare's `GET .../token` does not rotate; rotation means recreating the tunnel or resetting its secret. The fake issues fresh values, which the test `a_hub_that_lost_its_credential...` relies on.
    location: >-
      crates/arena-cloud/src/tunnel.rs
    severity: medium
  - summary: >-
      A hard-killed Hub leaves its `cloudflared` child running (no Windows Job Object), and the next start runs a second connector.
    evidence: |-
      `kill_on_drop` only runs on a graceful drop. Assign the child to a job with KILL_ON_JOB_CLOSE.
    location: >-
      crates/arena-tasks/src/tunnel.rs
    severity: medium
  - summary: >-
      `cloudflared.exe` is looked for beside the Hub but no installer step bundles it, and there is no version or integrity check.
    evidence: |-
      The diff changes no installer or release build. Packaging belongs to the release story; `--no-autoupdate` means nobody updates the binary.
    location: >-
      crates/arena-hub/src/lib.rs
    severity: medium
  - summary: >-
      Hubs of two platforms on one branch (Windows and Android) would make the tunnel flap between created and deleted.
    evidence: |-
      One row per branch is created for a Windows beat and deleted for an Android beat. Unverified that a branch can pair both (a standby is its own Hub device). Decide before the standby epic.
    location: >-
      crates/arena-cloud/src/api/heartbeat.rs
    severity: medium (unverified)
  - summary: >-
      A tunnel deleted in Cloudflare out of band makes `credential()` fail on every beat, so the row never recovers.
    evidence: |-
      `credential` errors map to `off` / nothing; the row is never marked deleting on a not-found.
    location: >-
      crates/arena-cloud/src/api/heartbeat.rs
    severity: low
  - summary: >-
      `TunnelAnswer` has no catch-all variant, so a newer cloud's new state would fail the whole heartbeat parse on an older Hub.
    evidence: |-
      Internally tagged enum without `#[serde(other)]`; adds a public variant, left for the next protocol change.
    location: >-
      crates/arena-proto/src/tunnel.rs
    severity: low
  - summary: >-
      Missing tests: Hub-side wiring of the shared inbox, `ProcessConnector` with a real child, and the provider-unset heartbeat.
    evidence: |-
      Only the pieces are tested; the wiring in `arena-hub/src/lib.rs` is not.
    location: >-
      crates/arena-hub/src/lib.rs
    severity: low
---

<intent-contract>

## Intent

**Problem:** A branch with the `tunnel` licence flag (Premium, or the Tunnel add-on) has no way to be reached from outside its LAN, so live view (8.40) and remote kiosks (8.41) have nothing to connect through, and nothing may be opened at the café.

**Approach:** The cloud creates a named Cloudflare Tunnel plus an Access application per entitled branch behind a swappable `TunnelProvider` (fake for tests/dev, HTTP for Cloudflare) and hands the credential to the Windows Hub once in the heartbeat answer; the Hub keeps it DPAPI-protected and runs the bundled `cloudflared` as a supervised child, outbound only. Removing the flag reverses all of it.

## Boundaries & Constraints

**Always:**
- The decision is made per heartbeat from the signed licence flags (`tunnel`) and the paired Hub's `platform`. Answer `tunnel` is always present for a paid or Free branch: `grant {hostname, credential}`, `on {hostname}` (tunnel exists and the Hub reported its credential), `off`, or `unsupported_platform`.
- The credential is never stored by the cloud, never logged, never in `Debug`, and is returned when the tunnel is created or when the Hub reports it holds none (`has_credential: false`: replaced PC, lost file); a repeat of a heartbeat that already delivered it sends `on`.
- The tunnel is `<branch_id>.tunnel.arena360.cloud`; its ingress forwards only to the Hub's tunnel listener (`TUNNEL_ORIGIN`, `http://127.0.0.1:7403`), everything else 404. The connector is outbound only; the Hub opens no port.
- Flag removed (or Hub revoked/Android): the Hub stops the connector and deletes the credential; the cloud deletes the Access app and the tunnel and its row. Provider failures never fail the heartbeat; the next heartbeat retries (row states make create/delete idempotent).
- Connector crash or WAN drop: restart with `backoff`; never touches trading (own task, no writer calls).
- Free/Android Hub: `unsupported_platform` for Android (any paid tier with the flag); the Hub System screen shows "Needs the Windows Hub".

**Never:** a tunnel for an Android Hub; opening any inbound port; live view or kiosk traffic handlers (8.40/8.41); billing for the add-on (Epic 10).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| First flag sight | Windows Hub, `tunnel` flag, no row | tunnel + Access app created, row stored, `grant` returned | Provider error: row not marked delivered, `off` answer, retried next beat |
| Steady | row delivered, Hub `has_credential` true | `on {hostname}` | — |
| Lost credential | row delivered, Hub `has_credential` false | `grant` with a fresh token | — |
| Flag removed | row exists, no flag | provider deletes app + tunnel, row removed, `off` | Delete error: row kept `deleting`, retried |
| Android | Android Hub with flag | `unsupported_platform`, nothing created | — |
| Crash | connector exits | restart after backoff, state `reconnecting` | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/tunnel.rs` (new) -- `TUNNEL_FLAG`, `tunnel_hostname`, `TUNNEL_ORIGIN`, `TunnelAnswer`, `TunnelReport`, `TunnelLine`; exported from `lib.rs`.
- `crates/arena-proto/src/pairing.rs` -- `HeartbeatRequest.tunnel`, `HeartbeatResponse.tunnel` (additive).
- `crates/arena-store/control-migrations/0026_branch_tunnels.sql` + `control.rs` list; `control/tunnels.rs` (new) -- `branch_tunnel`, `insert`, `mark_delivered`, `set_state`, `delete`.
- `crates/arena-cloud/src/tunnel.rs` (new) -- `TunnelProvider` (Fake, Cloudflare HTTP), `from_env`; `config.rs` env docs; `auth.rs`/`lib.rs` `AuthState.tunnels`; `api/heartbeat.rs` -- `tunnel_answer`.
- `crates/arena-tasks/src/tunnel.rs` (new) -- `TunnelInbox`, `TunnelTask`, `Connector`/`ProcessConnector`; `heartbeat.rs` -- `with_tunnel`, report + offer; `lib.rs` exports.
- `crates/arena-core/src/status.rs` -- `HubStatus.tunnel`; `crates/arena-api/src/health.rs` and `ui_tunnel.rs` (new) -- `GET /ui/tunnel`.
- `crates/arena-hub/src/lib.rs` (+ `config.rs`) -- wires the task on the Windows platform with `--cloudflared`.
- `apps/hub/src/routes/system/connection/+page.svelte` (new), `+layout.svelte`, message catalogue -- the System tunnel line.
- `docs/runbooks/cloudflare-tunnel-cost-spike.md` (new) -- the per-branch cost against the ₹200 add-on.

## Tasks & Acceptance

**Execution:**
- Protocol, store, cloud provider + heartbeat, Hub task + heartbeat, API + UI line, cost spike doc, tests for every matrix row.

**Acceptance Criteria:**
- Given a Windows Hub on a branch whose licence has `tunnel`, when the heartbeat arrives, then the provider creates the tunnel and Access app once and the answer carries the credential once.
- Given the Hub gets a `grant`, when the task applies it, then the credential is stored through the DPAPI keystore and the connector runs; flag removal deletes both on the Hub and the tunnel in the cloud.
- Given an Android Hub with the flag, then the answer is `unsupported_platform`, nothing is created and the System line reads "Needs the Windows Hub".
- Given a connector crash, then it is restarted with backoff and no writer call is made.

## Spec Change Log

## Review Triage Log

### 2026-10-30 — Review pass
- verdicts: about 35 raw findings (blind 14, edge-case 20, verification-gap 5 plus 3 others, intent-alignment descriptive) grouped by root cause into the rows below — high 0, medium 11, low 9, false 0, maybe-false 2 (counted per row)
- findings:
  - `[medium]` `patch` Provider unset/credential/create error answers `off`, so a Hub that holds a working credential stops and deletes it — answer nothing when the Hub reports it holds one (`unsettled`); test `a_cloud_failure_never_tells_a_hub_that_holds_its_credential_to_stop`
  - `[medium]` `patch` Tunnel step runs inline up to ~8 Cloudflare calls × 15 s beside the Hub's 15 s timeout — wrapped in an 8 s budget answering nothing
  - `[medium]` `patch` Ingress and CNAME were created before the Access app, and delete removed the app first, leaving a routed name without a gate — Access first on create, DNS first on delete (mock-API order tests updated)
  - `[medium]` `patch` A 404 on the ingress PUT or DNS write counted as success — now `CF_NOT_FOUND` (test `a_write_that_finds_its_object_missing_is_an_error_not_a_success`)
  - `[medium]` `patch` A Hub sending no report was re-granted the credential on every beat — now answered `on` once delivered
  - `[low]` `patch` Connection page kept a stale badge after the Hub became unreadable — the line is cleared with the failed read
  - `[low]` `patch` A fresh grant waited out the old credential's backoff — retry timer cleared when `fails` is reset
  - `[medium]` `patch` Missing tests: delete retry on 404 objects, `/ui/tunnel` through `ui_router` with its origin guard — added (`a_delete_retried_after_a_partial_one...`, `the_tunnel_line_is_served_by_the_ui_router...`)
  - `[low]` `patch` Hub config test path had lost its backslashes — fixed
  - `[low]` `patch` Runbook: wildcard certificate cover for a two-label hostname and the revoked-Hub cleanup recorded as open items
  - `[medium]` `defer` Revoked/abandoned Hub never cleaned up (heartbeat-only cleanup) — needs a sweep job; the spec's "Hub revoked" wording is met for flag removal and Android only
  - `[medium]` `defer` "On" does not prove an edge connection (no readiness probe, output discarded)
  - `[medium]` `defer` Re-grant returns the same Cloudflare token (no rotation)
  - `[medium]` `defer` No Job Object for a hard-killed Hub's child
  - `[medium]` `defer` `cloudflared` bundling/integrity is a release-build concern, not in this repo's code
  - `[medium]` `defer` Windows + Android Hubs on one branch could flap the tunnel (unverified)
  - `[low]` `defer` Out-of-band deleted tunnel never recovers; `TunnelAnswer` lacks a catch-all variant; Hub wiring / `ProcessConnector` / provider-unset tests
  - `[low]` `reject` Provider choice follows SMS config (matches the push provider's pattern); optional policy id warning; Free tier with flag untested (guarded by `tier != Free`); `unsupported_platform` is derived each beat, not stored (the Live view story can derive it from `hubs.platform`); DPAPI test strength (same as the existing keystore); Access redirect and list pagination (a branch has one tunnel, one app, one record)
  - `[maybe-false]` `reject` Cost spike is a model, not a measurement — recorded as such in the runbook, which names prices to re-read on the production account; the spike could not be run against a real account here
  - `[maybe-false]` `reject` Ingress points at a placeholder origin (`127.0.0.1:7403`) — the listeners land in 8.40/8.41, as the spec's Design Notes say

## Design Notes

The Hub's tunnel listener (`:7403`) and the Cloudflare account settings (`ARENA_CF_*`) are configuration only here; the listeners arrive in 8.40/8.41. The build spike is a recorded document, not code.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-cloud -p arena-tasks -p arena-api` -- expected: pass
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` -- expected: clean
- `pnpm -C apps/hub check && pnpm check:strings && pnpm format:check` -- expected: ok

## Auto Run Result

Status: review

Implemented: a Cloudflare Tunnel per entitled branch. The cloud (`TunnelProvider`: Fake or Cloudflare over `ARENA_CF_*`) creates the tunnel, Access application and proxied DNS name on the first heartbeat that carries the `tunnel` flag from a Windows Hub, returns the connector credential in the heartbeat answer (`grant`, later `on`), and deletes everything when the flag goes or the Hub is Android (`unsupported_platform`). The Hub (`TunnelTask`) stores the credential through the DPAPI keystore, runs `cloudflared` as a supervised child (credential in `TUNNEL_TOKEN`), restarts it with backoff and never touches the writer. System → Connection shows the line, including "Needs the Windows Hub". Premium's tier flags gained `tunnel`. The cost spike is recorded in `docs/runbooks/cloudflare-tunnel-cost-spike.md`.

Files: `crates/arena-proto/src/{tunnel,pairing,tier,lib}.rs` (types, additive heartbeat fields); `crates/arena-store/{control-migrations/0026_branch_tunnels.sql,src/control/tunnels.rs,src/control.rs}`; `crates/arena-cloud/src/{tunnel,auth,config,lib}.rs`, `api/heartbeat.rs`, `tests/tunnel.rs`; `crates/arena-tasks/src/{tunnel,heartbeat,lib}.rs`; `crates/arena-core/src/status.rs`; `crates/arena-api/src/{ui_tunnel,health,lib}.rs`; `crates/arena-hub/src/{lib,config}.rs`; `apps/hub` Connection page, `lib/tunnel.ts`, `tests/tunnel.spec.ts`; strings and generated types; runbook.

Review: 9 patches applied (5 medium, 4 low/test), 9 items deferred (see frontmatter), rejects recorded above. Follow-up review recommended: yes — two or more medium patches changed the cloud provider order and the heartbeat answer rules, and none of it has run against a real Cloudflare account or a real `cloudflared`.

Verification: `cargo fmt --check` clean; clippy `-D warnings` clean on arena-proto/store/cloud/tasks/api/hub; passing: `arena-cloud --test tunnel` (13), cloud `tunnel` unit tests (10), arena-tasks `tunnel` (15), arena-api `tunnel` (4), arena-hub config, Hub Playwright `tunnel.spec.ts` (6), `check:strings`, `format:check`, `check:protocol`, `check:layers`. Known failures not from this story: `arena-cloud --test backup a_hung_run_times_out_with_its_own_code` (zero timeout vs a fast run), `arena-tasks heartbeat::trusted_time_is_cloud_time_plus_monotonic_elapsed` (needs 8 days of uptime) and `printer::lan a_printer_that_stops_reading_times_out`, `arena-api ui_auth lock_and_unlock_log...` (log-capture race; passes with `--test-threads=1`), `pnpm -C apps/hub check` (2 errors in `playwright.kitchen.config.ts`), `check:deploy` (no kubectl).

Residual risks: Cloudflare API shapes and `TUNNEL_TOKEN` are proven only against a mock and a fake connector; the cost spike is a model with list prices from memory; the Hub tunnel listener (`:7403`) does not exist until 8.40/8.41.
