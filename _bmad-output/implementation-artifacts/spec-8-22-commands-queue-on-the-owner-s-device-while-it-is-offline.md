---
title: "Story 8.22: Commands queue on the owner's device while it is offline"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '129b183'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-21-preview-the-target-branches-and-follow-each-command-row-to-a.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** With no signal the Owner app's Send fails ("Couldn't send"), so a change made on a bad connection is lost or has to be sent twice by hand.

**Approach:** A send that cannot reach the cloud is stored in an IndexedDB `queue` store (client ULID key, `base_rev`, `created_at`) and shows as "Queued on this device". The queue is read when the app opens and sent in creation order on start, on the browser `online` event and on each command-list poll. The preview with no signal shows each branch as last cached, with its age.

## Boundaries & Constraints

**Always:**
- Queue item: `{key, cafe, branch_id|"all", kind, payload, base_rev?, created_at, state: queued|expired|refused}`; only the signed-in café's items are shown or sent.
- Send is `OFFLINE` (fetch failed or response lost) -> queue with the draft's own key, close the preview, announce once in the polite region. The same key is sent on the drain, so a lost response returns the same server row.
- Drain: creation order; the item leaves the queue when the cloud accepts it (its rows join the list as "Waiting for branch"); `OFFLINE`, `RATE_LIMITED`, `INTERNAL`, `UNAVAILABLE`, `AUTH_FAILED` stop the drain and keep the order; any other refusal (`FEATURE_NOT_ENTITLED`, `BRANCH_UPDATE_REQUIRED`...) marks that item `refused` ("Couldn't apply at branch", Retry) and the drain goes on.
- An item older than 7 days is never sent: marked `expired` ("Expired", Resend) when the queue is read or drained.
- Retry / Resend of a queued item replaces it with a new item (new key, now, current `base_rev`); Retry / Resend of a cloud row with no signal queues the new command.
- A queued `all` change shows one row per live paid branch known to the app.
- The offline preview shows each branch's cached online state and "Last known {age}" (or Unknown / "Not known yet"); offered only when the branch is live and not Free.
- Sign out with queued items asks "Keep queued changes" / "Sign out and delete them"; the cache (`payloads`) is always cleared, the queue only on delete. A session that expires keeps the queue.

**Never:** a server-side change (the post is already idempotent by key), a Manage form (8.23+), MFA (8.25).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Send offline | no network | item in IndexedDB, row "Queued on this device" (muted) | none |
| Back online | 2 queued | posted in creation order, rows "Waiting for branch", queue empty | stop on a transient code |
| Lost response | cloud made the row, answer lost | queued, sent again at once with the same key, one server row | none |
| Older than 7 days | queued 8 days ago | not sent, "Expired" + Resend | Resend sends a new key |
| Refused on send | `FEATURE_NOT_ENTITLED` | that row "Couldn't apply at branch" + Retry, the next item is sent | none |
| Sign out | items queued | owner picks keep or delete | none |

</intent-contract>

## Code Map

- `apps/owner/src/lib/cache.ts` -- DB version 2, `queue` store (keyPath `key`), `queueAll/Put/Delete`; `clearCache` leaves it.
- `apps/owner/src/lib/commands.svelte.ts` -- `Queued`, `Row`, `localRows`, `enqueue`, `loadQueue`, `drainQueue`, `startQueue`, `discardQueue`, offline `lastKnown` preview, `again` queues when offline.
- `CommandRow` / `CommandList` / `CommandPreview` -- local rows first in the list, the last-known preview.
- `routes/+layout.svelte` (start + `online`), `routes/account/+page.svelte` (keep or delete), `today.svelte.ts` (`livePath` exported).
- `packages/ui/src/messages/en.json` -- `owner.cmd.state.queued`, `owner.cmd.preview.last_known*`, `owner.account.queue.*`.
- Tests: `apps/owner/tests/queue.spec.ts`; `stub.ts` (idempotent post by key, `lose`).

## Tasks & Acceptance

**Execution:**
- [x] IndexedDB `queue` store and the queue module (enqueue, read, drain, expiry, discard).
- [x] Offline preview with last-known statuses and ages; Send queues when the cloud cannot be reached.
- [x] Queued rows in the command list; Retry / Resend for queued and cloud rows.
- [x] Keep-or-delete choice at sign out.
- [x] Playwright tests for every matrix row on desktop and phone.

**Acceptance Criteria:**
- [x] No network: the command (ULID key, `base_rev`, `created_at`) is stored in IndexedDB, its rows read "Queued on this device", the preview shows last-known statuses with their age.
- [x] Network back or app reopened online: posted in creation order, rows move to "Waiting for branch"; a retry after a lost response returns the same row.
- [x] Older than 7 days: not sent, "Expired" with Resend.
- [x] A refusal on send shows "Couldn't apply at branch" with Retry and the rest of the queue continues.
- [x] Logout keeps the queue only on "Keep queued changes".

## Spec Change Log

## Review Triage Log

## Design Notes

- The queue is its own IndexedDB store so `dropCache` (used on a refused session as well as sign out) never loses unsent work.
- A refused or expired queued item that is retried is replaced, not kept: it never reached the cloud, so there is no server history to keep. Retry on one row of a refused `all` item re-queues the whole item (the cloud refuses an `all` post only when no branch can take it).
- Items carry the café name so another owner's kept queue on the same device is neither shown nor sent.

## Verification

**Commands:**
- `pnpm run check:strings`, `check:layers`, `check:protocol`, `check:secrets`, `format:check`; in `apps/owner`: `pnpm run check` and `pnpm exec playwright test` -- expected: clean (390 tests)

## Auto Run Result

Status: done (code review passed, no blocking findings). Implemented the on-device command queue, the last-known offline preview, queued rows with Retry / Resend, expiry at 7 days and the keep-or-delete choice at sign out. Verified as listed above. No Rust changes.
