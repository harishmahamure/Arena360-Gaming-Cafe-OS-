---
title: 'Story 7.1: Kitchen settings at the Hub, locked on Free'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'dd6d04b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-26-branch-operating-settings-opening-hours-fulfilment-kiosk-vol.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-27-printer-detection-and-test-print-at-the-hub.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub has no Kitchen section and no way to refuse kitchen work on a branch whose plan lacks it.

**Approach:**
- A Settings → Kitchen page that shows `tier-lock` on Free and the KOT printer and kitchen-display list on Basic and above.
- A kitchen tier gate in `apply()`'s permission step that later stories register their intents with.
- Settings stored through the existing `settings.set` machinery. No new intent kind and no Hub migration.

## Boundaries & Constraints

**Always:**
- **Flag source.** The system-managed setting `licence.flags` (comma list, default empty, seeded for new and existing branches, never writable by `settings.set`). The `kitchen` flag unlocks. Story 2.12 will write it from the verified licence.
- **Settings.** `kitchen.kot_mode` in {`receipt` (default), `another`, `none`}; `kitchen.kot_printer` (the Story 2.27 `PrinterSelected` JSON, empty = unset; a LAN address is normalised to port 9100); `kitchen.kot_paper_mm` in {58, 80}, default 80. Validation failures are `VALIDATION_FAILED` with `field`.
- **Gate.** `arena_proto::kitchen_gate(&IntentPayload) -> Option<TierGate>` is the registry (`New` or `ExistingWork`). `guards::tier` runs straight after `permission`. A `New` intent on a branch without the flag is `TIER_LOCKED` (HTTP 403) and nothing is written. `ExistingWork` is never refused by the gate. This story registers the three `kitchen.*` keys of `settings.set` as `New`.
- **Test print.** `POST /ui/printer/test-kot` (Manager or Owner) sends the Story 2.27 test page to the saved KOT printer at the saved KOT paper width and returns the status without recording it, so the receipt printer's status is untouched. `TIER_LOCKED` without the flag.
- **Hub UI** `/settings/kitchen`, linked from the Settings nav:
  - Free: `tier-lock` with a lock glyph, "Available on Basic", "Ask Arena 360 to change your plan.", no fields, no Upgrade link.
  - Basic and above: KOT printer choice, and for "Another printer" Find printer, manual IP, paper width and Test print; an empty "Kitchen displays" list with a disabled "Pair a kitchen display" (enabled in Story 7.6).
  - A Counter or Kitchen role sees the same fields; Save opens `inline-pin`, and one manager PIN approves the changed keys.
  - Save sends one `settings.set` per changed key with `base_rev` (`STALE` on conflict); the audit shows before/after as for any setting.
  - The page reloads when `licence.flags` changes, so an upgrade unlocks it without a restart.
  - Strings in `en.json`; targets at least 44 px; axe passes.
- **Lapse.** A lapse locks the keys again and keeps their stored values. Ticket and KOT creation arrive in later stories, each registering its intents with the gate.

