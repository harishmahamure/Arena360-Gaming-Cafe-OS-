---
title: 'Story 7.6: Pair a kitchen tablet by code as a scoped display'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: 'd2feb851f44ebbfd2b87cee6e7b675979673356a'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-5-a-kot-prints-within-3-s-and-can-be-reprinted-from-the-hub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-18-kiosk-pairs-with-a-6-digit-code-from-the-hub-or-a-manager-pi.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-19-device-cap-and-pairing-rate-limits-at-the-hub.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub can open and print kitchen tickets but a kitchen tablet cannot yet reach the Hub: `:7401/kitchen` serves a placeholder, there is no way to give a browser a scoped token, and Settings → Kitchen says "Pairing opens in a later update."

**Approach:**
- A Manager or Owner on a Basic+ branch presses "Pair a kitchen display" in Settings → Kitchen. The Hub shows `http://<hub-lan-ip>:7401/kitchen` and a 6-digit single-use code with a 10-minute countdown. The code is held in memory by a second instance of the Story 2.18/2.19 `Pairing` (separate from the kiosk code): same 10-minute expiry, 5 wrong tries kill the code, more than 20 attempts per IP per hour is `RATE_LIMITED`.
- The tablet opens that address. The Hub's plain-HTTP side of `:7401` serves the Kitchen display app at `/kitchen`: a pairing screen (6-digit field, display-name field defaulting to "Kitchen"), on a Free branch the line "Kitchen display is available on Basic. Ask the owner to upgrade." The page posts the code to `POST /kitchen/pair`; the Hub submits the system intent `kds.pair`, which inserts a `kitchen_displays` row (id, name, `token_hash` Argon2id, created_by = the Manager who issued the code, last_seen, revoked_at, high_contrast) and returns the `kds` token once. The page keeps it in `localStorage` and connects to `/ws` with `hello{class: kds}`. A missing or invalid stored token shows the pairing screen again.
- The Hub lists each display in Settings → Kitchen (name, Connected / Not connected, last seen, Revoke) and in Settings → Devices & display with a high-contrast toggle that saves `display.hc.<display id>` through `settings.set` with manager elevation, stored in `kitchen_displays.high_contrast` and pushed to the live display within 2 s. Revoke (`kds.revoke`) closes the socket with `TOKEN_REVOKED`; the page clears its token and returns to the pairing screen. At most 4 non-revoked displays; kitchen displays never count toward the device cap.
- Plain HTTP on `:7401` answers only `/kitchen` (page and its assets), `/kitchen/pair`, `/kitchen/status`, `/ws` and the already existing `/hub-info` and `/console/<tv>`; every other path, and any request to `/kitchen*` that carries a non-`kds` or paired-device bearer token, is `FORBIDDEN` (403). TLS clients on the same port are unaffected.

## Boundaries & Constraints

**Always:**
- A `kds` session may submit only `kitchen.advance` and `kitchen.advance.reverted` (the existing scope); a CI test asserts the scope holds no staff, money, cancel or device intent (existing test in `arena-proto/src/scope.rs` stays and also covers `kds.*`).
- `kds.pair` is tier-gated `New` and `kds.revoke` is `ExistingWork` through `kitchen_gate`; on Free the pairing code endpoint and `kds.pair` return `TIER_LOCKED` and nothing is written. The code endpoint is the Hub UI route `POST /ui/kds-pairing-code` (Manager/Owner via `device.manage`, primary Hub only) which applies the same flag test.
- Every write is an `apply()` intent with change rows: `kds.pair` (system actor), `kds.revoke` (`device.manage`), `kds.seen` (system actor, written when a display connects or its last-seen is more than 60 s old), and the `display.hc.<id>` setting. Add each to the Story 1.8 `proptest` generator.
- `kitchen_displays` rows join the change log (new `Table`/`RowKey::KitchenDisplays`), replay and the snapshot. The token hash is never returned to a client and never logged.
- The `kds` hello is verified against `kitchen_displays` (not `devices`); a revoked row gives `TOKEN_REVOKED`, an unknown or wrong secret `TOKEN_INVALID` (same timing path as device tokens, same per-IP throttle).
- The wrong-code message on the page is "That code didn't work. Get a new one from the counter." and the field clears. The fifth display is refused with "This branch already has 4 kitchen displays — remove one at the counter." (use `ErrorCode::DeviceLimit`; no new error code).
- Screens use shared UI components and tokens; every user-visible string is in `packages/ui/src/messages/en.json`; touch targets 44 px.

