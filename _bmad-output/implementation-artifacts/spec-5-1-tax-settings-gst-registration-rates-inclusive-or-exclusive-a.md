---
title: 'Story 5.1: Tax settings — GST registration, rates, inclusive or exclusive, and simple tax outside India'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'dd6d04b14adce64c76170f7d7eb75db6aba1ecea'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-26-branch-operating-settings-opening-hours-fulfilment-kiosk-vol.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-12-branch-locale-settings-currency-number-format-and-timezone.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub has no tax configuration. Story 5.2 (per-rate tax on every bill) and the invoice stories have nothing to read: not whether the branch is GST registered, its GSTIN, the default rates, or whether prices include tax.

**Approach:**
- One new setting, `branch.tax`, a JSON value holding country, GST registration, GSTIN, inclusive flag, default product and play-time rates (basis points), optional SAC code, and, outside India, a tax label and one default rate.
- Stored and changed only through the existing `settings.set` intent and `SettingRule` machinery; validated as a whole so cross-field rules (GSTIN needed when registered, GST only in India) are atomic.
- A Hub screen Settings → Branch → Tax, editable by a Manager or Owner; a Counter member edits and approves the save with a manager PIN (the Story 1.13 pattern).

## Boundaries & Constraints

**Always:**
- **Setting `branch.tax`** (TEXT JSON, key order fixed, unknown keys refused): `{country, gst, gstin, inclusive, product_bp, play_bp, sac, label, rate_bp}`.
  - Default for new and existing branches (via `DEFAULT_SETTINGS` and `branch.seed_defaults`): `IN`, GST off, empty GSTIN, inclusive, product 500, play 1800, empty SAC, label `Tax`, rate 0.
  - `country`: two uppercase ASCII letters (ISO 3166-1 alpha-2).
  - `gst` true only when `country == "IN"`; then `gstin` must be valid: 15 characters, state code 01 to 38, 97 or 99, PAN shape, entity character, `Z`, and the mod-36 check digit. When `gst` is false `gstin` must be empty.
  - India: `product_bp` and `play_bp` within 0 to 2800. Elsewhere: `rate_bp` within 0 to 10000. Rates are integer basis points (1% = 100); the UI converts from a percent with up to two decimals.
  - `sac`: empty or 4 to 8 digits. `label`: 1 to 20 characters, trimmed, no control characters.
  - `tax_mode` is derived, never stored: `gst_in` for `country == "IN"` and `gst`, else `simple` (`TaxSettings::tax_mode`).
- **Errors:** `VALIDATION_FAILED` with `field` = `branch.tax.<part>` and a stable reason in the message (`GSTIN_INVALID`, `RATE_RANGE`); nothing is written. `STALE` on a `base_rev` mismatch. The audit shows the before and after values through the ordinary settings change rows.
- **Not a kiosk setting:** `branch.tax` is not readable by paired kiosks (`may_read_setting` unchanged).
- **Hub UI** (`apps/hub`): Settings → Branch → Tax, after Service.
  - Country field. India: GST registered, GSTIN (only when registered), inclusive/exclusive, default product rate, default play-time rate, SAC. Elsewhere: label, rate, inclusive/exclusive. Invoice prefix = the branch code, read-only.
  - Inline per-field errors, ≥ 44 px targets, strings in `en.json`, axe clean.
  - New values apply to bills opened after the save (nothing reads them before Story 5.2).
- **Tests:** Rust (checksum and shape, mode, rate bounds both modes, SAC, label, settings.set save/refusal/STALE, seeding count); Playwright (GST register with a bad then a good GSTIN, outside India, STALE, Counter PIN prompt, axe).

