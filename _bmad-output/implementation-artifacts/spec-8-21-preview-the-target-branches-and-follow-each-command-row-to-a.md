---
title: 'Story 8.21: Preview the target branches and follow each command row to Applied'
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: 'ebf4722'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-20-commands-are-swept-expire-after-7-days-and-are-fenced-by-epo.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Owner app can post commands (8.18) but has no way to show which branches a change will reach before it is sent, and nothing follows a sent change to Applied, Rejected or Expired.

**Approach:** A cloud `POST /v1/commands/preview` answers per branch (online from the heartbeat, offered or not and why, the same `refusal` check a post makes). The Owner app gets a `command-preview` (targets, Send, Cancel), a `command-row` list on Manage grouped Today / Earlier that polls every 10 s, Retry / Resend, and a "Waiting for branch" list on an offline branch's Today card. No Manage form exists yet, so a fixture change reaches the preview through the `arena:propose-command` window event (Story 8.23 calls `propose` directly).

## Boundaries & Constraints

**Always:**
- Preview (`{branch_id|"all", kind}`): `{targets:[{branch_id, name, online, last_heartbeat_at?, offered, error?}]}`; online = heartbeat within `BRANCH_ONLINE_WINDOW_MS`; `error` is `FEATURE_NOT_ENTITLED` or `BRANCH_UPDATE_REQUIRED`; an unknown or reserved kind is `UNSUPPORTED`, another café's branch `NOT_FOUND`; it writes nothing and does not count against the post limit.
- `OwnerCommandView` carries `payload` (with `base_rev` beside its fields) so Retry / Resend can send it again.
- Row states: `pending`/`fetched` "Waiting for branch · {age}" (info); `applied` "Applied · {time in the branch zone}" (ok); `rejected` "Device/Hub revoked" when the code is `TOKEN_REVOKED` (no action), else "Couldn't apply at branch" with Retry (danger); `expired` "Expired" with Resend (muted).
- Retry / Resend posts a new command (new ULID key, the payload without its old `base_rev`, `base_rev` = the `rev` the branch's newest summary lists for the row, else the old one) to that branch only; the old row stays.
- One key per proposed change, fixed at `propose`, so a second Send never queues it twice.
- The list polls every 10 s while the page is visible and on becoming visible; every non-waiting state change is announced once in a polite live region.
- An offline branch's Today card lists its pending/fetched commands under "Waiting for branch".
- `StatusBadge` gains `tone` (info/ok/danger/muted) and `text` for the command states; Hub states are unchanged.

**Never:** a Manage form (8.23+), "Queued on this device" and the IndexedDB queue (8.22), MFA (8.25), paging beyond the newest 50 rows.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| All branches | one paid online, one offline, one Free | targets with badges; Free "Not offered" with the reason | Send off when none offered |
| Send fails | cloud 500 | preview stays, "Couldn't send. Nothing was changed." | second Send reuses the key |
| Retry | rejected plan row, summary rev 9 | new command `base_rev` 9, new key; two rows | a failed Retry says so and can be pressed again |
| Hub applies | row `pending`, then `applied` | within 10 s the badge changes; one announcement | none |
| Offline branch | no heartbeat for minutes | Today card lists "Waiting for branch" rows | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/owner_commands.rs` + `lib.rs` -- `OwnerCommandPreviewRequest`, `OwnerCommandTarget`, `OwnerCommandPreview`, `OwnerCommandView.payload`; generated TS in `packages/ui/src/generated`.
- `crates/arena-store/src/control/owner_commands.rs` -- the owner list reads `payload`.
- `crates/arena-cloud/src/api/commands.rs` -- `preview_command`, route `/commands/preview`.
- `packages/ui/src/components/StatusBadge.svelte`, `messages/en.json` (`owner.cmd.*`).
- `apps/owner/src/lib/{commands.svelte.ts,ulid.ts,CommandRow,CommandPreview,CommandList,ManageBoard}.svelte`, `routes/manage/+page.svelte`, `TodayCard.svelte`.
- Tests: `crates/arena-cloud/tests/owner_commands.rs`, `apps/owner/tests/commands.spec.ts`, `tests/stub.ts` (commands, preview, post), `shell.spec.ts` / `setup.spec.ts` (Manage now shows one list).

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` / `arena-store` / `arena-cloud` -- preview types, endpoint, payload on the view.
- [x] `StatusBadge` tone/text and the `owner.cmd.*` strings.
- [x] Owner app: commands module, `command-preview`, `command-row`, list with polling and live region, Retry/Resend, Today waiting list, Manage gates for Free and setup branches.
- [x] Tests for every matrix row (cloud HTTP and Playwright on desktop and phone).

**Acceptance Criteria:**
- [x] Preview and command-row are exercised with a fixture `rate.upsert` built in the test.
- [x] Send opens a preview of target branches with online/offline badges and not-offered reasons, with Send and Cancel.
- [x] A sent change shows one row per target branch (what, scope, badge, time queued) grouped Today / Earlier.
- [x] Row states read as specified; Retry and Resend send a new command with a new key and the current `base_rev`; the old row stays.
- [x] Rows update within 10 s while visible and are announced once in a polite live region.
- [x] An offline branch's Today card lists "Waiting for branch" commands.

## Spec Change Log

## Review Triage Log

## Design Notes

- The eligibility rules live in the cloud (`refusal`), so the app never guesses tier, flag or Hub version.
- `all` in the preview expands to every branch of the café, so Free and setup branches show as not offered; the post still skips them the same way.
- Manage uses `Section scope="all"`: one command list for the café, with `SetupGate` / `TierLock` entries beside it for branches that cannot take changes.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy --locked --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings` -- expected: clean
- `cargo test -p arena-proto -p arena-store -p arena-cloud` -- expected: pass (known unrelated failures per Stories 8.19/8.20: `backup::a_hung_run_times_out_with_its_own_code`, `binary::unreachable_at_start_*`)
- `pnpm run gen:types`, `check:protocol`, `check:strings`, `check:layers`, `check:secrets`, `format:check`; in `apps/owner`: `pnpm run check` and `pnpm exec playwright test` -- expected: clean (372 tests)

## Auto Run Result

Status: in-review. Implemented the preview endpoint, the payload on the owner command view, `StatusBadge` tones, and the Owner app's command-preview, command-row list, Retry/Resend, polling with a live region and the offline Today list. Verified as listed above. Known unrelated: `check:tokens-used` (`--ad-reserved` in the Hub floor page, present before this story), `check:generated` lists the regenerated files until committed, `check:deploy` needs `kubectl`, Hub `svelte-check` complains about `playwright.kitchen.config.ts`.
