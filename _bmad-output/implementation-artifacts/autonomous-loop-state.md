# Autonomous loop: how to resume

Read this file after a context flush (`/clear`). It's the single entry point for picking the loop back up.

## Standing instructions from the user

- Develop every epic in `sprint-status.yaml` in order (1 → 14), one local commit per story.
- Take the recommended option at every decision and log it in `autonomous-decision-log.md`. Don't stop for spec approval.
- Never push or deploy, and never run anything against GitHub.
- Flush context after each story. Everything the next story needs lives in files.

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
