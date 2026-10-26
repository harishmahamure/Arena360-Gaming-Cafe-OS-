---
title: 'Story 3.1: Branch billing rules — rounding, minimum, grace and time-up mode'
type: 'feature'
created: '2026-10-26'
status: 'done'
baseline_commit: '628df76cb901da6836ee77309b20d0df2f70db61'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-26-branch-operating-settings-opening-hours-fulfilment-kiosk-vol.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-1-tax-settings-gst-registration-rates-inclusive-or-exclusive-a.md'
---

<intent-contract>

## Intent

**Problem:** The Hub has no billing rules. Session start (3.9), billing (3.11), grace and time-up (3.26) have nothing to snapshot or read, and an owner cannot say how time is rounded or what happens at time-up.

**Approach:**
- Four settings stored through the existing `settings.set` intent and `SettingRule` machinery, edited in Settings → Branch → Billing by a Manager or Owner, with a Counter getting inline manager-PIN elevation as the tax page does.
- A `BillingRules` value that later stories snapshot at session start.
- Grace and time-up mode pushed to connected kiosks (AD-12).

## Boundaries & Constraints

**Always:**
- **Keys and rules** (plain `Integer` / `OneOf` rules, no new rule type; defaults seeded for new and existing branches through `DEFAULT_SETTINGS`; a failure is `VALIDATION_FAILED` with `field` = the key via `KEYED_FIELD_KEYS`):
  - `billing.rounding_min`: integer 1–60, default `15`.
  - `billing.minimum_min`: integer 0–240, default `30`.
  - `billing.grace_min`: integer 0–10, default `2`.
  - `billing.time_up_mode` ∈ {`clean_up`, `staff_handles`}, default `clean_up`.
- **`BillingRules {rounding_min, minimum_min, grace_min, time_up_mode}`** lives in `arena-proto` (consts, defaults, `TimeUpMode`). `arena-core` reads it from the settings rows inside a transaction, falling back to defaults per key. Value semantics: a copy taken at one time never changes when settings change later. Story 3.9 stores that copy on the session; this story adds no session table.
- **Permissions:** `settings.set` already needs `settings.edit`. A Counter's save goes through `elevate` with one ticket item per changed key (`settings.set` + key) and is audited with `actor` and `elevated_by`. No new permission.
- **Kiosk push:** `billing.grace_min` and `billing.time_up_mode` are readable by paired sessions (`may_read_setting`) and pushed in `settings.changed` (`paired_setting_keys`). The kiosk service stores them in its KV cache with validation, like `kiosk.volume_default`. Nothing on the kiosk acts on them yet.
- **Hub UI** (`apps/hub`): a Billing tab under Settings → Branch with:
  - three number inputs (rounding, minimum, grace) with unit and range hints;
  - a two-option time-up control;
  - a one-line explanation for each field;
  - inline errors per field and nothing written on an invalid value;
  - the Save button visible for a Counter, opening `InlinePin`;
  - live refresh when `settings.changed` arrives for these keys.
  Every string lives in `en.json`. Targets ≥ 44 px; axe passes in the Hub themes and HC.
- Non-numeric input (empty, `1e1`, `+5`, `-1`, `12.5`) is rejected by the UI before submit and by the rule if it arrives anyway.

**Never:**
- Session, segment, billing arithmetic or the settle path (Stories 3.9–3.12).
- New tables or migrations.
- Kiosk behaviour on grace or time-up (Stories 3.10, 3.26).
- A raw `settings.set` that bypasses `apply()`.
- Code comments (user rule).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Defaults | new or upgraded branch | `15 / 30 / 2 / clean_up` | N/A |
| Valid save | rounding 10, minimum 0, grace 10, `staff_handles` | stored; audit row; `settings.changed` to Hub views and kiosks | N/A |
| Bounds | rounding 1 and 60; minimum 240; grace 0 | accepted | N/A |
| Out of range | rounding 0 or 61; minimum 241; grace 11 | `VALIDATION_FAILED` field = key; inline error; nothing written | N/A |
| Non-numeric | `abc`, empty, `1e1`, `+5`, `12.5` | rejected | UI blocks submit |
| Bad mode | `Clean up` | `VALIDATION_FAILED` | N/A |
| Counter save | Counter session | Save visible, opens `InlinePin`; ticket applies; audit has both actors | No ticket: `PIN_REQUIRED`; wrong PIN rejected inline |
| Stale | another save moved the row | `STALE`, form reloads | message shown |
| Running session | rules saved after a snapshot was read | the earlier `BillingRules` value is unchanged; a new read sees the new rules | N/A |

</intent-contract>

## Code Map

