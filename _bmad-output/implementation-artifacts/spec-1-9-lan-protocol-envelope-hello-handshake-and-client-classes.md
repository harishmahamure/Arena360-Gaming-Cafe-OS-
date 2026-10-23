---
title: 'Story 1.9: LAN protocol envelope, hello handshake and client classes'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '6eaa21dcea60511961671a96cea99a3ded83081b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-8-idempotent-intents-optimistic-row-versions-and-property-test.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** No device can talk to the Hub yet. Without one authenticated, versioned protocol with class scopes, guests on the shared café Wi-Fi could reach staff, money or device actions.

**Approach:**
- **Hub listeners:** TLS WebSocket on `:7400` using a self-signed certificate generated once at first start. `:7401` sniffs each connection: TLS goes to paired apps, plain HTTP and WS by path go to browser classes.
- **Protocol:** one `{v, kind, rid}` JSON envelope, a mandatory `hello` handshake, and argon2id device tokens stored in a new `devices` table.
- **Scopes and revocation:** class scopes live in `arena-proto`. A `device.revoke` intent closes the device's socket.
- **Client and firewall:** a shared Rust client in `arena-kiosk-core` with pinned TLS and 5 s `HUB_SLOW` timeouts, plus a Windows firewall script for the future installer.

## Boundaries & Constraints

**Always:**
- **Pins:** exact, with README rows: `tokio-tungstenite` 0.30.x, `rustls`, `tokio-rustls`, `rcgen`, `sha2`, `argon2` 0.6.x, `serde_json`, plus `futures-util` if needed.
- **Certificate:**
  - At first start, generate a self-signed ECDSA P-256 certificate with `rcgen`. Its SAN is `arena360.local` plus the hostname.
  - Store it as `<data>/tls/hub.crt` and `hub.key`; the key file gets owner-only permissions where the OS supports them.
  - Store the SHA-256 fingerprint (lowercase hex) in `hub_meta.tls_fingerprint`, through the writer as a meta write like the instance id.
  - Reuse the stored certificate on later starts. `/healthz` gains `tls_fingerprint`.
- **Listeners:**
  - Bind `0.0.0.0:7400` (TLS, WS at `/`) and `0.0.0.0:7401`. The ports can be overridden with `--lan-port`/`--web-port` for tests.
  - On `:7401`, peek the first byte: `0x16` means TLS for paired apps (same handler as 7400). Anything else is plain HTTP:
    - `GET /ws` upgrades to WS for the `kds`/`phone` classes;
    - `/kitchen` and `/console/<tv>` return a 200 placeholder;
    - every other path returns 404.
  - Only the primary serves intents; a follower answers `hello` and then returns `NOT_PRIMARY` for every intent.
- **Frames:**
  - UTF-8 JSON text frames: `{v: 1, kind, rid, ...body}`. All frame types live in `arena-proto` and are exported to TS.
  - The kinds are `hello`, `hello.ok`, `intent.submit`, `intent.result`, `event`, `error` and `ping`/`pong`.
  - Unknown fields are ignored; an unknown `kind` gets `error{UNSUPPORTED}` with the same `rid`; an unparseable frame gets `error{BAD_FRAME}`. None of these closes any session. A `v` other than 1 gets `UNSUPPORTED`.
- **Hello handshake:**
  - The first frame must be `hello{credential, class: paired|kds|phone, versions: {app: "x.y.z"}, capabilities: [string]}`. Any other first frame gets an error frame and the connection is closed.
  - **Credential by class:**
    - `paired` and `kds` present a device token, verified with argon2id against `devices` rows that are of that class and not revoked. A wrong or unknown token gets `TOKEN_INVALID` and is closed; a revoked token gets `TOKEN_REVOKED` and is closed.
    - `phone` is anonymous and its credential is ignored. Its real per-console token arrives in a later epic.
  - `paired` is accepted only over TLS; `kds` and `phone` only on `:7401/ws`.
- **Version window:**
  - A release is a `major.minor` pair. A paired client is accepted when it is in the same major and `client.minor >= hub.minor - 2`.
  - Otherwise it gets `UNSUPPORTED` with the message naming the minimum `x.y`, and the connection is closed.
  - The reported version is recorded on the device row through an intent `device.version_reported`. That intent uses the system actor and runs only when the version changed.
