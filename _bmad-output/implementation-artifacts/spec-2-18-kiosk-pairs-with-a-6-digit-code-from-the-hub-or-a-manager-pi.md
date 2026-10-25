---
title: 'Story 2.18: Kiosk pairs with a 6-digit code from the Hub (or a manager PIN) and shows as Unassigned'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '378feb60b1e9cdd2268d6967d44dfe4e7156e5c0'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-17-kiosk-finds-the-hub-on-the-lan-or-by-typed-ip.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-20-kiosk-admin-exit-with-manager-pin-offline-verifier-and-hub-r.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-9-lan-protocol-envelope-hello-handshake-and-client-classes.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** After discovery (Story 2.17), a kiosk still can't join the branch. The only path to a device token is Epic 1's dev provisioning.

**Approach:**
- A Manager or Owner opens "Pair a kiosk" on the Hub and gets a 6-digit single-use code, renewed after each use.
- The kiosk sends the code, or a manager PIN, over TLS to the selected Hub's `:7401`, with the fingerprint pinned from discovery. The Hub mints a device token and registers the device through `apply()` (`device.register`).
- The kiosk connects as `paired` and appears in the floor's Unassigned section within 60 s.

## Boundaries & Constraints

**Always:**
- **Hub migration** `crates/arena-store/migrations/0012_devices_meta.sql` (number reserved; 2.26 holds 0011):
  - `devices_meta`: device_id PRIMARY KEY referencing `devices`, `kind` TEXT CHECK in (`pc`, `console`, `vr`, `sim`), label, `group_id` TEXT NULL (no FK yet; 2.20 adds it), `rev` INTEGER, `deleted_at` NULL.
  - It's 1:1 with `devices` rows that have a token. Existing dev-provisioned devices are backfilled as `kind = pc` with their stored label or name.
- **Intent `device.register`** (permission `device.manage`, held by Manager and Owner per Epic 1's catalogue; add the permission to the catalogue and regenerate if it's missing):
  - It creates or edits a `devices_meta` row through `apply()`. It follows the Epic 1 rules for idempotency, `base_rev`/`STALE` and the change log, with a typed actor and `elevated_by` for the PIN path.
  - It's the only writer of `devices_meta`.
- **Pairing code (Hub):**
  - `POST /ui/pairing-code` (staff session with `device.manage`; Counter gets `PERMISSION_DENIED`) returns `{code, expires_at}`: 6 random digits, single use, 10 minutes.
  - Only the hash (SHA-256 of a per-process random key plus the code) is kept in Hub memory, with at most one live code per Hub. A new request replaces the old code.
  - When the code is used or expires, the Hub mints a fresh one for an open "Pair a kiosk" panel and announces it with SSE `pairing.code_changed`, so many PCs can pair in a row.
  - A code is never logged.
- **Pair endpoint (Hub):** `POST /pair` on `:7401` over **TLS only** (plain HTTP answers 404).
  - **Body:** `{code?: "123456", pin?: "...", pc_name, version}`, with exactly one of `code` or `pin`. `pc_name` is 1–63 printable characters. `version` is the kiosk's semver.
  - **Valid code:** consumed atomically. The Hub mints a 256-bit random device token (base64url), stores its Argon2id hash in `devices` the way Epic 1's dev provisioning does, and applies `device.register` (`kind = pc`, `label = pc_name`, `group_id = null`) with the system actor.
    - It answers 200 `{device_id, token, hub_fingerprint}`. The token is returned this once only.
  - **`pin`:** verified with Argon2id against Manager and Owner PINs using Epic 1's PIN check (1.13/1.20). On success it pairs the same way and records `elevated_by` as that manager.
  - **Errors:**
    - **Wrong, used or expired code:** 401 `PAIR_CODE_INVALID`.
    - **Wrong PIN:** 401 `PIN_REJECTED`.
    - **5 wrong PINs from one source IP within 10 minutes:** the PIN option pauses for 60 s from the 5th, answering 429 `PAIR_PIN_PAUSED` with `Retry-After`.
  - New codes `PAIR_CODE_INVALID` and `PAIR_PIN_PAUSED` are reserved for the Hub track.
