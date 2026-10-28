---
title: 'Story 6.7: The attract screen shows branding, rates and the active rule, with no ads'
type: 'feature'
created: '2026-10-29'
status: 'in-review'
baseline_commit: '35ada76f63e387d172cd5b3bc50053d1c2f9b809'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-6-game-media-on-the-kiosk-falls-back-from-video-to-cover-to-so.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** An idle kiosk shows the Epic 1 "locked" placeholder: a player walking up cannot see the café's logo, what an hour costs, or whether a happy-hour rule is on. Nothing carries the logo, this PC's group rate or the active rule from the Hub to the kiosk.

**Approach:** A Manager sets a logo (`kiosk.logo`, PNG or SVG, at most 512 px and 200 KB) in Settings → Branch → Logo. The Hub pushes each paired kiosk a small per-device `attract` event (branch name, this PC's group rate, the active rule, the media of the library's curated videos) every tick; the kiosk service caches the last one and the logo and hands both to the app, which paints the attract screen from that cache before any network call.

## Boundaries & Constraints

**Always:**

- **Logo.** Stored by `settings.set` under `kiosk.logo` as a `data:image/png;base64,…` or `data:image/svg+xml;base64,…` URI (empty = none; the screen then shows the Arena 360 mark). Validated by the Hub (`arena-proto::logo_problem`): decoded size ≤ 200 KB; PNG: signature and IHDR, width and height ≤ 512; SVG: UTF-8, an `<svg` root, no `<script`, `<foreignObject`, event-handler attribute, `javascript:`, `<!ENTITY`, `<image` / `<use` / `href` / `url(` pointing outside the document. The Hub UI resizes a chosen PNG to ≤ 512 px (canvas) and rejects anything over 200 KB; an SVG is accepted as text if it passes the same rules.
- **Push.** `HubEvent::Attract(AttractView)` goes to every paired device on the 5 s fan-out tick (rule starts and ends are wall-clock, not intents, so a tick is the trigger); a rule or rate change is therefore on the kiosk within 5 s (the AC allows 1 min). `AttractView` = `branch_name`, `state` (`ready` | `ask_counter`), live `rate_minor` (the active rule's rate when one applies), `group_name`, `rule: SnapshotActiveRule`, `locale: SnapshotLocale`, `media: [{video, cover?}]` (up to 20 stored video sha256 of enabled library entries linked to the catalogue, in name order).
- **`ask_counter`.** The device is unassigned, in maintenance, or its group has no active rate: no rate and no chip; the screen shows "Ask at the counter" and the login panel is not offered.
- **Cache.** The service stores the last `attract` JSON in its `kv` (`attract.view`) and the logo through the `kiosk.logo` setting (rev-ordered like the theme). On connect it sends the cached attract message to the app first; the screen paints from it with no network wait.
- **Screen.** Replaces the locked placeholder (`data-testid="locked"` becomes `attract`): `logo-slot` top-left (the logo, else the Arena 360 mark), branch name, the group rate ("₹120 an hour", branch locale), the `chip-rule` when a rule applies, and full-bleed `GameMedia` cycling the curated videos (`ATTRACT_VIDEO_MS` = 30 000 each, then the next; a single item keeps playing); no ad slot, no ad code.
- **Login panel.** Any key, click or tap opens a `login-panel` over the same media (the 6.8 form replaces its body; this story renders its title and the "Ask at the counter to start your time." hint); Esc, or 60 s without input, closes it. Never opened in `ask_counter`.
- **IPC.** `IpcMsg::Attract{view?, logo?}` carries the logo, so the service → app direction accepts lines up to 512 KiB (app → service stays 64 KiB). The shell keeps the latest value and answers a getter command on cold start, like `pc_screen`.
- No `backdrop-filter`, no remote fonts, solid `bg` in high contrast (GameMedia already does that); all copy from the catalogue.

**Never:** an ad slot or any ad call on the kiosk; fetching media from the internet; the login form, wallet, plans (6.8, 6.9); authenticating `:7401` requests from `<video>` / `<img>` (kiosk media transport, deferred with 6.6 to 6.23).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Ready PC | assigned, rate 12000, no rule | logo, branch, "₹120.00 an hour", no chip | — |
| Rule on | rule "Happy hour" −20% until 6 pm | live rate shown, chip "Happy hour −20% · until 6:00 pm" | — |
| No rate / maintenance / unassigned | `state=ask_counter` | "Ask at the counter", no rate, login not offered | key press does nothing |
| Cold start offline | cached view + logo | paints at once from cache | — |
| No logo | `kiosk.logo` empty | Arena 360 mark in `logo-slot` | — |
| Bad logo | oversized / scripted SVG / 600 px PNG | `settings.set` fails `VALIDATION_FAILED` on `kiosk.logo` | Hub UI shows the field error |
| Many videos | 3 items | each 30 s, wraps | failed video silently shows cover/solid |
| Key pressed | any key / click | login panel over the media; Esc or 60 s closes | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/attract.rs` (new) -- `AttractView`, `AttractState`, `AttractMedia`, `KEY_KIOSK_LOGO`, `logo_problem`; `frame.rs` `HubEvent::Attract`; `lib.rs` re-exports (ts-rs types generated to `packages/ui/src/generated`).
- `crates/arena-core/src/settings_defaults.rs` (+ `apply/seed_defaults.rs`) -- `kiosk.logo` default `""` and `SettingRule::KioskLogo`.
- `crates/arena-core/src/attract.rs` (new) -- per-device `AttractView` from the branch store (devices_meta, rates, pricing rules via `pricing::resolve`, locale, library entries) plus a media lookup; `devices.rs` `DeviceDirectory::attract_views`.
- `crates/arena-core/src/media_cache.rs` + `crates/arena-store/src/reference.rs` -- `video_sha256(game_id)` beside `cover_sha256`.
- `crates/arena-api/src/lan/registry.rs` -- `tick_attract` in the fan-out tick (media handle passed from `listener.rs`).
- `crates/arena-kiosk-core/src/ipc.rs` -- `IpcMsg::Attract`, `MAX_APP_LINE_BYTES`, `read_msg_limited`.
- `crates/arena-kiosk-svc/src/attract.rs` (new), `hub_link.rs`, `runtime.rs`, `app.rs`, `lib.rs` -- cache + logo + relay to the app, modelled on `theme.rs`.
- `apps/kiosk/src-tauri/src/{bridge,main}.rs`, `ipc-names.json` -- event `attract`, command `attract`.
- `apps/kiosk/src/lib/{bridge.ts,attract.ts,Attract.svelte}`, `routes/+page.svelte`, `tests/{attract.spec.ts,locked.spec.ts,pc-screen.spec.ts,theme.spec.ts}` -- screen, cycle rule, login panel, stubs.
- `packages/ui/src/components/BrandMark.svelte` (new), `index.js`, `messages/en.json` (`player.attract.*`, `hub.settings.branch.logo.*`).
- `apps/hub/src/routes/settings/branch/logo/+page.svelte` (new), `branch/+layout.svelte`, `apps/hub/src/lib/logo.ts` (resize/validate), `apps/hub/tests/logo.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- `attract` types, `HubEvent::Attract`, `logo_problem` + tests; regenerate types
- [x] `arena-core` / `arena-store` -- `kiosk.logo` setting and rule; `attract_views`; `video_sha256`; tests
- [x] `arena-api` -- push `attract` on the fan-out tick; registry tests
- [x] `arena-kiosk-core` / `arena-kiosk-svc` -- `IpcMsg::Attract`, larger service → app lines, attract cache and logo relay; tests
- [x] Tauri shell -- event, getter, names list and tests
- [x] Kiosk app -- `Attract.svelte`, cycle rule, login panel, BrandMark; Playwright suite
- [x] Hub -- Settings → Branch → Logo page and tests
- [x] Catalogue strings; update the locked-placeholder specs

**Acceptance Criteria:**

- Given a Manager uploads a PNG or SVG, then it is stored by `settings.set` (≤ 512 px, ≤ 200 KB), pushed to kiosks, and with no logo the Arena 360 mark shows.
- Given an idle kiosk, then it shows the attract screen with `logo-slot`, branch name, this group's rate, the `chip-rule` when a rule applies and full-bleed cycling `GameMedia`, with no ad slot.
- Given a rule or rate change on the Hub, then the attract rates and chip update within 1 minute without a reload.
- Given a cold start, then branding, rates and rule paint from the cached copy before any network call.
- Given the attract screen, when a key, click or tap arrives, then the login panel opens over the media; Esc or 60 s idle closes it.
- Given a PC in maintenance, unassigned or with no rate, then "Ask at the counter" replaces the rates and the login panel is not offered.

## Spec Change Log

## Review Triage Log

## Design Notes

The attract view is Hub-computed per device (the kiosk does not know its group) and pushed on the existing 5 s tick like `session.tick`; the kiosk service dedupes by value before telling the app. The logo rides the settings path (rev-ordered, cached, pushed on change) rather than the tick, so the tick stays small. Media transport to the webview is the 6.6 deferral: the screen passes `/media/<sha>` URLs to `GameMedia`, which falls back to the solid background silently until the 6.23 transport exists.

## Verification

**Commands:**

- `cargo test -p arena-proto -p arena-core -p arena-api -p arena-kiosk-core -p arena-kiosk-svc` -- expected: pass
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` -- expected: clean
- `pnpm gen:types && pnpm check:generated`, `pnpm check:strings`, `pnpm check:hex`, `pnpm check:no-blur`, `pnpm format:check` -- expected: pass
- `pnpm --filter @arena/kiosk check`, `pnpm --filter hub check` -- expected: 0 errors
- Playwright: `apps/kiosk/tests/attract.spec.ts`, `locked.spec.ts`, `apps/hub/tests/logo.spec.ts` -- expected: pass

## Implementation Notes

- Not run here (Windows host, no Linux target): `crates/arena-kiosk-svc/tests/runtime.rs` and `arena-e2e` are `cfg(unix)`; the new service integration tests (attract tick, rule change, logo, cold-start cache) are written but unexecuted. Their logic is covered by unit tests (`procs::tests`, `app::tests`, `attract::tests`) that do run. 14 `arena-kiosk-svc` lib tests (lockdown, pipe ACL, DPAPI, config ownership) fail on this host for SYSTEM-ownership reasons unrelated to this story; `pnpm check:generated` reports the new generated types as untracked until they are committed.
- A modifier key alone (Shift, Ctrl, Alt, Meta) does not open the login panel, so the hidden admin-exit combination never flashes it; any other key, click or tap does. Input in another surface's field (the admin PIN) is ignored by the screen.
- With no attract view cached yet (never reached the Hub), the screen shows the Arena 360 mark and name with "Ask at the counter" and offers no login.
- `HubEvent::Attract` and `IpcMsg::Attract` box the view (clippy `large_enum_variant`); the wire JSON is unchanged.
- The rate reads as the branch formats it ("₹120.00 an hour"), as in the I/O matrix.
