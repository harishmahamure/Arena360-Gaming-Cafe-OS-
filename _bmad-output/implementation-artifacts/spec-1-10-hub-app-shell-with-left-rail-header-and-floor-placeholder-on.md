---
title: 'Story 1.10: Hub app shell with left rail, header and floor placeholder on the seeded branch'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'ae49c89fcd6ab3ca2c4b83953a449d0de69fa8f9'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-5-shared-base-components-meet-the-accessibility-floor.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub app is still a placeholder page. Staff have no frame to navigate in, no branch header and no floor area, and there is no installer that starts the Hub service.

**Approach:**
- **Hub UI shell:** build it in `apps/hub`: rail, header, floor home, placeholder routes, a bottom bar below 840px and Ctrl+1–4. It reads a branch snapshot from the running `arena-hub` over a loopback-only endpoint.
- **Starting the service:** the Tauri app launches `arena-hub` as a bundled sidecar.
- **Installer:** CI builds the Windows NSIS installer, which refuses Windows versions older than 10 1803.
- **Benchmark:** a release benchmark workflow runs on the reference-hardware runner.

## Boundaries & Constraints

**Always:**
- **Snapshot API:**
  - `arena-api` adds `GET /ui/snapshot` on the existing loopback health server (127.0.0.1:7409). It returns `HubSnapshot { branch: {name, branch_code} | null, devices: [{device_id, label, class, zone: null}] }`.
  - The type lives in `arena-proto` and is exported to TS. The read goes through `arena-core` → `arena-store`, so the layering holds.
  - CORS allows only the Tauri origins (`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`) and the vite dev/preview origin `http://localhost:5173`.
  - Revoked devices are excluded.
- **Hub UI (`apps/hub`):**
  - **Routes:**
    - `/` redirects to `/floor`.
    - The routes are `/floor`, `/pos`, `/players`, `/sessions`, `/inventory`, `/shift`, `/reports`, `/settings` and `/system`.
    - Each non-floor route is a placeholder with one `h1` from the catalogue.
  - **Layout:**
    - `+layout.svelte` renders `<header>` (`hub-header`, 72px), `<nav aria-label>` (`hub-rail`, 88px) and `<main>`.
    - Rail items have an icon and a text label, in this order: Floor view · POS · Players · Sessions · Inventory · Shift · Reports · Settings · System.
    - The current item has `aria-current="page"` and is never shown only by colour; it also has an active fill plus a text weight change.
  - **Header:** shows the branch name and a clock (local time via `Intl`, refreshed each minute). Signed-in staff (1.11) and `chip-offline` (1.22) are out of scope here.
  - **Data loading:** the root `+layout.js` `load` fetches the snapshot before first render, so there is no skeleton.
    - If the Hub is unreachable, the layout renders a catalogue error state with a Retry `Button`, not a blank page.
    - If the branch is `null` (not seeded), the floor shows a catalogue "branch not set up" message.
  - **Floor:**
    - It shows an empty-zone message from the catalogue.
    - It has an "Unassigned" section: an `h2` plus a list of devices by label, each item carrying the text "Unassigned" and a dashed `state-unassigned` outline.
  - **Below 840px:**
    - A 64px `hub-bottom-bar` replaces the rail, with Floor · POS · Players · Shift · More.
    - More opens a bottom sheet listing Sessions, Inventory, Reports, Settings and System. The sheet has `role="dialog"` and `aria-modal`, traps focus, closes on Esc or backdrop, and closing it returns focus to More and keeps the current screen.
    - Every target is ≥ 44px.
  - **Shortcuts:** Ctrl+1/2/3/4 open Floor, POS, Players and Shift. They are ignored when `document.activeElement` is an input, textarea, select or `contenteditable`, and when Alt or Meta is held.
  - **Styling:** shared tokens and components only. All strings come from the catalogue, and every gate stays green.
- **Tauri shell (`arena-hub-ui`):**
  - On start, it probes `127.0.0.1:7409/healthz`. If that fails, it spawns the bundled `arena-hub` sidecar (`bundle.externalBin: ["binaries/arena-hub"]`) with `std::process::Command`, located next to the app executable, with no extra plugin. It then shows the window.
  - The service keeps running when the UI closes.
  - The CSP adds `connect-src 'self' http://127.0.0.1:7409`.
- **Installer (CI):**
  - A new `installer` job on windows-latest:
    - builds `arena-hub` in release;
    - copies it to `apps/hub/src-tauri/binaries/arena-hub-x86_64-pc-windows-msvc.exe` (gitignored);
    - runs `tauri build` with the NSIS bundle;
    - uploads the installer artifact.
  - An NSIS `installerHooks` file (`apps/hub/src-tauri/windows/hooks.nsh`) aborts in `NSIS_HOOK_PREINSTALL` on any Windows build older than 17134, with a catalogue-equivalent message: "Arena 360 Hub needs Windows 10 version 1803 or later."
    - NSIS can't read the JS catalogue, so the string is duplicated with a comment pointing at the key.
  - `ci-ok` needs the `installer` job.
