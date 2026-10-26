---
title: 'Story 2.29: Step 9 — 2-minute test session locks a kiosk at zero'
type: 'feature'
created: '2026-10-26'
status: 'review'
baseline_commit: '41cf8f50b098fdcfd6cfb57ccd9d767d4ea1f94b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-28-wizard-steps-5-6-and-8-follow-the-hub-s-setup-progress.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Wizard step 9 is a placeholder and `setup.test_session` is accepted by the cloud but unsupported at the Hub, so an owner can't prove a kiosk really locks before opening. `SetupStatus.test_session` is hard-wired to `unset`.

**Approach:**
- Add two Hub intents, `device.test_session.start` and `.completed`, and a `setup.test_session` command that maps to `start`.
- The Hub pushes a `TestSession{lock_after_ms: 120000}` event to the chosen kiosk, which shows a test screen and counts down on its monotonic clock.
- At zero the kiosk locks and sends `completed`; the Hub records `passed`, or `failed` if nothing confirms within 10 s of zero.
- The cloud completes step 9 from the heartbeat `setup` block, and the owner starts the test from step 9 or from the Hub's Setup panel.

## Boundaries & Constraints

**Always:**
- **State:** one system-managed setting `setup.test_session` (const `KEY_TEST_SESSION` in `arena-proto`, default `{"state":"unset"}`, added to `DEFAULT_SETTINGS` and `SYSTEM_MANAGED_KEYS`) holding `{state, device_id?, at?}`. Model it on `printer.status` (`apply/printer_status.rs`, `PrinterStatus::to_json/parse`). `TestSessionState` gains `Running`; `setup_status()` reads the key instead of `Default::default()`. Regenerate `packages/ui/src/generated/*`.
- **Intents** (register in `arena-proto/src/intent.rs`: enum, `ALL`, `as_str`, `category` Device, `requirement`, `reasons` None, payload structs, `IntentPayload`, and the pinned tests):
  - `start{device_id}`: requirement `Perm(DeviceManage)`. Refuses with `ValidationFailed` (field `device_id`, plain message) when the device is unknown, revoked, not a kiosk, has no group, is in `maintenance`, or a test is already running (started under 130 s ago). Sets `running` with `device_id` and `at = issued_at`.
  - `completed{device_id, outcome: locked|timed_out}`: requirement `Device` for the kiosk (`locked`, added to `PAIRED_SCOPE` in `scope.rs`, must name its own device id) and the Hub itself for `timed_out` (system actor). Only applies while the state is `running` for that `device_id`, else a no-op refusal; `locked` sets `passed`, `timed_out` sets `failed`.
  - Both write only the setting row (one change row each, so exactly two audit intents exist). No session, bill, money or `devices_meta` row is ever written.
