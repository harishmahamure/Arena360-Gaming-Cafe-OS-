#!/usr/bin/env bash
# Unattended BMAD sprint loop: pick a story, build it, review it, commit, push. Repeat.
# Usage: scripts/bmad-sprint-runner.sh [max-stories]
# Env:   BMAD_MODEL BMAD_EFFORT BMAD_STORY_TIMEOUT BMAD_REVIEW BMAD_LIMIT_POLL BMAD_PUSH BMAD_REMOTE
set -euo pipefail

MAX_STORIES=${1:-25}
MODEL=${BMAD_MODEL:-sonnet}
EFFORT=${BMAD_EFFORT:-high}
STORY_TIMEOUT=${BMAD_STORY_TIMEOUT:-5400}
REVIEW=${BMAD_REVIEW:-1}
PUSH=${BMAD_PUSH:-1}
REMOTE=${BMAD_REMOTE:-origin}
LIMIT_POLL=${BMAD_LIMIT_POLL:-900}
COUNT=0
FAILED=0
LOG_DIR=".bmad-logs"
LIMIT_RE='credit balance|out of (extra )?usage|usage limit|rate limit|limit reached|insufficient credits'
mkdir -p "$LOG_DIR"

if [ -e ".git" ] && ! grep -qxF ".bmad-logs/" .git/info/exclude 2>/dev/null; then
  echo ".bmad-logs/" >> .git/info/exclude
fi

BRANCH=$(git rev-parse --abbrev-ref HEAD)
if ! git remote get-url "$REMOTE" >/dev/null 2>&1; then
  echo "⚠️  Remote '$REMOTE' not found; disabling push."
  PUSH=0
fi

RUN_TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
MASTER_LOG="$LOG_DIR/sprint_${RUN_TIMESTAMP}.log"
# Symlinks need Developer Mode on Windows; fall back to copying the log after every write.
LATEST_IS_COPY=0
ln -sf "sprint_${RUN_TIMESTAMP}.log" "$LOG_DIR/latest.log" 2>/dev/null || LATEST_IS_COPY=1
sync_latest() { [ "$LATEST_IS_COPY" = "1" ] && cp "$MASTER_LOG" "$LOG_DIR/latest.log" 2>/dev/null || true; }

log() { echo "$@" | tee -a "$MASTER_LOG"; sync_latest; }

