---
title: 'Story 5.15: Receipt and notification settings'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '974422f'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-1-kitchen-settings-at-the-hub-locked-on-free.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-27-printer-detection-and-test-print-at-the-hub.md'
---

<intent-contract>

## Intent

**Problem:** A manager cannot set the receipt text, the default receipt choice or which service messages the branch sends, and the Hub has no place that shows how a receipt will wrap on 58 or 80 mm paper.

**Approach:**

- Settings → Receipts (`/settings/receipt`) shows the Story 2.27 printer (with a Find printer link and Test print), paper width, header (branch name, address, phone, GSTIN added automatically for GST branches), footer (up to 3 lines), the default receipt choice, and a live text preview at the chosen width.
- Settings → Notifications (`/settings/notifications`) lists WhatsApp receipt, credit reminder, low balance and session ending, each with on/off and a preview of its template. Free shows `tier-lock` "Available on Basic" on each message. No provider key is asked for anywhere.
- All values are stored through the existing `settings.set`; no new intent kind and no Hub migration.

## Boundaries & Constraints

**Always:**

- **Keys.** `receipt.footer` (up to 3 lines), `receipt.address` (up to 3 lines), `receipt.phone` (1 line), each line at most 96 characters, `\n`-separated, no control characters, empty allowed. `receipt.default` in {`print`, `none`}, default `print`. `notify.whatsapp_receipt`, `notify.credit_reminder`, `notify.low_balance`, `notify.session_ending`: `true`/`false`, default `false` (a branch opts in). The branch name stays `branch.name`; the paper width stays `printer.paper_mm`. Validation failures are `VALIDATION_FAILED` with `field` for the new keys.
- **Seeding.** All new keys are in `DEFAULT_SETTINGS`, so a new branch gets them at bootstrap and an existing one from `branch.seed_defaults`.
- **Tier.** `arena_proto::notify_gate` registers the four `notify.*` keys of `settings.set` as `TierGate::New`. `guards::paid_tier` (straight after `guards::tier`, so after `ttl_dedupe`) refuses them with `TIER_LOCKED` while the effective tier `licence.tier` is not `basic` or `premium` (absent is Free). A lapse keeps the stored values and refuses writes. The receipt keys are open to every tier.
- **Hub UI.**
  - Receipts: the preview wraps each header and footer line at word boundaries at 32 (58 mm) or 48 (80 mm) characters, cutting a word longer than the line, and updates as the manager types or changes the paper width. A text block over its line limit shows a message and disables Save. Save sends one `settings.set` per changed key with `base_rev` (`STALE` keeps the edit and shows the value now on the Hub); a Counter's Save is `PERMISSION_DENIED` inline. Test print sends the page at the saved paper width, so it is disabled while the width has unsaved changes.
  - Notifications: a paid branch gets a switch per message that saves at once; the switch always shows the Hub's value after the answer. Each message shows its template with sample values.
  - Both pages reload when `settings.changed` names one of their keys; the Settings nav gains "Notifications" after "Receipts". Strings in `en.json`; targets at least 44 px; axe passes.

**Never:**

- A provider key, API key or sender id field.
- Sending a message, the receipt layouts (5.16), the print choice at settle (5.17) or the drawer (5.18).
- A new intent kind, table or Hub migration.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open Receipts | any tier | printer, paper, header, footer, default, preview | load fails: Retry |
| Long line | footer line longer than 32 on 58 mm | preview wraps at a word boundary | N/A |
| Too many lines | footer with 4 lines | message, Save disabled | server: `VALIDATION_FAILED` |
| GST branch | `branch.tax.gst` with a GSTIN | `GSTIN <no>` under the header in the preview | N/A |
| Free, Notifications | `licence.tier` free or absent | `tier-lock` per message, no switch | write: `TIER_LOCKED` |
| Basic, switch on | `notify.low_balance` true | applied with `base_rev` | `STALE` / `TIER_LOCKED` shown, switch resets |
| Lapse | tier back to free | writes refused, values kept | `TIER_LOCKED` |
| Counter saves | no `settings.edit` | refusal inline | `PERMISSION_DENIED` |

