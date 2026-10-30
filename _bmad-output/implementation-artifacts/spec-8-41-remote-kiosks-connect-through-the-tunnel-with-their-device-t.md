---
title: 'Story 8.41: Remote kiosks connect through the Tunnel with their device token'
type: 'feature'
created: '2026-10-30'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** A kiosk outside the café network cannot pair with or reach its Hub: the tunnel (8.39/8.40) forwards to `127.0.0.1:7403`, which serves live viewers only.

**Approach:** The tunnel listener also serves `POST /pair` (code only) and admits the `paired` class with its device token; the Hub app's code screen offers "Remote device" with the tunnel address; the kiosk accepts that address in its Hub field, pairs over HTTPS and reconnects over `wss://<host>/`; the floor labels such devices "Remote".

## Boundaries & Constraints

**Always:**
- Through the tunnel only `live_view` (signed token) and `paired` (device token) are admitted. A bad or missing device token is `TOKEN_INVALID`; a revoked device is `TOKEN_REVOKED`; `kds`/`phone` are `TOKEN_INVALID`.
- Remote pairing needs the `tunnel` licence flag and a primary Hub, takes a pairing code only (no staff PIN), and registers a normal `paired` device (the device cap counts it).
- Remote sessions re-check the `tunnel` flag every keepalive tick and are dropped when it is gone; the Hub writes no intent and ends no session.
- Pairing and hello limiters key on `CF-Connecting-IP` when the connector supplies it.
- The floor shows "Remote" for a device whose live sessions all came through the tunnel.

**Never:** a LAN class (`kds`, `phone`) through the tunnel; a PIN pairing across the internet; ending a session because the tunnel went away; pinning claims the tunnel hop cannot keep.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| Remote pairing | tunnel flag, valid code via `POST /pair` | `200`, device registered, token returned | wrong code refused; rate limit 429 per client IP |
| No flag | `POST /pair` | refused | `FEATURE_NOT_ENTITLED` |
| PIN over tunnel | `{pin}` body | refused | `FORBIDDEN` |
| Remote connect | `hello{paired, token}` | `hello.ok`; tile `remote` on the floor | bad token: `TOKEN_INVALID` + close |
| Revoked | revoked device reconnects | refused | `TOKEN_REVOKED` |
| Browser class | `hello{kds or phone}` | refused | `TOKEN_INVALID` |
| Flag removed | connected remote device | connection dropped within 15 s; no session ended | reconnect: `FEATURE_NOT_ENTITLED` |

</intent-contract>

## Code Map

