# Autonomous loop: how to resume

Read this file after a context flush (`/clear`). It's the single entry point for picking the loop back up.

## Standing instructions from the user

- Develop every epic in `sprint-status.yaml` in order (1 → 14), one local commit per story.
- Take the recommended option at every decision and log it in `autonomous-decision-log.md`. Don't stop for spec approval.
- Never push or deploy, and never run anything against GitHub.
- Flush context after each story. Everything the next story needs lives in files.
- (2026-10-24) The user asked to continue from 2.2 and complete all of Epic 2 in one run.
- (2026-10-24) Sentry and the OpenTelemetry/Grafana export (Story 2.3's observability ACs) are moved to the **end of Epic 2**: build them after 2.31, before closing the epic. Until then, logs stay JSON on stdout with no PII.

- **(2026-10-24) Speed-ups chosen by the user:**
  - **Overlap stories:** when story N's implementation returns, commit it to master as `wip: story N`. Then create a worktree for N+1 from that commit (`git worktree add .claude/worktrees/<key> -b story-<key> HEAD`) and dispatch N+1's implementer there, while N goes through review and fixes on master.
  - When N is final, squash its WIP and fixes into one `feat:` commit (`git reset --soft` to before the WIP), then `git rebase --onto <new N commit> <old WIP commit> story-<N+1>` and fast-forward master when N+1 is done.
  - Reserve migration numbers and error codes per story in its spec to avoid conflicts.
  - **Risk-based review:** auth, security, crypto, payments and data-integrity stories get all three reviewers. UI-only or config-only stories get the Verification Gap reviewer plus one Blind Hunter.
  - **Reviewers run on Sonnet** (`model: sonnet`). Implementers stay on the session model.

## Where things stand

- **Story order and status:** `_bmad-output/implementation-artifacts/sprint-status.yaml`. A story at `review` is committed; `in-progress` is mid-pipeline.
- **Specs:** `_bmad-output/implementation-artifacts/spec-<story-key>.md`. Frontmatter `status` is `draft`, `in-progress`, `in-review` or `done`, and `baseline_commit` is set when work starts. Specs for later Epic 1 stories are pre-drafted.
- **Decisions:** `autonomous-decision-log.md`. Deferred items: `deferred-work.md`.
- **Epic context:** `epic-<N>-context.md`, compiled by the `compile-epic-context` step for each new epic.

## Per-story pipeline (bmad-build workflow, `_bmad/render/bmad-build/.../step-0*.md`)

1. Draft the spec if it's missing. Self-approve it and log the decisions.
2. Set `status: in-progress`, `baseline_commit` = HEAD, and the sprint status to `in-progress`.
3. Have one implementation subagent run the spec.
4. Verify:
   - `cargo test --workspace --locked`, plus `-p arena-hub` and `-p arena-api` with `--features dev-fixtures`;
   - clippy with dev-fixtures;
   - every `pnpm run check:*` and `test:scripts`;
   - the Hub `test:shell` and gallery `test:a11y`;
   - app builds excluding marketing (local Node is 22.22.3; Astro needs 22.23.4).
5. Write the diff to `target/review/diff-<story>.patch` (gitignored), excluding lockfiles, `_bmad-output/implementation-artifacts` and `packages/ui/src/generated`.
6. Run three reviewers in parallel: Blind Hunter, Edge Case Hunter (with the spec as claims_file) and Verification Gap.
7. Triage. Write the Review Triage Log into the spec, send patch items back to the same implementation subagent, and add deferrals to `deferred-work.md`.
8. Verify again. Set the spec to `done` and the sprint status to `review`, append the outcome to the decision log, and commit with only that story's files (exclude the other untracked pre-drafted specs). Then run `pnpm check:generated` after the commit.

## Blocked stories (skip; need a human)

- **1-19 (kiosk escape suite):** safety checks stop automated implementation. Don't retry it in the loop; continue with the next story.

## Resume prompt (after /clear)

> Continue the autonomous BMAD loop: read `_bmad-output/implementation-artifacts/autonomous-loop-state.md` and pick up from the first story not at `review` in sprint-status.yaml, skipping the blocked stories listed in that file.
