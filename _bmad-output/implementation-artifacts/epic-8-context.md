# Epic 8 Context: Owner sees and controls branches remotely

## Goal

The café owner sees every branch and changes it from a phone or laptop, without being on the LAN. The Hub uploads change-log batches, a branch summary every cadence and a daily rollup; the cloud never holds a queryable copy of branch rows. The Owner PWA reads those files through the owner read API, sends commands through the Epic 2 mailbox (fan-out, step-up MFA, epoch fencing), gets alerts, and can open a Premium live view through a tunnel.

## Rules that shape every story

- Free uploads nothing and its Owner app shows only Subscription, pairing (Branches) and Support; Today, Reports, Alerts and Manage show a `tier-lock` "Available on Basic".
- Until Epic 10 the Subscription screen is read-only ("Plan changes are available soon") and no `tier-lock` shows an "Upgrade" link; until Epic 11 Support shows the support email and "Help centre — available soon".
- Cadence: `upload_interval_s` = `summary_interval_s`: Basic 900 s, Premium 300 s, floor 30 s.
- The Owner app shows only two command rejection reasons: "Device/Hub revoked", and "Couldn't apply at branch" with Retry.
- Paid tiers before Epic 10 come from the Story 2.8 operator-issued comp entitlement.

## Owner app (UX)

- Web surface: light theme "Cool paper" and dark Cinema Electric, System default following `prefers-color-scheme` live, `theme-toggle` in Account.
- `owner-nav`: Today, Reports, Alerts, Manage, Account; phone 64 px bottom tab bar (icon + `hub-micro`, `border-strong` top edge), desktop 256 px left sidebar (icon + `hub-label`, active filled). Gutter 40 (phone 16), content up to about 1200 px.
- Every screen caches its last payload with `as_of` in IndexedDB and shows the age; offline shows cached screens and a "You're offline" status badge.
- Web screen-reader baseline: `header`, `nav`, `main`, one `h1`, labelled controls, polite live regions.

## Story 8.10 neighbours

- 8.11 adds the high-contrast theme; 8.12 hardens the Free limits; 8.13 fills Today; 8.14 handles setup mode; 8.22 queues commands offline.

## Story 8.14: The Owner app stays in setup mode until a branch goes live, then opens its dashboard

- **Branch list.** `GET /v1/branches` now carries `live_at` (`live` stays) and `setup {steps_done, steps_total, next_step}`; done counts every step that is not pending (done or skipped) and `next_step` is the wizard's current step, so a branch that is not live shows at most 9 of 10.
- **Setup beats the tier lock.** A branch with `live_at` null gets a setup card on Today (Free included) and "Available after <branch> goes live" on branch-scoped screens; the Free limits apply only once it is live.
- **Shared list.** `liveBranches()` and `setupBranches()` in the shell are the single source; `showable` (Today) is live and paid. The branch picker is not built here: it would have no consumer until Story 8.15, which adds it on `liveBranches()`.
- **Polling.** Today re-reads the branches every 60 s while visible and while some branch is in setup, and on Refresh/pull; a branch that turns live is fetched at once. With no live paid branch the Refresh bar and Needs attention are hidden.
- **Open dashboard.** The onboarding done screen and the wizard URL of a live branch now open the Owner app root (Today).
- **Standby.** Unchanged: the Standby item is already advisory in the Go live view.

## Story 8.15: Reports — revenue and cash reconciliation

