---
title: 'Story 8.40: Live view — a read-only floor through the Tunnel'
type: 'feature'
created: '2026-10-30'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner on Premium (or with the Tunnel add-on) cannot watch the floor of a branch from outside the café: the tunnel (8.39) forwards to `127.0.0.1:7403`, where nothing listens, and the cloud issues no way to authenticate a viewer.

**Approach:** The cloud signs a branch-scoped live token (≤ 5 minutes) after checking tier, `tunnel` flag, Hub platform and tunnel; the Hub opens a loopback listener for the tunnel and admits a new read-only `live_view` client class that presents the token in `hello`; the Hub pushes one trimmed floor frame (`live.floor`) on connect, on floor changes and every second; the owner app renders it.

## Boundaries & Constraints

**Always:**
- `GET /v1/live/{branch}` (owner session): Free or no `tunnel` flag → `FEATURE_NOT_ENTITLED`; Android Hub → `PLATFORM_UNSUPPORTED`; no Hub or no ready tunnel → `UNAVAILABLE`; else `{url: wss://<branch>.tunnel…/live, token, expires_at}`.
- The token is an EdDSA JWT from the licence signer with its own claims (`aud: "live"`, `tenant_id`, `branch_id`, `iat`, `exp`, `exp - iat ≤ 300`); a licence token cannot pass as a live token or the reverse (different required fields).
- The Hub checks signature (licence verify keys), `aud`, `branch_id` (its own), expiry against trusted time, and its own `tunnel` flag. Failures: `TOKEN_INVALID` (bad/expired/other branch), `FEATURE_NOT_ENTITLED`, `NOT_PRIMARY` (follower). Then it closes.
- A `live_view` session may only send `ping`/`pong` and `live.token` (re-auth). Any other frame → `FORBIDDEN`, session stays open. It never sees applied-change summaries. The session ends with `TOKEN_INVALID` when its token expires without a fresh `live.token`.
- `live.floor` carries zones, tiles (device id, label, zone, state, session start/pause/end times, `player_id` or guest number) and `generated_at`; no names, balances, bills or orders.
- The listener is loopback only, serves a WebSocket at `/` or `/live`, everything else 404; it never blocks trading (a bind failure is logged).
- App: Live view in the desktop sidebar and a button on each Today card (phone). No `tunnel` flag → `tier-lock` "Available on Premium"; Android Hub → "Needs the Windows Hub"; unreachable Hub → "Branch offline" with a link back to Today. Token refresh at 4 minutes over the open socket.