- **Sessions:**
  - The actor is derived from the session as `{device, device_id}` and never from frame contents; any client-supplied actor is overwritten.
  - `intent.submit{intent}` goes through `Writer::submit_intent` and the reply is `intent.result{rid, applied}` or `error{rid, AppError}`.
  - Every `Applied` from the writer broadcast is pushed to every connected session as `event{kind:"applied", intent_id, changes}`. Clients never poll.
  - A slow or failing session never blocks the others: each session has its own bounded queue, and a full queue drops the session.
- **Scopes:**
  - `arena-proto` has `IntentKind::category()` (branch|settings|device|staff|money|kitchen|console) and `class_scope(class) -> &[IntentKind]`.
    - `paired`: none yet.
    - `kds`: kitchen only.
    - `phone`: `console.plan_requested` only, a kind that exists in the scope table but has no handler yet.
  - An intent outside the session's scope gets `FORBIDDEN` and a `warn` log line with the class, kind and device id.
  - A unit test asserts that no `kds` or `phone` scope contains a staff, money or device intent.
- **Devices (migration 0004):**
  - `devices(device_id TEXT PK, branch_id, class CHECK(class IN ('paired','kds')), label, token_hash TEXT NOT NULL, reported_version TEXT, revoked_at INTEGER, rev, created_at, updated_at)`.
  - New intents, all through `apply()` with change rows:
    - `device.register{class, label, token_hash}`: dev/system only; the hash is computed outside the transaction.
    - `device.revoke{device_id}`: needs permission `device.remote`.
    - `device.version_reported`.
  - New `Permission` variants `device.remote` and `device.pair`. The permission stub remains "system actor allowed" until Story 1.11.
  - `dev seed` also registers one `paired` kiosk device labelled "Dev Kiosk" and prints its token once.
- **Revocation:**
  - The dev command `arena-hub dev revoke-device <device_id>` (feature `dev-fixtures`) calls a dev-only loopback endpoint `POST /dev/revoke-device` on the health port. That endpoint exists only with the feature, and the running Hub applies the intent.
  - After that `Applied`, any session for that device gets `error{TOKEN_REVOKED}` and a close within 1 s, and later hellos with that token are refused.
- **Client (`arena-kiosk-core`):**
  - `HubClient::connect(url, pinned_fingerprint, hello)` uses rustls with a custom verifier that accepts only the pinned SHA-256 fingerprint, with no fallback.
  - `request(kind, body) -> Result<Frame, ClientError>` has a 5 s timeout per request (const) that returns `ClientError::HubSlow` without blocking other concurrent requests, using a map keyed by `rid`.
  - An `events()` stream delivers pushed events.
  - It depends only on `arena-proto`.
- **Firewall:** `scripts/windows/firewall.ps1` adds inbound allow rules for TCP 7400 and 7401 on the Private profile only, plus a removal mode. It is not run by CI. The README documents that the installer story calls it.
- **New `ErrorCode`:** `TOKEN_INVALID`. Regenerate the TS types.

