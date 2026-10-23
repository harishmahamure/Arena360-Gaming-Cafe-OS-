---
title: 'Story 1.21: High-contrast theme as a per-device branch setting'
type: 'feature'
created: '2026-10-23'
status: 'draft'
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

- `crates/arena-core/src/apply/{settings_set.rs,seed_defaults.rs,devices}` and `SETTING_RULES` -- add the pattern rule, row creation and the seed.
- `crates/arena-api/src/lan/session.rs` (1.18 `settings.get` scope and push) and `ui_events.rs` / `snapshot` -- extend them.
- `crates/arena-kiosk-svc` and `crates/arena-kiosk-core` IPC (1.16–1.18) -- add `Theme`.
- `apps/kiosk/src-tauri/src/main.rs` -- the window and IPC client. Add hidden start and the init script.
- `apps/hub/src/routes/+layout.svelte` and `settings/**` (1.12/1.13 sections and the elevation flow) -- add the section and the theme application.
- `packages/ui/src/base.css` and the generated tokens -- the HC rules.

## Tasks & Acceptance

**Execution:**
- [ ] Core, API and kiosk delivery and caching, with tests for every API/kiosk matrix row.
- [ ] The kiosk pre-paint theme, plus a unit test of the decision function (cached value vs. IPC with timeout).
- [ ] The Hub Devices & display section and live theme; Playwright tests (list, toggle, Counter elevation, live switch, HC axe for the Hub and kiosk).
- [ ] `scripts/no-theme-toggle.test.mjs`; catalogue keys; README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` (plus the dev-fixtures runs), `pnpm check` and the Hub and kiosk Playwright suites, then all pass with every matrix row covered.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures && cargo test --locked -p arena-kiosk-svc --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub exec playwright test` -- expected: pass
