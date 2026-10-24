---
title: 'Story 2.17: Kiosk finds the Hub on the LAN or by typed IP'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '050c4fc2d257527878ce5f182f7c286475ba4b89'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-20-kiosk-admin-exit-with-manager-pin-offline-verifier-and-hub-r.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A new kiosk only reaches the Hub through Epic 1's dev provisioning (a typed `config.json`). There's no way to find the Hub on a café network.

**Approach:**
- The Hub advertises itself over mDNS and answers an unauthenticated `GET /hub-info` on `:7401`. Settings → Devices shows its LAN address.
- An unpaired kiosk shows a discovery screen inside the locked shell: Hubs found within 10 s, else a typed-IP field. The typed IP is verified against `/hub-info` and the TLS certificate.
- The choice is kept as the selected Hub that pairing (Story 2.18) will use. The kiosk is still not paired.

## Boundaries & Constraints

**Always:**
- **Protocol** (`crates/arena-proto/src/discovery.rs`, re-exported from `lib.rs`, ts-rs exported):
  - `MDNS_SERVICE = "_arena360-hub._tcp.local."`.
  - TXT keys `branch`, `hub`, `proto` (`PROTOCOL_VERSION`) and `fp` (the 64-hex Hub TLS fingerprint).
  - `HubInfo {product: "arena360-hub", branch_name, hub_name, protocol_version, fingerprint, lan_port, web_port}`.
  - New `ErrorCode`s: `NO_HUB` and `HUB_INCOMPATIBLE`. These names are reserved for this story; the control track mustn't use them.
- **Hub name:** the machine hostname (`lan::tls::hostname()`), sanitised to ≤ 63 printable characters. A Hub name setting is out of scope.
- **Hub `/hub-info`:**
  - `GET /hub-info` on `:7401`, over both plain HTTP and TLS (`lan/listener.rs` `route()`), answers 200 `application/json` `HubInfo` with no auth.
  - It answers 503 `{"code":"NO_HUB"…}`-shaped JSON before the branch is bootstrapped.
  - It adds `Cache-Control: no-store`. Only GET; anything else keeps today's behaviour.
- **mDNS advertiser:**
  - `crates/arena-tasks/src/mdns.rs`, `MdnsTask`, on `mdns-sd` pinned to the architecture's 0.21 line (exact version recorded in the README pin table).
  - It registers instance `<hub_name>` with port `lan_port` (7400) and the TXT above, on every non-loopback IPv4 interface. It starts once a branch exists, re-registers when the branch name or fingerprint changes, and unregisters on shutdown.
  - A failure to advertise (no multicast) is logged once per minute and never stops the Hub.
  - It's spawned in `crates/arena-hub/src/main.rs` next to `ConnectivityTask`.
- **Hub UI** (Settings → Devices, `apps/hub/src/routes/settings/devices-display/+page.svelte`):
  - The `HubSnapshot` gains `lan: {addresses: [ipv4], lan_port, web_port, fingerprint}`, refreshed on change via the existing `devices.changed` SSE or snapshot reload.
  - The page shows a "Hub address" row with each IPv4 address (copyable), the short fingerprint (first 4 groups of 4 hex) and the hint text the kiosk refers to.
  - Every string lives in `en.json`.
- **Kiosk service** (`crates/arena-kiosk-svc`, plus `crates/arena-kiosk-core` for the shared client pieces):
  - **Unpaired state:** with no device token or no config, `KioskHubState` gains `unpaired`, and `hub_link` stops looping on "not provisioned" and reports `unpaired`.
  - **IPC requests** (the existing `Request{rid, kind, body}`/`Reply`):
    - **`discover`:** browses `MDNS_SERVICE` for 10 s and pushes `IpcMsg` events `discovery.found {hubs: [{addr, branch_name, hub_name, fingerprint, protocol_version}]}` as Hubs resolve, deduplicated by fingerprint, then `discovery.done`. Hubs whose `proto` differs from ours are listed with `compatible: false`.
    - **`probe_ip {ip}`:** validates the IPv4 or IPv6 literal, then `GET http://<ip>:7401/hub-info` with a 2 s timeout. Then a TLS handshake to `<ip>:7401` captures the certificate fingerprint, using the existing TOFU-capture path in `client.rs`. It requires `HubInfo.product == "arena360-hub"`, a captured fingerprint equal to `HubInfo.fingerprint`, and a matching `protocol_version`.
      - **Success:** the reply is `{hub}`.
      - **Failure:** the reply is `NO_HUB`, or `HUB_INCOMPATIBLE` when only the version differs.
    - **`select_hub {addr, fingerprint}`:** stores `selected_hub.json` (addr `host:7400`, fingerprint, branch_name) in the kiosk data dir. Pairing (2.18) reads it. It never creates a device token or a `config.json`.
  - The `Browser` and `Prober` traits keep the logic testable with fakes.
