---
title: 'Story 1.22: The branch runs with the WAN down and shows which link is offline'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '8dc3efb945ead7d4e0079baef0e91585433d8f03'
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

Paths are from the repo root; line numbers are approximate (HEAD `8dc3efb`).

- **Task pattern** -- `crates/arena-tasks/src/{lib.rs,housekeeping.rs,lease.rs}`:
  - A struct holds `clock: Arc<dyn Clock>` and an interval field (shortened in tests), with `tick()` and `run()` built on `interval_at` and `MissedTickBehavior::Delay`. `LeaseTask` holds the `watch::Sender<HubStatus>` it publishes through.
  - Add `connectivity.rs` exporting `ConnectivityTask`, `Prober` (trait), `decide(internet_ok, cloud_ok)` (pure) and `PROBE_INTERVAL`.
  - The clock is `arena_core::clock::{Clock, SystemClock, ManualClock}`. Layers allow arena-tasks to depend on arena-core, arena-store and arena-proto, plus any third-party crate.
- **Status** -- `crates/arena-core/src/status.rs:68` `HubStatus`:
  - Add `connectivity: Connectivity` (the proto type: `online | internet_offline | cloud_unreachable`, with a `checked_at` ms). It starts as `online` until the first probe; that choice is recorded in Implementation Notes.
  - Publish with `send_if_modified`.
  - Spawn the task in `crates/arena-hub/src/main.rs` (~l.212-233) next to housekeeping and lease, with `status_tx.clone()`.
- **Flags** -- `crates/arena-hub/src/config.rs`:
  - Add `--cloud-host <host:port>` (default `api.arena360.in:443`) and `--probe-internet <addr,addr>`: a `Config` field, a match arm (~l.214), both USAGE strings and unit tests.
  - Add rows to the README flags table at `README.md:77-84`.
- **TLS** -- use the pinned `rustls` / `tokio-rustls` (ring), plus one new pinned `webpki-roots` 1.x that is already in `~/.cargo/registry/cache` and compatible with the lockfile's `rustls-pki-types`. Add it to the README pin table, which `check:pins` checks.
  - The healthz GET is a hand-written HTTP/1.1 request over the TLS stream. There is no HTTP client crate in the workspace; don't add one.
- **Healthz and UI:**
  - `crates/arena-api/src/health.rs`: `HealthBody` (l.29) and `HealthBody::new` (l.41) get `connectivity`; update the whole-JSON tests at l.353/381.
  - `ui_snapshot` (l.160) adds `connectivity` to `HubSnapshot` (`crates/arena-proto/src/snapshot.rs:12`) from the status watch, the way `online` comes from Presence.
  - SSE: `crates/arena-api/src/ui_events.rs` already holds `status: Option<watch::Receiver<HubStatus>>` (l.130), and the arm at ~l.286-301 handles a role change. Add a `connectivity.changed` send (as `devices.changed` does, l.268) when the value differs from the last one sent, and update the module doc (l.1-20).
  - Regenerate the ts-rs types.
- **Hub header** -- `apps/hub/src/routes/+layout.svelte`:
  - Put the chip in `.header-end` (~l.395-406) before `<Clock/>`, using `--color-chip-offline-bg` / `-text` (tokens.css:36-37, HC at :71-72). No shared Chip component exists; a small scoped element is fine.
  - Announce through the existing polite region `data-testid="hub-announce"` (~l.480, `announcement` state ~l.68) rather than a second live region.
  - The SSE client is `apps/hub/src/lib/events.ts`. The second, simpler header (~l.381) doesn't get the chip.
  - Tests: `apps/hub/tests/shell.spec.ts` header tests, with SSE pushed through `pushRaw(hub, text)` (pattern in `devices-display.spec.ts:88`). Keep the single-`<header>` landmark test passing.
- **e2e driver** -- new `crates/arena-e2e` (`publish = false`, a binary plus helpers; add it to `LAYER_RULES` only if it needs restricting):
  - Reuse the patterns from `crates/arena-hub/tests/lan.rs`: raw TcpStream HTTP helpers (l.28-44), SSE `open_events`/`wait_for` (l.59/69), `dev seed` (l.94), the `Running(Child)` guard (l.108) and `hello()` (l.119). LAN client: `arena_kiosk_core::HubClient` / `connect_ws`.
  - The kiosk is `arena-kiosk-svc --console` (`cli.rs:123`), provisioned with `dev provision`. The process watcher fake is `procs/fake.rs`.
  - There are no dev hooks yet for a process-block event or an admin exit from console mode. Add them behind the existing `dev-fixtures` feature (Cargo.toml:35), for example an IPC or loopback dev request, and test them.
