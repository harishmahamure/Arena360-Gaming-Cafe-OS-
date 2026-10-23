# Autonomous run: decision log

Started 2026-10-23, while the user was away. The instruction was: "continue developing stories in loop, take recommended decision wherever needed and keep log."

Each entry gives the story, the decision point, the choice taken (the recommended option) and why. Review these when you're back; anything here can be reversed.

## Story 1.5: shared base components meet the accessibility floor

- **Spec approval:** self-approved, as instructed. Spec: `spec-1-5-shared-base-components-meet-the-accessibility-floor.md`.
- **Gallery location:** a separate dev-only app, `apps/gallery`, rather than a hidden route inside the Hub. Why: the Hub bundle stays clean, and Playwright/axe can run against a static preview.
- **StatusBadge themes:** dark and HC only. Why: the 1.4 decision keeps Hub-only tokens out of light.
- **TabControl URL sync:** the component reports the selected tab through `onchange`; the page updates the URL. Why: it keeps the component router-agnostic.

## Scope update (user, 2026-10-23)

- "Complete all of epics, one commit after stories." Scope is now every epic in `sprint-status.yaml` (1–14), in order, with one local commit per story. Still no pushes, deploys or remote operations.

## Story 1.6: Hub service starts as the single writer of its branch database

- **Spec approval:** self-approved. Spec: `spec-1-6-hub-service-starts-as-the-single-writer-of-its-branch-databa.md`.
- **Healthz port:** loopback `127.0.0.1:7409`, overridable with `--health-port`. Why: `/healthz` is loopback-only, so it doesn't break the "no inbound ports beyond 7400/7401" rule, and 7409 stays next to the Arena range.
- **Follower takeover:** none automatic. A follower stays a follower even after the lease expires. Why: promotion is manual and epoch-fenced (AD-11), and it arrives in a later story.
- **Data dir:** `%PROGRAMDATA%\Arena360\Hub` on Windows and the XDG data dir elsewhere, overridable with `--data-dir` or `ARENA_HUB_DATA_DIR`.
- **Writer job API:** a closure over the transaction, as a stand-in until Story 1.7 replaces it with `apply()`.
- **Crash test:** a re-invoked child process is killed with `Child::kill` right after it commits, so the test works the same on Linux and Windows.
- **Log ring:** daily JSONL files; files older than 14 days are deleted, then the oldest until the total is ≤ 50 MB.
- **Review process:** to save context over 417 stories, I check each diff by running the verification commands and reading the `--stat`, instead of reading every line of large diffs. The three independent reviewers still read the full diff.
- **1.5 review outcome:** 14 fixes and 10 rejections. Notable fix: disabled Buttons use `aria-disabled` and stay focusable, so the reason can be reached from the keyboard. AlertBadge keeps an empty live region mounted at count 0, so going from 0 to 1 is still announced.

## Story 1.7: apply() and the gap-free change log

- **Spec approval:** self-approved. Spec: `spec-1-7-every-change-goes-through-apply-into-a-gap-free-change-log-s.md`.
- **Permission step before Story 1.11:** `branch.bootstrap` requires `actor.kind == system`; any other actor gets `FORBIDDEN`. Story 1.11 replaces this with the permission catalogue, so the Permission enum stays empty for now.
- **New error codes:** `VALIDATION_FAILED` (with `field`) and `ALREADY_BOOTSTRAPPED`.
- **Dev seed values:** "Dev Branch" / DEV01 / INR / Asia/Kolkata / business day starts 06:00 (360) / financial year starts in April. These are India defaults that match the product's target market.
- **Default settings seeded:** `idle_lock_minutes=5`, `hub.locale=en-IN`, `receipt.footer=""`. Later stories add more.
- **Currency validation:** a small const ISO 4217 table (INR, USD, EUR, GBP, AED, SGD and similar), to be widened later.
- **Dev seed lease:** `dev seed` takes the lease one-shot, the same way as the Hub. If a live primary holds the lease, it fails with `NOT_PRIMARY`.
- **business_day/fy stamping:** deferred to Story 1.12 (locale and timezone). The stamp step sets rev and timestamps only.
- **1.6 implementation deviations, accepted:**
  - An OS file lock on `branch.db.lock` gives one writer per data dir. Two processes on one dir share a `hub_instance_id`, so the lease row alone can't tell them apart.
  - Taking another instance's expired lease bumps the counter, `(counter+1, own_id)`, so a paused old primary sees a higher epoch and demotes.
  - A fresh lease held by another instance means follower, even if its epoch is lower.
  - New error code `INTERNAL` for storage and I/O failures.
- **1.6 review outcome:** 21 fixes and 6 rejections. Key fixes: a primary now demotes when its own lease expires; the writer lock is taken before any migration or write; a job panic is reported as INTERNAL instead of NOT_PRIMARY; SIGTERM is handled; a single day's log now counts toward the 50 MB cap.