- **Offline refusal** (apply has no presence): `ui_auth.rs` `intent()` refuses `start` when `!presence.online(device_id)` exactly like `device.admin_unlock` ("the device is offline"); `CommandTask` (`arena-tasks/src/commands.rs`, wired in `arena-hub/src/lib.rs` ~L374) gets the same presence closure the heartbeat has and reports `setup.test_session` for an offline device as rejected with that error without applying it.
- **Command:** `arena-core/src/commands.rs` handles `setup.test_session` (payload `{device_id}`) by building a `device.test_session.start` intent (the existing `command_intent` copies the kind into the payload tag, so special-case it). Add it to `HANDLED`; update the three tests that pin it as `UNSUPPORTED`.
- **Push:** `arena-proto/src/frame.rs` `HubEvent::TestSession{lock_after_ms}` (`TEST_SESSION_MS = 120000` and `TEST_SESSION_CONFIRM_MS = 10000` in `arena-proto`). In `arena-api/src/lan/registry.rs` `fan_out`, when `applied` changed the `setup.test_session` settings row and `DeviceDirectory::settings` reads `running`, call a new `push_test_session` modelled on `push_admin_unlock`/`push_paired(msg, Some(device_id))`.
- **Timeout:** a Hub task (where the writer handle lives, `arena-tasks`) resolves a `running` state: 130 s after `at` it submits `completed{timed_out}` as system. It also resolves one found at startup. A late kiosk `locked` after `failed` is ignored.
- **Kiosk service** (`arena-kiosk-svc`): `hub_link.rs` handles `HubEvent::TestSession`: record a `tokio::time::Instant` deadline in `Ctx` (a `watch` like `ctx.admin.active`), send `IpcMsg::TestSessionStarted{remaining_ms}` and, at the deadline (or if the app is absent), `IpcMsg::TestSessionEnded` and queue a `device.test_session.completed{locked}` intent via `intents.rs` + `ctx.queue` (so it survives a Hub drop and replays). The countdown never reads the wall clock or the Hub after the event arrives. A service restart mid-test drops the test without sending `completed`.
- **Kiosk UI:** `arena-kiosk-core/src/ipc.rs`, `apps/kiosk/src-tauri/src/bridge.rs` + `ipc-names.json`, `apps/kiosk/src/lib/bridge.ts`, `apps/kiosk/src/routes/+page.svelte`: while a test runs show "Test session" and `m:ss` in tabular figures from the shared tokens; on end return to the locked screen. This is a display state only — no lockdown suspension. Strings live in the catalogue.
- **Cloud** (`arena-store/src/control/setup_status.rs`, `arena-cloud/src/api/wizard_devices.rs`): add `setup.test_session` to `SETUP_KINDS` so `POST /v1/wizard/devices` enqueues it and `GET /v1/wizard/devices` returns its pending/rejected state; make the Devices step's `overlay()` ignore that kind. `record_setup` marks `test_session` done when `status.test_session == passed`; never reverts `done`. `running`, `failed`, `unset` leave it pending.
- **Wizard UI:** `apps/onboarding/src/lib/TestSessionStep.svelte`, wired in `routes/setup/[step]/+page.svelte` (replaces the placeholder for `test_session` only), using `watchSetup()`, `HubAge`, `sendDeviceCommand`. Lists kiosks that are online and in a group (select + "Start test session"); a rejected command shows its plain reason and keeps the list open. `running` says to watch the PC; `failed` shows "That PC didn't confirm it locked — check it and try again" with Retry; `passed` shows "The PC locked on cue." and the step then advances by itself. Strings in the catalogue; axe passes.
- **Hub UI:** `apps/hub/src/routes/settings/setup/+page.svelte` Test session row gets a kiosk `select` (online, grouped kiosks from the snapshot) and "Start test session" (hidden without `device.manage`), submitted with `submitIntent` from `lib/auth.ts`; it shows running/passed/failed and a refusal inline. Strings in `en.json`; axe passes.

