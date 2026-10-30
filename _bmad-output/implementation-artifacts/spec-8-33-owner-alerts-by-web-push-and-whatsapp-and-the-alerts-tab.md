---
title: "Story 8.33: Owner alerts by web push and WhatsApp, and the Alerts tab"
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: '74a399aeb8904f74fa54675b399f53d46251d17d'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The owner only learns of a problem by opening the Owner app. Nothing in the cloud can raise an owner alert, keep it, push it to a phone or send it on WhatsApp, and the Alerts tab is an empty placeholder.

**Approach:** The cloud gains `owner_alerts`, `push_subscriptions` and a VAPID web-push sender. A raised alert is stored, pushed to each of the owner's subscriptions and sent on WhatsApp through the messaging adapter, each once per alert id. The Owner app's Alerts tab lists alerts and holds the notification settings. This story's source of alerts is a dev-only fixture route; Stories 8.34 and 8.35 add the real sources.

## Boundaries & Constraints

**Always:**
- `arena-proto` holds the `OwnerAlertKind` enum (`test`, `cash_variance`, `low_stock`, `credit_overdue`, `large_refund`, `large_discount`, `branch_offline`) with each kind's required display parameters and plain-text line (staff register, English); a later kind is one enum arm. Alerts carry player ids at most, never a phone, name or email.
- Control migration 23 adds `push_subscriptions` (id, owner_user_id, endpoint UNIQUE, p256dh, auth, user_agent, created_at, last_success_at), `owner_alerts` (id ULID, tenant_id, branch_id, kind, params JSON, created_at), `owner_alert_deliveries` (alert_id, target, PK together, claimed before sending, removed with the alert), `owner_alert_settings` (owner_id, whatsapp_enabled default 1) and version 1 of the `owner_alert` message template (params `branch`, `text`). Alerts older than 90 days are deleted on each raise and never listed.
- Raising is idempotent by alert id: a repeat adds no row and sends only what is not yet claimed. Push claims `(alert_id, subscription_id)`, WhatsApp is the send ledger row `outbox_id = <alert_id>.wa` (kind `owner_alert`, purpose `owner_alert`, consent version `owner`), sent to the tenant owner's phone only when `whatsapp_enabled`. The ledger send is the code of `POST /v1/notify` moved into a shared function; that route behaves as before.
- Push is a `WebPush` sender: `Fake` (recorded, per-endpoint outcome; dev and tests) or `Http` (VAPID JWT ES256 from `ARENA_VAPID_PRIVATE_KEY` PKCS8 base64url and `ARENA_VAPID_SUBJECT`, payload encrypted per RFC 8291 `aes128gcm`, `TTL` 86400). A 404/410 deletes the subscription; any other failure is logged by alert id and subscription id, claimed and not retried; success sets `last_success_at`. Without a VAPID key the push routes answer `UNAVAILABLE` and alerts still go to the list and WhatsApp.
- Owner routes (owner session, `RequireSetupComplete`): `GET /v1/alerts/settings` `{vapid_public_key, whatsapp, subscriptions}`; `PATCH /v1/alerts/settings {whatsapp}`; `POST /v1/alerts/push {endpoint, keys{p256dh,auth}}` (https endpoint, at most 10 per owner, upsert by endpoint, `User-Agent` stored); `POST /v1/alerts/push/remove {endpoint}`; `GET /v1/alerts?before=&limit=` `{alerts, next_before}` newest first for the owner's tenant, each `{id, branch_id, branch_name, kind, text, url, created_at}`. Dev only: `POST /v1/dev/owner-alerts {id?, branch_id, kind, params}`.
- Push payload `{title: branch name, body: text, url, tag: alert id}`. `url` is a path: `/#today-<branch>` for `branch_offline`, `/reports?tab=cash|stock|credit&branch=<id>` for `cash_variance|low_stock|credit_overdue`, `/reports?branch=<id>` for refunds and discounts, `/alerts` for `test`. Reports opens on `?tab=` and `?branch=` when given.
- Owner app: the Alerts section uses `scope="all"`. Notification settings has a push switch and a WhatsApp switch (default on). Turning push on requests permission and subscribes with `vapid_public_key`; on iOS not running as an installed Home Screen app it shows "Add Arena 360 to your Home Screen to get alerts here." and never asks; a browser without push says so. Denied permission shows how to allow it and leaves the switch off. The list refreshes every 30 s while visible, on focus and on a push message; each new alert is announced once in a polite live region. Empty reads "All quiet."
- The service worker shows a notification on `push` and on `notificationclick` focuses an open window and navigates it to `url`, else opens one.