- **CI** -- `.github/workflows/ci.yml`:
  - Add a `wan-down` job (ubuntu-latest) and add it to `ci-ok` `needs` (l.207). `ci-ok` fails on a skipped need, so the job must always run.
  - `release-bench.yml` gets `workflow_dispatch.inputs.soak` (boolean) and a `soak` job that runs only when the input is set.

## Tasks & Acceptance

**Execution:**
- [x] `arena-tasks::connectivity` with its decision and timeout tests; status, healthz, snapshot and SSE wiring with tests.
- [x] Hub header chip and live region, with Playwright tests (stubbed SSE): appears, the text per state, announced once, disappears.
- [x] `crates/arena-e2e` driver, with the flows and normalised comparison.
- [x] `scripts/wan-down/{run.sh,soak.sh}`, the CI `wan-down` job, the release-bench soak input, README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked`, `pnpm check` and the Hub Playwright suite, then all pass.
- Given `scripts/wan-down/run.sh` on Linux with sudo, then it passes. If the local macOS environment can't run it, record that as unverified locally; CI runs it.

## Implementation Notes

- **States.** The proto type is `Connectivity {state: ConnectivityState, checked_at: number | null}` (`arena-proto/src/connectivity.rs`), with `ConnectivityState` `online | internet_offline | cloud_unreachable`; `decide()` returns it directly (the spec's `Offline::Internet` / `Offline::Cloud` / `Online`). It starts `online` with `checked_at: null` until the first probe finishes, so the chip never shows before the Hub knows. The snapshot field is `#[serde(default)]`; the UI treats a missing or malformed one as online.
- **Probe.** `Prober` uses `impl Future + Send` trait methods (no async-trait crate). The internet check connects to every endpoint at once (a `JoinSet`), so one hung endpoint cannot eat the other's 3 s. The cloud check is `TcpStream::connect((host, port))` + `tokio-rustls` with `webpki-roots =1.0.9` (pinned, in the README table) + a hand-written `GET /healthz HTTP/1.1`, 200 on the status line only. `send_if_modified` skips a round that found the same state at the same `checked_at`; a state change is logged once at info. The task stops when the status channel closes; `main.rs` aborts it at shutdown. Hung-probe test: `an_intent_applies_while_the_probe_hangs` submits `branch.bootstrap` through a real writer while a fake probe hangs.
- **SSE.** `connectivity.changed` is sent from the existing status arm, once per change of `state` (not of `checked_at`). `ui_router` takes the status receiver as a new 4th argument.
- **UI.** The chip is a scoped `<p data-testid="hub-chip-offline">` in the signed-in header's `.header-end` before `<Clock/>`, using the component tokens `--chip-offline-background` / `--chip-offline-foreground` (they switch to the `-hc` twins under `data-theme="hc"`; the raw `--color-chip-offline-*` would not). The recovery announcement is a new catalogue line, `hub.connectivity.restored` ("Internet and cloud back online"), so "each change" includes coming back. Playwright: `apps/hub/tests/connectivity.spec.ts` (stubbed SSE; the announcements are recorded with a `MutationObserver` to prove "once"). Snapshot literals in the other specs gained `connectivity`.
- **Kiosk dev hooks** (`dev-fixtures`): `--console --dev-fake-procs` (enforcement against `procs::fake`, plus a `DevLauncher` whose admin-exit desktop is a no-op, since `NoopLauncher` refuses it); IPC requests `dev.spawn_process`, `dev.hub_link {online}` (a `Ctx::link_hold` watch the Hub link honours: it closes a live link and waits), `dev.relock`, `dev.status`. Tested in `tests/runtime.rs` (`dev_hooks_…` with the feature, `dev_requests_are_unsupported_without_the_feature` without). `link_hold` lives on `Ctx` in every build but only the dev request sets it.
- **e2e driver** (`crates/arena-e2e`, not in `LAYER_RULES`): `run` / `check` / `compare`. It verifies through the real APIs and reads the change log read-only from `branch.db` for the ledger, `seq` and the table. The normalised table masks ULIDs, epochs, timestamps (integers ≥ 10^12), argon2 hashes and long hex, is sorted (kiosk events and UI intents interleave differently run to run) and leaves out the kiosk watchdog's own `app_restarted` / `no_session` logs (timing, not flows). The ledger counts distinct intents per kind (`settings.set`, `kiosk.process_blocked`, `device.admin_exit`, `device.admin_relock`).
- **Order.** The scripts run `dev seed` on the fresh data dir *before* starting the Hub (seed is a one-shot writer; against a running Hub it would be a follower and fail `NOT_PRIMARY`).
- **Tracing.** One strace log per traced process (`trace.{hub,kiosk,driver,ui}.log`), not one `trace.log`; the Hub UI smoke (vite preview, Playwright and the browser) runs under strace too, in `trace.ui.log`. The probe's `--allow` endpoints apply only to `trace.hub.log` (the probe's process); every other trace must stay on loopback and local sockets, with one exception: `trace.ui.log` may contain Chromium's built-in IPv6 reachability check, a UDP connect to `[2001:4860:4860::8888]:443` from the browser's network service before its first navigation (ENETUNREACH in the namespace, nothing sent). No launch flag disables it (tried: `--host-resolver-rules` MAP rules, `--disable-ipv6`, `--disable-features=AsyncDns,HappyEyeballsV3,DnsOverHttps,NetworkQualityEstimator`, `--headless=old`, `--disable-background-networking`, `--enable-features=EnableIPv6ReachabilityOverride`), so it is allowed by exact address and port in the UI trace only (`CHROMIUM_IPV6_PROBE` in `check-trace.mjs`). Inside the namespace `/etc/netns/<ns>/{resolv.conf,nsswitch.conf}` send lookups only to files and a loopback resolver, so no NSS module reaches the host resolver through a local socket; the allow-list still adds the cloud host's host-resolved addresses so a lookup that did succeed is not counted against the probe.
- **Review fixes.** `arena-e2e check` also fails on an intent that inserted the same row twice (`check_no_duplicates`), and the ledger subtraction cannot underflow. Each newly opened `/ui/events` stream first gets the current `connectivity.changed`; the layout keeps the answer with the newer `checked_at` (null oldest) and shows the chip (and announces) only while signed in.
- **Soak.** The restore adds a veth pair + NAT into the running namespace and asserts the probe leaves `internet_offline` within 40 s, then `arena-e2e check` and the Hub log. The release-bench `soak` job runs on `[self-hosted, linux, x64]`: GitHub-hosted jobs stop at 6 h.
- **Verified locally (2026-10-24):** `cargo test --workspace` (all pass except the `arena-hub-ui` sidecar timing test `a_sidecar_that_exits_stops_the_wait_early`, which already failed before this story), the dev-fixtures runs for arena-hub, arena-api and arena-kiosk-svc, clippy (workspace and dev-fixtures), every `pnpm check` step (`check:generated` compares against HEAD, so it was checked by regenerating and diffing the working tree), the Hub Playwright suite (242 passed), and `scripts/wan-down/run.sh --soak-hours 0.05` end to end inside a privileged Debian bookworm arm64 container (before the review fixes; not re-run after them, so the traced UI smoke and the per-trace allow-list are unverified on Linux) (both passes, the tables equal at 116 rows, 31 traced connects all loopback/local/probe, the down Hub `internet_offline`, a 71-iteration soak, the restore seen as `cloud_unreachable`). Re-run after the review fixes (traced UI smoke, per-trace allow-list, the Chromium IPv6 check allowed in the UI trace only) in the same container setup with its own cargo target dir: PASS (tables equal at 116 rows; 69 connects in 4 traces all allowed; a 72-iteration soak; the restore seen as `cloud_unreachable`). Not run on an actual GitHub `ubuntu-latest` x64 runner, and the 8-hour soak was not run.

