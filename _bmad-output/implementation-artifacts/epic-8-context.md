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