- **Kiosk UI** (`apps/kiosk`, inside the existing locked layout; lockdown and the Ctrl+Alt+Shift+A admin exit unchanged):
  - **While `unpaired`, the discovery screen:**
    - the `h1` "Looking for the Hub…" with a busy indicator (reduced-motion safe), live-updating a list of found Hubs as buttons "<branch_name> · <hub_name>" (incompatible ones disabled, with "Needs a newer kiosk");
    - after `discovery.done` with none found, the field "Hub IP address" and the hint "The IP is shown on the Hub under Settings → Devices", a "Check" button and "Search again";
    - on a probe failure: "No Hub at that address" (or the incompatible text), with the field kept and focused;
    - on a pick or a valid probe: `select_hub`, then the next step's heading "Enter the pairing code", with "Pairing arrives with the next update" as a placeholder until 2.18 replaces it, and "Choose a different Hub".
  - It's announced politely once per state change. Targets are ≥ 44 px. Every string lives in `en.json`.
- **Tests:**
  - **Rust:**
    - `/hub-info` (bootstrapped and not);
    - TXT building and re-register on change (fake daemon);
    - the discovery dedupe, compatibility flag and 10 s window (paused clock);
    - each probe path against a real local Hub listener: ok, wrong product, fingerprint mismatch, version mismatch, nothing listening;
    - `select_hub` persistence;
    - an optional real-mDNS round trip that skips with a printed reason when multicast is unavailable.
  - **Playwright** (`apps/kiosk`, IPC stubbed the existing way): searching, found list, none found → IP, probe fail, probe ok → next step, Search again, admin-exit hotkey still works on this screen, axe.
  - **Hub Playwright:** the address row.