- **Shifts in the range.** `OwnerRange` gains `shifts` (`business_day` + the rollup's `RollupShift`, oldest closing first) from the accepted days; the rollup already carries the figures the Cash list needs, so the cloud only passes them through.
- **Over threshold.** `RollupShift` gains optional `over_threshold`, set by the Hub's rollup fold from the branch threshold at build time (`RollupRows.variance_threshold`). It is additive, so no schema bump: older files lack it and the app never flags them.
- **Per-branch calls.** The app asks `summary` and `live` for each live paid branch and sums per currency in the browser, rather than using `all`, so setup branches never enter totals or "partial" and the shared `liveBranches()` list stays the one source. Free live branches get a tier lock beside the picker.
- **So far today.** The branch's live `business_day` row ("So far today", net of refunds, with its data age) is shown in the by-day list when its rollup does not exist yet, and that day is not a missing day. It is not added to the rollup totals, which are gross and split by type and method.
- **Totals.** Labelled "Revenue, days with data"; "Partial" shows when any other day has no rollup. Refunds, discounts and voids are negative lines.
- **Dates.** Today/week/month use the date in the first picked branch's time zone; weeks start on Monday; Custom is capped at 400 days and refuses an inverted range before any call.
- **Badge colours.** `StatusBadge` takes its background from the dark Hub tile, which fails contrast on the paper theme, so Reports sets `--status-badge-background` to the surface. The same flaw exists on Today's badges (not changed here).

## Story 8.16: Reports — stock

- **Stock on the range answer.** `OwnerRange` gains `stock` (`OwnerRangeStock`): levels and expiring lots from the newest accepted rollup of the range (`levels_as_of` names its day, null with no accepted day) and waste summed over every accepted day per product and reason, with names resolved from the newest rollup that lists the product (the id when none does). Built by `OwnerRangeStock::from_rollups` in `arena-proto`; the cloud only passes it through.
- **Category.** `RollupStockLevel` gains optional `category` (the Hub's `ThresholdRow` now carries it), additive like `over_threshold`: no schema bump, and files built before it match across branches by name alone with an empty category.
- **Two reads.** Waste comes from the picked range. Levels and expiring lots come from a separate `summary` call over the last 7 days (`LEVELS_DAYS`), made only while the Stock tab is open, so "Today" (no rollup yet) and past custom ranges still show the newest levels with their date.
- **Low stock.** Two lists: "Low right now" is the live summary's `low_stock` attention items (same line as Today); "Low stock" is every level with `low_qty` set and `qty <= low_qty` from the rollup, with its date.
- **Expiring.** Lots in the newest rollup with `expires_on` within 7 days of today (already-expired ones carry an "Expired" badge). The Hub's rollup lists lots inside the branch's own warning window (default 3 days), so a lot 4–7 days out shows only when that window is at least as wide.
- **All branches.** One section per currency; levels and waste are tables with a column per branch and a Total, rows matched by trimmed, case-insensitive name and category, "—" where a branch lacks the product. Waste cells read "qty · value" and value is summed only inside a currency.
- **Untracked.** A branch whose newest rollup lists no tracked products reads "No tracked products at this branch."; one with no rollup in the week reads "No stock figures yet."

## Story 8.17: Owner reports export to CSV and PDF and match the Hub's Z-reports exactly

- **Cloud CSV.** `GET /v1/branches/{id|all}/export?from&to&report=revenue|cash|stock` answers `text/csv` with a filename; it uses `branches_for` and `range_of`, so entitlement, tenant checks, the rate limit and the sums are exactly `summary`'s. `all` may carry `branches=<ids>` so the app exports only the live, paid branches it shows; an id outside the café narrows to nothing.
- **Format.** Header rows (Report, Range, Branches, Generated in UTC), a blank line, then one row per day per branch; amounts are major units by the currency's exponent with the currency code in a column; days without a rollup read "No figures"; Stock lists Level, Waste and Expiring rows. Text cells starting `=`, `+`, `-`, `@`, tab or return get a leading `'`.
- **PDF.** No server PDF: Export PDF shows the report tab and calls `window.print()`. Print media hides the header/nav and the controls, switches to the light tokens and shows `.print-header` (report, branches, range, generated time, data age).
- **Exports tab.** Lists Revenue, Cash reconciliation and Stock with CSV and PDF for the current range and pick.
- **NFR10.** `thirty_days_of_rollups_sum_to_the_thirty_z_reports` (arena-core) runs 30 business days (sales, undone sale, refunds, shorts, closes) and compares the summed rollups to the summed Z-reports. It does not go through MinIO.

## Story 8.18: The cloud accepts owner command kinds, checks them, and fans "all branches" out

- **Catalogue.** `arena_proto::OWNER_COMMAND_KINDS`: `rate.upsert`, `plan.upsert`, `rule.upsert`, `coupon.upsert`, `product.upsert`, `backup.now` (handled, Basic, not sensitive) and `hub.revoke`, `backup.restore` (reserved, sensitive, no handler, so `UNSUPPORTED`). Payloads are validated by parsing the matching intent payload struct; `base_rev` rides in the payload object. Later stories add their kinds to this list with their Hub handlers.
- **Post.** `POST /v1/commands {branch_id|"all", idempotency_key, kind, payload, base_rev?}` (owner session) answers `{commands, skipped}`. `all` writes a row per passing branch with `group_id` = the user's key and a ULID key derived from key + branch; skipped branches carry a code (`FEATURE_NOT_ENTITLED`, `BRANCH_UPDATE_REQUIRED`). When nothing passes, the first skip's error is returned instead.
- **Schema range.** The heartbeat carries `command_schema` (the newest command `schema_version` the Hub runs); the cloud stores it on `hubs.command_schema_max`. No value counts as 1. A branch below the kind's version is `BRANCH_UPDATE_REQUIRED` (new `ErrorCode`, 409).
- **List.** `GET /v1/commands?status=&branch=&group=&before=&limit=` with an owner session returns `{commands, next_before}` newest first with `status`, `error`, `queued_at`, `done_at`. A request with `Authorization` or an `epoch` query is the Hub's mailbox fetch, unchanged.
- **Limit.** 60 posts a minute per tenant (`RATE_LIMITED`).
- **Until 8.19** a Hub that fetches one of the new kinds records it `rejected` `UNSUPPORTED`.

## Story 8.19: The Hub applies owner commands exactly once, and the owner's change wins

- **Handled kinds.** The Hub runs the seven setup kinds plus `rate|plan|rule|coupon|product.upsert` and `backup.now` (`HANDLED_COMMAND_KINDS`, checked against `OWNER_COMMAND_KINDS`). Anything else is `rejected` `UNSUPPORTED`.
- **Owner wins.** `owner_wins` sets `base_rev` to the row's current `rev` for plan, rule, product and coupon; a rate always applies. When the command's `base_rev` was lower than the row's `rev`, the same transaction stores the overwritten change (intent id, actor, time) in `owner_overrides` keyed by the owner change's `(epoch, seq)` (migration 0077).
- **Toast.** The UI stream sends `updated_by_owner {kind, entity, row_id, overwritten?}`; copy is keyed by kind (`OWNER_UPDATES`, `apps/hub/src/lib/owner-update.ts`). For rates and rules `groups.changed` carries `by_owner:false` so toasts do not stack.
- **backup.now.** `BackupRequester`/`BackupResponder` link `CommandTask` to `UploadTask`; it forces a cycle and waits up to 30 s (`HUB_SLOW` on timeout), reporting the cycle's transport error code; a Free branch rejects `FEATURE_NOT_ENTITLED`.

## Story 8.20: Commands are swept, expire after 7 days, and are fenced by epoch

- **Sweeper.** `arena-cloud --role jobs --task command-sweep` (CronJob every minute): `pending` older than 7 days -> `expired` with `COMMAND_EXPIRED` (owner list filter `status=expired`), then `fetched` with no answer for 10 minutes -> `pending`. It logs `command_backlog_pending|fetched|oldest_age_s` per branch (the exporter is deferred).
- **Mailbox.** A Hub fetch returns every `pending` row regardless of `after`, plus `fetched` rows past it; the Hub no longer drops rows at or below its cursor (the `applied_commands` dedupe re-reports the stored result).
- **Fence.** Fetch or result at an epoch below `branch_fence` is `FENCED` (message names the fence); a refused result re-queues its `fetched` command. A result records `applied_epoch`. `ControlDb::move_branch_fence(branch, epoch, hub, fork_seq, now)` returns commands applied at the old epoch past `fork_seq`, and unanswered `fetched` ones, to `pending`; nothing calls it until Story 9.3. The Hub closes its writer and becomes a follower on `FENCED`.

## Story 8.21: Preview the target branches and follow each command row to Applied

- **Preview.** `POST /v1/commands/preview {branch_id|"all", kind}` answers `{targets}` with `online` (heartbeat within 3 min), `offered` and, when not offered, `error` (`FEATURE_NOT_ENTITLED`, `BRANCH_UPDATE_REQUIRED`); it uses the post's `refusal`, writes nothing and is not rate limited.
- **Payload.** `OwnerCommandView.payload` carries what was sent so Retry / Resend can send it again; the app drops the old `base_rev` and takes the current `rev` from the branch's newest summary catalogue (plans, rules, coupons, products by `id`), else keeps the old one. A rate has none.
- **Seam.** No Manage form exists, so `arena:propose-command` (a window `CustomEvent` whose `detail` is a `CommandDraft`) opens the preview; Story 8.23 calls `propose()` in `commands.svelte.ts` directly. One idempotency key per proposed change.
- **Rows.** `pending`/`fetched` show "Waiting for branch · age", `applied` "Applied · time" in the branch zone, `rejected` "Device/Hub revoked" (`TOKEN_REVOKED`, no action) or "Couldn't apply at branch" with Retry, `expired` "Expired" with Resend. Today / Earlier by the day in the branch zone. The list is the newest 50 rows, polled every 10 s while visible.
- **Manage.** `scope="all"`: one list for the café; Free and setup branches get a `TierLock` / `SetupGate` entry beside it.
- **Today.** An offline branch's card lists its waiting commands (read with each live fetch).
- **Badge.** `StatusBadge` takes `tone` and `text` for command states.

## Story 8.22: Commands queue on the owner's device while it is offline

- **Queue.** IndexedDB `arena-owner` is version 2 with a `queue` store (keyPath `key`). A Send that gets `OFFLINE` queues `{key, cafe, branch_id, kind, payload, base_rev?, created_at, state}` with the draft's ULID key; `clearCache` never touches it.
- **Drain.** `startQueue` (layout mount after `refresh`), the `online` event, the command-list poll and a send that failed on a working line call `drainQueue`: creation order, stop on `OFFLINE`/`RATE_LIMITED`/`INTERNAL`/`UNAVAILABLE`/`AUTH_FAILED`, mark any other refusal `refused` and go on. Older than 7 days -> `expired`, never sent.
- **Rows.** `localRows()` stands queued items in the command list (`Row.local`); `all` expands to the live paid branches. `rowKey` replaces the numeric id for list keys and Retry state.
- **Preview offline.** Built from `shell.branches` and each branch's cached `live` answer (`cmd.known` = cached `as_of`); unseen branches read Unknown.
- **Sign out.** Queued items prompt "Keep queued changes" / delete; session expiry keeps the queue.

## Story 8.23: Manage → Pricing — plans and rates, pricing rules and coupons

- **Fields.** `CatalogueRow.fields` (optional, additive, no schema bump) carries a rate, plan, rule or coupon as its upsert payload without `id`/`active` (plans add `group_ids`, coupons `targets`). An older summary lacks it: the row lists by name and has no Edit. `arena-core` test `pricing_rows_carry_the_fields_of_their_upsert_payload`.
- **Board.** `PricingBoard` (tabs `?tab=plans|rules|coupons`, default `plans`, always written to the URL) reads each live paid branch's `live` answer (the shared `live` store, `fetchDue` on the Today cadence) and shows each branch's data age. `lib/pricing.ts` holds name matching, the Hub's validators (reusing `hub.plans|rules|coupons.invalid.*` and `hub.groups.rate_invalid`), the payload builders and rule status.
- **Ids are per branch.** Forms hold group/plan/product names. `resolve()` builds one `Variant` per branch (own `id`, `base_rev`, group ids, ULID key) matched by trimmed, case-insensitive name (coupon by code, rate by group name). `CommandDraft.variants/excluded/choices`; `confirm` posts one part per variant (`partsOf`), skipping the ones already taken (`done`); offline each is queued. The preview post stays `all`.
- **Choices.** A branch missing the item (scope All) asks "Create here" / "Exclude" and Send waits (`choose`); a branch missing a named group/plan/product, an amount that does not fit its currency, or (when adding) an item with that name is left out with the reason inline.
- **Rule status.** `active now` / `scheduled` / `ended` / `off` in the branch zone; a weekly window past midnight belongs to the day it starts on.
- **Closing.** `cmd.sent` counts sent or queued changes; open forms close when it moves. `whatOf` names a coupon by its code.

## Story 8.24: Manage → Products and bundles

- **Areas.** Manage has two areas, `?area=pricing|products` (default pricing); leaving Pricing drops its `?tab=`. `CatalogueScope` (branch pick, data ages, reads) is shared by `PricingBoard` and `ProductsBoard`.
- **Fields.** Product and bundle catalogue rows carry `fields` like 8.23 (no `image`, no `kitchen_notes`; plus `low_stock_qty`, and `components` on a bundle). Forms hold names; `resolve()` maps them per branch, so a missing component leaves that branch out with the reason.
- **Picture.** `shrinkImage` makes a JPEG ≤ 512 px and ≤ 200 KB. An owner `product.upsert` with no `image` keeps the stored one (`IMAGE_KEEP`, swapped in `owner_wins`); `""` removes it. The cloud allows 288 KB payloads for `product.upsert` only (`command_payload_max`) and `POST /v1/commands` bodies of that size.
- **Low-stock level.** `stock.threshold` is an owner kind (`{product_id, low_stock_qty | null}`, no `base_rev`, 14 handled kinds); a tracked non-bundle row has its own "Low-stock level" action.

## Story 8.26: Manage → Staff users and roles by command

- **Kinds.** `staff.upsert` (owner payload `OwnerStaffUpsert`, no credential), `staff.deactivate` and `role.upsert` are owner kinds that are `handled` and `sensitive`, so each send passes step-up (8.25); the Hub runs 16 kinds. The Hub treats the owner as an `Owner` actor for all three (`staff.deactivate` now accepts it) and still refuses the last Owner, a built-in role edit, a taken login.
- **Setup code.** The cloud mints it per branch (`staff_part`): payload gets `staff_id` (derived from key + branch), `setup_code_hash`, `setup_expires_at = now + 72 h`; the answer carries `setup_codes` (`no-store`), shown once in `SetupCodes` (`cmd.codes`, memory only). A repeat of a post returns no code; `setup_code: true` on an edit issues a new one. The owner list hides the hash and shows `setup_code: true`.
- **Per-branch posts.** Staff are told apart by login and roles by name; `proposeStaff*` / `proposeRole` build one variant per branch (own ids, `base_rev`, role id). `role.upsert` perms are checked against the `arena-proto` catalogue by the cloud (`perms[i]`); the app's checklist is `PERMISSIONS`, typed `Record<Permission, true>` so a new permission breaks the build.
- **Summary.** `CatalogueStaff.login/role_id` and role `fields.permissions` (additive).
- **Toasts.** `OWNER_UPDATES` has staff (`staff` table, also for `staff.deactivate` via `OwnerUpdate::covers`) and role (`roles`); an owner-actor `staff.upsert` intent in a test stream now raises `updated_by_owner`.

## Story 8.27: Manage → Devices and groups, and revoking a device

- **Kinds.** `group.upsert` and `device.assign` are plain owner kinds, `device.revoke` is sensitive (step-up) and carries `reason` (`OwnerDeviceRevoke`, lost|stolen|replaced|other); the Hub runs 17 kinds. It moves `reason` into the intent and applies its own intents as the owner. A revoke of an already revoked device is `applied` with no change (`already_revoked`, answered before any intent).
- **Summary.** Group rows carry `fields {name, group_kind, colour, sort_order}`; a revoked device stays in `catalogue.devices` (`revoked: true`).
- **Revoked Hub token.** The `command-sweep` job rejects waiting commands of a branch whose Hub rows are all revoked with `TOKEN_REVOKED` ("Device/Hub revoked" in the app); `SweepReport.revoked` counts them.
- **Owner app.** `?area=devices`: groups by name over the branches in scope (swatch dot via `GroupChip`), devices per branch with Move (groups of the device kind, or Unassigned) and Revoke (reason, then step-up). A device belongs to one branch, so a move or revoke is one command to that branch.

## Story 8.28: The cloud sends player and owner messages through a swappable messaging adapter, exactly once

- **Route.** `POST /v1/notify {outbox_id, kind, purpose, consent_policy_version, recipient, params}` with a paid Hub's device token answers `{outbox_id, state queued|sent|failed, channel, attempts, provider_message_id?, error_code?}`. Free is `FEATURE_NOT_ENTITLED`; no adapter is 503.
- **Ledger.** `send_ledger` (migration 22, `outbox_id` unique) is inserted and then claimed before a send, so a repeat, a lost reply or a race never sends twice and a repeat gets the stored result. The recipient is kept only as `SHA-256("notify\0" ‖ pepper ‖ phone)`. Connect failures and HTTP 429 leave the row `queued` and unclaimed (the Hub posts again); a timeout or 5xx leaves it `queued` and claimed (in doubt, never resent).
- **Fallback.** A non-retryable WhatsApp rejection is followed by an SMS in the same call (attempts 2, `attempt_log` holds both); a rejected SMS ends `failed` with the code.
- **Templates.** `message_templates` has version 1 of `credit_reminder`, `low_balance`, `session_ending`, `console_5min`, `receipt` (placeholder provider ids until approved). A change is a new version via `add_template_version`; a ledger row keeps its version, also on retry.
- **Adapter.** `Notifier` is Fake (recorded, per-channel `Behaviour`, `GET /v1/dev/notify` under `--dev`), Manual (`ARENA_NOTIFY_MODE=manual`, logs the message for an operator) or Gupshup (WhatsApp template API with `ARENA_GUPSHUP_WA_API_KEY|SOURCE|APP`, SMS through the sign-up account with a per-template DLT id).
- **Hub side.** The drain, consent re-check, `notify.sent|failed` and the irreversible-effect guard are Story 8.29.

## Story 8.29: The Hub drains its notification outbox after the undo window

- **Task.** `NotifyDrainTask` (primary, paid, paired; every 10 s) takes `SnapshotReader::notify_plan`: each due `notify_outbox` row is `Send`, `Wait`, `Skip(reason)` or `Expire` (> 7 days past `not_before`). Posts go to `/v1/notify` keyed by the row ULID in `not_before` order; the WAN down or a busy cloud leaves rows queued, a cloud `queued` answer (in doubt) is asked again after 60 s.
- **Rules.** `notify_drain::RULES` is the extendable per-row eligibility list: `player_exists`, `consent_current`, `receipt_ready` (settled order, invoice number present or wait). Story 8.30 and Epic 9 append their rules there.
- **Results.** System intents `notify.sent` (channel), `notify.failed` (code), `notify.skip` (reason), `notify.expire`; migration 78 adds the `expired` state and `channel`/`error_code`/`reason` columns. `notify.sent` records the irreversible effect `notification_sent` on the row's order, so Undo is refused afterwards.
- **Receipt params.** `name`, `receipt_no`, `amount` (branch money format) and `cafe` (branch name), as the cloud's `receipt` template names them.

## Story 8.30: Low-balance, session-ending and console 5-minute messages

- **Trigger.** `NotifyTriggerTask` (primary, every 10 s) takes `notify_trigger::triggers`: `Request` or `Rearm` per running session; each is a system intent (`notify.requested`, `notify.rearm`) that re-checks tier, switch, consent and "already queued". The row is due at the end of the Undo window.
- **Rows.** Migration 79: `order_id` nullable, `session_id`, `params` (JSON of raw values: `amount_minor`, `minutes`, `console`), `rearmed_at`; unique `(session_id, kind)` for `session_ending`/`console_5min` (an Extend never repeats them). `low_balance` is once per crossing, closed by `rearmed_at`.
- **Switches.** `notify.low_balance` for low balance; `notify.session_ending` covers both the PC and the console message. No console-specific setting.
- **Drain.** `RULES` gains `session_near_end` (skip `session_changed`); the drain adds `balance` (branch money format) for `low_balance` and `cafe` for every kind. A sent session message registers no irreversible effect.

## Story 8.31: Credit reminders go out on cycle days, and Free shows an overdue list

- **Check.** `CreditReminderTask` (primary, every 5 min) takes `credit_reminder::due`: from 11:00 branch time, each player with credit outstanding whose oldest unpaid spend is exactly as old as one of their `reminder_days` (FIFO aging), on a paid branch with `notify.credit_reminder` on and transactional consent granted. Each is a `notify.requested` system intent (kind `credit_reminder`, no `session_id`, `amount_minor`).
- **Once a day.** Migration 80 adds `notify_outbox.reminder_day` and a unique index on `(player_id, reminder_day)` for credit reminders: repeats, restarts and a late start the same business day queue nothing twice, and a missed day is never made up. 8.32's `credit.remind` shares the rule.
- **Drain.** `RULES` gains `credit_still_owed` (skip `credit_cleared`); the `amount` parameter is what the player owes at send time.
- **Overdue.** `GET /ui/players/overdue` (`player.view`) answers `OverdueView {as_of_day, reminders, rows}`: oldest unpaid first, `bucket` of the oldest spend (`0_7`, `8_14`, `15_plus`), `consent` (true, false or none) and `last_reminder_day` (latest `sent` row, paid branches only). Players has a view switch (`?view=overdue`); Free shows a lock line and no reminder column.

## Story 8.32: Reports — credit, with a one-tap reminder

- **Credit on the range answer.** `OwnerRange.credit` (`OwnerRangeCredit {as_of, outstanding, aged, players}`) is the newest accepted rollup's credit (`from_rollups`, like stock); `as_of` is null with no accepted day. Players are id, username, outstanding, oldest unpaid date and `consent`; never phone, name or email. No rollup schema change.
- **Two reads.** Like stock levels, the Credit tab reads the last 7 days (`loadLevels`, which now also keeps `credit`) so "Today" still shows the latest figures with their date. A cached range answer without `credit` reads "No credit figures yet."
- **`credit.remind {player_id}`.** A handled, non-sensitive Basic kind (16 owner kinds, 18 handled). The Hub's `command_intent` builds an owner-actor `notify.requested` (kind `credit_reminder`, no `base_rev`, placeholder `amount_minor` "0" that `notify_request::mutate` overwrites with what the player owes). `rbac` lets an Owner actor submit only a `credit_reminder` `notify.requested`. Every refusal (no consent, nothing owed, Free/switched off, one already queued for the business day) is `rejected`, shown as "Couldn't apply at branch".
- **Reminded today.** `BranchSummary.reminded_today` (additive, empty omitted): ids with a `credit_reminder` row for the current business day in state `queued` or `sent` (`credit_reminded_on`). The app also counts a reminder it sent itself (a non-refused `credit.remind` row from today), since the summary lags by a cadence.
- **App.** `CreditPanel` (per branch: date, outstanding, 0–7/8–14/15+ buckets, player rows). "Send reminder" calls `sendNow` (one post, fresh ULID key, queued on this device when offline) and the row shows its `command-row`; a reminded player asks "Send anyway" / "Cancel"; no consent shows "No consent for messages" and no button.
- **Exports.** `report=credit` CSV: one row per player per branch (branch, currency, player id, username, outstanding, aging bucket, oldest unpaid date, as-of); PDF is the print view with the buttons hidden; Credit is the fourth entry in Exports.

## Story 8.33: Owner alerts by web push and WhatsApp, and the Alerts tab

- **Tables.** Control migration 23: `push_subscriptions`, `owner_alerts` (kept 90 days, pruned on each raise), `owner_alert_deliveries` (the once-only claim per alert and target), `owner_alert_settings` (`whatsapp_enabled`, default on) and `owner_alert` template v1 (params `branch`, `text`). `owner_alert` is not in `MESSAGE_KINDS`, so a Hub cannot post it to `/v1/notify`.
- **Kinds.** `arena_proto::OwnerAlertKind` (`test`, `cash_variance`, `low_stock`, `credit_overdue`, `large_refund`, `large_discount`, `branch_offline`) holds each kind's parameters, plain-text line and deep-link url; a later kind is one arm. Stories 8.34 and 8.35 raise real alerts through `owner_alerts::raise()`; this story's only source is the dev route `POST /v1/dev/owner-alerts`.
- **Raise.** Idempotent by alert id. Push is claimed per `(alert, subscription)` and never retried; a 404/410 deletes the subscription. WhatsApp is the 8.28 send ledger with `outbox_id = <alert_id>.wa`, through the shared `api::notify::deliver`. A Free branch is refused.
- **Push.** `push::WebPush` is Fake (dev, tests) or Http (VAPID ES256 from `ARENA_VAPID_PRIVATE_KEY` and `ARENA_VAPID_SUBJECT`, RFC 8291 `aes128gcm`, no redirects). Without a key the push routes answer `UNAVAILABLE`; the list and WhatsApp still work. The variables are not in the deploy manifests yet.
- **Routes.** `GET|PATCH /v1/alerts/settings`, `POST /v1/alerts/push` and `/push/remove`, `GET /v1/alerts?before=&limit=`. Subscription endpoints must be https with a named host.
- **App.** `AlertsBoard` (list, "All quiet.", polite live region once per alert, 30 s poll, focus and push-message refresh) with push and WhatsApp switches; iOS outside the Home Screen shows the install line and never asks. The service worker shows the push and focuses or opens the alert's `url`; Reports opens on `?tab=` and `?branch=`, Today scrolls to `#today-<branch>`.

## Story 8.34: The Hub raises owner alerts for cash variance, low stock, overdue credit and large refunds or discounts

- **Rows.** Migration 81 rebuilds `notify_outbox`: purpose `owner_alert`, the five alert kinds, nullable `player_id`, channel `app` and `source_ref`, with `(kind, source_ref)` unique for alerts. `apply/owner_alert.rs::queue` is the one writer: paid tier only, due at the end of the Undo window, once per event (it finds an existing row and queues nothing), text parameters cut to 40 characters, money kept raw as `amount_minor`.
- **Sources.** `shift.close` (|variance| over `cash_variance_minor`, else the branch threshold), `raise_low` (a Hub alert), the `credit.auto_pause` flip on (`<player>:<day>`), `refund.create` (refunded total) and a manual discount (`discount::record`, amount or percent of the bill after the coupon; `order_id` set so the Undo of the settle skips it).
- **Limits.** Setting `owner_alert.thresholds` (`OwnerAlertThresholds`, in `DEFAULT_SETTINGS`, so `branch.seed_defaults` also gives it to an existing Hub); reads fall back to the defaults. The owner sets it with `settings.set` (now `handled("settings.set")` in the owner catalogue, 17 kinds; the cloud accepts only this key and a valid value).
- **Drain and cloud.** Alert rows skip the player and consent rules; `low_stock` is skipped (`stock_recovered`) when its Hub alert is acknowledged. The request is `{kind, purpose owner_alert, consent_policy_version "owner", recipient "", params}` with `amount` in the branch money format and no `cafe`. The cloud raises it with `id = outbox_id` for a Hub-sourced kind and answers `{state sent, channel app}`.
- **App.** Alerts has an Alert limits form (`ThresholdsForm`, `alert-limits.ts`), the preview and the command list; the fields start at the defaults in the branches' money unit (the app does not read the limits back from the Hub).

## Story 8.35: The cloud alerts the owner when a branch goes offline during opening hours

- **Hours cache.** A paid Hub's heartbeat carries `opening {opening_hours, timezone}` (the stored hours else the default, and the effective zone; never on Free). The cloud keeps it in `branch_opening` (control migration 24) only when it parses (`BranchOpening::parse`, at most 1 KiB) and only on a paid tier, and writes nothing when unchanged; a bad block is ignored.
- **Watch.** `arena-cloud --role jobs --task offline-watch` (CronJob every 5 minutes, same `AuthState` as the web role) raises `branch_offline` (`off-<branch>-<last_seen_at>`) and `branch_not_uploading` (`up-<branch>-<newest upload>`) once per episode and a push-only `branch_online` (`on-<branch>-<offline_key>`) when the heartbeat returns; episode keys live in `branch_watch`. The window rule uses `arena_proto::opening_window_start`.
- **Mass hold.** At least 5 branches that would alert and more than 20% of the paid branches with a live Hub: nothing is raised or recorded, the on-call is paged through `ARENA_ALERT_WEBHOOK_URL` and the job succeeds. `owner_alerts::Raise.whatsapp` is false for `branch_online`.

## Story 8.36: The audit builder rebuilds a branch month on demand

- **Jobs.** Control migration 25 adds `builder_jobs (branch_id, month, state, progress, key, built_at, error, attempt, requested_by, reason, …)`, one row per branch and month. `POST /v1/branches/{id}/audit/build {month}` (owner session, paid branch, a real month no later than the current one) queues a job or reuses a `queued`, live `building` or fresh `ready` one, and answers the job's `AuditBuildStatus`: 202 with `code AUDIT_BUILDING` and the progress, 200 when ready. `GET …/audit/build?month=` follows it; a `failed` job answers `AUDIT_BUILD_FAILED` (502) there, and the next `POST` starts a new attempt (`attempt` counts claims and restarts). `audit_build::request_build` is the one entry point the support path of Story 8.37 will reuse with `requested_by = admin:…` and a `reason`.
- **Limit.** A tenant may start 6 builds an hour (`AUDIT_BUILDS_PER_HOUR`, `rate_limits` key `auditbuild:<tenant>`); a request that reuses a job does not count. Over it: `RATE_LIMITED` with `Retry-After`. "Builder queue depth" (`builder_jobs` in `queued`) is logged as `builder_queue_depth` when a job is queued and claimed; the OpenTelemetry export is still deferred with the Story 2.3 exporter.
- **Builder.** `arena-cloud --role builder` (Deployment `arena-cloud-builder`, no Service) claims the oldest queued job (compare-and-set on `updated_at`; a `building` job untouched for 15 minutes is claimed again and its `attempt` goes up, so the dead builder's late writes are refused). It opens the newest `snapshots/<date>.db.zst` dated on or before the month's first day (or an empty database), lists `changes/`, keeps the batches after the snapshot's last `(epoch, seq)`, reads each epoch's first batch for its fork point, then downloads, checks (`decode_batch`: zstd, SHA-256, row count) and replays one batch at a time with `arena_replay::MonthBuilder`. Progress is the batch share of 95%; the upload is the rest.
- **Replay.** `arena_replay::replay_history` adds fork points and a starting position to `replay`: rows of a forked epoch's parent above `parent_seq` are returned as orphans, not applied; a forked epoch starts at `parent_seq + 1`; changes at or before the position are skipped. `MonthBuilder` stops at the first change whose `business_day` is in a later month, writes every applied change into the audit database's `changes` table (the search of Story 8.37 reads it) and the orphans into `orphaned_changes`, and `finish` packs the database (`VACUUM INTO` + zstd). `state_checksum` hashes the rows of every change-log table; the round-trip tests compare the rebuilt database with the Hub's.
- **Failure.** A gap between batches (`seq gap`), a missing batch object, a batch that fails its checksum or does not match its name, or a snapshot above a fork point fails the job with that reason (kept 300 characters).
- **Expiry.** The builder writes `audit-cache/<yyyy-mm>.db.zst` under the branch prefix and deletes it 24 hours after `built_at` (`sweep_expired`, run when idle): the object key starts with the branch prefix, so one R2 lifecycle rule cannot match it. A stale `ready` job reads as no build and is rebuilt by the next request.
- **Not here.** The builder's local cache and `GET /v1/branches/{id}/audit?q=` (Story 8.37), the admin endpoints, and a build from a snapshot taken after a fork point it should have honoured (the job fails; an earlier snapshot is needed).

## Story 8.37: The builder serves audit queries from a local cache

- **Catalogue.** `arena_proto::AuditSearch::parse` is the only way a query is shaped: `q` is one of `recent`, `actor` (`actor_kind`, optional `actor_id`), `module`, `intent`, `entity` (`tbl`, optional `row_id`) or `time` (`from` and `to` both required, `to` exclusive); every entry also takes `from`/`to`, `month` and `cursor`. Unknown `q` or an extra parameter is `UNSUPPORTED` (400), a bad value `VALIDATION_FAILED` (422). Pages are `AUDIT_QUERY_PAGE` = 50 rows newest first; `AuditSearchPage {branch_id, month, built_at, items, orphaned, next_cursor}` carries metadata only (no row image), and `orphaned` (the `orphaned_changes` rows under the same filters) only on the first page.
- **Route.** `GET /v1/branches/{id}/audit?month=&q=…` (owner session, paid branch) runs in the control role, which authenticates the caller; `--role builder` still only builds. `audit::search_answer` is shared with the support path: no job is `NOT_FOUND`, `queued`/`building` is 202 `AUDIT_BUILDING` with progress, `failed` is 502 `AUDIT_BUILD_FAILED`, a `ready` job past 24 h or whose object is gone is queued again (`request_rebuild` forces a fresh `ready` one) and answers 202.
- **Local cache.** `audit_query::AuditCache` (`ARENA_AUDIT_CACHE_DIR`, `ARENA_AUDIT_CACHE_MAX_BYTES`, default 50 GiB) unpacks `audit-cache/<yyyy-mm>.db.zst` to `<branch>/<month>-<built_at>.db` on a miss and opens it read-only; a hit refreshes the file's modified time and eviction drops the oldest files until the total fits (never the month in use). An evicted month is fetched again while its object exists.
- **Search.** `arena_store::list_audit` (now with `intent_kind`, `tbl`, `row_id` filters) and `list_orphaned`; the cursor is base64url of `at.seq.epoch`.
- **Support path.** `POST /v1/admin/branches/{id}/audit/build {month, reason}` and `GET /v1/admin/branches/{id}/audit?month=&q=&reason=` take `Authorization: Bearer <staff token>`; the service holds only SHA-256s in `ARENA_ADMIN_TOKENS` (`<staff-id>:<hex>,…`) and answers 404 when none is set, 401 for a wrong token. A blank `reason` is 422 (`field: reason`). Each call that passes those checks writes one `admin_audit` row (`audit.build` / `audit.search`, `actor_kind = admin`, staff id, branch target, reason, summary `{month, query}`) before any data is returned; jobs are queued with `requested_by = admin:<staff>`. Cloudflare Access and the admin app UI are Epic 11.

## Story 8.38: Audit search in Reports with "Preparing audit…"

- **Tab.** Reports → Audit (`?tab=audit&branch=`) replaces the range controls and export buttons with `AuditPanel`: branch (paid ones), month (defaults to this month in the branch's zone) and one search. The search is one catalogue entry: Everything → `recent`, Who → `actor` (+ optional `actor_id`), Part of the app → `module`, What was done → `intent` (lower-cased, `[a-z0-9_.]`), A player → `entity` with `tbl=players`; changing the entry clears the typed text.
- **Flow.** `audit.ts::searchAudit` is a `GET …/audit?month=&q=…` read through `api.probe` (keeps the status, never cached). `page` shows the first page; `building` shows "Preparing audit…" with the progress and asks again every `AUDIT_POLL_MS` (2 s); `missing` (`NOT_FOUND`) posts `…/audit/build` first; `failed` shows "Couldn't prepare the audit." and Retry posts a new build. A newer search, or leaving the tab, drops the old poll (ticket). Load more passes `cursor`; `orphaned` comes with the first page.
- **Rows.** `hub.audit.intent.*` / `hub.audit.module.*` / `owner.audit.module.*` labels are looked up by key, so a kind with no label shows as it is. Actor names the cloud sends as `hub.audit.actor.*` keys are translated. `elevated_by` reads "PIN by {name}" or "Owner MFA".

## Story 8.39: A Cloudflare Tunnel for entitled branches, run by the Windows Hub

- **Flag.** `arena_proto::TUNNEL_FLAG` (`tunnel`) is now in Premium's `TIERS` flags (the add-on on Basic arrives with Epic 10). `tunnel_hostname(branch_id)` is `<branch_id>.tunnel.arena360.cloud`; the connector's ingress is only `TUNNEL_ORIGIN` (`http://127.0.0.1:7403`, no listener yet: 8.40/8.41), everything else `http_status:404`.
- **Wire.** `HeartbeatRequest.tunnel {has_credential}` (Hub with the tunnel task only) and `HeartbeatResponse.tunnel`, always present for a paid or Free branch: `grant {hostname, credential}`, `on {hostname}`, `off`, `unsupported_platform`. `TunnelAnswer`'s `Debug` redacts the credential.
- **Cloud.** `api/heartbeat.rs::tunnel_answer` decides from the signed claims' `tunnel` flag and `hubs.platform`. Control migration 26 `branch_tunnels` (`ready`/`deleting`, `delivered_at`; the credential is never stored). A grant is sent when the tunnel is created, or when `delivered_at` is empty or the Hub reports no credential (a fresh `credential()` call, the tunnel is not recreated). A provider error answers `off` and the next heartbeat retries; a delete error leaves the row `deleting`. A database read error answers no `tunnel` at all, which tells the Hub to change nothing.
- **Provider.** `tunnel::TunnelProvider` is `Fake` (`--dev`/tests, `FakeTunnels` counts creates, deletes and credentials and fails an `FakeOp` on demand) or `Cloudflare` (`ARENA_CF_API_TOKEN|ACCOUNT_ID|ZONE_ID`, optional `ARENA_CF_ACCESS_POLICY_ID` and `ARENA_CF_API_BASE`): `ensure` looks up by name before it creates the tunnel, its configuration, the CNAME and the Access app; `delete` drops the app, the CNAME, the connections and the tunnel, and a missing object is success. Without a token no tunnel is made and the answer is `off`.
- **Hub.** `arena_tasks::tunnel`: `TunnelInbox` (answer slot, `has_credential` flag), `TunnelCredential` (`tunnel-credential.bin` in the data dir through `named_keystore`, so DPAPI on Windows), `Connector`/`ProcessConnector` (`cloudflared tunnel --no-autoupdate run`, the credential in `TUNNEL_TOKEN`, never on the command line) and `TunnelTask`. The task waits for the cloud's word before it starts anything, runs only while the Hub is primary, restarts a connector that exits after `backoff` (2 s doubling to 60 s, reset after 60 s up) and never calls the writer. `--cloudflared <path>` (default `cloudflared.exe` beside the executable). Android runs no task; the heartbeat sets the `unsupported_platform` line itself.
- **UI.** `HubStatus.tunnel` (`TunnelLine {state, hostname?}`: `off|starting|on|reconnecting|unsupported_platform`), `GET /ui/tunnel` (loopback, origin-guarded), and System → Connection, which reads "Needs the Windows Hub" on an Android Hub.
- **Cost.** `docs/runbooks/cloudflare-tunnel-cost-spike.md`: tunnel, DNS and bandwidth are free; Access seats are the cost.

## Story 8.40: Live view — a read-only floor through the Tunnel

- **Cloud.** `GET /v1/live/{branch}` (`api/live.rs`, owner session, same tenant/rate rules as the owner read API): Free or no `tunnel` flag in the signed claims is `FEATURE_NOT_ENTITLED`, a non-Windows active Hub is `PLATFORM_UNSUPPORTED` (new code, 409), no Hub or no `ready` tunnel row is `NO_HUB` (503). The answer is `LiveGrant {url: wss://<branch>.tunnel…/live, token, expires_at}`. The token is `arena_proto::LiveClaims {aud:"live", tenant_id, branch_id, iat, exp}` (life ≤ `LIVE_TOKEN_TTL_S` = 300) signed by `LicenceSigner::sign_live` with the licence key, so the Hub's existing verify keys trust it; the claims differ from `LicenceClaims`, so neither passes as the other.
- **Class.** `ClientClass::LiveView` (`live_view`, no device token, empty `class_scope`). It is accepted on the tunnel transport only (`Transport::Tunnel`); the tunnel transport accepts nothing else. `devices.class` still allows only `paired|kds`; `device.register` refuses `live_view`.
- **Listener.** `LanListeners::bind_tunnel(addr)` (loopback only) adds a third accept source to `serve_lan`; `serve_tunnel` upgrades `GET /` or `/live` to a WebSocket and runs `session::run` with `Transport::Tunnel` (other paths 404, non-loopback peers dropped). The Hub binds `127.0.0.1:TUNNEL_PORT` (7403, `TUNNEL_ORIGIN`) on the Windows platform only; a bind failure is logged and costs only the live view. Origin is not checked: the signed token is the authority (Cloudflare Access fronts the hostname).
- **Admission.** `session::verify_live` (`LanHub::with_live_view(verify_keys)`): the Hub must be primary (`NOT_PRIMARY`), the token must verify against its keys for its own branch id and trusted time (`TOKEN_INVALID`), and its own `licence.flags` must carry `tunnel` (`FEATURE_NOT_ENTITLED`). The per-address hello throttle is skipped for the tunnel (every peer is the local connector).
- **Read-only.** `Ctx::handle` answers every frame but `ping`/`pong`/`hello` with `FORBIDDEN` for a live viewer (and keeps the session); `live.token {token}` is handled in the session loop: a good token extends the deadline (`ack`), a bad one is refused (`TOKEN_INVALID`) and changes nothing. A session whose token expires gets `TOKEN_INVALID` and is closed. `browser_tables(LiveView)` is empty: a viewer never sees applied-change summaries.
- **Feed.** `HubEvent::LiveFloor(LiveFloor)` (`live.floor`): `{generated_at, zones[{id,name}], tiles[{device_id,label,zone?,state,session?{started_at,paused_at?,paused_total_ms,ends_at?,grace_ends_at?,player_id?|guest_no?}}]}`. Built by `arena_core::live_floor::build_live_floor` from devices, `devices_meta`, `floor_zones` and `live_sessions` (paired, not revoked, with a floor row; names, balances, bills and orders are never read), presence overlaid by the registry (`DeviceDirectory::live_floor` + `set_presence`). Sent at hello, on applied changes to `sessions|devices|devices_meta|floor_zones` and every `LIVE_TICK_MS` (1 s) while a viewer is connected.
- **App.** `/live` (`LiveBoard.svelte`, `live.ts`): sidebar entry (desktop only, `wide` nav item) and a link on each Today card (phone only; "Needs the Windows Hub" instead when the branch's Hub is Android). `entryOf(branch)` decides tier-lock ("Available on Premium", no Upgrade link) / platform note / view from the branch list; the cloud's `FEATURE_NOT_ENTITLED` / `PLATFORM_UNSUPPORTED` answers win. `connectLive` sends `hello` with the token, answers `ping` with `pong`, asks for a new grant after `REFRESH_MS` (4 min) and sends `live.token` on the open socket. A failed grant or a closed socket shows "Branch offline" with a link back to `/#today-<branch>`. The data age is measured by the owner's own clock from the newest frame ("Live" ≤ 3 s, else "Updated n s ago"); running times use the Hub's `generated_at` plus the time since receipt, so clock skew does not matter.
