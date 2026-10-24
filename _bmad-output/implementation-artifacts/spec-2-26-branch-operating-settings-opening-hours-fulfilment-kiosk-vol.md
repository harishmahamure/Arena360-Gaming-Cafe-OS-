---
title: 'Story 2.26: Branch operating settings — opening hours, fulfilment, kiosk volume, backup location, ad consent'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '050c4fc2d257527878ce5f182f7c286475ba4b89'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-12-branch-locale-settings-currency-number-format-and-timezone.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-21-high-contrast-theme-as-a-per-device-branch-setting.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub has no opening hours, fulfilment mode, kiosk volume, backup location or ad consent. Pricing, passes, alerts and the go-live checklist have nothing to read.

**Approach:**
- New Hub settings, stored through the existing `settings.set` intent and `SettingRule` machinery and edited in Hub Settings by a Manager or Owner.
- Kiosk-relevant values are pushed by `settings.changed`.
- The Hub computes a `SetupStatus` that later stories (the heartbeat `setup` block in 2.22 and 2.28) will report.

## Boundaries & Constraints

**Always:**
- **Setting keys and rules.** Extend `setting_rule` as Epic 1 did for locale and `display.hc.*`, with defaults created for new and existing branches the way 1.21 seeded its rows. Each value is validated, and a failure is `VALIDATION_FAILED` with `field`.
  - `branch.opening_hours`: a JSON array of 7 entries (Mon…Sun). Each entry is `{closed: true}` or `{open_min, close_min}`, with integer local minutes 0–1439.
    - `close_min < open_min` means the day closes past midnight (wrap).
    - `open_min == close_min` is invalid, and the `field` is `branch.opening_hours[<day>]`.
    - The default is all 7 days closed, with `set: false` tracked through a separate `branch.opening_hours_set` flag that the first valid save sets.
  - `branch.fulfilment` ∈ {`seat`, `counter`}, default `counter`.
  - `kiosk.volume_default`: an integer 0–100, default 50.
  - `system.backup_dir`: a string path, default empty (meaning unset).
    - **Before save:** the Hub checks the path exists, is a directory, and is writable by creating and deleting a probe file `.arena360-write-test-<ulid>`.
    - **Not writable:** `VALIDATION_FAILED` with `field:"system.backup_dir"` and a reason code in the message key (`not_found`, `not_dir`, `not_writable`).
    - Removable drives are ordinary paths.
  - `branch.ads_consent`: `{given: bool, at, staff_id}`. It's set only by the dedicated `ads.consent` path below, never as a raw `settings.set` payload.
- **Ads consent:**
  - Settings → Branch → Ads shows the consent text, an ad-policy link and the consent tick.
  - Saving records `given`, `at` (UTC ms) and the acting staff id via an intent with the typed actor, from the session.
  - The section is shown only while the branch is on Free. Until the licence stories land, "Free" means no paid entitlement is recorded on the Hub: add a `hub_tier()` helper that returns `free` today, and that 2.12 will replace.
- **Kiosk push:** `kiosk.volume_default` and `branch.fulfilment` go to every connected kiosk via the existing `settings.changed` LAN frame and are readable by the kiosk session, following 1.21's scoping. The kiosk stores them; behaviour that uses them comes in later epics.
- **Permissions:** writes need the existing branch-settings permission that a Manager or Owner holds, enforced through `apply()` as Epic 1 does. The Counter role gets read-only views.
- **Setup status:** `SetupStatus {opening_hours_set, backup_location_set, ad_consent: given|not_needed|missing}`, computed in `arena-core` from the settings. It's exposed on the Hub `GET /ui/snapshot` (`setup` field) and `GET /healthz` (`setup`). 2.22 and 2.28 add their own fields later.
- **Hub UI** (`apps/hub`):
  - **Settings → Branch → Hours:** 7 rows. Each has a Closed toggle and open/close time inputs (24 h, locale format per 1.12). A row whose close time is earlier than its open time shows "closes next day". Errors appear inline per field.
  - **Settings → Branch:** fulfilment as a two-option control, and a kiosk default volume slider (0–100, with its number shown, keyboard operable).
  - **Settings → System → Backup location:** a path field, a Browse button where the Tauri dialog is available, and the inline writable check.
  - **Settings → Branch → Ads:** as above.
  - Every string lives in `en.json`. Targets are ≥ 44 px. axe passes in both Hub themes and HC.
- **Tests:**
  - **Rust:**
    - opening-hours validation (wrap, closed, equal, out-of-range, wrong length);
    - fulfilment and volume bounds;
    - backup dir: ok, missing, file not dir, read-only (chmod on Unix; `#[cfg(windows)]` for an ACL check if feasible, else skipped with a reason);
    - ads consent recorded with actor;
    - permission denial for Counter;
    - kiosk push of both keys;
    - the `SetupStatus` matrix.
  - **Playwright** (`apps/hub`): each section, error states, consent tick, axe.