**Never:**
- Real session, bill, wallet or money rows; the go-live checklist (2.31); player shell or real session screen (Epic 3/6); a new cloud table, endpoint or heartbeat field; making a Hub intent depend on the cloud.
- Writing the wizard-step state from anything but the heartbeat block.
- A comment in code.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Start | online grouped kiosk, owner command or Hub click | state `running`; kiosk shows test screen with 2:00 | N/A |
| Locks on cue | countdown reaches zero | kiosk locked, `completed{locked}` applied, state `passed`, step 9 `done` | N/A |
| Hub drops mid-test | no Hub link after the unlock | kiosk still locks at zero, `completed` replays on reconnect | late `locked` after `failed` ignored |
| No confirm | no `completed` 10 s after zero | state `failed`; step shows retry line | N/A |
| Kiosk offline | device not connected | refused, device list stays open | `ValidationFailed` device_id "the device is offline" |
| Maintenance / unassigned | device in maintenance or no group | refused | `ValidationFailed` device_id |
| Already running | second start within 130 s | refused | `ValidationFailed` |
| Wrong device completes | kiosk B sends `completed` for A | refused | `FORBIDDEN` per scope check |
| Old Hub block | no `test_session` in block | parses as `unset`; step stays pending | N/A |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{intent.rs,scope.rs,frame.rs,operating.rs,commands.rs}` -- intent kinds, payloads, `PAIRED_SCOPE`, `HubEvent`, `TestSessionState`, `KEY_TEST_SESSION`; pinned tests in `intent.rs` (`every_kind_has_exactly_one_requirement`, `ALL` length) and `scope.rs`.
- `crates/arena-core/src/apply/{mod.rs,guards.rs,validate.rs,mutate.rs,printer_status.rs}` -- add `test_session.rs` modelled on `printer_status.rs`; `Target` variant, `load`, `authorize`, `base_rev` arms.
- `crates/arena-core/src/{commands.rs,setup.rs,settings_defaults.rs,snapshot.rs}` -- `HANDLED`/`command_intent`, `setup_status`, default + system-managed key.
- `crates/arena-core/src/apply/tests/commands.rs:83`, `tests.rs` (~4394–4542 `SetupStatus` pins) -- tests that must change.
- `crates/arena-api/src/ui_auth.rs:~1899` (offline refusal template), `src/lan/registry.rs` (`push_admin_unlock`, `fan_out`, `DeviceDirectory::settings`), `lan/session.rs` (`check_intent`).
- `crates/arena-tasks/src/{commands.rs,heartbeat.rs}`, `crates/arena-hub/src/lib.rs:~367-374` -- presence closure and writer wiring; timeout task home.
- `crates/arena-kiosk-svc/src/{hub_link.rs:260,admin_exit.rs,intents.rs,runtime.rs,app.rs}`, `crates/arena-kiosk-core/src/ipc.rs` -- admin-exit is the template.
- `apps/kiosk/src-tauri/src/bridge.rs`, `ipc-names.json`, `apps/kiosk/src/{lib/bridge.ts,routes/+page.svelte,routes/+layout.svelte}`.
- `crates/arena-store/src/control/setup_status.rs`, `crates/arena-cloud/src/api/{wizard_devices.rs,commands.rs}`, `crates/arena-cloud/tests/{pair.rs,wizard.rs}`.
- `apps/onboarding/src/lib/{DevicesStep.svelte,RatesStep.svelte,devices.ts,api.ts,setupView.svelte.ts}`, `routes/setup/[step]/+page.svelte`, `tests/{stub.ts,progress.spec.ts,a11y.spec.ts}`.
- `apps/hub/src/routes/settings/setup/+page.svelte`, `apps/hub/src/lib/auth.ts`, `apps/hub/tests/setup.spec.ts`, `packages/ui/src/messages/en.json`, `packages/ui/src/generated/*` (`pnpm gen:types`).

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` + `crates/arena-core` -- intents, setting, state, command mapping, `setup_status`, regenerated types -- Hub can start and complete a test; unit-test every matrix row.
- [x] `crates/arena-api` + `crates/arena-tasks` + `crates/arena-hub` -- offline refusal, push, timeout task -- the kiosk is told and the Hub times out; tests for push, refusal, timeout.
- [x] `crates/arena-kiosk-svc` + `arena-kiosk-core` + `apps/kiosk` -- monotonic countdown, `completed` intent, test screen -- locks at zero without the Hub; paused-time runtime test and a UI test.
- [x] `crates/arena-store` + `crates/arena-cloud` -- `SETUP_KINDS`, step 9 completion; integration tests (passed completes, running/failed/unset don't, never reverts).
- [x] `apps/onboarding` + `apps/hub` + `packages/ui` -- `TestSessionStep`, Hub Setup row, messages; Playwright tests with axe.

**Acceptance Criteria:**
- Given step 9 and an online grouped kiosk, when the owner presses "Start test session", then the kiosk shows "Test session" with a 2:00 countdown within 15 s while the branch is online.
- Given a running test, when the Hub drops, then the kiosk still locks at zero and sends `device.test_session.completed`.
- Given the kiosk locks, then step 9 completes by itself and the wizard says "The PC locked on cue."
- Given no confirmation 10 s after zero, then the step shows "That PC didn't confirm it locked — check it and try again" with Retry.
- Given the kiosk is offline or in maintenance, when the test is requested, then it is refused with a plain reason and the device list stays open.
- Given a test, then no session, bill or money row exists; only the two audit intents.

## Spec Change Log

## Review Triage Log

## Design Notes

The kiosk's "unlock" here is a display state: the locked page shows the test screen while the lockdown stays on. The 15 s budget relies on the 10 s pre-go-live heartbeat poking the command poller; no faster path is added.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy --locked --workspace --all-targets -- -D warnings`; `cargo test --locked --workspace --no-fail-fast` -- expected: all pass.
- `pnpm gen:types && pnpm check:generated`; `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell`; `pnpm --filter @arena/onboarding check` and its Playwright tests; `pnpm --filter @arena/kiosk check`; `pnpm run check:strings` -- expected: all pass.

## Repo Rules

- Do not add any comments to code (line, block or doc), in any language. Leave existing comments alone unless the code they describe changes.
- Short, compact code; reuse existing helpers and patterns; no extra abstractions.
