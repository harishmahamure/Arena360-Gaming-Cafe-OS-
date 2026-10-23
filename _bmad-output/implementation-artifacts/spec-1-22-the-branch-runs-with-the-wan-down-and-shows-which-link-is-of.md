---
title: 'Story 1.22: The branch runs with the WAN down and shows which link is offline'
type: 'feature'
created: '2026-10-23'
status: 'draft'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-21-high-contrast-theme-as-a-per-device-branch-setting.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Nothing proves the branch works with the WAN down, and staff can't tell whether the internet or the cloud is unreachable.

**Approach:**
- **Probe:** a non-blocking connectivity probe in `arena-tasks` tells "internet unreachable" apart from "cloud unreachable" every 30 s. The Hub header shows `chip-offline` with a polite live-region announcement.
- **Network-namespace test:** a CI integration job runs the Epic 1 flows inside a Linux network namespace with no WAN route, under `strace`. It fails on any outbound connection other than the probe.
- **Long outage:** a configurable WAN-down soak script, with a short CI run and a full 8-hour manual or release run.

## Boundaries & Constraints

**Always:**
- **Probe (`arena-tasks::connectivity`):**
  - Every 30 s (const), and once at start, it runs two independent checks, each with a 3 s timeout. They run on their own task and never touch the Writer, so no intent waits on them.
  - **Internet:** a TCP connect to 2 well-known anycast endpoints (`1.1.1.1:443`, `8.8.8.8:443`; configurable with `--probe-internet`). Any success means `internet = up`.
  - **Cloud:** a TCP connect plus TLS handshake (`tokio-rustls` with the webpki roots, pinned) to the configured cloud host (`--cloud-host`, default `api.arena360.in:443`, documented as a placeholder until the cloud epic), then `GET /healthz` expecting 200.
  - **States:**
    - internet down → `Offline::Internet`;
    - internet up and cloud down → `Offline::Cloud`;
    - both up → `Online`.
  - A pure decision function, with the network calls behind a `Prober` trait and fakes in tests.
  - The state goes into `HubStatus.connectivity` and `/healthz`.
- **UI:**
  - The snapshot gains `connectivity`, and the SSE stream gets `event: connectivity.changed` when the state changes.
  - The `hub-header` shows `chip-offline`, using the `chip-offline` tokens and the HC twin, with the catalogue text "Internet offline" or "Cloud unreachable" plus a glyph, so colour isn't the only cue.
  - A polite live region announces each change once. Nothing else in the UI changes.
  - When the state returns to `Online`, the chip disappears on the next probe, within 30 s.
- **Network isolation:** the probe's endpoints are the only allowed outbound destinations. Every other component must make no outbound connection; any it attempts counts as a failure in the namespace test.
- **WAN-down integration job** (`.github/workflows/ci.yml` job `wan-down`, ubuntu-latest, part of `ci-ok`):
  - `scripts/wan-down/run.sh` builds the release binaries and starts a network namespace with only `lo` up (`sudo unshare -n`, or `ip netns add`).
  - Inside the namespace, under `strace -f -e trace=connect -o trace.log`, it runs:
    - the Hub (fresh data dir), then `dev seed`;
    - `arena-kiosk-svc --console`, provisioned with the seeded token against `127.0.0.1:7400`;
    - the Epic 1 flows through a Rust e2e driver (`crates/arena-e2e`, a test-only binary using the real loopback HTTP and LAN APIs): staff sign-in, branch settings save, PIN elevation, idle lock and unlock via the API, the audit list, a kiosk connection and online status, a process-block event through the kiosk fake watcher's dev hook, admin exit (Hub and cached-verifier paths through the dev hooks of the kiosk console mode) and the HC toggle;
    - the Hub UI Playwright smoke (sign-in, floor, settings) against the real Hub with the bundled build.
  - **Assertions:**
    - every flow succeeds;
    - the results equal those of the same driver run with the network up (compare the normalised change-row tables, with ids and timestamps masked);
    - `trace.log` contains no `connect()` to any non-loopback address except the probe endpoints;
    - `changes.seq` has no gaps.
  - Windows-only enforcement (real lockdown and process killing) is out of this Linux job's reach and is covered by the 1.19 suite. This is documented.
- **Assets:** the existing `check:no-remote` (1.4) already guards the bundles. The WAN-down Playwright smoke also asserts that no request left loopback (`page.on('request')`).
- **Long outage:**
  - `scripts/wan-down/soak.sh --hours N` runs the Hub and the kiosk console in the namespace and loops the e2e flows for N hours.
  - When it ends it restores the network (moving processes, or reconnecting the namespace's veth) and asserts that `seq` is gap-free and that there are no duplicated or lost change rows (counts versus the driver's ledger), and that the Hub log has no `error` lines.
  - CI runs `--hours 0.05` (3 minutes) in the `wan-down` job. `release-bench.yml` gets a manual `soak` input that runs 8 hours.

**Never:**
- Blocking or slowing an intent on the probe.
- Polling the cloud for anything else in Epic 1.
- UI changes beyond the chip.
- Counting the probe's own connections as violations.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| All up | both probes ok | `Online`; no chip | N/A |
| Internet down | the TCP probes fail | `Offline::Internet`; chip "Internet offline"; announced once | N/A |
| Cloud down | TCP ok, cloud TLS or HTTP fails | `Offline::Cloud`; chip "Cloud unreachable" | N/A |
| Recover | the next probe ok | the chip is gone within 30 s | N/A |
| Slow probe | the probe hangs | the 3 s timeout fires; intents are unaffected (a test submits during a hanging fake probe) | N/A |
| No egress | the namespace job | all flows pass; only probe destinations appear in `connect()` | job fails otherwise |
| Equal results | WAN-down vs WAN-up driver runs | the normalised tables are equal | N/A |
| Soak | 3 min (CI) / 8 h (manual) | seq gap-free; counts match; no error logs | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-tasks` -- the lease and housekeeping task patterns (ManualClock, status watch). Add `connectivity.rs`.
- `crates/arena-core/src/status.rs`, `crates/arena-api/src/{health.rs,ui_events.rs}`, `crates/arena-proto/src/snapshot.rs` -- carry `connectivity`.
- `apps/hub/src/routes/+layout.svelte` -- the header. Add the chip and the live region.
- The existing integration test helpers in `crates/arena-hub/tests/*.rs` and `crates/arena-api/tests/lan.rs` -- reuse them in `crates/arena-e2e`.
- `crates/arena-kiosk-svc` console mode and its dev hooks (1.16–1.20) -- add the hooks the driver needs, behind `dev-fixtures`.
- `.github/workflows/{ci.yml,release-bench.yml}` -- add the jobs.

## Tasks & Acceptance

**Execution:**
- [ ] `arena-tasks::connectivity` with its decision and timeout tests; status, healthz, snapshot and SSE wiring with tests.
- [ ] Hub header chip and live region, with Playwright tests (stubbed SSE): appears, the text per state, announced once, disappears.
- [ ] `crates/arena-e2e` driver, with the flows and normalised comparison.
- [ ] `scripts/wan-down/{run.sh,soak.sh}`, the CI `wan-down` job, the release-bench soak input, README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked`, `pnpm check` and the Hub Playwright suite, then all pass.
- Given `scripts/wan-down/run.sh` on Linux with sudo, then it passes. If the local macOS environment can't run it, record that as unverified locally; CI runs it.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub exec playwright test` -- expected: pass
- `bash scripts/wan-down/run.sh` -- expected: pass on Linux (CI)
