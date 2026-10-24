---
title: 'Story 1.21: High-contrast theme as a per-device branch setting'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '4b88493f0ea5ecf4a9718e4669b479fed1355fb2'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-18-kiosk-process-allow-block-list.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The HC tokens and component rules exist, but nothing switches a Hub or kiosk into HC. It can't be set per device, and a kiosk would flash dark before HC applied.

**Approach:**
- **Setting:** add a per-device `display.hc.<device>` setting, edited from Settings → Devices & display through `settings.set`, with the 1.13 elevation for a Counter.
- **Live switching:** push the value live to the Hub UI (SSE) and to that kiosk (LAN `settings.changed`).
- **Kiosk cold start:** the kiosk service caches the value, and the kiosk app applies `data-theme="hc"` before its first paint.
- **CI coverage:** HC axe and contrast checks run on the Hub shell and the kiosk placeholder.

## Boundaries & Constraints

**Always:**
- **Settings keys:**
  - `display.hc.hub` is for the Hub UI on this branch's Hub.
  - `display.hc.<device_id>` is one key per paired kiosk.
  - Values are `"true"` or `"false"`, validated by a pattern rule in `SETTING_RULES`.
  - **Row creation:** `device.register` creates the device's row as `"false"`, and bootstrap creates `display.hc.hub`. `branch.seed_defaults` (1.12) also adds any missing rows for existing devices and the Hub.
  - **Permission:** `settings.edit`.
  - Each change records a normal change row, and the 1.13 elevation appears in the audit with both actors.
- **Delivery:**
  - The paired scope's `settings.get` (1.18) also allows `display.hc.<own device_id>`, and only the session's own device id.
  - The `settings.changed` push to a paired session includes that key when it changes.
  - The Hub UI gets `ui.hc` in the snapshot (from `display.hc.hub`) and `settings.changed` over SSE.
- **Kiosk:**
  - The service caches the value in `queue.db` kv. A change arriving over the LAN goes over IPC to the app as a new `Theme{hc}` message.
  - The app applies it within 2 s, without a restart, by setting `document.documentElement.dataset.theme`.
  - **Cold start before first paint:**
    - The Tauri window starts hidden (`visible: false`).
    - The Rust side asks the service for `Theme` over IPC, with a 1 s timeout falling back to the last value the app cached in its own local file.
    - It injects an initialization script that sets `data-theme` before any page script runs (`WebviewWindowBuilder::initialization_script`, or `on_page_load` plus an eval before show).
    - Only then does it show the window.
- **Hub UI:** the root layout sets `data-theme` on `<html>` from `ui.hc` before rendering the shell, and updates it on `settings.changed`.
- **Settings → Devices & display** (a new settings section):
  - A list of the Hub ("This Hub") and each non-revoked paired device by label, each with an on/off switch built as a `TabControl` or a labelled checkbox switch from shared components with ≥44px targets.
  - Saving per row uses `base_rev`.
  - A Counter gets the 1.13 InlinePin flow with the items `[{settings.set, display.hc.<id>}]`.
  - The list updates live on device changes through the snapshot refetch.
- **HC rendering:** reuse the generated `-hc` tokens and the 1.5 `base.css` HC rules: pure black surfaces, white text and hairlines, accent `#FFE14D`, inverted chips, 2px white outlines on interactive surfaces, a 4px yellow focus ring, no shadows or translucency, and instant motion.
  - Audit the Hub shell and the kiosk placeholder components for any rule that bypasses the HC tokens, and fix them.
  - There is no art or video yet; the rule is documented for later epics.
- **No theme control:**
  - No `theme-toggle` and no light theme on Hub, Kiosk or Kitchen.
  - A script test (`scripts/no-theme-toggle.test.mjs`) asserts that `apps/{hub,kiosk,kitchen}/src` contain no `theme-toggle`, no `data-theme="light"` and no `ThemeToggle` import.