**Never:**
- No board feed, columns, chime or `kitchen.advance` UI on the display (Stories 7.7 to 7.10).
- A code or token in a URL, a log line or an audit row; a second live kds code per branch (issuing replaces the previous one).
- A kitchen display counted in `licence.device_cap` or listed in the kiosk device lists.
- Shipping the built Kitchen app inside the Hub installer (the Hub serves the build from a directory, see Design Notes).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Issue code | Manager, Basic+, primary | address + 6-digit code + 10-min expiry | Counter role `PERMISSION_DENIED`; Free `TIER_LOCKED`; follower `NOT_PRIMARY` |
| Pair | valid code, name "Pass" | row inserted, token returned once, display listed | n/a |
| Wrong or expired code | bad 6 digits | 4xx `PAIR_CODE_INVALID`, page clears field with the message | 5th wrong code kills the code; 21st attempt/IP/hour `RATE_LIMITED` |
| Fifth display | 4 live rows | refused with the 4-displays message | `DEVICE_LIMIT`, code stays valid |
| Free branch | open `/kitchen`; `kds.pair` | page says available on Basic; pair `TIER_LOCKED` | nothing written |
| Kds out of scope | `kds` session sends e.g. `order.void` | `FORBIDDEN`, logged | n/a |
| Plain HTTP strays | `GET /media/x`, `GET /nope`, bearer of a paired device on `/kitchen` | 403 `FORBIDDEN` | TLS `:7401` unchanged |
| Revoke | Manager revokes a connected display | socket closes `TOKEN_REVOKED`; page clears token, shows pairing | already revoked: `VALIDATION_FAILED` |
| High contrast | Manager toggles display HC | column updated, `settings.changed` reaches the display within 2 s | non-manager needs elevation |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0072_kitchen_displays.sql` (new) + `src/migrations.rs`, `src/kitchen_displays.rs` (new, modelled on `kitchen_tickets.rs`), `src/branch.rs` (`Table::KitchenDisplays`, `RowKey::KitchenDisplays{id}`, same sites as `KitchenTickets`), `src/lib.rs` (exports; `table_exists` guards as in the tickets code).
- `crates/arena-proto/src/kitchen.rs` -- add `KdsPair`, `KdsRevoke`, `KdsSeen`, `KdsPairRequest/Response`, `KitchenDisplayView`, constants (`KDS_MAX_DISPLAYS = 4`, `KDS_PAIR_PATH = "/kitchen/pair"`, `KDS_STATUS_PATH`, default name) and extend `kitchen_gate` (`KdsPair` -> New, `KdsRevoke`/`KdsSeen` -> ExistingWork). `intent.rs` (every site a `KotReprint`/`DeviceRevoke` touches: enum, `ALL`, `as_str`, category, requirement, scope/system-only lists, payload enum, `kind()`), `scope.rs` (`may_read_setting`: a `Kds` session may read only its own `display.hc.<display id>`), `lib.rs`, `error.rs` untouched.
- `crates/arena-core/src/apply/kds_display.rs` (new; copy the shape of `apply/devices.rs` and `apply/kot.rs`), plus `mod.rs` (`load`, `Target::KdsDisplay`), `validate.rs`, `mutate.rs`, `settings_set.rs`/`settings_defaults.rs` (an `display.hc.<id>` key whose id is a kitchen display updates the column and its change row instead of requiring a settings row; seed nothing in `settings`), `rbac.rs`, `devices.rs` (`DeviceDirectory::verify` dispatches `ClientClass::Kds` to `kitchen_displays`), `snapshot.rs` (`HubSnapshot.kitchen_displays: Vec<KitchenDisplayView>` with `connected` filled by the API layer), `arena-core/tests/proptest_apply.rs`.
- `crates/arena-api/src/pairing.rs` -- add `Taken::owner()`; reuse `Pairing` for the kds code (owner = issuing staff id, no owner check so a used or expired code is not renewed).
- `crates/arena-api/src/lan/mod.rs` -- `LanHub.kds_pairing: Option<Arc<Pairing>>` + `with_kds_pairing`; `crates/arena-hub/src/lib.rs:376` builds and wires it next to `pairing`.
- `crates/arena-api/src/lan/listener.rs` -- `route()`/`serve_plain`: `/kitchen`, `/kitchen/<asset>` (served from `hub.kitchen_dir`, traversal-safe, falling back to a built-in minimal page), `POST /kitchen/pair`, `GET /kitchen/status` (`{available, branch_name}`), unknown path and bad-bearer cases 403 `FORBIDDEN` as an `AppError` JSON body; update the `routes_by_method_and_path` test. New `lan/kds_pair.rs` modelled on `lan/pair.rs` (`read_body`, rate limit via `kds_pairing.attempt`, `take`, hash on a blocking task, `submit_intent(KdsPair)`, `restore` on failure, `used` on success).
- `crates/arena-api/src/lan/session.rs` (`authenticate`: kds branch + `kds.seen`), `lan/registry.rs` (kill sessions of a display on `kitchen_displays` revoke with `TOKEN_REVOKED` like `kill_device` at ~798/849; fan out `settings.changed` for a display's own HC key to that display within 2 s; `connected` set per display).
- `crates/arena-api/src/ui_pairing.rs` + router in `ui_auth.rs` -- `POST /ui/kds-pairing-code` and `/ui/kds-pairing-code/discard` modelled on `issue`/`discard`, returning `KioskPairingCode` plus the LAN address if the Hub knows it (otherwise the Hub UI builds it from the Hub host).
- `apps/hub/src/routes/settings/kitchen/+page.svelte` (enable "Pair a kitchen display", code dialog with countdown and address, display list with Revoke), `apps/hub/src/routes/settings/devices-display/` (kitchen display rows with the HC toggle through the existing elevation helper), `apps/hub/src/lib/audit/labels.ts`, `packages/ui/src/messages/en.json`.
- `apps/kitchen/src/routes/+page.svelte` (pairing screen, Free message, stored-token check, WS hello `kds`, `TOKEN_REVOKED` handling, HC class from `settings.changed`), `apps/kitchen/src/lib/` (client helpers), `svelte.config.js` with `paths.base = '/kitchen'` and `fallback: 'index.html'`.
- Tests: `apply/tests/kds_display.rs`, store tests, `arena-api/tests/lan.rs` (HTTP/WS flow, 403s, revoke close, HC push, rate limit, Free), `scope.rs`, listener route tests, `apps/hub/tests/kitchen.spec.ts` (pair dialog, list, revoke, HC), `pnpm gen:types`.

## Tasks & Acceptance

**Execution:**
- `crates/arena-store/**` -- migration 0072, `kitchen_displays` store module, change-log table/row key -- persistence
- `crates/arena-proto/**` -- three intents, wire types, constants, gate entries, scope read rule, generated types -- contract
- `crates/arena-core/**` -- `kds.pair`/`kds.revoke`/`kds.seen`, HC for displays, `verify` for `kds`, snapshot list, proptest ops -- single writer
- `crates/arena-api/**`, `crates/arena-hub/src/lib.rs` -- kds `Pairing`, `POST /kitchen/pair`, `GET /kitchen/status`, static `/kitchen`, plain-HTTP 403 rules, kds hello, revoke kill, HC push, Hub UI code route -- transport
- `apps/hub/**`, `packages/ui/src/messages/en.json` -- pair dialog, list, Revoke, HC toggle, audit labels, strings -- Hub screens
- `apps/kitchen/**` -- pairing screen, Free message, token storage and `kds` WS connect -- tablet screen
- Tests for every matrix row (Rust apply, API integration, listener routes, Hub Playwright).

**Acceptance Criteria:**
- Given Settings → Kitchen on a Basic+ branch, when a Manager or Owner presses "Pair a kitchen display", then the Hub shows `http://<hub-lan-ip>:7401/kitchen` and a 6-digit code with a 10-minute countdown.
- Given a tablet on the LAN, when it opens that address over plain HTTP, then it sees the pairing screen with a 6-digit field and a name field defaulting to "Kitchen".
- Given a valid code, when it is entered, then the Hub stores a `kitchen_displays` row with an Argon2id token hash, returns the `kds` token once, the page stores it and connects with `hello{class: kds}`, and the display is listed with name, connected state and last seen.
- Given a `kds` connection, when it submits anything outside the scope, then the Hub returns `FORBIDDEN` and logs it; a CI test asserts the scope holds no staff, money, cancel or device intent.
- Given a plain-HTTP request to any other path, or a non-`kds` or paired-device token on `/kitchen*`, then `FORBIDDEN`; TLS clients are unaffected.
- Given a wrong or expired code, then the field clears with the exact message; 5 failures invalidate the code; more than 20 attempts per IP per hour return `RATE_LIMITED`.
- Given 4 non-revoked displays, when a fifth pairs, then it is refused with the 4-displays message and the device cap is untouched.
- Given a paired display, when a Manager presses Revoke, then its socket closes with `TOKEN_REVOKED` and the page returns to the pairing screen.
- Given a Free branch, when the address is opened, then the page says "Kitchen display is available on Basic. Ask the owner to upgrade." and `kds.pair` returns `TIER_LOCKED`, writing nothing.
- Given a stored token that is missing or invalid, when the page loads, then it shows the pairing screen.
- Given Settings → Devices & display, when a Manager toggles a display's high contrast, then `kitchen_displays.high_contrast` changes through `settings.set` with elevation and the live display receives it within 2 s without re-pairing.

## Spec Change Log

## Review Triage Log

- patch (fixed): `kds.seen` rewrote the display row (bumping `rev`) on every reconnect, so a Manager's high-contrast or revoke based on the snapshot `rev` could go STALE and a reconnect loop churned the change log. `kds.seen` is now refused inside 60 s of the last write; test added.
- patch (fixed): a paired tablet waited on `/kitchen/status` before connecting; it now connects first. The display name is cut by code point, not UTF-16 unit.
- defer: built-in page without high contrast, missing Rust snapshot/overlay tests, lost-token on HubSlow, symlink/caching of static serving, stale pair-panel code — see deferred-work.md.
## Design Notes

- **`kds.pair_code`** is not a stored intent: issuing a code writes nothing (the code lives in memory like the kiosk's). It is the manager route `POST /ui/kds-pairing-code`, which runs the same `kitchen` flag check as the gate and returns `TIER_LOCKED` on Free; `kds.pair` is the intent the gate covers. Recorded in the decision log.
- **Serving the page.** `LanHub.kitchen_dir: Option<PathBuf>` (env `ARENA_KITCHEN_DIR`, default `<hub exe dir>/kitchen`) holds the `adapter-static` build of `apps/kitchen` (`paths.base = '/kitchen'`). Missing directory: a built-in single-file page that does the same pairing flow in plain JS, so the Hub works before the installer bundles the build. Bundling is deferred.
- **Why a table and not `devices`:** displays are not counted against the device cap, never appear on the floor, and carry no PC fields; the AC fixes the `kitchen_displays` shape. `devices.class = 'kds'` stays unused.
- Token format is the existing `format_token(id, secret)` so `parse_token` and the dummy-hash timing path are reused.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --locked` -- expected: pass (pre-existing failures listed in the decision log excepted)
- `cargo clippy --locked -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --all-targets -- -D warnings`; `cargo fmt --all --check`
- `pnpm gen:types`; `pnpm --filter @arena/hub check`; `pnpm --filter @arena/kitchen check`; `pnpm --filter @arena/kitchen build`; Hub Playwright `kitchen`, `devices-display` specs; `pnpm check:strings`, `check:layers`, `check:protocol`, `format:check`

## Auto Run Result

Status: in-review (implementation and verification complete; the multi-reviewer pass is left to the next loop iteration).

- **Change.** Kitchen displays pair by a 6-digit code over plain HTTP at `:7401/kitchen`, hold a scoped `kds` token in `kitchen_displays` (migration 0072), connect with `hello{class: kds}`, are listed and revoked from Settings → Kitchen, and have a per-display high-contrast toggle in Devices & display. New intents `kds.pair` (system, tier `New`), `kds.revoke` (`device.manage`, existing work) and `kds.seen` (system).
- **Plain-HTTP rules.** Unknown paths, `/media`, `POST /pair` and a non-`kds` or paired-device bearer on `/kitchen*` are 403 `FORBIDDEN`. Three older tests that asserted 404/401 now assert 403.
- **Verification.** `cargo fmt --check`, `cargo clippy -D warnings` (proto, store, core, api, hub) clean; `cargo test` on those crates 2108 passed, 0 failed; Hub and Kitchen svelte-check 0 errors; `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `format:check` ok; Hub `kitchen` and `devices-display` Playwright specs passed (subagent run). `check:tokens-used` reports the pre-existing `--ad-reserved`. A dev Hub was driven in a browser through wrong code, pair, HC push, revoke and the Free message.
- **Residual risks.** The Settings → Kitchen screen was only run against stubbed Playwright specs, not a real Hub; the Kitchen app has no spec of its own; bad bearer tokens on `/kitchen*` share the per-IP hello throttle; the Hub does not yet bundle the Kitchen build (built-in fallback page serves meanwhile).