</intent-contract>

## Code Map

- `crates/arena-proto/src/receipt.rs` -- NEW: keys, defaults, `valid_receipt_lines`, `notify_gate`, `tier_is_paid`; `lib.rs` exports.
- `crates/arena-core/src/{settings_defaults.rs,entitlement.rs,apply/guards.rs,apply/mod.rs}` -- `SettingRule::ReceiptLines`, defaults, keyed fields, `is_paid`, `paid_tier`.
- `apps/hub/src/lib/receipt.ts` -- NEW: keys, `wrapLine`, `wrapText`, `previewReceipt`. `lib/printer.ts` -- `PRINTER_ERROR_MESSAGES`.
- `apps/hub/src/routes/settings/{receipt,notifications}/+page.svelte`, `settings/+layout.svelte`, `packages/ui/src/messages/en.json`.
- Tests: `settings_defaults.rs`, `proto/receipt.rs`, `apply/tests.rs` (`the_notification_keys_follow_the_effective_tier`), `tests/receipt.unit.spec.ts`, `tests/receipts.spec.ts`; `auth`, `lock`, `settings` and `elevation` specs gain the new rows.

## Tasks & Acceptance

**Execution:**

- [x] `crates/arena-proto` + `arena-core` -- keys, rules, defaults, tier gate -- settings
- [x] Hub lib -- wrapping and preview, error messages -- preview
- [x] Receipts page -- printer, paper, header, footer, default, preview, test print -- UI
- [x] Notifications page and nav -- locked on Free, switches on paid -- UI
- [x] Tests -- Rust matrix, wrap unit tests, Playwright; existing specs updated for the new rows -- proof
- [x] Verification gate

**Acceptance Criteria:**

- Given Settings → Receipts, then it shows the chosen printer with Find printer and Test print, paper 58 or 80, the header, up to 3 footer lines, the default receipt choice and a live preview at the chosen width.
- Given a line longer than the paper width, then the preview wraps it at word boundaries as it will print.
- Given Settings → Notifications, then the four messages each have on/off and a template preview, the page says "Messages are sent by Arena 360 — no provider keys needed" and asks for no keys.
- Given a Free branch, then each message shows `tier-lock` "Available on Basic" and a `notify.*` write is `TIER_LOCKED`.
- Given valid changes, then `settings.set` stores them with `base_rev` and the audit records them.

## Design Notes

- **Find printer** is a link to Settings → Printer rather than a second copy of the finder; Test print runs from the Receipts page.
- **No PIN on Save.** The Receipts page keeps its Story 1.11 behaviour: a user without `settings.edit` is refused inline.
- **Tier source.** The Basic flag list has no notifications flag; the gate reads `licence.tier`, which every applied licence already writes, so no licence format changes.
- **Message defaults are off.** A message costs the café money and needs the player's consent, so a branch turns each on; the sending stories (5.19, 8.28 to 8.31) read these keys.

## Verification

**Commands:**

- `cargo test` on `arena-proto`, `arena-core`, `arena-store`, `arena-tasks`, `arena-api`, `arena-hub`, `arena-replay`; `cargo clippy -D warnings` on the crates touched; `cargo fmt --check`
- `pnpm --filter @arena/hub check`; Playwright `test:shell` with a unique `ARENA_PW_PORT`; `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `check:generated`, `format:check`
- Pre-existing, unrelated: `arena-tasks` `heartbeat::trusted_time_*` and `printer::lan::*stops_reading*`, `arena-hub` `android_fleet` (`SHIFT_NOT_OPEN`), `check:tokens-used` (unknown `--ad-*` tokens), and `arena-e2e` and `arena-kiosk-svc` tests that do not build on Windows. `players.spec.ts` "Ctrl+K from another screen" is flaky on the phone project.

## Spec Change Log

## Review Triage Log

- Code review (2026-10-28): no blocking findings. Rust tests, clippy, fmt, svelte-check, string/layer/protocol/hex/generated checks, prettier, and the receipt unit, receipts and settings Playwright specs pass. Non-blocking: the client counts line length in UTF-16 units, the Hub in code points, so an astral character is counted stricter in the UI only.