- **CI:** Playwright projects run the Hub shell (sign-in, floor, settings) and the kiosk placeholder with `data-theme=hc`. They assert zero serious or critical axe violations and the HC computed styles: a 2px white outline on buttons, the 4px focus ring and `transition-duration` 0s. The existing token contrast gate already covers the HC pairs.

**Never:**
- A player-facing theme control.
- A light theme on branch surfaces.
- An OS high-contrast auto-detect override (DESIGN rule: HC is set by a manager per device).
- Restarting the app to apply the theme.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| List | a manager opens Devices & display | "This Hub" plus "Dev Kiosk", each with a switch showing its current value | N/A |
| Kiosk on | the manager turns HC on for Dev Kiosk | a change row; the kiosk applies HC within 2 s; the Hub UI is unchanged | N/A |
| Hub on | the manager turns HC on for This Hub | the Hub UI switches to HC live; the kiosk is unchanged | N/A |
| Cold start offline | kiosk cached HC=on, Hub down | the first paint is already HC (the window shows only after data-theme is set) | N/A |
| IPC slow | the service doesn't answer within 1 s | the app uses its own last cached value | N/A |
| Counter | a Counter toggles | InlinePin; after manager approval, applied with elevated_by | N/A |
| Scope | a kiosk asks for another device's HC key | FORBIDDEN | N/A |
| New device | `device.register` | a `display.hc.<id>` row = false exists | N/A |
| No toggle | the script test | passes; it would fail on a `theme-toggle` in a branch app | N/A |
| HC axe | Hub shell and kiosk with HC | zero serious or critical | N/A |

</frozen-after-approval>

## Code Map

Paths are from the repo root. Line numbers are approximate (as of HEAD `4b88493`).

- **Rules** -- `crates/arena-core/src/settings_defaults.rs`:
  - `SettingRule` :77 and `SETTING_RULES` :96. `setting_rule(key)` :130 matches exactly.
  - **Decision:** add `SettingRule::Bool` (`"true"|"false"`). `setting_rule` keeps the exact match, then falls back to the prefix `display.hc.` followed by `hub` or a valid device id (ULID). Put the key helpers (`HC_PREFIX`, `hc_key(id)`, `KEY_HC_HUB`) in `arena-proto` next to the `kiosk.*` constants (`crates/arena-proto/src/process.rs:32-36`) or in a new `display.rs`.
  - `every_default_has_a_rule_and_satisfies_it` (:178) counts rules against defaults. Adjust it for the prefix rule and don't break its intent.
- **Rows:**
  - `display.hc.hub = "false"` goes in `DEFAULT_SETTINGS` (:52), which covers bootstrap (`apply/mutate.rs:58-59`) and seed_defaults.
  - `device.register` (`apply/devices.rs` `mutate_register` :122) also inserts `display.hc.<id> = "false"` in the same change.
  - `apply/seed_defaults.rs` `missing()` :40 also adds `display.hc.<id>` for every non-revoked device that lacks one.
  - `settings.set` (`apply/settings_set.rs` `validate` :50) is unchanged: it updates existing rows only, with `base_rev` required.
- **LAN scope:**
  - `arena-proto/src/scope.rs` `may_read_setting` :78 gets a device id: `may_read_setting(class, key, device_id: Option<&str>)`. It allows `kiosk.*`, or `display.hc.<own id>`.
  - Update its callers: `arena-api/src/lan/session.rs` `check_settings_get` :665 and `lan/registry.rs` `paired_setting_keys` :571.
  - Push: `push_settings` :399 broadcasts `kiosk.*` as today. A changed `display.hc.<id>` goes only to that device through `push_paired(msg, device_id)` :241.
- **Hub snapshot and SSE:**
  - Add `hc: bool` to `SnapshotUi` (`arena-proto/src/snapshot.rs:26`), read in `arena-core/src/snapshot.rs` `read_ui` :75 from `display.hc.hub`.
  - `ui_events.rs` `setting_keys` :155 / :246 / :318 must include the key, so the `ui` payload carries `hc`.
  - Regenerate the ts-rs types (`pnpm gen:types`).