**Never:** Hub-raised alert sources, the Hub's thresholds and the offline detector (8.34, 8.35); the quiet-hours or digest settings; SMS as an alert channel (the adapter's own WhatsApp-rejected fallback is unchanged); logging an endpoint, key, phone or alert text; adding a Free-tier alert path.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Raise | Fixture alert for a branch; owner has 2 subscriptions, WhatsApp on | One row, 2 pushes, 1 WhatsApp | No error expected |
| Repeat | Same alert id again | Still one row; no second push or WhatsApp | No error expected |
| Gone | Push service answers 410 | That subscription is deleted; the other still gets its push | No error to the raise |
| WhatsApp off | `whatsapp: false` | Push and list only; no ledger row | No error expected |
| Bad fixture | Unknown kind or missing parameter | Nothing written | `VALIDATION_FAILED` |
| Subscribe | Valid subscription twice | One row (upsert) | `VALIDATION_FAILED` for a non-https endpoint or an 11th subscription |
| Other tenant | Alert of another café | Not listed | `NOT_FOUND` for a dev raise on an unknown branch |
| iOS in a tab | Push switch tapped | The Home Screen line; no permission prompt | None |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0023_owner_alerts.sql`, `crates/arena-store/src/control.rs` (`CONTROL_MIGRATIONS`, `pub use`), new `crates/arena-store/src/control/owner_alerts.rs` -- tables and `ControlDb` methods; copy the shape of `control/send_ledger.rs`; `tests/control.rs` and the migration-count tests move to 23.
- `crates/arena-proto/src/owner_alert.rs` (new, `lib.rs` re-export), `src/notify.rs` -- `OwnerAlertKind`, request/response types with `ts(export)`; `MESSAGE_KINDS` stays the Hub-postable list, so `owner_alert` is not added to it.
- `crates/arena-cloud/src/push.rs` (new) -- `WebPush` Fake/Http, VAPID, RFC 8291 encryption; `config.rs` and `lib.rs` build it beside `Notifier` (`AuthState.push: OnceLock<WebPush>`); `--dev` forces the fake.
- `crates/arena-cloud/src/api/notify.rs` -- extract the ledger send into `pub(crate) deliver(st, branch, &NotifyRequest)`; `src/owner_alerts.rs` (new) `raise()`; `src/api/alerts.rs` (new) owner routes, mounted in `api/mod.rs`; `api/dev.rs` the fixture route. `api/owner_read.rs` is the pattern for owner-session routes.
- `apps/owner/src/routes/alerts/+page.svelte`, new `lib/AlertsBoard.svelte`, `lib/push.ts`, `lib/alerts.svelte.ts`; `src/service-worker.ts` push and click handlers; `lib/ReportsBoard.svelte` (`?tab=`, `?branch=`); `lib/TodayCard.svelte` already has `id="today-{branch.id}"`.
- `packages/ui/src/messages/en.json` (`owner.alerts.*`), `packages/ui/src/generated/*` (regenerate; `pnpm check:generated`).
- `apps/owner/tests/stub.ts`, new `apps/owner/tests/alerts.spec.ts`; cloud `crates/arena-cloud/tests/owner_alerts.rs` (pattern: `tests/notify.rs`, `tests/owner_read.rs`).

## Tasks & Acceptance

**Execution:**
- `crates/arena-store/control-migrations/0023_owner_alerts.sql` and `control/owner_alerts.rs` -- tables, template v1 and methods (insert-if-absent, list by tenant with cursor, prune, claim delivery, subscription upsert/delete/count/list, settings get/set) -- the durable once-only guard
- `crates/arena-proto/src/owner_alert.rs` -- kind enum, parameter checks, text and url per kind, API types, unit tests of every kind's line -- one path for later kinds
- `crates/arena-cloud/src/push.rs` -- sender, VAPID, encryption with a unit test against the RFC 8291 §5 vector (RustCrypto `p256`, `hkdf`, `aes-gcm`, pinned `=` as direct dependencies) -- real push without a service dependency
- `crates/arena-cloud/src/api/notify.rs`, `owner_alerts.rs`, `api/alerts.rs`, `api/dev.rs`, `api/mod.rs`, `lib.rs`, `config.rs` -- shared send, raise, routes, wiring, env -- the cloud half
- `crates/arena-cloud/tests/owner_alerts.rs` -- the I/O matrix over the router with the fakes -- proves exactly-once, 410, tenant scope
- `apps/owner/...` and `packages/ui/...` -- Alerts tab, settings, service worker, deep links, strings, regenerated types -- the owner half
- `apps/owner/tests/alerts.spec.ts` -- list, "All quiet.", live region once per alert, iOS line, permission flow with a stubbed `PushManager`, settings, axe check -- the surface

**Acceptance Criteria:**
- Given an owner with a paid branch, when a fixture alert is raised for it, then the Alerts tab lists it newest first with branch, kind line and time, and announces it once.
- Given two subscriptions and WhatsApp on, when the alert is raised twice with one id, then each subscription got one push and the owner's phone one WhatsApp message.
- Given a 404/410 from the push service, when a push is sent, then that subscription is gone.
- Given a delivered push, when it is tapped, then the app opens at the alert's branch card or report.
- Given WhatsApp turned off in Notification settings, when an alert is raised, then only push and the list are used.
- Given no alerts, when the tab opens, then it reads "All quiet."
- Given the typecheck, lint, Rust and Playwright suites and `pnpm check:generated`, `check:strings`, `check:pins`, when run, then they pass.

## Spec Change Log

## Review Triage Log

## Design Notes

The WhatsApp send reuses the 8.28 ledger rather than a second guard, so "once per alert id" is the ledger's `outbox_id` uniqueness; the push guard is the same idea per subscription. Raising claims before sending and never retries a push, matching the ledger's "never send twice" rule. Params in the template are `branch` and the rendered `text`, so a new kind needs no new template.

## Verification

**Commands:**
- `cargo test -p arena-store -p arena-proto -p arena-cloud` -- expected: all pass
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` -- expected: clean
- `pnpm check:generated && pnpm check:strings && pnpm check:pins` -- expected: ok
- `pnpm --filter @arena/owner check && pnpm --filter @arena/owner test` -- expected: pass
