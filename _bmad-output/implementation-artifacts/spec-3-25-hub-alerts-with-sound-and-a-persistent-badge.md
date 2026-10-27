---
title: 'Story 3.25: Hub Alerts with Sound and a Persistent Badge'
type: 'feature'
created: '2026-10-27'
status: 'in-review'
baseline_revision: 'c1d703047acc969ceec053247675e81dd96a38ca'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-23-session-timer-engine-with-time-warnings.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Nothing on the Hub can interrupt staff. A time-critical event on a PC (3.26 time-up, later kiosk and printer events) has no tile badge, no header count, no sound, and nowhere to be acknowledged. The header's two condition badges (licence unverified, PC clock wrong) sit in a separate slot.

**Approach:** A Hub-held alert service: an `alerts` table written only through `apply()` by two intents, `alert.raise` (system; any feature supplies a kind and optional device; a `fixture` kind exists for tests) and `alert.ack` (staff). Open alerts ride the snapshot, so they survive idle lock and reload. The Hub UI shows an `alert-badge` tag on the tile, a count on its zone row (also when collapsed) and in a header alert list; plays a bundled sound at once and every 30 s until every alert is acknowledged; opens acknowledge by opening the tile sheet or the list row. The licence-unverified and clock-wrong conditions become branch-level rows in the same list (derived from the snapshot, no sound, cleared when the condition clears) and the status-badge slot goes.

## Boundaries & Constraints

**Always:**
- Raising an alert for a device that already has an open alert of the same kind (or a second open branch alert of that kind) is idempotent: `alert.raise` is `STALE` and writes nothing. `alert.ack` of an unknown id is `NOT_FOUND` on `alert_id`; of an already acknowledged alert `STALE` (the UI ignores it). Acknowledging keeps the row (`acked_at`, `acked_by` = staff id); the change log and audit viewer carry who and when.
- `alert.ack` requires `session.start` (counter staff). `alert.raise` is `Requirement::System`, so the UI endpoint cannot raise one. Neither takes `base_rev`.
- Kinds: `fixture`, `time_up` (3.26 raises it). Each has a catalogue label and a tile tag (`fixture` -> `new`, `time_up` -> `time-up`). A device alert needs a live device (`NOT_FOUND` on `device_id`).
- The tile sheet opening acknowledges every open alert of that device; the list row acknowledges that one and opens the sheet. Sound repeats from the Hub UI while any open device alert exists and is not played for branch-level conditions; a screen reader hears each newly raised alert once in a polite live region; the tile, zone and header carry text counts or tags so sound is never the only cue.
- Alerts reach every open Hub view within 2 s through the existing `devices.changed` event (the UI reloads its snapshot). Sounds are bundled files, never fetched. Money and times are unchanged; no code comments beyond doc comments in the existing style (user rule).

**Never:**
- Raising `time_up` or any real feature alert (3.26, 3.36, Epic 5 and 6 alerts), owner or WhatsApp alerts (Epic 8), changing the Android service alert in the header, changing the timer engine.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Raise | `alert.raise{kind: fixture, device_id}` | open row, snapshot lists it, tile tag, header +1, sound | No error |
| Raise twice | same kind, device, still open | nothing written | `STALE` |
| Raise after ack | previous one acknowledged | new open row | No error |
| Unknown device / kind | missing device; unknown kind | nothing written | `NOT_FOUND` / `VALIDATION_FAILED` |
| Ack | open alert, signed-in staff | `acked_at`, `acked_by` set; badge and sound clear | No error |
| Ack again / unknown | acked id / missing id | nothing written | `STALE` / `NOT_FOUND` |
| Collapsed zone | device in it raises | zone row shows its alert count, tile hidden | No error |
| Lock or reload | open alerts | listed again after unlock | No error |
| Branch conditions | licence unverified / clock wrong | list rows, no sound; gone when the condition clears | No error |

</intent-contract>

## Code Map