**Never:**
- mDNS or real pairing (Epic 2).
- Kitchen or console features beyond placeholders.
- Polling endpoints.
- Unpinned TLS in the client.
- Any inbound port other than 7400 and 7401 (the health port stays loopback).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Paired hello | the seeded token, class paired, current version, over TLS | `hello.ok`; version recorded; later applies are pushed as `event` | N/A |
| Wrong token | a garbage token | `error TOKEN_INVALID`, closed | N/A |
| First frame not hello | `intent.submit` first | an error frame, closed | N/A |
| Unknown kind | `{v:1, kind:"foo", rid:"7"}` after hello | `error UNSUPPORTED rid 7`; the session stays open | N/A |
| Malformed | `{not json` | `error BAD_FRAME`; this session and other sessions stay open | N/A |
| Unknown fields | an extra `x: 1` in hello | accepted | N/A |
| Old client | hub 0.5, client 0.2 | `UNSUPPORTED` "minimum 0.3"; 0.3 is accepted | closed |
| Revoke | revoke-device while connected | `TOKEN_REVOKED` and a close within 1 s; a reconnect is refused | N/A |
| kds scope | a kds session submits `device.revoke` | `FORBIDDEN`, logged | N/A |
| Paired on plain | a paired hello on `:7401/ws` | rejected, closed | N/A |
| Sniff | TLS ClientHello on 7401 / `GET /kitchen` on 7401 / `GET /nope` | WS-over-TLS works / 200 / 404 | N/A |
| Pinning | the client is given the wrong fingerprint | the connect fails, with no fallback | N/A |
| Slow | the hub delays a reply by 6 s, while a second request answers at once | the first gets `HubSlow` after about 5 s; the second succeeds | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-api` -- the healthz router (axum) on loopback. Add a `lan/` module with the listeners, session and registry; layering is api → core, proto.
- `crates/arena-core` -- the Writer (`submit_intent`, the `Applied` broadcast), `apply/` and HubStatus. Add device intents and argon2 verify helpers; hashing happens outside the transaction.
- `crates/arena-store` -- add migration 0004 and the devices queries.
- `crates/arena-hub/src/{main.rs,dev.rs,config.rs}` -- wire the listeners and the new flags; extend `dev seed` and add `dev revoke-device`.
- `crates/arena-proto` -- the frames, class scopes, categories and permissions; then `pnpm gen:types`.
- `crates/arena-kiosk-core` -- the placeholder; add the client.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/{frame.rs,scope.rs,intent.rs,permission.rs,error.rs}` + generated TS -- protocol types and the scope test.
- [x] `crates/arena-store/{migrations/0004_devices.sql,src/devices.rs}` -- the table and queries.
- [x] `crates/arena-core/src/{devices.rs,apply/*}` -- device intents and verification.
- [x] `crates/arena-api/src/lan/{tls.rs,listener.rs,session.rs,registry.rs}` -- the server, with integration tests in `crates/arena-api/tests/lan.rs` for every matrix row.
- [x] `crates/arena-kiosk-core/src/client.rs` -- the client and its tests (pinning, HubSlow, concurrency).
- [x] `crates/arena-hub/src/*` -- wiring, the dev commands and the dev endpoint behind the feature.
- [x] `scripts/windows/firewall.ps1`, `README.md`, `Cargo.toml`, lockfile -- the firewall script, docs and pins.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` and the `dev-fixtures` test step, then every matrix row passes on Linux and Windows.
- Given `pnpm check`, then it passes, including generated freshness and layers.
- Given a seeded Hub running, when a paired client connects with the printed token, then it receives `hello.ok`, and revoking the device closes it.

## Implementation Notes

- **Token format** is `<device_id>.<secret>` (ULID + 64 lower-case hex from the OS RNG). The id selects one `devices` row and argon2id verifies the secret against its `token_hash`, so a hello costs one hash check, not one per row. A wrong secret on a revoked device is `TOKEN_INVALID`, so revocation is only revealed to the token holder. `device.register` mints the id in mutate; `dev seed` hashes the secret before submitting.
- **Token checks never use the writer.** `arena_core::DeviceDirectory` holds its own read-only connection (`Store::open_reader`, no writer lock, no migration), and argon2 runs on `spawn_blocking`. This works the same on a follower. argon2/blake2 are built at `opt-level = 3` in the dev profile, because unoptimised argon2id makes debug tests crawl.
- **Scope gate order** for `intent.submit`: primary (`NOT_PRIMARY` for every intent on a follower), then class scope on both the raw `intent.kind` and `intent.payload.kind` before the typed parse (`FORBIDDEN` plus a `warn` line with class, kind and device id), then a handler (`console.plan_requested` gets `UNSUPPORTED`), then the typed parse (`BAD_FRAME`). The session actor then overwrites the frame's actor. The paired and kds scopes are empty, so no LAN intent reaches the writer yet; the submit path runs only once a scope gains a handled kind.
- **Frames:** `event` nests its body as `event: {kind: "applied", intent_id, changes}` and `error` as `error: AppError`, because the top-level `kind` is the frame kind. Pushes carry Hub-minted rids `ev-<n>`; an error frame with an empty rid is a session-level error such as `TOKEN_REVOKED`.
- **Revocation:** after any `Applied` that touches a `devices` row (and after a lagged broadcast, for every connected device), the fan-out re-reads the row and kills that device's sessions with `TOKEN_REVOKED`. A kill reason that is an error is never overwritten by a plain drop (a flaky test found this). The session re-checks revocation right after it registers, which closes the race between the hello check and a revoke.
- **Version window:** only `paired` is version-checked (`Release::accepts`). `device.version_reported` (actor `{system, "hub-lan"}`) is submitted only on a primary, and only when the version differs from the row's.
- **Follower certificate:** a follower reuses the writer's cert files, or serves an in-memory one if none exist yet, and never writes files or `hub_meta`. `record_tls_fingerprint` writes only when the value changed.
- **Health / dev endpoint:** `arena_api::health::dev_router` exists only with `arena-api/dev-fixtures` (enabled by `arena-hub/dev-fixtures`) and is merged into the loopback health router. `dev revoke-device` talks to it over a raw HTTP/1.1 request on `127.0.0.1:<health-port>`.
- **Layer rule:** `arena-kiosk-core` now has a `check:layers` row (`arena-proto` only).
- **`check:generated`** compares against `HEAD`, so it passes only once the regenerated `packages/ui/src/generated` is committed. Regeneration is idempotent.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Kill set before `subscribe()` is missed, so a revoked or shut-down session stays open (blind, edge) | high | A `watch::subscribe` treats the current value as already seen. | patch |
| 2 | Lagged broadcast loses `applied` events; clients never resync (blind, edge) | medium | `Lagged` only re-checks revocations. | patch (drop all sessions on lag) |
| 3 | `applied` events go to every class, including anonymous phone and kds | high | `push_all` leaks device row ids and, later, staff/money changes to browser classes. | patch |
| 4 | Unthrottled argon2 work on hello, plus a timing oracle for device ids (blind, edge) | medium | No semaphore or backoff; a missing row returns fast. | patch |
| 5 | No connection cap, idle timeout or keepalive (blind, edge) | medium | Half-open sockets go undetected; unbounded tasks. | patch |
| 6 | `acquire_owned().await` inside `select!` blocks kill handling (blind, edge) | medium | 16 in-flight intents stall revocation past 1 s. | patch |
| 7 | Shutdown doesn't drain sessions; handshaking sessions survive `kill_all` (blind, edge) | low | No task tracking or shutdown flag. | patch |
| 8 | TLS files: one file missing regenerates silently; shared and possibly stale tmp file; fingerprint change unlogged (blind, edge) | medium | Pinned clients break silently, or the key ends up world-readable. | patch |
| 9 | `Closed.error` records a late request error as the close reason (blind, edge) | low | Any error frame sets `last_error`. | patch |
| 10 | Token argon2 hash is in the change log and in dedupe records | low | The secret is 256-bit random, so an offline attack on the hash is infeasible. The standby needs the row to accept devices after failover. | reject |
| 11 | Anonymous phone recorded as a `device` actor | low | The spine's actor kinds are fixed at five; the `phone:` prefix distinguishes it, and no phone intent is handled yet. | reject |
| 12 | `firewall.ps1` hard-codes ports and doesn't warn on a Public profile | low | Direct parameters and a warning. | patch |
| 13 | Untested: hello timeout, lag re-check, max frame size, in-flight backpressure; arena-api dev-fixtures tests not in CI (blind, verify-gap) | low | Pre-verified gaps. | patch |
| 14 | A revoked device's intent is applied in the fan-out window | low | Check device revocation in the permission step. | patch |
| 15 | An `is_revoked` DB error is reported as TOKEN_REVOKED | low | It should be INTERNAL. | patch |
| 16 | A failed `close_revoked` check leaves the session open | low | Drop on error. | patch |
| 17 | Mutex poisoning in the registry | low | Only reachable after a panic elsewhere. | reject |
| 18 | Fingerprint parser accepts `+` | low | Validate hex digits. | patch |
| 19 | Requests after the reader exits wait 5 s, then report HubSlow | low | Check `closed` first. | patch |
| 20 | An empty-rid error leaves the pending request hanging | low | Fail the pending request. | patch |
| 21 | Concurrent `version_reported` causes duplicate writes | low | Pass `base_rev`. | patch |
| 22 | Unsorted dedup; hard-coded label limit; schema query on every hello | low | Direct fixes. | patch |
| 23 | Revoke during handshake untested (verify-gap, deferred by reviewer) | low | Covered by #1's fix and its test. | reject |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check` -- expected: exit 0