**Never:**
- Pairing, tokens or a `config.json` written by discovery (2.18).
- Trusting a typed IP without the fingerprint cross-check.
- Leaving the locked shell or disabling lockdown on this screen.
- Advertising before a branch exists.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Hub info | bootstrapped Hub | 200 `HubInfo` with branch, hostname, proto, fp | N/A |
| Hub not bootstrapped | no branch | 503 `NO_HUB`; no mDNS registration | N/A |
| Branch renamed | name changes | mDNS re-registered with new TXT | N/A |
| No multicast | daemon error | logged ≤ 1/min; Hub runs | N/A |
| Discover found | 2 Hubs (one twice) | 2 entries, deduped by fp, within 10 s | N/A |
| Discover none | nothing in 10 s | `discovery.done` empty → IP field | N/A |
| Incompatible | `proto` ≠ ours | listed, disabled | N/A |
| Probe ok | real Hub IP | `{hub}` with branch and fp | N/A |
| Probe nothing | closed port | `NO_HUB` within ~2 s | N/A |
| Probe not ours | HTTP server, wrong JSON | `NO_HUB` | N/A |
| Probe spoof | `HubInfo.fp` ≠ TLS cert fp | `NO_HUB` | N/A |
| Probe version | proto differs | `HUB_INCOMPATIBLE` | N/A |
| Bad input | `999.1.1.1` / text | `NO_HUB` without network | N/A |
| Select | pick a Hub | `selected_hub.json` written; no token/config | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-api/src/lan/listener.rs` -- `route()` ~l.192-209 (plain router), `respond()` l.240 (text/html only; add a JSON variant), `serve_web` l.168, ports l.25/27; `LanHub` needs branch name + fingerprint.
- `crates/arena-api/src/lan/tls.rs` -- `fingerprint()` l.32, `hostname()` l.129.
- `crates/arena-store/src/branch.rs:299` `find_branch_summary` -> `BranchSummary{name, branch_code}`.
- `crates/arena-core/src/status.rs:79` `HubStatus.tls_fingerprint`; `crates/arena-api/src/health.rs:197-208` builds `HubSnapshot`; `crates/arena-proto/src/snapshot.rs:12` (`HubSnapshot`), `:159` (`SnapshotBranch`).
- `crates/arena-tasks/src/lib.rs` (modules connectivity/housekeeping/lease) -- add `mdns.rs`; spawn in `crates/arena-hub/src/main.rs` beside `ConnectivityTask` (~l.27 import, ~l.212-233 spawns).
- `apps/hub/src/routes/settings/devices-display/+page.svelte` (`hc-row-hub` ~l.42), `apps/hub/src/lib/{snapshot.ts,events.ts}`, `apps/hub/tests/*` (pattern `devices-display.spec.ts`).
- `crates/arena-kiosk-svc/src/{config.rs (KioskConfig l.15-19, hub_url l.46), hub_link.rs (unprovisioned loop l.57-87, attempt l.126), runtime.rs:89, dev.rs:153-176 (dev provision), ipc/{pipe.rs,unix.rs}}`.
- `crates/arena-kiosk-core/src/{client.rs (parse_fingerprint l.111, CertMismatch capture l.63-98, pinned_config l.196), ipc.rs:84 (IpcMsg)}`; `crates/arena-proto/src/kiosk.rs:82` (`KioskHubState`); `crates/arena-proto/src/frame.rs:21` (`PROTOCOL_VERSION`).
- `apps/kiosk/src/routes/{+page.svelte,+layout.svelte}` (lock screen, link-status pill l.164-170, lockdown + admin exit), `apps/kiosk/src/lib/bridge.ts`, `apps/kiosk/src-tauri/src/bridge.rs`, `apps/kiosk/tests/*.spec.ts` (IPC stub pattern `status.spec.ts:8-37`), `playwright.config.ts` (:5174).
- `Cargo.toml` + README pin table (`check:pins`) -- add `mdns-sd`.

## Tasks & Acceptance

**Execution:**
- [ ] `crates/arena-proto/src/{discovery.rs,lib.rs,error.rs,kiosk.rs,snapshot.rs}`, generated TS -- constants, `HubInfo`, codes, `unpaired`, `lan`.
- [ ] `crates/arena-api/src/lan/listener.rs`, `crates/arena-api/src/health.rs` -- `/hub-info`, snapshot `lan`.
- [ ] `crates/arena-tasks/src/mdns.rs`, `crates/arena-hub/src/main.rs` -- advertiser.
- [ ] `crates/arena-kiosk-core/src/*`, `crates/arena-kiosk-svc/src/{discovery.rs,hub_link.rs,ipc/*,runtime.rs}` -- unpaired state, discover/probe/select.
- [ ] `apps/kiosk/**`, `apps/hub/src/routes/settings/devices-display/**`, `packages/ui/src/messages/en.json`, tests.
- [ ] `Cargo.toml`, `README.md` -- pin + docs.

**Acceptance Criteria:**
- Given a dev Hub (`dev seed`) and `arena-kiosk-svc --console` with no provisioning on the same machine, when the kiosk UI starts, then the Hub appears in the list within 10 s (or, where multicast is unavailable, typing `127.0.0.1` finds it) and selecting it writes `selected_hub.json` without creating a token.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` -- pass (known `arena-hub-ui` sidecar failures excepted); `cargo test --locked -p arena-hub --features dev-fixtures`, `-p arena-api --features dev-fixtures`, `-p arena-kiosk-svc --features dev-fixtures`; clippy incl. dev-fixtures; fmt.
- `pnpm --filter @arena/kiosk check && pnpm --filter @arena/kiosk test:ui`; `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell` (if port 5173 is held by the user's dev server, run against it and record); every `pnpm run check:*`; `pnpm run test:scripts`.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; on Sonnet). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH: mDNS-selected Hub not TLS-verified; any LAN host can pose | high | P | `select_hub` trusts TXT `fp` |
| 2 | BH: two Hubs collide on host `arena360.local.` | medium | P | constant host name |
| 3 | BH/ECH: lowest IPv4 / link-local chosen | medium | P | `min_by_key` |
| 4 | ECH: dedupe keeps a stale first resolution | low | P | direct |
| 5 | ECH: hub-info names unsanitised; uppercase fp rejected; link-local v6 typed | low–medium | P | direct |
| 6 | ECH: probe addr uses an unverified `lan_port` | low | R | later connections pin the fingerprint on 7400, so a mismatch fails safely |
| 7 | ECH: follower answers `/hub-info` | medium | P | no role check |
| 8 | BH/ECH: `selected_hub.json` never cleared | medium | P | stale choice |
| 9 | BH/ECH/VG: `provisioned` never notified | low | P | dead wake-up arm |
| 10 | ECH/BH: stale done ends a new search; dropped found sends | medium | P | no search id |
| 11 | ECH: spinner forever without done | medium | P | no client timeout |
| 12 | ECH: daemon rebuilt per intent while failing | low | P | retry on tick |
| 13 | ECH: unregister not awaited; leak on early return | low | R | the next tick re-registers; startup failure exits the process |
| 14 | BH: web port hard-coded | low | P (TXT `web`) | typed path keeps 7401 |
| 15 | BH: all probe failures read `NO_HUB` | low | R | the spec defines `NO_HUB` for a spoof; mismatch is logged |
| 16 | ECH: TLS head read may drop large WS heads | low | R | browser heads ≪ limit |
| 17 | ECH: chunked or non-UTF-8 `/hub-info` | low | R | only our Hub answers that route |
| 18 | BH: `/hub-info` exposes branch and fp to the LAN | low | R | intended by the spec (discovery data) |
| 19 | BH: README ports and wording wrong; firewall rule | medium | P (docs) / D (installer rule) | |
| 20 | VG: binary wiring untested | medium | P | extend `crates/arena-hub/tests/lan.rs` |
| 21 | VG: real mDNS conversion untested | medium | P | pure conversion test |
| 22 | VG: Search-again lifecycle untested | low | P | service-level test |
| 23 | BH: Copied status stale; no clipboard fallback | low | P | direct UI |
| 24 | VG: Tauri param names untested | low | R | stable names; a shell smoke would catch a rename |