- **Kiosk service:**
  - **IPC `pair {code | pin, pc_name}`:**
    - It requires `selected_hub.json` (2.17) and connects over TLS to `<selected addr host>:7401`, pinning `selected.fingerprint` exactly with the existing `pinned_config`.
    - It posts to `/pair` and checks that `hub_fingerprint` matches the pin.
    - It stores the token through the existing token store (DPAPI on Windows, Epic 1's dev/console store elsewhere) and writes `config.json` (`hub_addr` host:7400, `hub_fingerprint`, `device_id`).
    - It deletes `selected_hub.json` and lets `hub_link` connect with `hello{class: paired}`.
  - `pc_name` defaults to the machine hostname, and the UI can edit it.
  - Errors map to replies with the same codes.
- **Kiosk UI** (replaces 2.17's "Enter the pairing code" placeholder):
  - A 6-cell `CodeField` and a "PC name" field (prefilled). "Use a manager PIN instead" switches to a PIN entry that reuses the existing kiosk PIN component.
  - On `PAIR_CODE_INVALID`: the cells clear, and a plain line shows "That code didn't work. Check the code on the Hub and try again."
  - On `PIN_REJECTED`: the cells clear. On `PAIR_PIN_PAUSED`: the PIN option is disabled with the countdown, and the code path still works.
  - On success: "Paired with <branch>", then the normal locked screen once connected.
  - The lockdown and the admin-exit hotkey stay unchanged.
- **Hub UI:**
  - **Settings → Devices:** a "Pair a kiosk" button (only with `device.manage`) opens a panel with the code shown large, grouped 3 + 3 with tabular figures, and an "Expires in m:ss" countdown that's announced at 5 and 1 minutes and at renewal. The panel renews the code on `pairing.code_changed`, and closing it discards the code.
  - **Floor:** the floor gains an "Unassigned" section listing devices with `group_id` null as dashed entries (the 1.10 floor placeholder grows this section) showing label and online state, updated through `devices.changed`.
  - Every string lives in `en.json`.
- **Release builds:** Epic 1's dev token, dev provisioning and any baked Hub IP exist only behind `dev-fixtures`. A test asserts that a build without that feature has no `dev provision` command and no default `config.json`, and that the installer config bundles neither.
- **Remote unlock (1.20):** it must work against a kiosk paired this way. An integration test pairs a console kiosk with a code, then runs the existing Remote-unlock API flow against it.
- **Tests:**
  - **Rust:**
    - code issue, replace, expire and single use;
    - a concurrent double use (exactly one succeeds);
    - pairing by code and by PIN, with `elevated_by`;
    - PIN pause and its recovery;
    - TLS only;
    - the kiosk pinning check: a wrong fingerprint is refused before the body is sent;
    - the token stored and `config.json` written;
    - `device.register` permission, `base_rev` and the change-log row;
    - Unassigned in the snapshot within 60 s of pairing (an e2e test with a real Hub and a console kiosk);
    - Remote unlock against the paired kiosk;
    - the release-build assertions.
  - **Playwright:** the kiosk code and PIN flows and errors (stubbed IPC); the Hub pair panel and its countdown and renewal, and the Unassigned section; axe.

**Never:**
- Pairing over plain HTTP.
- Returning the token twice.
- Storing a code or token in plain text.
- Device caps or pairing rate limits beyond the PIN pause (Story 2.19).
- Assigning a group (2.20).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Issue code | Manager opens panel | 6 digits, expires +10 min | N/A |
| Counter issue | Counter session | `PERMISSION_DENIED` | N/A |
| Pair by code | valid code | 200 token once; `devices` + `devices_meta` (`pc`, label, group null); code consumed; new code via SSE | N/A |
| Reuse code | same code again | 401 `PAIR_CODE_INVALID` | N/A |
| Expired | after 10 min | 401 `PAIR_CODE_INVALID` | N/A |
| Race | two kiosks, same code | exactly one 200 | N/A |
| Pair by PIN | Manager PIN | 200; `elevated_by` = manager | N/A |
| Wrong PIN ×5 | same IP | 5th → paused 60 s, `PAIR_PIN_PAUSED` + Retry-After | N/A |
| Plain HTTP | `POST /pair` on plain | 404 | N/A |
| Fingerprint mismatch | kiosk pin ≠ cert | kiosk refuses before sending | reply `NO_HUB` |
| Floor | paired kiosk connects | Unassigned dashed entry with PC name ≤ 60 s | N/A |
| Release build | no dev-fixtures | no dev provision, no default config | test fails otherwise |

</frozen-after-approval>

## Code Map

Investigate from the `context:` specs (1.9 LAN hello/classes, 1.20 PIN verify + verifier + Remote unlock + kiosk console/dev hooks, 2.17 discovery + `/hub-info` + `selected_hub.json` + `capture_fingerprint`). Known places:
- `crates/arena-api/src/lan/listener.rs` (`route()`, TLS/plain split), `crates/arena-api/src/ui_*.rs` (staff session + permission checks, SSE `ui_events.rs`), `crates/arena-api/src/health.rs` (snapshot).
- `crates/arena-core` (`apply()`, permission catalogue, intents), `crates/arena-store/src/devices.rs` + `migrations/` (**0012 reserved**).
- `crates/arena-kiosk-svc/src/{discovery.rs,config.rs,hub_link.rs,dev.rs,token store}`, `crates/arena-kiosk-core/src/client.rs` (`pinned_config`).
- `apps/kiosk/src/lib/Discovery.svelte` (2.17 placeholder step), `apps/hub/src/routes/settings/devices-display/`, floor route (1.10), `packages/ui` (`CodeField`, PIN component), `en.json`.

## Tasks & Acceptance

**Execution:**
- [ ] `crates/arena-store/migrations/0012_devices_meta.sql`, store + `crates/arena-core` intent/permission.
- [ ] `crates/arena-api/**` -- pairing-code UI route, SSE, `/pair` TLS route, snapshot Unassigned.
- [ ] `crates/arena-kiosk-svc/**`, `crates/arena-kiosk-core/**` -- pair IPC, token + config write.
- [ ] `apps/kiosk/**`, `apps/hub/**`, `en.json`, tests.
- [ ] `crates/arena-e2e` or `crates/arena-hub/tests` -- real pair → floor → Remote unlock.

**Acceptance Criteria:**
- Given a dev Hub and an unprovisioned console kiosk, when a Manager opens "Pair a kiosk" and the code is entered on the kiosk, then within 60 s the Hub floor shows the PC as Unassigned and Remote unlock from its entry works.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast`; `-p arena-hub --features dev-fixtures`; `-p arena-api --features dev-fixtures`; `-p arena-kiosk-svc --features dev-fixtures`; clippy incl. dev-fixtures; fmt -- pass (known flaky tests excepted, recorded).
- `pnpm --filter @arena/kiosk check && pnpm --filter @arena/kiosk test:ui`; `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell`; every `pnpm run check:*`; `pnpm run test:scripts`.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; on Sonnet). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH/VG: the pairing code can be brute-forced | high | P | no wrong-code counter; global invalidation after 10 (per-IP limits stay in 2.19) |
| 2 | BH/ECH: PIN throttle resets; IPv6 rotation; no global ceiling | high | P | about 5 guesses/min forever |
| 3 | BH/ECH: peer map unbounded | medium | P | `retain` evicts nothing |
| 4 | BH/ECH/VG: code burned on a post-consume failure | medium | P | |
| 5 | ECH: handshake timeout covers handle → registered, no answer | medium | P | |
| 6 | ECH: lost answer → orphan devices on retry | medium | D | needs an idempotency design; orphans can be removed through 2.24 maintenance |
| 7 | BH: a rogue Hub gets the PIN without a human check | medium | P | show fingerprints on both ends |
| 8 | BH: lax HTTP parsing | low | P | direct |
| 9 | BH/ECH: `devices_meta` rev/timestamps inconsistent | medium | P | |
| 10 | BH: conditional assertion, sleep in tests | low | P | |
| 11 | BH/ECH: two managers share one slot | low | P | notify the displaced panel |
| 12 | BH: issue on non-primary; `owner_alive` fails open | medium | P | |
| 13 | ECH: renewal continues with the panel gone | medium | P | tie to the SSE stream |
| 14 | BH: thin audit (before, method, elevator) | low | P | |
| 15 | BH: discard skips the permission check | low | R | owner-scoped |
| 16 | ECH: system actor may register or edit any device | medium | P | |
| 17 | ECH: panel fetch failure, SSE race, announce | low | P | |
| 18 | ECH: kiosk stuck on Connecting; code mapping; name validation; change-hub race; focus | low–medium | P | |
| 19 | ECH: pair overwrites a provisioned kiosk; wrong web port; truncated answer | medium | P | |
| 20 | VG: panel failure/retry/unmount tests; `devices.changed` emission test | medium | P | |
| 21 | VG: Tauri handler registration unpinned | low | D | structural; parse `generate_handler!` later |
| 22 | ECH claims: migration name, Owner permission, Remote-unlock e2e | false | R | 0011 is the intended number; Owner holds every permission; `arena-kiosk-svc/tests/pairing.rs` runs pair → floor → Remote unlock |