**Never:**
- New tables outside the settings mechanism (other than what its rules need).
- Kiosk behaviour beyond storing the pushed values.
- Ads shown on a paid tier once 2.12 says so.
- A backup write beyond the probe file.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Hours ok | Mon 10:00–23:00 | stored as `{open_min:600, close_min:1380}`; `opening_hours_set` | N/A |
| Past midnight | Fri 18:00–02:00 | `{1080, 120}` accepted (wrap) | N/A |
| Equal | 10:00–10:00 | `VALIDATION_FAILED` field `branch.opening_hours[0]` | N/A |
| Closed day | Sun closed | `{closed:true}` | N/A |
| Bad length | 6 entries | `VALIDATION_FAILED` | N/A |
| Volume | 101 | `VALIDATION_FAILED` | N/A |
| Fulfilment push | set `seat` | connected kiosk receives `settings.changed` | N/A |
| Backup ok | writable dir | stored; probe file removed | N/A |
| Backup bad | missing / file / read-only | `VALIDATION_FAILED` with reason | N/A |
| Ads consent | tick on Free | `{given, at, staff_id}` recorded | N/A |
| Counter write | Counter session | `PERMISSION_DENIED` | N/A |
| Setup status | defaults | `{false, false, missing}`; after all set `{true, true, given}` | N/A |

</frozen-after-approval>

## Code Map

Investigate from the Epic 1 specs in `context:` (1.12 locale settings, 1.21 `display.hc.*` rule + push + seeding). Key known places:
- `crates/arena-core` -- `setting_rule`, `SettingRule` (incl. `Bool` added in 1.21), `settings.set` apply path, `HubStatus`.
- `crates/arena-store/src/branch.rs`, `crates/arena-store/migrations/` -- settings rows + seeding (`seed_defaults`); **Hub migration number reserved: `0011_*` (2.18 holds 0012).** Only add one if the settings mechanism needs it.
- `crates/arena-api/src/{health.rs,ui_events.rs,lan/*}` -- snapshot/healthz builders, `settings.changed` LAN push and `may_read_setting` scoping.
- `apps/hub/src/routes/settings/**`, `apps/hub/tests/*`, `packages/ui/src/messages/en.json`.
- `crates/arena-kiosk-svc` -- where pushed settings are received and stored (1.21 HC pattern).

## Tasks & Acceptance

**Execution:**
- [ ] `crates/arena-core/**`, `crates/arena-store/**` -- rules, defaults, ads-consent intent, `SetupStatus`.
- [ ] `crates/arena-api/**`, `crates/arena-proto/**` (+ generated TS) -- snapshot/healthz `setup`, kiosk push.
- [ ] `crates/arena-kiosk-svc/**` -- store pushed values.
- [ ] `apps/hub/src/routes/settings/**`, tests, `en.json`.

**Acceptance Criteria:**
- Given a dev Hub signed in as a Manager, when all four sections are completed, then `/healthz` reports `setup: {opening_hours_set: true, backup_location_set: true, ad_consent: "given"}` and a connected console kiosk has the new volume and fulfilment values.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast`; `-p arena-hub --features dev-fixtures`; `-p arena-api --features dev-fixtures`; `-p arena-kiosk-svc --features dev-fixtures`; clippy incl. dev-fixtures; fmt -- pass (known `arena-hub-ui` sidecar failures excepted).
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell` (if 5173 is held by the user's dev server, run against it and record); every `pnpm run check:*`; `pnpm run test:scripts`.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter + Verification Gap on Sonnet; risk-based: settings story). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/VG: backup probe before authorization (bogus ticket), on stale and replay | medium | P | gated on `elevation_ticket.is_some()` before redeem |
| 2 | BH/VG: core `Path` rule accepts relative paths; other callers skip the probe | medium | P (absolute rule) / D (probe on non-HTTP paths, with the mailbox in 2.15) | |
| 3 | BH: the probe can hang on a dead network path | medium | P | no timeout |
| 4 | BH/VG: `/healthz` SQLite read per request; unauth setup flags | medium | P (cache) / R (exposure: flags only) | |
| 5 | BH: withdrawn consent reads as Missing | low | R | the checklist needs consent either way |
| 6 | BH: consent stores no policy version | medium | P | consent audit |
| 7 | BH: `hub_tier()` hard-coded Free | low | R | per spec until 2.12 |
| 8 | BH: Ads tab shown when the tier is unknown | low | R | Free is the only tier today |
| 9 | BH: all-closed week marks hours set | medium | P | checklist bypass |
| 10 | BH: reason code carried in `message` | low | R | tested contract, stable names |
| 11 | BH: kiosk caches values unused; UI copy | low | R | the spec says the kiosk only stores them in Epic 2 |
| 12 | BH: service page saves two settings sequentially | low | R | each save is independent and refusals show inline |
| 13 | BH: hours validation duplicated in Rust and TS | low | R | both tested; UI validation is advisory |
| 14 | BH: missing doc comments | false | R | the user's no-comments rule |
| 15 | BH: API refusal tests; root skip hides coverage | low | P | |
| 16 | VG: STALE, 403, live refresh and withdraw untested in Playwright | medium | P | |