## Spec Change Log

## Review Triage Log

Iteration 0. Sources: BH = Blind Hunter, ECH = Edge Case Hunter, VG = Verification Gap.

| # | Source | Finding | Verdict | Route | Evidence |
|---|--------|---------|---------|-------|----------|
| 1 | BH, VG | The ledger check counts distinct `intent_id`s, so a doubled apply of one intent passes | medium (gap) | patch | `the_ledger_counts_distinct_intents` collapses repeated ids, and new seqs keep `check_gap_free` green. Add a duplicate-insert check with a test. |
| 2 | BH, ECH | `check_ledger` subtracts `u64`s and can underflow | low | patch | `after - before` on u64 in `table.rs`. Use checked_sub and report a mismatch. |
| 3 | BH, ECH, VG | An older snapshot can overwrite a newer `connectivity.changed`, and a change between snapshot load and stream open is never pushed | medium | patch | The layout assigns `link` from every snapshot, and the SSE task only sends on later changes. Send the current connectivity once when a stream opens, and keep the value with the newer `checked_at`. |
| 4 | BH, ECH | The chip goes stale while nobody is signed in (no event stream) | low | patch | The stream opens only with staff signed in. Render the chip only while the event stream is live. |
| 5 | BH, ECH | The probe allow-list applies to every trace, so the kiosk or driver could connect to the probe IPs unseen | medium | patch | `check-trace.mjs` gets one `--allow` set for all three logs. Apply it only to `trace.hub.log`. |
| 6 | ECH | The Playwright smoke is not run under strace, against the frozen intent ("under strace … it runs … the Hub UI Playwright smoke") | medium | patch | A deviation recorded in Implementation Notes. The smallest fix runs the smoke under `strace -f -e trace=connect` into its own trace, checked with an empty allow set. |
| 7 | VG | No test covers a snapshot with no or malformed `connectivity`, or a malformed event | (gap) | patch | `normalizeConnectivity` and `isConnectivity` are untested. Add the Playwright cases. |
| 8 | ECH | The e2e IPC reader busy-loops on a persistent error; pending entries leak on send failure or timeout | low | patch | `ipc.rs` reader loop and `pending` map. Return on error, and remove the entry on failure. |
| 9 | ECH | Positional args to `run`/`check` are silently ignored | low | patch | `main.rs` argument handling. Reject them with exit 64. |
| 10 | ECH | `run.sh` doesn't require node up front | low | patch | The required-tool loop lacks node and pnpm. Add them. |
| 11 | BH | README: "The chip goes on the first probe…" reads as appearing | low | patch | Reword to "disappears". |
| 12 | BH | The default cloud host is a placeholder, so every Hub with internet shows "Cloud unreachable" until the cloud epic | medium | defer | The frozen intent sets `--cloud-host` default `api.arena360.in:443` "documented as a placeholder until the cloud epic". Changing that edits the intent. Recorded for Epic 2 (2-1 makes the cloud real) and raised with the user. |
| 13 | BH | Cloud success while TCP probes fail still reports Internet offline; a captive portal passes the TCP check | low | reject | The frozen intent defines the state table (internet down → `Offline::Internet`; TCP connect probe). |
| 14 | BH | Publishing `checked_at` wakes watchers every 30 s | low | reject | SSE sends only on a state change; one wake per 30 s is negligible. |
| 15 | BH | `sendto`/`sendmsg` and io_uring are not traced | low | reject | The frozen intent specifies `strace -e trace=connect`. |
| 16 | BH | The soak lacks strace, the `internet_offline` assertion before restore, and a resolver after restore | low | reject | The intent's soak assertions are seq gap-free, counts and no error logs, which are implemented. The extras are beyond scope. |
| 17 | BH | The tests run the dev-fixtures build, not the shipped one | false | reject | The intent requires the driver's hooks behind `dev-fixtures`; the connections under test are the same code paths. |
| 18 | BH | `check_hub_log` greps an exact JSON level string; kiosk logs unchecked | low | reject | It matches the tracing JSON format in use; the intent names only the Hub log. |
| 19 | BH | The PIN flow accepts `PERMISSION_DENIED` as well as `PIN_REQUIRED` | false | reject | A Counter's `settings.set` is refused `PERMISSION_DENIED` before elevation (see the 1.21 test), and the flow then requires the elevated save to apply. |
| 20 | BH | Process-wide statics in `dev.rs` leak between tests | low | reject | It is one console process per run, and tests use their own fakes. |
| 21 | BH | The up-pass smoke asserts nothing about connectivity | low | reject | The up pass's runner state is not controlled (placeholder cloud). The down pass asserts. |
| 22 | ECH | A hanging cloud DNS lookup leaves abandoned blocking threads | low | reject | It only accumulates while every lookup hangs more than 30 s, and the blocking pool is bounded. |
| 23 | ECH | A TLS close without close_notify before CRLF counts as cloud down | low | reject | A healthy HTTP/1.1 server ends the status line with CRLF. Rare. |
| 24 | ECH | An empty internet endpoint list through the library API means offline forever | low | reject | The CLI validates the list; there is no other caller. |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub exec playwright test` -- expected: pass
- `bash scripts/wan-down/run.sh` -- expected: pass on Linux (CI)