**Never:**
- A stored `tax_mode`, a new table, a new intent kind or a new error code.
- Tax computation (Story 5.2), HSN on products (Story 5.3), or invoice numbering.
- Changing the `branch.bootstrap` payload (Story 2.13 owns the profile).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Register | IN, gst, valid GSTIN | stored; mode `gst_in` | N/A |
| Bad GSTIN | gst, wrong check digit or empty | refused, field `branch.tax.gstin`, `GSTIN_INVALID` | nothing written |
| GSTIN while off | gst false, GSTIN set | refused `GSTIN_INVALID` | nothing written |
| GST rate | product 28.01% | refused `RATE_RANGE`, field `branch.tax.product_bp` | nothing written |
| Simple rate | AE, 100% / 100.01% | accepted / refused `RATE_RANGE` | nothing written |
| GST abroad | AE, gst true | refused, field `branch.tax.gst` | nothing written |
| Stale | `base_rev` behind | `STALE` | UI shows the stale notice |
| Counter | member without `settings.edit` | Save opens the manager PIN | UI |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/tax.rs` -- key, default, `TaxSettings` (parse, check, mode), `gstin_valid`.
- `crates/arena-core/src/settings_defaults.rs`, `apply/settings_set.rs` -- default, `SettingRule::Tax`, field-specific refusal.
- `apps/hub/src/lib/tax.ts`, `apps/hub/src/routes/settings/branch/tax/+page.svelte`, `branch/+layout.svelte`, `packages/ui/src/messages/en.json`, `apps/hub/tests/tax.spec.ts`.

## Tasks & Acceptance

- [x] Proto, core rule and default, tests.
- [x] Hub page, nav link, messages, Playwright.
- [x] Verification commands from the loop state.

**Acceptance Criteria:** the Given/When/Then of Story 5.1 in `epics.md`, except as recorded in the decision log (country inside `branch.tax`, no migration, locked-prefix note deferred).

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter with the spec as claims, Verification Gap, all on Sonnet; money/tax data-integrity). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/EC: GSTIN state code parsed with `u8::parse` accepts a leading `+` | medium | P | digits required; test with a valid-checksum `+1...` |
| 2 | BH/EC: stale form kept after STALE or a remote change, so Save re-sends old values on a fresh rev | medium | P | STALE reloads the form; a remote change reloads only a clean form; Playwright test |
| 3 | BH/EC: a rate above u16 gives a field-less error the UI never shows | medium | P | UI refuses above 100 percent client side; an unmapped server field shows the generic failure |
| 4 | BH/EC: cross-mode rates unchecked (India `rate_bp`, abroad `product_bp`/`play_bp`) | medium | P | all three rates bounded always, GST cap on the active India pair |
| 5 | BH/EC: stored JSON not canonical (spacing, key order) | medium | P | the stored value is `TaxSettings::to_json`; test |
| 6 | EC: PIN wasted when a bad rate is typed by a Counter member | low | P | client check runs before the PIN opens |
| 7 | BH: ticket dropped by a string match on the status | low | P | `done` flag |
| 8 | EC: country, GSTIN, SAC, label not trimmed or upper-cased | low | P | `taxJson` normalises |
| 9 | VG: boundaries (state 38/39/97/99, SAC 3/4/8/9, label 20/21, country, unknown keys, overflow, `play_bp`, mode) and error messages untested | medium | P | tests added |
| 10 | VG: Counter elevated save, field mapping, STALE reset untested in Playwright | high | P | three tests added |
| 11 | BH: India without GST has no editable rate (`product_bp`/`play_bp` vs `rate_bp`) | medium | D | Story 5.2 decides which rates a non-registered Indian branch uses |
| 12 | EC: existing non-India branches are seeded `IN` | low | D | the owner sets the country; Story 2.13 can seed it from the cloud profile |
| 13 | EC: Epic AC says Counter fields are read-only; code follows Story 1.13 (edit, PIN on Save) | low | R | spec decision, logged |
| 14 | EC: label Cf characters, country `ZZ` accepted | low | R | cosmetic, no integrity effect |
| 15 | BH: stored row that fails `parseTax` cannot be fixed in the UI; errors not cleared on edit; first error only | low | R | only reachable through a non-UI client |
| 16 | VG: audit rows, seeding for an upgraded branch, 44 px on checkboxes, axe on error state | low | R | covered by the generic settings tests and existing shell axe pass |