- **Kiosk service:**
  - `hub_link.rs` (`fetch_settings` ~:230, `PROCESS_KEYS`) also asks for `display.hc.<own device_id>` (`config.rs:21`).
  - `apply_settings` (`procs/mod.rs:441`) caches it with the same rev guard (`AppliedRevs`) in `queue.kv_set`.
  - A watch feeds `app.rs`, which sends `IpcMsg::Theme{hc}` on connect (with HubStatus, :130-146) and on change (watch arms ~:205-226).
  - Add the `Theme{hc: bool}` variant to `IpcMsg` (`arena-kiosk-core/src/ipc.rs:84`).
- **Kiosk shell** (`apps/kiosk/src-tauri/src`):
  - The window is already `visible: false`. The real show is `shell.rs:120` `cover_all_monitors`.
  - Before showing: read the theme from IPC with a 1 s timeout, falling back to `<app local data>/theme.json`. Keep the decision as a pure, unit-tested function.
  - Inject `document.documentElement.dataset.theme` through `initialization_script` (window builder) or an eval before show.
  - Live updates: `bridge.rs` gets an `Emit::Theme` plus a `THEME_EVENT` name in `ipc-names.json` (checked by the test at :444). `apps/kiosk/src/lib/bridge.ts` listens and sets the dataset. The shell persists each new value to `theme.json`.
- **Hub app:**
  - `+layout.svelte` :148/:159 (setUiSettings, onSettingsChanged) sets `document.documentElement.dataset.theme = hc ? "hc" : "dark"`.
  - Add `devices-display` to the settings `SECTIONS` (`routes/settings/+layout.svelte:10`) and a page modelled on `settings/screen-lock/+page.svelte`, with the Counter elevation via `elevate(...)` and `InlinePin` (:45, :132, :265).
- **Tests:**
  - Hub: Playwright stubs (`apps/hub/tests/*`: `newHub` and `snapshot` with `ui.hc`, SSE `push`, `expectNoSeriousAxe` :227).
  - Kiosk: `apps/kiosk/tests` mock Tauri events (`status.spec.ts:17-26`).
  - HC CSS: `packages/ui/src/base.css:121-162` and the `[data-theme="hc"]` tokens (`tokens.css:581`). Don't edit generated tokens by hand.
- **No migration.** 1.18 added settings the same way; the next free number would be 0011.

## Tasks & Acceptance

