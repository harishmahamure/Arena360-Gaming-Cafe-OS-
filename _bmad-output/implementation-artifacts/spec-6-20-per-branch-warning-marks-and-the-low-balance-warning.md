---
title: 'Story 6.20: Per-branch warning marks and the low-balance warning'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '4f7c6fc'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-19-time-warnings-as-hud-toasts-a-chip-warning-and-a-taskbar-fla.md'
warnings: []
deferred:
  - 'Extend as a button on the low-balance toast (and on the time warning): Story 6.21. The key caps show now and Ctrl+Shift+E opens the HUD panel.'
  - 'The low-balance figure is read from the Hub tick once a second; with the Hub unreachable it holds its last value (the kiosk does not count the wallet down on its own clock).'
---

<intent-contract>

## Intent

**Problem:** The warning marks are fixed at 10, 5 and 1 and nothing tells a wallet-paid player that their balance is running out.

**Approach:** Two branch settings, `billing.warn_marks` ("10,5,1": one to three distinct minutes, 1–30) and `billing.low_balance_minor` (default 5000 minor units, 0 is off), edited on Settings → Branch → Billing and pushed to kiosks like the grace settings. The Hub timer engine and the kiosk service both read the marks. The Hub sends each wallet session's remaining wallet (`wallet_left_minor`) on its tick; the kiosk service marks it low when it is at or below the amount and the HUD raises a persistent warning toast once per crossing.

## Boundaries & Constraints

**Always:** marks reach running sessions from the next tick and never repeat or fire late; a duplicate, out-of-range or malformed mark is a field error naming `billing.warn_marks`; the time-chip warning stays at 10 min or less whatever the marks; the low-balance toast is solid, never takes focus, stays until dismissed, and says the amount at the crossing.

**Never:** a new frame, intent or permission (both ride `settings.set` and the existing `session.tick`); a low-balance warning for guests or for sessions not paid from the wallet.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Save marks | "2, 20 ,7" | stored `20,7,2`; `settings.changed` reaches kiosks |
| Bad marks | "", "1,2,3,4", "0", "31", "5,5" | field error, nothing written |
| Marks change mid-session | 12 min left, new 15,10,2 | 15 is not raised late; 10 and 2 follow |
| Low balance set | `0` | warning off |
| Wallet falls to the amount | wallet_left ≤ amount | toast "Balance running low — ₹40.00 left.", "Top up at the counter.", Ctrl+Shift+E caps, once |
| Keeps falling | next ticks | same toast, same words |
| Dismissed | next ticks | not raised again |
| Top-up above the amount, falls again | left > amount then ≤ | raised again |
| Lock, end of session | | toast removed |
| Pack, pass, postpaid, guest | no `wallet_left_minor` | no low-balance warning |

</intent-contract>

## Code Map

- `crates/arena-proto/src/billing.rs` -- the two keys, defaults, `parse_warn_marks`, `warn_marks_from`, `low_balance_from`; `KIOSK_BILLING_KEYS` now four keys (kiosks read and are pushed them).
- `crates/arena-core/src/settings_defaults.rs`, `apply/settings_set.rs` -- defaults (so `branch.seed_defaults` backfills upgraded branches), rules, `SettingRule::WarnMarks`, specific field errors.
- `crates/arena-core/src/session_timer.rs`, `snapshot.rs`, `crates/arena-tasks/src/session_timer.rs` -- `TimerEngine::set_marks`, `SnapshotReader::warn_marks`, the task resyncs marks on start, every 30 ticks and on a marks write.
- `crates/arena-proto/src/session.rs`, `crates/arena-core/src/session_screen.rs` -- `SessionScreen.wallet_left_minor` (free wallet plus the unused share of a wallet session's reserve).
- `crates/arena-kiosk-svc/src/{operating,session,pc,app}.rs`, `crates/arena-kiosk-core/src/ipc.rs` -- cached marks and amount, marks on the kiosk clock, `low_balance_minor` on `PcSession`.
- `apps/kiosk/src/lib/{hud.ts,Hud.svelte,bridge.ts}`, `packages/ui/src/messages/en.json` -- `followLowBalance`, `LOW_BALANCE_KEY`, `player.low_balance.*`.
- `apps/hub/src/lib/billing.ts`, `apps/hub/src/routes/settings/branch/billing/+page.svelte` -- "Warn players at" and "Low balance warning at".

## Tasks & Acceptance

- [x] Settings `billing.warn_marks` and `billing.low_balance_minor` with rules, defaults, field errors and kiosk scope
- [x] Hub timer engine uses the branch marks from the next tick, including running sessions
- [x] Kiosk service uses the cached marks on its monotonic clock (Hub unreachable)
- [x] Hub sends the wallet left on the session tick for wallet-paid sessions
- [x] Kiosk raises the persistent low-balance toast once per crossing, again after a top-up and a new fall
- [x] Settings → Branch → Billing fields with validation (duplicate and out-of-range marks name their error)
- [x] Chip warning unchanged at 10 min or less; the fullscreen taskbar flash covers any mark of 5 min or less
- [x] Tests: Rust (proto, core, tasks, kiosk-svc), Playwright (`billing.spec.ts`, `hud.spec.ts`), proptest generator

## Spec Change Log

- "Remaining wallet balance after what the session has used" is read as the free wallet plus the unused part of the session's reserved share (even use over the reserve), since a wallet session is debited at settle, not as time passes.

## Verification

- `cargo test -p arena-proto -p arena-api -p arena-core -p arena-kiosk-core` -- pass; `cargo test -p arena-tasks --lib` -- new tests pass; `heartbeat::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::a_printer_that_stops_reading_times_out` fail on this machine, in files this story does not touch
- `cargo test -p arena-kiosk-svc --lib` -- new tests pass; the same 14 lockdown/pipe/DPAPI/config tests as before fail on this machine (Windows privileges)
- `cargo clippy -D warnings` clean on every touched crate (`--lib --bins` for kiosk-svc; `tests/discovery.rs` has existing unused-import errors)
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check` -- 0 errors; `billing.spec.ts` (74) and `hud.spec.ts` (39) pass
- `pnpm check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` -- pass; `check:generated` reports `SessionScreen.ts` only because it is uncommitted

Not run: a real Hub-to-kiosk round trip, the Tauri shell.

## Code Review (2026-10-29)

Layers: Blind Hunter, Edge Case Hunter, Verification Gap, Acceptance Auditor. Outcome: approved.

- **Fixed:** `settings.set` kept `billing.warn_marks` as typed ("2, 20 ,7"), so only the Hub form stored `20,7,2`. `settings_set::mutate` now stores the canonical longest-first form for any writer; test added in `warning_marks_and_the_low_balance_amount_are_seeded_and_validated`.
- **Deferred (not blocking):** the kiosk-svc reads its operating settings from the cache on every tick (perf); a HUD that restarts while the balance is already low raises no toast until the next crossing (same rule as the 6.19 time warning); no unit test for `screen_message` mapping `low_balance_minor` or for the `touches` settings arm; `Top up at the counter.` is fixed text for every venue.
- **Dismissed:** Hub form leniency on "10,,5" (normalised before save, the server still rejects raw malformed input); currency exponent loading late (the form follows `branchLocale`); `wallet_left_minor` with no `ends_at` (wallet sessions are always timed).