- `crates/arena-core/src/settings_defaults.rs` -- `DEFAULT_SETTINGS`, `SETTING_RULES`, `KEYED_FIELD_KEYS` (array length 13 → 17); `every_default_has_a_rule_and_satisfies_it` counts both lists, so add equally.
- `crates/arena-core/src/apply/settings_set.rs` -- `validate`: `Integer`/`OneOf` paths already apply; no change expected.
- `crates/arena-proto/src/operating.rs`, `lib.rs` -- pattern for key consts, defaults and re-exports; new `billing.rs` beside it.
- `crates/arena-proto/src/scope.rs` -- `may_read_setting`; `crates/arena-api/src/lan/registry.rs` `paired_setting_keys` -- add the two kiosk keys.
- `crates/arena-kiosk-svc/src/operating.rs` -- `KEYS`, `valid`, `load`, `KioskOperating`; `tests/runtime.rs` (~1764) -- push test to extend.
- `crates/arena-core/src/locale.rs` (`read_rows`) -- pattern for reading settings inside a transaction; new `crates/arena-core/src/billing.rs`.
- `crates/arena-core/src/apply/tests.rs` -- `mod tax` (~6173) and the settings tests: the pattern for the apply tests, permission denial and the Counter-with-ticket path.
- `apps/hub/src/routes/settings/branch/+layout.svelte` -- add the Billing link; `branch/service/+page.svelte` -- multi-field save loop; `branch/tax/+page.svelte` -- `InlinePin` elevation with `elevate(items, pin)` and `submitIntent(..., {elevationTicket})`.
- `apps/hub/src/lib/operating.ts` (key consts), `apps/hub/tests/tax.spec.ts` (mocked Hub, Manager and Counter tokens, axe), `packages/ui/src/messages/en.json` (`hub.settings.branch.nav.*`, `hub.settings.service.*`).

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/billing.rs`, `lib.rs` -- keys, defaults, bounds, `TimeUpMode`, `BillingRules` with `defaults()` and `from_lookup`, `KIOSK_BILLING_KEYS`; unit tests.
- [x] `crates/arena-core/src/settings_defaults.rs`, `crates/arena-core/src/billing.rs`, `lib.rs` -- defaults, rules, keyed fields, `read(tx)`.
- [x] `crates/arena-core/src/apply/tests.rs` -- bounds, bad values, permission denial, Counter with ticket (audit both actors), defaults seeded on an upgraded branch, snapshot-value test, the `settings.set` proptest generator covers the new keys.
- [x] `crates/arena-proto/src/scope.rs`, `crates/arena-api/src/lan/registry.rs`, `crates/arena-kiosk-svc/src/operating.rs`, `crates/arena-kiosk-svc/tests/runtime.rs` -- kiosk read, push and store.
- [x] `apps/hub/src/routes/settings/branch/billing/+page.svelte`, `+layout.svelte`, `apps/hub/src/lib/billing.ts`, `en.json` -- the Billing tab.
- [x] `apps/hub/tests/billing.spec.ts` -- defaults shown, valid save, inline errors write nothing, Counter PIN path, stale, live refresh, axe.

**Acceptance Criteria:**
- Given a Manager on Settings → Branch → Billing, when the screen opens, then it shows the four values (defaults `15`, `30`, `2`, "Clean up") each with a one-line explanation.
- Given valid values, when saved, then `settings.set` stores them, the audit shows the change, and an open second Hub view and a connected kiosk receive `settings.changed` without a reload.
- Given an out-of-range or non-numeric value, when saved, then the field shows an inline error and nothing is written.
- Given a billing-rules change after `BillingRules` was read, when it is read again, then the new read has the new rules and the earlier value is unchanged.
- Given a Counter, when they change a rule, then Save stays visible, `InlinePin` opens, and the audited change carries both actors.

## Spec Change Log

## Review Triage Log

## Design Notes

Four plain keys (not one JSON setting) reuse `Integer` and `OneOf` rules with no new `SettingRule` variant or parser. A multi-field save loops one `settings.set` per changed key, as the service page does, and a Counter's single PIN ticket carries one item per changed key (a ticket item is single-use per intent and key).

`BillingRules::from_lookup(|key| Option<String>)` keeps the parse in `arena-proto` so core (tx reads) and any later reader share it. A missing or malformed value yields the key's default.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast`, `-p arena-api --features dev-fixtures`, `-p arena-kiosk-svc --features dev-fixtures`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check` -- pass (known `arena-hub-ui` sidecar failures excepted).
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell`; every `pnpm run check:*` (generated types, message catalogue lint); `pnpm run test:scripts`.
