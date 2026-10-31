---
title: "Story 10.19: Kiosks follow the new Windows Hub without pairing again"
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: 'feadeede'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-17-the-android-hub-hands-over-a-final-snapshot-and-steps-down.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-18-the-windows-hub-imports-the-snapshot-as-the-next-epoch.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** After the move a kiosk still pins the tablet. The tablet cannot tell it where the branch went, the kiosk has no way to trust another Hub, and the snapshot blanks every device token, so the Windows Hub would refuse every kiosk.

**Approach:** The Windows Hub sends its LAN address and certificate fingerprint with the handover claim. When the tablet stops, it signs `hub.successor {address, fingerprint, branch_id, expires_at = +24 h}` with its own TLS key and pushes it to every connected kiosk, which checks and stores it beside its pin. When the tablet closes the kiosks' links at the acknowledgement (or is unreachable), the kiosk connects to the successor, accepts only the named fingerprint, presents its existing token and, on success, replaces its pin. A `migrate` snapshot keeps the kiosks' token hashes so the Windows Hub accepts them.

## Boundaries & Constraints

**Always:** the kiosk trusts a successor only if the signing certificate hashes to its current pin, the signature verifies, the branch id is the one its Hub told it, and it has not expired; the successor is tried only when the current Hub refuses, closes or is unreachable; a rejected successor leaves the old pin untouched and is reported as a `kiosk.log` event (`successor_rejected`) to the Hub the kiosk next reaches; the kiosk's token is the same and is never written to `config.json`; a successor that is not yet serving (`RESTORE_PENDING`) is retried at the minimum back-off until it expires.

**Never:** no change to the `.a360` format or the Hub schema; no change to licence or fence wire shapes; export files that are not `purpose: migrate` still blank every token hash; kitchen displays and a kiosk offline for the handover are not followed (they pair again).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Claim with the PC's address | | tablet signs and pushes `hub.successor` to each connected kiosk; kiosk stores it |
| Claim without an address (older PC) | | nothing pushed |
| Successor, bad signature / other branch / expired / certificate not the pin | | ignored, old pin kept, `successor_rejected` logged |
| Tablet closes the link at the acknowledgement | kiosk holds a good successor | kiosk connects to the successor, token accepted, `config.json` now names the PC, successor removed |
| Successor Hub not serving yet | `RESTORE_PENDING` | kiosk keeps the successor and retries |
| Successor presents another fingerprint | | `PIN_MISMATCH`: successor dropped, old pin kept, logged |
| Current Hub serves again (move cancelled) | good hello | stored successor dropped |
| Migrate snapshot | | device token hashes kept; any other export blanks them |

</intent-contract>

## Code Map

- `crates/arena-proto/src/handover.rs` (`SuccessorOffer`, `HandoverClaim.successor`), `frame.rs` (`HubSuccessor`, `SignedSuccessor`, `HubEvent::Successor`, `HelloOk.branch_id`), generated TS.
- `crates/arena-api/src/lan/tls.rs` (`HubCert::sign`, `cert_der`), `lan/mod.rs` (`with_signer`, announce at claim, close sessions at ack), `lan/registry.rs` (push, close), `lan/session.rs` (`hello.ok` branch id), `lan/handover.rs`.
- `crates/arena-store/src/export.rs` (`copy_and_strip` keeps `devices.token_hash` for `migrate`).
- `crates/arena-tasks/src/handover_pull.rs` (claim carries the PC's address and fingerprint), `crates/arena-hub/src/lib.rs` (wiring).
- `crates/arena-kiosk-core/src/successor.rs` (verify), `client.rs`; `crates/arena-kiosk-svc/src/{hub_link,successor}.rs`.
- Tests: `arena-kiosk-core` successor unit tests, `arena-store` export tests, `arena-api/tests/handover.rs`, `arena-kiosk-svc/tests/runtime.rs`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: successor types, claim offer, `HelloOk.branch_id`; regenerate TS
- [x] Hub: sign and push at the claim, close links at the acknowledgement, branch id at hello, PC sends its address
- [x] Store: `migrate` exports keep device token hashes
- [x] Kiosk: verify, store, follow, drop; log rejections
- [x] Tests for every I/O row; README

**Acceptance Criteria:**
- Given a kiosk connected to the tablet, when the PC claims with its address and the PC then acknowledges, then the kiosk ends up connected to the PC with its old token and `config.json` names the PC.
- Given a bad successor, when the kiosk checks it, then it keeps its pin and logs `successor_rejected`.

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api -p arena-hub -p arena-kiosk-core -p arena-kiosk-svc --all-targets --features arena-store/control -- -D warnings`
- `cargo test --no-fail-fast` on the same crates with `--features arena-store/control`
- `pnpm gen:types`; `pnpm run check:strings`, `check:generated`

## Auto Run Result

Status: review. Risks and deviations to know: (1) the `arena-kiosk-svc` runtime tests for following (`a_kiosk_whose_hub_is_gone_follows_the_successor...`, the rejected-successor and fingerprint-mismatch tests) are `#![cfg(unix)]` and could not be compiled or run on this Windows machine; the pieces they cover are unit-tested (`arena-kiosk-core::successor`) and the tablet side is covered end to end in `arena-api/tests/handover.rs` (successor pushed, verified, links closed at the acknowledgement, no address means nothing pushed). (2) The kiosk trusts the signing certificate by hashing it to its pin and reading the P-256 key out of the DER by its `BIT STRING` header; a certificate of another key type is rejected as malformed. (3) A kiosk offline during the handover, and any move by file, still end in "Pair again" (no successor). (4) Expiry uses the kiosk's clock against the tablet's `expires_at`. (5) The 20-kiosk 30 s LAN proof belongs to Story 10.20.

Verify gate (2026-10-31): fmt, clippy `-D warnings` (all listed crates except `arena-kiosk-svc` tests, whose existing clippy errors in `tests/discovery.rs` and `launchers/tests.rs` are untouched; its lib is clean), string / hex / tier-lock / secrets / blur checks and the hub `check` (only the known `playwright.kitchen.config.ts` errors) pass; `check:generated` passes once the regenerated files are committed. Rust tests: all pass except the known environmental `arena-cloud binary::unreachable_at_start...`, `arena-tasks trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan a_printer_that_stops_reading_times_out`, and 14 `arena-kiosk-svc` lib tests that need Windows administrator rights (DPAPI, pipe ACLs, lockdown, config owner check).