**Never:**
- A new intent kind, table or Hub migration.
- An Upgrade link.
- Pairing a kitchen display (Story 7.6) or kitchen notes (Story 7.2).
- A way to set `licence.flags` outside the licence stories.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Free, open page | no `kitchen` flag | `tier-lock`, no fields | N/A |
| Free, write | `settings.set kitchen.kot_mode` | rejected, nothing written | `TIER_LOCKED` |
| Basic, save | mode `another`, paper 58, LAN IP | three updates with `base_rev`; IP stored as `ip:9100` | `STALE` on conflict |
| Basic, bad value | paper `57` | rejected | `VALIDATION_FAILED` |
| Counter | Save | `inline-pin`; manager PIN approves | `PIN_REQUIRED` re-prompts |
| Test print | saved KOT printer | page sent, status returned, nothing recorded | none saved: `VALIDATION_FAILED` on `kitchen.kot_printer`; Free: `TIER_LOCKED` |
| Lapse | flag removed | keys refuse writes, values kept | `TIER_LOCKED` |
| Existing-work intent | registered `ExistingWork`, no flag | passes the gate | N/A |
| Set the flag list | `settings.set licence.flags` | rejected | `VALIDATION_FAILED` |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/kitchen.rs` -- keys, defaults, `TierGate`, `kitchen_gate`, `has_flag`; `error.rs` -- `TierLocked`.
- `crates/arena-core/src/{settings_defaults.rs,apply/guards.rs,apply/mod.rs,apply/settings_set.rs}` -- rules, seeding, the gate.
- `crates/arena-api/src/{ui_printer.rs,ui_auth.rs}` -- `test-kot` route, status mapping.
- `apps/hub/src/routes/settings/kitchen/`, `apps/hub/src/lib/printer.ts`, `apps/hub/tests/kitchen.spec.ts`, `packages/ui/src/messages/en.json`.

## Tasks & Acceptance

**Execution:**
- [x] Proto and core: error code, keys, rules, seeding, gate, unit tests.
- [x] API: `test-kot` route and test.
- [x] Hub UI, messages and Playwright.

**Acceptance Criteria:**
- Given a Free branch, when Settings → Kitchen opens, then `tier-lock` shows and no field is editable, and a kitchen `settings.set` is `TIER_LOCKED`.
- Given the `kitchen` flag, when a Manager saves valid changes, then they apply with `base_rev` and the audit shows before/after.
- Given a fixture intent registered `New` and another `ExistingWork`, when both reach a Free branch, then only the first is refused.

## Verification

**Commands:**
- `cargo test --workspace --locked`; `-p arena-hub` and `-p arena-api` with `--features dev-fixtures`; clippy with dev-fixtures.
- `pnpm --filter @arena/hub check` and `test:shell` with a unique `ARENA_PW_PORT`; every `pnpm run check:*`; `pnpm run test:scripts`.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter + Verification Gap on Sonnet; risk-based: config/UI story). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH: a failed save reloads and wipes the edits, and the PIN re-prompt then approves nothing | high | P | `load(quiet, keepEdits)`: on any failure only the revs refresh |
| 2 | BH: Enter in the IP field submits the form | medium | P | Enter adds the IP |
| 3 | BH: "Another printer" saved with no printer | medium | P (UI: Save disabled) / D (server rule) | KOT creation arrives in a later story, which must fall back |
| 4 | BH: the gate ran before dedupe, so a retry of an applied write returned `TIER_LOCKED` | medium | P | gate moved after `ttl_dedupe`; replay test added |
| 5 | BH: only `settings.set` is gated; `ExistingWork` unused by the real registry | low | R | the only writer of these rows; later stories register more |
| 6 | BH: refusal text lost when a lapse flips to the lock panel | low | R | the lock panel is the answer |
| 7 | BH: Test print maps `TIER_LOCKED` to the generic failure | low | P | maps to the locked message |
| 8 | BH: the test print reads state outside the writer lock | low | R | advisory page, no write |
| 9 | BH: `pub mod licence` lost its `pub` | high | P | restored; mine was a bad replace |
| 10 | BH: nothing writes `licence.flags` yet | info | R | per spec, 2.12 writes it |
| 11 | VG: Counter PIN approval flow untested | high | P | Playwright: PIN approval carries one ticket |
| 12 | VG: AC says fields read-only for Counter; spec keeps them editable with a PIN on Save | medium | R | follows the screen-lock and branch precedent; logged |
| 13 | VG: "unlocks without restart" untested | medium | D | reload on `licence.flags` is wired; needs the 2.12 event path |
| 14 | VG: audit before/after, existing-branch backfill, kiosk queue mapping untested | low | D | generic `settings.set` and seeding paths |
| 15 | VG: STALE in the UI untested | medium | P | Playwright stale case keeps the edit |
