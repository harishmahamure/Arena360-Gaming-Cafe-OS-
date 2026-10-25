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

- **(2026-10-25) No code comments:** the user's global rule (`~/.claude/CLAUDE.md`). Every implementer dispatch and fix message adds: "Do not add code comments in anything you write or change (no line, block or doc comments; leave existing comments alone unless the code they describe changes)."

- **(2026-10-25) Two module tracks, requested by the user ("module by module … control plane, hub"):**
  - **Control-plane track:** 2.6 → 2.16, then 2.19 and 2.22 (worktrees `.claude/worktrees/<key>`, with the overlap rules above).
  - **Hub track:** 2.17 → 2.18 → 2.20 → 2.21 → 2.23 → 2.24 → 2.25 → 2.26 → 2.27, in sequence on worktree `.claude/worktrees/hub-<key>`, branched from current master. Hub migrations go in `crates/arena-store/migrations/` (the branch DB), control migrations in `control-migrations/`; they never collide.
  - **Joint stories:** 2.28–2.31 run after both tracks.
  - **Landing:** each finished story is rebased onto master and fast-forwarded, one `feat:` commit per story, with bookkeeping (sprint-status, decision log) amended in on master.
  - **Limits:** at most 3 implementers at once. Ports: the hub app's Playwright uses 5173 (the user's own dev server may hold it; then reuse or run against it, as in 1.20), onboarding uses 5179.

- **(2026-10-25) All modules in parallel, requested by the user ("ALL MODULES IN EPIC"):**
  - **Track A, control plane:** 2.6 → 2.16.
  - **Track B, Hub devices:** 2.17 → 2.18, then fan out 2.20 (→ 2.25), 2.21, 2.23 and 2.24 in parallel.
  - **Track C, Hub settings:** 2.26 and 2.27.
  - **Joint:** 2.19, 2.22 and 2.28–2.31 after A and B.
  - **Limits:** at most 4 implementers at once.
  - **Hub migration numbers** (the runner requires contiguous numbers): 2.26 added none, so 2.18 = 0011. Then, in landing order: 2.20 = 0012, 2.21 = 0013, 2.23 = 0014, 2.24 = 0015, 2.25 = 0016. A story that lands after a sibling renumbers to the next free number on rebase.
  - **Control migrations:** 2.6 = 0004, 2.7 = 0005, then sequential.

- **(2026-10-25) Playwright ports per worktree:**
  - Each app's preview binds `127.0.0.1` and honours `ARENA_PW_PORT`; the Hub tests take their origin from `apps/hub/tests/origin.ts`. This came from a chore commit.
  - Every verification run in a worktree sets a unique `ARENA_PW_PORT` (for example 54xx for Hub, 55xx for kiosk, 56xx for onboarding).
  - **Why:** parallel worktrees shared the fixed ports. One preview on `[::1]` and another on `127.0.0.1` both started, and `localhost` hit the wrong build, causing false failures.

- **(2026-10-25) STOP requested by the user:** "stop after current execution gets done".
  - Finish only what's in flight (2.9 verify and land, 2.18 rebase/verify/land, 2.10 implementation committed as WIP on `story-2-10` without review).
  - Then start nothing new. Resume only when the user asks.

- **(2026-10-25) Stopped as requested. Resume notes:**
  - **Landed on master (local, nothing pushed):** 2.1–2.9, 2.17, 2.18, 2.26, 2.27 (HEAD `67a1447`), plus chore `4a6da95`.
  - **2.10:** implemented, **not reviewed**. WIP commit on branch `story-2-10` in worktree `.claude/worktrees/2-10`, based on 2.9's old WIP. To resume:
    1. `git rebase --onto <master> <2.9 wip base> story-2-10`, then resolve:
       - **Hub migration clash:** 2.10's `0011_hub_identity.sql` must become `0012` (2.18 owns 0011).
       - **`PAIR_CODE_INVALID`:** added by both 2.10 and 2.18; keep one.
       - **`arena-proto` pairing types:** 2.18's moved to `kiosk_pairing.rs`, and its TS type is now `KioskPairingCode`.
    2. Run the three-reviewer pass on Sonnet, then fix, verify and land.
    3. Open question from the implementer: should pairing mark wizard step 3 done, or leave it for 2.11's heartbeat (the current behaviour)?
  - **Next specs drafted, untracked in `_bmad-output/implementation-artifacts`:** 2.11 (heartbeat) and 2.20 (device groups; Hub migration at the next free number). The Hub fan-out (2.20, 2.21, 2.23, 2.24) can start now that 2.18 has landed.

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
