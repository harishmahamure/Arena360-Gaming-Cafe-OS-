---
title: 'Story 6.8: A player logs in at the kiosk with username and password'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: 'd6e1b41eaa96766c807a620b6fe756c8ec667fa8'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-7-the-attract-screen-shows-branding-rates-and-the-active-rule.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The attract screen's login panel (6.7) is an empty shell: a player cannot identify themself at the PC, so every session needs the counter. Nothing carries a username and password from the kiosk to the Hub, checks them, or tells the kiosk who logged in.

**Approach:** The panel gets the login form. The app sends `{username, password}` to the Kiosk service, which sends the paired-only LAN frame `player.authenticate`; the Hub verifies the Argon2id hash, throttles by device, records the login and answers a typed outcome (`ok{player}`, `rejected`, `locked{retry_s}`, `busy{device_name}`). On `ok` the app shows a holding step for that player until the session starts, "Not you? Log out", or 60 s idle.

## Boundaries & Constraints

**Always:**

- **Frames.** `player.authenticate{username, password}` (paired sessions only, else `FORBIDDEN`) is answered by `player.auth_result{outcome, ...}`; `outcome` is `ok` (+`player{player_id, name}`), `rejected`, `locked` (+`retry_s`), `busy` (+`device_name`). A Hub that is not primary, has no player directory, or is slow answers an `error` frame; credentials are never in logs or `Debug`.
- **Verification.** The username is normalised (`normalize_username`); an unknown username, a deleted player, an imported player without a password and a wrong password all answer the same `rejected` (a dummy Argon2id check keeps the time alike). Only `$argon2id$` hashes verify. Checks run on a blocking thread under the Hub's verify permits.
- **Throttle.** Per device: 5 consecutive `rejected` pause that device for 60 s (`locked{retry_s}`, no hash is checked meanwhile); the count resets after the pause or on a correct login. The fifth failure raises the system alert `login_lockout` (tag `locked-out`) for the device, once while open; the Hub shows it on the tile with sound and badge (Story 3.25). The kiosk keeps its own count of the answers it saw (countdown source of truth is `retry_s`).
- **Audit.** A correct login records `kiosk.log` (level `info`, code `kiosk.player_login`, detail `player <player_id>`) as the device actor through the writer, so the change log and audit viewer show player and device.
- **Busy.** A correct login by a player with a session in `active`, `paused`, `grace` or unended `expired` state on another device answers `busy{device_name}` (the PC's label); it does not count as a failure and records nothing.
- **Panel.** The 6.7 `login-panel` body becomes: username, password (show/hide, paste allowed), primary "Log in", underlined "Forgot password?" and "Create account" links, footer "PC name · zone · kiosk version". No language picker, social or QR login. "Create account" is inert until 6.10 (it shows "Ask at the counter to sign up." in place). "Forgot password?" shows "Ask staff at the counter to reset it." in place; no reset flow.
- **Errors.** `rejected`: both fields show the error treatment with "Username or password didn't match.", focus returns to username, password clears. `locked`: both fields disabled, "Too many tries. Try again in 0:60." counting down (`tabular-nums`); at 0 the form re-enables. `busy`: "You're already playing on <PC name>. Ask at the counter." Hub unreachable (`hub-status` not `connected`, or no answer): Log in disabled with "Counter is reconnecting — try again shortly." inline; it re-enables when the link returns.
- **Holding step.** After `ok` the panel shows "Logged in as <name>", "Ask at the counter to start your time." and a secondary "Not you? Log out" which forgets the player and returns to attract. 60 s with no input does the same. When `pc-screen` brings a session the existing session screen replaces attract (player forgotten). The login panel's own 60 s idle close (6.7) applies to the form only; Esc closes the form.
- **Attract view.** `AttractView` gains `device_name` and `zone` (the zone label, empty when none) for the footer.
- Copy from the catalogue; no `backdrop-filter`; the password never reaches `localStorage`, the saved app state, or the DOM after submit; no player context is written by the kiosk.

**Never:** a plan picker, session start or wallet (6.9); sign-up (6.10); a password-reset flow; an intent that authenticates (the kiosk cannot assert a player); a new `ErrorCode`; per-account (rather than per-device) lockout.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Correct login | known user, right password | `ok{player}`; holding step "Logged in as Asha"; audit row | — |
| Wrong password / unknown user | any | `rejected`, same message, password cleared, focus on username | — |
| Fifth failure in a row | 5 × `rejected` | `locked{retry_s: 60}`, alert `login_lockout` on the tile | sixth try within 60 s: `locked`, no hash check |
| Correct password while locked | within 60 s | `locked` | — |
| Already playing | session live elsewhere | `busy{device_name:"PC 3"}` | — |
| Hub unreachable | link `reconnecting` | button disabled with reason; re-enables on reconnect | — |
| Holding step idle | 60 s | forgotten, back to attract | — |
| Session starts | `pc-screen` with session | session screen | — |
| Non-paired caller | phone/kds class | `FORBIDDEN` | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/player_login.rs` (new) -- `PlayerAuthenticate`, `PlayerContext`, `PlayerAuthResult`, limits; `frame.rs` kinds `player.authenticate` / `player.auth_result`; `alert.rs` `AlertKind::LoginLockout`; `attract.rs` `device_name`, `zone`; `lib.rs` re-exports; generated TS.
- `crates/arena-store/src/players.rs`, `lib.rs` -- `find_player_by_username`, `Store::player_by_username`, `Store::live_session_device_of_player`.
- `crates/arena-core/src/devices.rs` -- `DeviceDirectory::login_candidate(username)` (player row, wrapped key, device holding a live session); `attract.rs` fills `device_name` / `zone`.
- `crates/arena-api/src/lan/player_login.rs` (new), `session.rs`, `mod.rs` -- dispatch, throttle (reuses `AdminPinThrottle`), verify, audit, alert; `LanHub::with_player_kek`.
- `crates/arena-hub/src/lib.rs` -- wires the player KEK into the `LanHub`.
- `crates/arena-kiosk-core/src/ipc.rs` -- `IpcMsg::PlayerLogin{rid, username, password: SecretPin}`.
- `crates/arena-kiosk-svc/src/{player_login.rs,app.rs,lib.rs}` -- forwards to the Hub over `ctx.admin.hub`, replies the outcome.
- `apps/kiosk/src-tauri/{ipc-names.json,src/main.rs,src/bridge.rs}` -- command `player_login`.
- `apps/kiosk/src/lib/{bridge.ts,login.ts,Attract.svelte,Login.svelte}`, `routes/+page.svelte` -- form, outcomes, countdown, holding step, hub link.
- `packages/ui/src/messages/en.json` (`player.login.*`, `hub.alerts` tag), `apps/hub/src/lib/alerts.svelte.ts`.
- Tests: Rust unit/integration beside each; `apps/kiosk/tests/login.spec.ts`, `attract.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- login types, frame kinds, `LoginLockout`, attract footer fields; regenerate types; tests
- [x] `arena-store` / `arena-core` -- username lookup, live-session holder, `login_candidate`, attract footer; tests
- [x] `arena-api` / `arena-hub` -- `player.authenticate` handler with throttle, audit and alert; tests
- [x] `arena-kiosk-core` / `arena-kiosk-svc` -- `IpcMsg::PlayerLogin`, service forwarder; tests
- [x] Tauri shell -- `player_login` command, names list, tests
- [x] Kiosk app -- `Login.svelte`, `login.ts`, holding step, hub-link gating; Playwright `login.spec.ts`
- [x] Catalogue strings and Hub alert tag

**Acceptance Criteria:**

- Given the login panel, then it shows username and password (show/hide), "Log in", "Forgot password?", "Create account" and a footer of PC name, zone and kiosk version, with no language picker or social/QR login.
- Given valid credentials, then the Hub verifies Argon2id, records `kiosk.player_login` with player and device, and the holding step "Logged in as <name>" with "Ask at the counter to start your time." and "Not you? Log out" shows within 3 s on LAN.
- Given a wrong username or password, then both fields show "Username or password didn't match." without saying which, focus returns to username and the password clears.
- Given 5 consecutive failures on a kiosk, then the form disables with a 60 s countdown, the Hub raises a login-lockout alert on the tile, and the count resets after 60 s or a correct login.
- Given "Forgot password?", then "Ask staff at the counter to reset it." shows in place.
- Given a player with a live session on another PC, then login is refused with "You're already playing on <PC name>. Ask at the counter."
- Given the Hub is unreachable, then Log in is disabled with "Counter is reconnecting — try again shortly." and re-enables on reconnect.
- Given the holding step with 60 s of no input, then the player is forgotten and attract shows; given `session.started`/`pc-screen` with a session, then the session screen shows.

## Spec Change Log

## Review Triage Log

## Design Notes

Authentication is a request/response frame, not an intent: a kiosk must not be able to assert "player X is here", and the verdict (throttle, hash) is not a branch fact. Only the audit line is written, as `kiosk.log` through the writer, so no new intent kind, scope entry or proptest generator is needed. The throttle is the admin-PIN throttle type with a second instance; it counts in-flight checks, so parallel guesses cannot beat the limit. The player context lives only in the app (the Hub holds no kiosk login state until 6.9 starts a session).

## Verification

**Commands:**

- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-kiosk-core -p arena-kiosk-svc` -- expected: pass
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` -- expected: clean
- `pnpm gen:types`, `pnpm check:strings`, `pnpm check:hex`, `pnpm check:no-blur`, `pnpm format:check` -- expected: pass
- `pnpm --filter @arena/kiosk check`, `pnpm --filter hub check` -- expected: 0 errors
- Playwright `apps/kiosk/tests/login.spec.ts`, `attract.spec.ts` -- expected: pass

## Implementation Notes

- Not run here (Windows host, no Linux target): `crates/arena-kiosk-svc/tests/runtime.rs` is `cfg(unix)`; the two new service integration tests (login forwarded to a real Hub with the audit row and no password in the logs; Hub down is `HUB_SLOW`) were type-checked with the `cfg` lifted but not executed (that file already had unrelated unix-only compile errors). `crates/arena-e2e` and `arena-kiosk-svc/tests/discovery.rs` do not build on this host either (before this story). 14 `arena-kiosk-svc` lib tests (lockdown, pipe ACL, DPAPI, config ownership) fail on this host for SYSTEM-ownership reasons unrelated to this story. `pnpm check:generated` reports the new generated types as untracked until they are committed; `pnpm check:tokens-used` reports the Hub `--ad-*` tokens and `apps/kiosk/tests/theme.spec.ts` fails on a doubled drive-letter fixture path on Windows (both before this story).
- The key that wakes the attract screen is swallowed (`preventDefault`), so it is not typed into the username field the panel opens on.
- The fifth wrong try answers `locked{60}` itself (the matrix row), not `rejected`; the kiosk also locks on its own count of five `rejected` answers, and a correct login or the end of the pause resets both.
- A correct login that cannot be recorded (the writer refuses `kiosk.log`) fails with that error instead of logging the player in unrecorded. A `busy` answer resets the throttle count like any correct password and writes nothing.
- The kiosk version in the footer is the workspace version, read from `Cargo.toml` by `apps/kiosk/vite.config.js` at build time (`__KIOSK_VERSION__`).
- `AttractView.device_name` and `zone` are `#[serde(default)]`, so an attract view cached by an older service still reads (empty label and zone).
- The Hub tile tag is the new `locked-out` `AlertBadge` tag ("LOCKED OUT"), with the label `hub.alerts.kind.login_lockout`.

## Code Review (2026-10-29)

Four reviewers (blind, edge case, verification gap, acceptance). Fixed in code:

- A throttle reservation abandoned by a dropped/aborted request (session closed mid-check) leaked and could lock a device for good; `Reservation` guard in `arena-api/src/lan/player_login.rs` now gives it back on drop.
- The kiosk kept its wrong-try count across a `busy` answer while the Hub resets it on a correct password; `recordOutcome` now clears it on `busy`.
- Removed the unused `HOLDING_IDLE_MS`.

Deferred (non-blocking): Hub error frames (`NOT_PRIMARY`, `UNSUPPORTED`) read as "reconnecting" in the UI; 10 s app timeout vs a slow Hub answer; stale "reconnecting" text after the link returns; byte-length vs `maxlength` on the username; extra tests (busy for paused/grace/expired states, `retry_s` other than 60, lock surviving close/reopen, real-Hub wiring of the player KEK, shell command registration).