- `crates/arena-api/src/lan/listener.rs` -- `serve_tunnel` (head read, `POST /pair`, `CF-Connecting-IP`); `session.rs` -- `authenticate` (`Paired`/`Tunnel`), `require_tunnel_flag`, keepalive re-check; `pair.rs` -- `remote` argument; `registry.rs` -- `register_via`, `is_remote`; `mod.rs`/`health.rs` -- `Presence::remote` into the snapshot.
- `crates/arena-proto/src/{tunnel.rs,live.rs,snapshot.rs}` -- `parse_tunnel_host`, `LiveTile.remote`, `SnapshotDevice.remote`; `crates/arena-core/src/{live_floor.rs,snapshot.rs}`.
- `crates/arena-kiosk-core/src/{client.rs,pair.rs}` -- `web_config`, `connect_ws_remote`, `HubClient::connect_remote`, `pair_remote`; `crates/arena-kiosk-svc/src/{discovery.rs,pairing.rs,config.rs,hub_link.rs}`.
- `apps/hub/src/lib/PairKioskPanel.svelte`, `apps/hub/src/routes/floor/+page.svelte`, `apps/kiosk/src/lib/Discovery.svelte`, `apps/owner/src/lib/LiveBoard.svelte`, `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

- [x] Hub: tunnel `POST /pair` (code only, flag-gated), `paired` admission through the tunnel, flag re-check, real-client limiter key (`crates/arena-api`).
- [x] "Remote" on the floor and the owner Live view (`remote` on `SnapshotDevice`/`LiveTile`, registry transport flag).
- [x] Kiosk client: tunnel address in the Hub field, remote pairing, remote reconnect (`arena-kiosk-core`, `arena-kiosk-svc`, `apps/kiosk`).
- [x] Hub app: "Remote device" on the pairing code screen showing the tunnel address.
- [x] Tests: `arena-api --test remote_device` (6), `live_view` updated, `arena-proto` `parse_tunnel_host`, `arena-kiosk-core` `post_pair`, `arena-kiosk-svc` remote discovery/selection, Playwright (`pair-kiosk`, kiosk `discovery`, owner `live`).

**Acceptance Criteria:**
- Given an entitled branch, when staff choose "Remote device", then the code screen also shows the tunnel hostname, and a kiosk outside the LAN pairs by typing it and exchanging the code for a device token.
- Given a remote kiosk with its token, when it connects through the tunnel, then it is admitted only as `paired`, appears on the floor as "Remote" and counts toward the device cap.
- Given no valid device token, when `hello` arrives through the tunnel, then it is refused `TOKEN_INVALID` and closed; no `kds`/`phone` class is accepted.
- Given the `tunnel` flag removed, then remote kiosks lose the connection and no session is ended by the Hub.
- Given a revoked remote device, when it reconnects, then it is refused `TOKEN_REVOKED`.

## Spec Change Log

## Review Triage Log

Code review (2026-10-30): no blocking findings. fmt, clippy and the tests (`remote_device`, `live_view`, proto, core, kiosk-core, api lib, kiosk-svc discovery) pass. Noted, not blocking: with no `CF-Connecting-IP` from the connector, remote devices share the loopback limiter key.

## Design Notes

**TLS pinning (deviation).** The AC says "TLS pinned to the Hub certificate". Through a Cloudflare Tunnel the TLS ends at Cloudflare's edge, so the Hub certificate is never presented to the kiosk. The remote kiosk verifies the web PKI for the tunnel hostname and the Hub verifies the device token; the Hub fingerprint is still returned by `POST /pair` and stored. Real end-to-end pinning would need an inner TLS inside the WebSocket and is not done here.

**Cloudflare Access.** The hostname is fronted by an Access app (8.39). A non-browser kiosk needs a service-token policy to pass it; that policy and sending its headers from the kiosk are not part of this story.

**Remote vs LAN presence.** "Remote" is a property of the live session (transport), not of the device row, so a device that is also reachable on the LAN reads as local and nothing is persisted or synced.

## Verification

- `cargo test -p arena-proto -p arena-core -p arena-api -p arena-kiosk-core` -- pass (incl. `remote_device`, `live_view`).
- `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo clippy -p arena-kiosk-svc --lib --bins -- -D warnings`, `cargo fmt --check` -- clean.
- `pnpm check:strings`, `check:protocol`, `format:check`; `svelte-check` owner and kiosk; Playwright hub `pair-kiosk`, kiosk `discovery`, owner `live` -- ok.

## Auto Run Result

Status: done

Implemented as above. Known failures not from this story (also failing on a clean tree): `arena-cloud --test backup a_hung_run_times_out_with_its_own_code`, 14 `arena-kiosk-svc --lib` Windows lockdown/pipe/DPAPI tests, `arena-kiosk-svc` test-target clippy, `arena-e2e` (Unix-only), hub `svelte-check` (`playwright.kitchen.config.ts`).

Residual risks: no real `cloudflared`, Cloudflare Access service-token policy or kiosk-over-internet run; pinning not possible on the tunnel hop (see Design Notes); the remote kiosk's media/library fetches on `:7401` are LAN-only and do not work through the tunnel; the pairing code store is shared, so an in-café and a remote pairing cannot run at the same time.