**Execution:**
- [x] Core, API and kiosk delivery and caching, with tests for every API/kiosk matrix row.
- [x] The kiosk pre-paint theme, plus a unit test of the decision function (cached value vs. IPC with timeout).
- [x] The Hub Devices & display section and live theme; Playwright tests (list, toggle, Counter elevation, live switch, HC axe for the Hub and kiosk).
- [x] `scripts/no-theme-toggle.test.mjs`; catalogue keys; README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` (plus the dev-fixtures runs), `pnpm check` and the Hub and kiosk Playwright suites, then all pass with every matrix row covered.

## Implementation Notes

- **Keys** (`arena-proto/src/display.rs`): `HC_PREFIX`, `KEY_HC_HUB`, `HC_DEFAULT`, `hc_key(id)`, `parse_hc_key` (suffix `hub` or an upper-case ULID, `HcTarget::{Hub, Device}`), `parse_hc_value` (only `"true"` is on). `setting_rule` falls back to `SettingRule::Bool` for a parsed HC key; `every_default_has_a_rule_and_satisfies_it` now counts `display.hc.hub` as a default covered by the prefix rule, and also checks every exact rule has a default.
- **Rows:** `device.register` inserts `display.hc.<id> = "false"` only for `paired` devices (the spec says one key per paired kiosk; kds rows can follow when Kitchen needs them). `seed_defaults` backfills the Hub key (via `DEFAULT_SETTINGS`) and each non-revoked paired device's key.
- **LAN push:** `push_settings` now builds one `settings.changed` per paired session with only the values that session may read, so a device's HC value reaches only that device and `kiosk.*` still reaches all. `paired_setting_keys` selects `kiosk.*` and device HC keys.
- **Kiosk service:** `theme.rs` (cache under the real key `display.hc.<id>` in `kv`, so a re-provisioned device never inherits another id's rev); `AppliedRevs::{seed, record}`; `Ctx::theme` watch seeded from the cache at start. The HC key is fetched in its **own** `settings.get` after the process-list one, so a Hub that refuses it never costs the process settings. The supervisor sends `theme{hc}` right after `hub_status` on connect and on change; `theme` is also an IPC request kind.
- **Kiosk shell:** `tauri.conf.json` window gets `"create": false`; `setup` starts the bridge first, `block_on(bridge.wait_theme(1 s))`, falls back to `theme.json`, builds the window with `WebviewWindowBuilder::from_config(..).initialization_script(..)` (the script sets `data-theme` as soon as `<html>` exists, via a `MutationObserver` if needed), then the existing `place_window` shows it. "Asking the service" is the service's `theme` push on connect, not a request round trip. The bridge's value is re-read right before the build, so a late answer still makes the init script; a page that missed a change re-reads it through the new `theme` command (a `theme` event that arrived first wins). With no `main` window (missing config or a failed build) the app exits 1 so the service's watchdog relaunches it. The Playwright first-paint test injects the real init script from `apps/kiosk/tests/fixtures/theme-init-{hc,dark}.js`, which a Rust test keeps byte-identical to `init_script` (`ARENA_WRITE_FIXTURES=1` rewrites them; the folder is in `.prettierignore`). Each change is saved to `theme.json` and emitted as the `theme` event (`ipc-names.json`).
- **Hub UI:** `ui.hc` is applied in `+layout.js` `load` (before the shell renders) and in `setUiSettings` (snapshot and `settings.changed`). `normalizeUi` accepts a `ui` without `hc` (older Hub). The section uses a native checkbox with `role="switch"` (44px, state also in text), not `TabControl` — `TabControl` takes one catalogue label, so every row's tablist would have the same name; there is no shared switch component yet.
- **HC audit fixes:** the kiosk toast's Dismiss button had its own 3px focus ring that beat the HC rule (now the 4px HC ring and 2px white outline in HC); the Hub's More sheet backdrop was translucent (opaque `--bg` in HC). Disabled controls keep their 40% opacity in HC by design (`base.css` excludes `:disabled`).
- **Not verified here:** the Windows target could not be type-checked locally (the pinned toolchain lacks `x86_64-pc-windows-msvc`); the shell's Windows-only code was not changed. The pre-paint path itself (Tauri window build + init script) is not exercised by an automated test; the decision function, the bridge wait and the webview behaviour are.

## Spec Change Log

## Review Triage Log

Iteration 0. Sources: BH = Blind Hunter, ECH = Edge Case Hunter, VG = Verification Gap.

| # | Source | Finding | Verdict | Route | Evidence |
|---|--------|---------|---------|-------|----------|
| 1 | BH | `build_window` returning `None` leaves the kiosk with no window | medium | patch | With `create:false`, nothing else builds the window, and the service watchdog only relaunches a process that exits or goes silent. Exit with an error so the watchdog relaunches the app. |
| 2 | BH, ECH | The late-answer `eval` runs before navigation and can be lost | low | patch | It is eval'd right after `build()`, so it may hit the blank document. Re-read `bridge.theme()` just before building and drop the eval. |
| 3 | BH | `window.__ARENA_THEME__` is never read | low | patch | No reader in `apps/` or `packages/`. Delete it. |
| 4 | VG | No test runs the real `init_script` or the `main` window lookup | (gap) | patch | Pre-verified: the Playwright test injects a hand-copied stub. Add a fixture compared against `init_script` output, used by the kiosk test, plus a config test for `main` (`create:false`, `visible:false`). |
| 5 | VG | The kiosk layout's `currentTheme()` reply can overwrite a newer `theme` event | low | patch | Confirmed in `+layout.svelte` (it applies the reply unconditionally after registering `onTheme`). Skip the reply if an event arrived first. |
| 6 | ECH | Cancelling the PIN focuses a switch that is still disabled; a refused elevated save never returns focus | low | patch | Confirmed: `InlinePin` calls `oncancel()` then `returnFocusTo.focus()` synchronously (InlinePin.svelte:147-148), while the switch is still `disabled`. Focus after a `tick()`. |
| 7 | ECH | Overlapping `load()` calls can resolve out of order and overwrite newer revs | low | patch | Three triggers can overlap (version effect, rows effect, after a save). Add a sequence guard. |
| 8 | BH | The test name "a reloaded page..." doesn't reload | low | patch | `theme.spec.ts:135` never calls `reload()`. Rename it or make it reload. |
| 9 | ECH | A Hub DB restore with a lower rev is ignored by the kiosk cache | maybe-false (medium if true) | defer | The same rev guard as 1.18's process lists, so this predates the story. It would be settled by deciding whether an authoritative `settings.get` after hello may lower a rev. |
| 10 | BH, ECH | Blocking `block_on` for up to 1 s in setup | false | reject | Required by the frozen intent (1 s IPC timeout before show). The bridge runs on the multi-threaded async runtime, so it progresses during the wait. |
| 11 | BH | The player can edit `theme.json` | low | reject | It only affects first paint while the service is slow, and the service corrects it. No security impact; moving the storage adds complexity. |
| 12 | BH | Synchronous `theme.json` write in the emit callback | low | reject | A ~12-byte write, only on a theme change. Rarely felt; the fix adds a task. |
| 13 | BH | Kitchen displays get no HC | low | reject | The intent defines per-device keys for paired kiosks only. Kitchen HC is not in scope, and this diff makes no README claim about it. |
| 14 | BH, ECH | A revoked device keeps its HC row, which stays settable | low | reject | Harmless orphan row; the page lists no revoked devices. The fix adds a revoke branch. |
| 15 | BH, ECH | seed_defaults compares the class as text and silently skips non-ULID ids | low | reject | The writer mints ULID ids. The stored class text is `paired`, so the comparison is correct. |
| 16 | BH, ECH | A row with no setting is disabled with no note; a missing hub key fails the page | low | reject | It only happens before `seed_defaults`, which the Hub runs at start. The fix adds UI branches. |
| 17 | BH | STALE after a PIN if another window changed the row | low | reject | Refusing a stale base_rev is correct. Rare. |
| 18 | BH | The no-theme-toggle patterns are easy to slip past | low | reject | A guard rail rather than a proof, and the patterns are as specified. Widening them adds complexity for rare cases. |
| 19 | BH | The regenerated `SnapshotUi.ts` is not in the diff | false | reject | Excluded from the review diff on purpose (generated output). It is in the working tree and will be committed. |
| 20 | BH | README gaps (two change rows on register, no Hub offline cache) | low | reject | The Hub UI can't work without its Hub anyway. The extra change row is visible in the audit, as intended. |
| 21 | ECH | `loading` goes false while another load is in flight; extra fetch on snapshot reloads | low | reject | Only redundant fetches; no wrong state once #7 is fixed. |
| 22 | ECH | A settings.changed during a save is skipped when the save is refused without STALE | low | reject | A narrow window; the next change or a revisit reloads. |
| 23 | ECH | A `theme` event emitted before the listener exists is lost | false | reject | `currentTheme()` reads `bridge.theme()`, which the message has already updated, so mount applies the latest value. |
| 24 | ECH | The init script is fixed at build, so a webview reload paints the startup theme first | low | reject | The kiosk is relaunched as a process, not reloaded; a reload is rare and mount corrects it. |
| 25 | ECH | Claim: an HC kiosk can paint dark after a slow service start | false | reject | The frozen intent specifies the 1 s fallback to the app's own cache, and the app saves every value it receives. #2 narrows the late-answer window. |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures && cargo test --locked -p arena-kiosk-svc --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub exec playwright test` -- expected: pass