- `crates/arena-proto`: `intent.rs` (`IntentKind::AlertRaise`/`AlertAck`, `IntentPayload`, category `Session`, requirement, reasons, system list in the test), new `alert.rs` (`AlertKind`, `AlertRaise`, `AlertAck`, `SnapshotAlert`), `snapshot.rs` (`HubSnapshot.alerts`), `lib.rs` exports; generated types via `pnpm run gen:types`.
- `crates/arena-store`: `migrations/0036_alerts.sql`, `migrations.rs`, new `alerts.rs` (insert, find, open list, ack), `branch.rs` (`Table::Alerts`, `RowKey::Alerts`), `lib.rs` exports and `Store::alerts`.
- `crates/arena-core`: new `apply/alert.rs` (`load`/`validate`/`mutate` for both intents), `apply/{mod,validate,mutate,guards}.rs` (new `Target::Alert`), `snapshot.rs` (`alerts` in `read_snapshot`), `lib.rs`.
- `crates/arena-api/src/ui_events.rs` (`alerts` rows send `devices.changed`); `crates/arena-replay/src/lib.rs` and `crates/arena-core/tests/proptest_apply.rs` (alert ops and replay equality); other crates that match on `IntentKind::ALL` exhaustively (`arena-kiosk-core/src/queue.rs`, `arena-cloud/src/api/error.rs` if affected).
- Hub: `packages/ui/src/components/FloorTile.svelte` (`alertTag` prop -> `AlertBadge tag`), `AlertBadge.svelte`; `apps/hub/src/routes/+layout.svelte` (remove status slot, header alert button and list, sound loop), new `apps/hub/src/lib/alerts.svelte.ts` (derive list/counts, ack job, sound loop) and `AlertList.svelte`; `apps/hub/src/routes/floor/+page.svelte` (tile tag, zone count, ack on sheet open, `?device=` deep link already supported); `lib/audit/labels.ts`; `lib/snapshot.ts` (default `alerts: []`); bundled sound under `apps/hub/static/sounds/`; `packages/ui/src/messages/en.json`; `apps/hub/tests/alerts.spec.ts` modeled on `session-timer.spec.ts` with `floor-fixture.ts`.
- Read-only: header `Clock`, Android `service-alert` (`alertCount` in `service.svelte.ts`) stays as is.

## Tasks & Acceptance

**Execution:**
- [x] Protocol types, intent kinds and payloads, migration, store module and `Table::Alerts`, generated types
- [x] `apply/alert.rs` for raise and ack, wired through load/validate/mutate/guards; snapshot carries open alerts
- [x] Apply tests for every matrix row, proptest op, replay coverage, `devices.changed` on alert rows
- [x] Hub: alerts store with sound loop and live-region announcements, header badge and list, tile tag, zone count, ack on sheet open, branch-level rows replace the status slot, catalogue keys, audit labels
- [x] Playwright spec on desktop and phone

**Acceptance Criteria:**
- Given an alert raised for a device, when the snapshot reaches the Hub, then the tile shows the `alert-badge` tag, the header count increments, and the bundled sound plays once and again every 30 s until the alert is acknowledged.
- Given an alert on a tile, when staff open its sheet or its row in the header list, then it is acknowledged (staff and time in the audit), the sound stops and the badge clears.
- Given the header badge opened, then each open alert lists device, kind and age and choosing one opens that tile's sheet.
- Given licence unverified or a wrong PC clock, then each shows as a branch-level row in the list with no sound, clears with its condition, and the separate status-badge slot is gone.
- Given a collapsed zone, when a device in it raises or clears an alert, then the zone row shows its alert count without expanding.
- Given idle lock or a reload, then open alerts persist and reappear with badges after unlock.
- Given a screen reader, when an alert is raised, then it is announced once in a polite live region.

## Spec Change Log

## Review Triage Log

## Design Notes

Alerts are rows so "held by the Hub service" is true across restarts and the change log, replay and audit come free. The sound loop is a UI concern (one 30 s timer while any open device alert exists); the header list merges `snapshot.alerts` with two derived branch rows. `alert.raise` carries its own idempotence (`STALE`) so a repeating feature (a timer tick) can raise freely.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test alerts floor shell session-timer` in `apps/hub`.

## Auto Run Result

Status: in-review
Verified: fmt, clippy, arena-proto/store/core/replay tests, hub svelte-check, string/protocol/format checks, `alerts` and `session-timer` Playwright specs (desktop and phone) pass. Unrelated timing failures as in 3.23/3.24 (arena-cloud, arena-tasks); a plan proptest (3.21) can fail at 300 cases with `[Bootstrap, Plan, Plan]`.
Decisions: the `alert.raise` payload field is `alert_kind` (`kind` collides with the payload tag); a branch-level row is acknowledged-by-click with no sheet and no sound; the sound loop runs whenever the snapshot has open device alerts.
Left for later: real raisers (`time_up` in 3.26 and later stories); sound on a real Windows webview is untested.