- **Benchmark:**
  - `.github/workflows/release-bench.yml` runs on `workflow_dispatch` and on `v*` tags, `runs-on: [self-hosted, windows, reference-i3]`.
  - It installs the built installer artifact and seeds with a `dev-fixtures` build of `arena-hub`.
  - It runs `scripts/bench/hub-idle.ps1`, which waits 5 minutes and then sums working-set memory: the `arena-hub` process must be < 50 MB, and the Hub UI plus WebView2 child processes < 250 MB. It reads GPU engine utilisation counters and fails if any Hub process is above 0.
  - It is not part of `ci-ok`.
- **Tests (Playwright, in `apps/hub/tests/shell.spec.ts`):**
  - They run on `vite preview`, with `page.route` serving a fixture snapshot (one branch, one device "Dev Kiosk"), on desktop 1280 and phone 400.
  - They assert:
    - the landmarks, one `h1` per route and zero serious or critical axe violations;
    - the rail order and labels;
    - each placeholder route;
    - the bottom bar and More sheet behaviour, including focus return;
    - Ctrl+1–4, including the text-field ignore case (use a test-only input on the placeholder or the sheet);
    - 44px targets;
    - the unreachable-Hub state.
  - Wire them into the CI `a11y` job.
- **Rust tests:** snapshot endpoint unit tests (branch present or absent, a revoked device excluded, CORS headers) and a sidecar probe-or-spawn decision function with tests.

**Never:**
- Staff sign-in (1.11).
- Locale-driven formatting (1.12).
- The offline chip (1.22).
- Real floor tiles or zones (later epic).
- Windows service registration for the Hub (later).
- Exposing `/ui/*` beyond loopback.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Seeded | a branch plus a "Dev Kiosk" device | `/floor`: header "Dev Branch", the Unassigned list with "Dev Kiosk" | N/A |
| Not seeded | branch null | the floor shows the catalogue "not set up" message | N/A |
| Hub down | snapshot fetch fails | the error state with Retry; Retry refetches | N/A |
| Placeholder | click "Reports" | `/reports`, a single `h1` | N/A |
| Narrow | 400px wide | the bottom bar; More opens the sheet; Esc closes it, focus returns to More, the route is unchanged | N/A |
| Shortcut | Ctrl+3, focus on body | `/players` | N/A |
| Shortcut in a field | Ctrl+3 with an input focused | the route is unchanged | N/A |
| Revoked device | a device revoked | not in the snapshot | N/A |
| Old Windows | build 16299 | the installer aborts with the minimum-version message | N/A |

</frozen-after-approval>

## Code Map