# Pretty-print claude's stream-json. Perl (with core JSON::PP) ships with Git Bash; python does not.
format_stream() {
  perl -MJSON::PP -ne '
    BEGIN { $| = 1; binmode STDOUT, ":utf8"; }
    my $e = eval { decode_json($_) };
    unless (ref $e) { print; next }
    my $t = $e->{type} // "";
    if ($t eq "result") { print(($e->{result} // ""), "\n"); next }
    next unless $t eq "assistant";
    for my $b (@{ $e->{message}{content} // [] }) {
      my $bt = $b->{type} // "";
      if ($bt eq "text") { print $b->{text}, "\n" }
      elsif ($bt eq "tool_use") { print "\x{1F527} $b->{name} ", substr(encode_json($b->{input} // {}), 0, 160), "\n" }
    }'
}

run_claude() {
  perl -e 'alarm shift; exec @ARGV' "$STORY_TIMEOUT" \
    claude --dangerously-skip-permissions --verbose --output-format stream-json --model "$MODEL" --effort "$EFFORT" -p "$1" 2>&1 \
    | format_stream | tee "$2" | tee -a "$MASTER_LOG"
  sync_latest
}

sprint_file() {
  find . -name sprint-status.yaml -not -path "*/node_modules/*" -not -path "*/.claude/*" -not -path "*/.agents/*" | head -1
}

# Throw away whatever a failed/aborted agent run left behind (tracked edits and new files).
reset_tree() {
  git checkout -- . 2>/dev/null || true
  git clean -fd -e .bmad-logs 2>/dev/null || true
}

# Push the current branch; on rejection, rebase onto the remote and retry.
push_changes() {
  [ "$PUSH" = "1" ] || return 0
  local attempt
  for attempt in 1 2 3; do
    if git push "$REMOTE" "HEAD:$BRANCH" >>"$MASTER_LOG" 2>&1; then
      log "⬆️  Pushed $BRANCH to $REMOTE ($(git rev-parse --short HEAD))."
      return 0
    fi
    log "⚠️  Push attempt $attempt failed; rebasing onto $REMOTE/$BRANCH and retrying..."
    git fetch -q "$REMOTE" "$BRANCH" 2>>"$MASTER_LOG" || true
    if ! git rebase -q "$REMOTE/$BRANCH" >>"$MASTER_LOG" 2>&1; then
      git rebase --abort 2>/dev/null || true
      log "❌ Rebase conflict; leaving commits local. Resolve by hand: git pull --rebase $REMOTE $BRANCH && git push"
      return 1
    fi
  done
  log "❌ Could not push after 3 attempts. Commits remain local on $BRANCH."
  return 1
}

# Record a story as blocked in sprint-status.yaml, commit it, push it. Must run AFTER reset_tree.
mark_blocked() {
  local f
  f=$(sprint_file)
  [ -n "$f" ] && [ -n "$1" ] || return 0
  perl -pi -e "s/^( *\Q$1\E:).*/\$1 blocked/" "$f"
  log "🚫 Marked $1 as blocked in $f"
  if ! git diff --quiet -- "$f"; then
    git add -- "$f" && git commit -q -m "chore(sprint): mark $1 blocked" && log "📝 Committed blocked status for $1"
    push_changes || true
  fi
}

# Commit anything the agent left uncommitted after reporting success.
commit_leftovers() {
  if [ -n "$(git status --porcelain)" ]; then
    log "📝 Agent left uncommitted changes; committing them as part of $1."
    git add -A && git commit -q -m "$2" || true
  fi
}

# Start from the remote tip so we never rebuild stories another machine already finished.
sync_repo() {
  [ "$PUSH" = "1" ] || return 0
  log "🔄 Syncing $BRANCH with $REMOTE..."
  if ! git pull --rebase --autostash -q "$REMOTE" "$BRANCH" >>"$MASTER_LOG" 2>&1; then
    git rebase --abort 2>/dev/null || true
    log "⚠️  Could not fast-forward from $REMOTE/$BRANCH; continuing on local HEAD."
  fi
  log "📌 HEAD: $(git log --oneline -1)"
}

log "🚀 Starting Autonomous BMAD Sprint Runner ($MODEL / effort $EFFORT / timeout ${STORY_TIMEOUT}s / review=$REVIEW / push=$PUSH to $REMOTE/$BRANCH / limit-poll ${LIMIT_POLL}s)"
log "📂 Live logs saving to: $LOG_DIR/latest.log"

credits_ok() {
  local out rc
  set +e
  out=$(claude -p "Reply with exactly OK" --model "$MODEL" --max-turns 1 --output-format json 2>&1)
  rc=$?
  set -e
  [ $rc -eq 0 ] && ! echo "$out" | grep -qiE "$LIMIT_RE"
}

wait_for_credits() {
  until credits_ok; do
    log "⏳ $(date +"%H:%M:%S") No credits / rate limited. Retrying in $((LIMIT_POLL / 60)) min..."
    sleep "$LIMIT_POLL"
  done
  log "✅ Credits available, continuing."
}

log "🔎 Preflight: checking credits / rate limits..."
wait_for_credits
sync_repo
log "---------------------------------------------------------------------"

while [ $COUNT -lt $MAX_STORIES ]; do
  COUNT=$((COUNT + 1))
  STORY_LOG="$LOG_DIR/run_${RUN_TIMESTAMP}_story_${COUNT}.log"
  REVIEW_LOG="$LOG_DIR/run_${RUN_TIMESTAMP}_story_${COUNT}_review.log"

  log ""
  log "🔄 [Iteration $COUNT / $MAX_STORIES] Started at $(date +"%H:%M:%S")"
  log "📄 Story log: $STORY_LOG"

  set +e
  run_claude "
You are an autonomous BMAD Sprint Execution Agent running in an unattended loop.

Follow these exact steps in order:
1. INSPECT SPRINT: Read the sprint status file (sprint-status.yaml, usually under docs/ or the BMAD output folder; fall back to docs/stories/). A story is OPEN if its status is anything other than 'done' or 'blocked'. Pick ONE open story, preferring in this order: 'in-progress', 'review', 'ready-for-dev', then 'backlog'. Use the lowest-numbered story within the chosen status.
2. CHECK IF DONE: If there is no open story, output the exact string <SPRINT_COMPLETE> and exit immediately without making changes.
3. ANNOUNCE: Output a line of the exact form <STORY_ID:the-story-key-from-sprint-status> before changing anything.
4. PREPARE STORY: If the story is 'backlog' (no story file yet), run the bmad-create-story workflow to draft it and mark it 'ready-for-dev'.
5. BUILD STORY: Run the bmad-dev-story workflow for that story (read docs/ARCHITECTURE-SPINE.md first if present). Implement every task and acceptance criterion using Serena LSP and Context7. If the story is in 'review', address any open review findings in the story file instead.
6. VERIFY GATE: Run the project's typechecker, linter, and unit tests. Fix all failures before proceeding.
7. UPDATE SPRINT & COMMIT:
   - Check off all completed tasks in the story file and set its status to 'review'.
   - Update sprint-status.yaml so this story is 'review'.
   - Stage and commit all changes with message: 'feat(story): implement <story-id>'. Do NOT push; the runner pushes.
8. Output the exact string <STORY_DONE> as your final line.
" "$STORY_LOG"
  set -e

  STORY_ID=$(grep -oE "<STORY_ID:[^>]+>" "$STORY_LOG" | head -1 | sed -E 's/<STORY_ID:(.*)>/\1/')

  if grep -q "<SPRINT_COMPLETE>" "$STORY_LOG"; then
    log ""
    log "🎉 All open stories are COMPLETE ($FAILED blocked). Exiting loop cleanly."
    exit 0
  fi

  if grep -qiE "$LIMIT_RE" "$STORY_LOG"; then
    log ""
    log "⛔ Credits exhausted or rate limited mid-run. Will resume ${STORY_ID:-this story} when they reset."
    reset_tree
    COUNT=$((COUNT - 1))
    wait_for_credits
    continue
  fi

  if ! grep -q "<STORY_DONE>" "$STORY_LOG"; then
    FAILED=$((FAILED + 1))
    log ""
    log "⚠️  Story ${STORY_ID:-?} did not finish (timeout or no <STORY_DONE>). Log: $STORY_LOG"
    reset_tree
    mark_blocked "$STORY_ID"
    continue
  fi

  commit_leftovers "$STORY_ID" "feat(story): implement ${STORY_ID:-story}"
  push_changes || true

  if [ "$REVIEW" = "1" ]; then
    log "🧐 Reviewing $STORY_ID..."
    set +e
    run_claude "
You are an autonomous BMAD reviewer in an unattended loop. Story key: $STORY_ID.
1. Run the bmad-code-review workflow for this story (status 'review'). Read the story file and the diff of the last commit.
2. Fix every blocking finding directly in code, re-run the typechecker, linter and unit tests until green.
3. Set the story file status and sprint-status.yaml entry to 'done'. Commit with message: 'feat(story): complete $STORY_ID'. Do NOT push; the runner pushes.
4. Output the exact string <REVIEW_DONE> as your final line. If the implementation is fundamentally wrong and cannot be fixed, output <REVIEW_FAILED> instead and leave status as 'review'.
" "$REVIEW_LOG"
    set -e
    if grep -qiE "$LIMIT_RE" "$REVIEW_LOG"; then
      log "⛔ Credits exhausted during review of $STORY_ID. It stays in 'review' and will be picked up after reset."
      reset_tree
      COUNT=$((COUNT - 1))
      wait_for_credits
      continue
    fi
    if ! grep -q "<REVIEW_DONE>" "$REVIEW_LOG"; then
      FAILED=$((FAILED + 1))
      log "⚠️  Review of $STORY_ID failed. Marking blocked. Log: $REVIEW_LOG"
      reset_tree
      mark_blocked "$STORY_ID"
      continue
    fi
    commit_leftovers "$STORY_ID" "feat(story): complete ${STORY_ID:-story}"
    push_changes || true
  fi

  log "✅ Story $COUNT ($STORY_ID) finished, verified, committed and pushed. Cooling down 3s..."
  sleep 3
done

log "🛑 Reached safety limit of $MAX_STORIES stories ($FAILED blocked)."