**Never:** intents or commands from a live viewer; kiosk traffic through the tunnel (8.41); an "Upgrade" link (Epic 10); player names.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| Entitled | Premium, Windows Hub, tunnel ready | grant; WS hello ok; `live.floor` at once, then every ≤ 1 s | — |
| No flag | Basic without tunnel | tier-lock "Available on Premium" | `FEATURE_NOT_ENTITLED` |
| Android | flag, Android Hub | "Needs the Windows Hub" | `PLATFORM_UNSUPPORTED` |
| Expired token | exp passed | refused | `TOKEN_INVALID` |
| Other branch | token for branch B at Hub A | refused | `TOKEN_INVALID` |
| Intent frame | any `intent.submit`, `settings.get` … | reply | `FORBIDDEN`, session kept |
| Refresh | `live.token` with fresh token | `ack`, deadline extended | bad token → `TOKEN_INVALID`, old deadline kept |
| Unreachable | WS fails | "Branch offline" + Today link | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/live.rs` (new) -- `LiveClaims`, `LiveTokenError`, `verify_live_token`, `LiveGrant`, `LiveFloor` & friends; `licence.rs` -- shared decode; `scope.rs` -- `ClientClass::LiveView`, empty scope; `frame.rs` -- `live.token` frame, `live.floor` event; `error.rs` -- `PLATFORM_UNSUPPORTED`.
- `crates/arena-core/src/devices.rs` -- `DeviceDirectory::live_floor`, `live_gate` (branch id + tunnel flag).
- `crates/arena-api/src/lan/{session,registry,listener,mod}.rs` -- `Transport::Tunnel`, live authentication and expiry, floor push, tunnel listener, `LanHub::with_live_view`.
- `crates/arena-hub/src/lib.rs` -- binds the tunnel listener (Windows) and passes the verify keys.
- `crates/arena-cloud/src/{licence.rs,api/live.rs,api/mod.rs,api/error.rs}` -- signing, `GET /v1/live/{branch}`.
- `apps/owner/src/{routes/live,lib/live.ts,lib/LiveBoard.svelte,lib/TodayCard.svelte,routes/+layout.svelte}`, `packages/ui/src/messages/en.json`, `apps/owner/tests/live.spec.ts`.

## Tasks & Acceptance

**Execution:**
- Protocol, Hub authentication/listener/push, cloud route, owner app, tests for each matrix row.

**Acceptance Criteria:**
- Given an entitled branch, when the owner opens Live view, then the app gets a ≤ 5-minute token and opens the WebSocket to the tunnel hostname.
- Given a valid token, when the Hub receives `hello`, then it admits a `live_view` session that can only subscribe; any other frame gets `FORBIDDEN` and nothing closes.
- Given sessions starting and ending, when the floor changes, then the viewer gets a new `live.floor` within 2 s with a data age.
- Given 4 minutes, then the app refreshes the token on the open socket; an expired token is `TOKEN_INVALID`.
- Given no `tunnel` flag / Android Hub / unreachable Hub, then the tier-lock, "Needs the Windows Hub" or "Branch offline" shows.

## Spec Change Log

## Review Triage Log

## Design Notes

There is no subscribe frame in the LAN protocol (state is pushed from registration), so "may only subscribe" means registration; the floor is a separate read model so the paired-device `SessionScreen` (balances, orders) is never exposed. Cloudflare Access fronts the hostname; the browser's Access session/CORS and an `Origin` allow-list at the Hub listener are not handled here (the signed token is the authority).

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-core -p arena-api -p arena-cloud` -- expected: pass
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` -- expected: clean
- `pnpm -C apps/owner check`, `pnpm check:strings`, `pnpm format:check`, owner Playwright `live.spec.ts` -- expected: ok

## Auto Run Result

Status: done

Implemented: the cloud route `GET /v1/live/{branch}` (entitlement from the signed claims, Hub platform and tunnel row; a ≤ 5-minute `LiveClaims` token signed with the licence key), the read-only `live_view` client class and its loopback tunnel listener on the Hub (token, branch, trusted-time expiry and `tunnel` flag checks; `FORBIDDEN` for anything but ping/pong/`live.token`; closes with `TOKEN_INVALID` at expiry), the `live.floor` feed (hello, floor changes, 1 s tick, no names/money), and the owner app's Live view (sidebar, Today-card link, tier-lock, "Needs the Windows Hub", "Branch offline", 4-minute token renewal on the open socket).

Verification: `cargo fmt --check` and `cargo clippy -D warnings` clean for arena-proto/core/api/cloud/hub/store/tasks/kiosk-core (kiosk-svc lib); new tests pass: arena-proto `live` + `scope`, arena-core `live_floor` (3), `arena-api --test live_view` (6), `arena-cloud --test live` (5); owner Playwright `live.spec.ts` (8 × 2 viewports), whole owner suite green after updating `shell.spec.ts`/`theme.spec.ts` for the desktop-only nav entry; `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `format:check` ok, `svelte-check` owner/kiosk/kitchen/onboarding/admin ok.

Known failures not from this story (also failing on a clean tree): `arena-replay replaying_the_seeded_log…`, `arena-cloud --test binary unreachable_at_start…`, `arena-api ui_auth lock_and_unlock_log…` (log-capture race), `arena-kiosk-svc` Windows lockdown/pipe tests and its test-target clippy, `arena-e2e` (Unix-only), hub `svelte-check` (`playwright.kitchen.config.ts`), `check:tokens-used` (`--ad-reserved`).

Residual risks: no real `cloudflared`/Cloudflare Access/browser run (the Access session and `Origin` handling for a browser WebSocket to the tunnel hostname are not exercised; the Hub trusts the signed token only); `LICENCE_VERIFY_KEYS` ships empty, so the live view works only with dev keys until the release key is baked in; the 1 s floor read opens a database read per tick only while a viewer is connected.

## Code Review (2026-10-30)

Fixed: cloud `PLATFORM_UNSUPPORTED` now shows "Needs the Windows Hub" (was "Branch offline"); connection no longer restarts when a shell refresh replaces branch objects; unknown tile state falls back to offline; malformed `live.floor` ignored; `new WebSocket` throw is treated as unreachable Hub.

Deferred (not blocking): no auto-reconnect/retry on dropped socket; tunnel connections share the LAN connection cap and a failed hello costs one local DB read (the tunnel is loopback behind Cloudflare Access); a demoted Hub keeps pushing its floor until the ≤ 5-minute token lapses; no direct unit tests for `tileView`/`byZone`, change-driven push timing, or the real Hub wiring in `arena-hub::run`; `NO_HUB` is used for "no Hub/tunnel not ready" (no `UNAVAILABLE` code exists).