- `apps/hub/src/routes/{+layout.svelte,+layout.js,+page.svelte}` -- currently the Placeholder with base.css in the layout and `ssr=false`. Rebuild them as the shell.
- `packages/ui/src/components/*` (Button, Card, ...), `base.css` and the generated tokens (`--hub-*`, `--space-hub-rail-width`, `--space-hub-header-height` and the hub-rail/hub-bottom-bar component tokens) -- use these, adding shell components to `packages/ui` only if they are generic.
- `apps/gallery` -- the Playwright config pattern (webServer builds, then previews; the phone project).
- `crates/arena-api/src/health.rs` -- the loopback axum router. Add `/ui/snapshot` there.
- `crates/arena-core`, `crates/arena-store` -- add the branch and device read queries, and respect the layering.
- `apps/hub/src-tauri/{tauri.conf.json,src/main.rs}` -- the Tauri shell, with the CSP set in 1.2.
- `.github/workflows/ci.yml` -- the `a11y` job and the `build` matrix. Add the `installer` job.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/snapshot.rs` + generated TS; `crates/arena-store`, `arena-core` read functions; `crates/arena-api` `/ui/snapshot` + CORS + tests.
- [x] `apps/hub/src/**` -- the shell, routes, floor, bottom bar, sheet and shortcuts; catalogue keys in `packages/ui/src/messages/en.json`.
- [x] `apps/hub/tests/shell.spec.ts`, `apps/hub/playwright.config.ts` -- the tests; add them to the CI `a11y` job.
- [x] `apps/hub/src-tauri/{tauri.conf.json,src/main.rs,windows/hooks.nsh}`, `.gitignore` -- the sidecar, the CSP and the installer hook.
- [x] `.github/workflows/{ci.yml,release-bench.yml}`, `scripts/bench/hub-idle.ps1`, `README.md` -- the installer job, the benchmark and docs.

**Acceptance Criteria:**
- Given `pnpm --filter @arena/hub test:shell` (Playwright), then every matrix row except the installer row passes.
- Given `cargo test --workspace --locked` and `pnpm check`, then both pass.
- Given a seeded data dir with `arena-hub` running, when `pnpm --filter @arena/hub dev` is opened, then the shell shows "Dev Branch" and "Dev Kiosk". This is a manual check.

## Implementation Notes

- **`externalBin` lives in `apps/hub/src-tauri/tauri.installer.conf.json`**, merged with `tauri build --config` by the installer job only, together with the NSIS target and `installerHooks`. In `tauri.conf.json` it would make tauri-build fail every `cargo build --workspace` without the binary, and tauri-build copies the sidecar to `target/<profile>/arena-hub`, which would overwrite the `arena-hub` binary cargo builds there (and that `CARGO_BIN_EXE_arena-hub` tests run). `scripts/installer-hooks.test.mjs` pins this split, the CSP and the hook text.
- **Snapshot read path:** `arena-store` gains `find_branch_summary`/`Store::branch_summary`; `arena-core::SnapshotReader` (its own read-only connection, like `DeviceDirectory`) builds `HubSnapshot` and drops revoked devices; `arena-api::ui_router` serves it on a blocking thread and is merged into the loopback health router in `arena-hub`.
- **Origin guard:** besides echoing an allowed `Origin`, `/ui/*` answers 403 to any other `Origin` and to a `Host` that is not `127.0.0.1`/`localhost`/`[::1]` (DNS-rebinding guard). Requests without `Origin` (curl, the shell's probe) are served.
- **SvelteKit 3 has no `$lib` alias**, so the Hub imports `src/lib` relatively. `vite preview` is pinned to 5173 so the preview origin matches the CORS allow-list.
- **Rail labels** use `hub-micro` size/weight in sentence case (the class's uppercase and tracking broke "Inventory"/"Sessions" mid-word at 88px).
- **The Tauri window starts hidden**; a setup thread probes, spawns if needed, waits up to 10 s for `/healthz`, then shows it. The sidecar is spawned detached (`CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP` on Windows) and its handle dropped, so it outlives the UI.
- **Ctrl+1–4 text-field case** injects test-only fields (input, textarea, select, contenteditable) into the placeholder from the test, so nothing test-only ships.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Installer never stops the running `arena-hub` before an upgrade or uninstall | medium | The sidecar is detached on purpose, so its exe stays locked. | patch |
| 2 | Bench never uninstalls on the self-hosted runner | low | Installs pile up across runs. | patch |
| 3 | Snapshot read once; the shell goes stale; a cold start shows "unreachable" (blind, edge) | medium | `hub:snapshot` is only invalidated by Retry. | patch |
| 4 | Unreachable screen has no automatic retry | medium | A slow HDD start needs manual Retry. | patch |
| 5 | Retry button swap drops focus; no live region (blind, edge) | low | Two `{#if}` Buttons. | patch |
| 6 | Sidecar exit not detected or logged; probe overruns the deadline (blind, edge) | low | The `Child` is dropped and output goes to stderr in a GUI build. | patch |
| 7 | More sheet stays open when the window widens; picking the current screen loses focus (blind, edge) | low | No media listener. | patch |
| 8 | Any HTTP 200 on 7409 counts as the Hub | low | Probe only checks the status line. | patch |
| 9 | Installer build steps duplicated; full NSIS build on every PR | low | Cost and maintenance trade-off, accepted for now. | reject |
| 10 | Hub built twice in the a11y job; port clash with a dev server | low | CI time only. | reject |
| 11 | Untested 500/403/malformed snapshot responses; branch shape not validated (blind, edge) | low | Direct tests and validation. | patch |
| 12 | A request with no Host header skips the loopback guard | low | HTTP/1.0 requests have no Host. | patch |
| 13 | `/ui/snapshot` error leaks the internal message | low | Return the code only. | patch |
| 14 | Leftovers: hard-coded schema check, empty `+page.svelte`, unbalanced NSIS stack | low | Direct fixes. | patch |
| 15 | `ensure_service` spawn path untested (verify-gap) | low | Pre-verified gap. | patch |
| 16 | Clock minute refresh untested; no refresh after wake (verify-gap, edge) | low | Pre-verified gap. | patch |
| 17 | Ctrl+N key repeat queues gotos | low | Direct fix. | patch |
| 18 | Devices with a non-null zone are hidden | low | Zones don't exist yet; every device's zone is null. | reject |
| 19 | Empty branch name shows a blank header | low | 1.7 validation requires 1–80 characters. | reject |
| 20 | Two UI instances can both spawn the service | low | The second sidecar fails to bind 7409 and exits; the writer lock holds. | reject |
| 21 | Snapshot reads span two statements | low | Wrap them in one read transaction. | patch |
| 22 | Device with an unknown class dropped silently | low | Add a `warn` log. | patch |
| 23 | Bench throws if GPU counters are missing | low | Report it as a failure instead. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked && pnpm check` -- expected: pass
- `pnpm --filter @arena/hub build && pnpm --filter @arena/hub test:shell` -- expected: pass (Node 22.23.4)
- `pnpm -r --workspace-concurrency=1 --filter "./apps/*" build && node scripts/check-no-remote.mjs` -- expected: pass
