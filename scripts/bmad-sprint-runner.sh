#!/usr/bin/env bash
set -euo pipefail

MAX_STORIES=${1:-25}
MODEL=${BMAD_MODEL:-sonnet}
EFFORT=${BMAD_EFFORT:-high}
STORY_TIMEOUT=${BMAD_STORY_TIMEOUT:-5400}
REVIEW=${BMAD_REVIEW:-1}
LIMIT_POLL=${BMAD_LIMIT_POLL:-900}
COUNT=0
FAILED=0
LOG_DIR=".bmad-logs"
LIMIT_RE='credit balance|out of (extra )?usage|usage limit|rate limit|limit reached|insufficient credits'
mkdir -p "$LOG_DIR"

if [ -e ".git" ] && ! grep -qxF ".bmad-logs/" .git/info/exclude 2>/dev/null; then
  echo ".bmad-logs/" >> .git/info/exclude
fi

RUN_TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
MASTER_LOG="$LOG_DIR/sprint_${RUN_TIMESTAMP}.log"
ln -sf "sprint_${RUN_TIMESTAMP}.log" "$LOG_DIR/latest.log"

log() { echo "$@" | tee -a "$MASTER_LOG"; }

run_claude() {
  perl -e 'alarm shift; exec @ARGV' "$STORY_TIMEOUT" \
    claude --dangerously-skip-permissions --verbose --output-format stream-json --model "$MODEL" --effort "$EFFORT" -p "$1" 2>&1 \
    | python3 -u -c '
import sys, json
for line in sys.stdin:
    try: e = json.loads(line)
    except ValueError: print(line, end=""); continue
    if e.get("type") == "result": print(e.get("result", "")); continue
    for b in e.get("message", {}).get("content", []) if e.get("type") == "assistant" else []:
        if b.get("type") == "text": print(b["text"])
        elif b.get("type") == "tool_use": print("🔧", b["name"], json.dumps(b["input"])[:160])
' | tee "$2" | tee -a "$MASTER_LOG"
}

mark_blocked() {
  local f
  f=$(find . -name sprint-status.yaml -not -path "*/node_modules/*" -not -path "*/.claude/*" -not -path "*/.agents/*" | head -1)
  [ -n "$f" ] && [ -n "$1" ] && sed -i '' -E "s/^( *$1:).*/\1 blocked/" "$f" && log "🚫 Marked $1 as blocked in $f"
}

log "🚀 Starting Autonomous BMAD Sprint Runner ($MODEL / effort $EFFORT / timeout ${STORY_TIMEOUT}s / review=$REVIEW / limit-poll ${LIMIT_POLL}s)"
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
   - Stage and commit all changes with message: 'feat(story): implement <story-id>'.
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
    git checkout -- . 2>/dev/null || true
    git clean -fd -e .bmad-logs 2>/dev/null || true
    COUNT=$((COUNT - 1))
    wait_for_credits
    continue
  fi

  if ! grep -q "<STORY_DONE>" "$STORY_LOG"; then
    FAILED=$((FAILED + 1))
    log ""
    log "⚠️  Story ${STORY_ID:-?} did not finish (timeout or no <STORY_DONE>). Log: $STORY_LOG"
    mark_blocked "$STORY_ID"
    git checkout -- . 2>/dev/null || true
    git clean -fd -e .bmad-logs 2>/dev/null || true
    continue
  fi

  if [ "$REVIEW" = "1" ]; then
    log "🧐 Reviewing $STORY_ID..."
    set +e
    run_claude "
You are an autonomous BMAD reviewer in an unattended loop. Story key: $STORY_ID.
1. Run the bmad-code-review workflow for this story (status 'review'). Read the story file and the diff of the last commit.
2. Fix every blocking finding directly in code, re-run the typechecker, linter and unit tests until green.
3. Set the story file status and sprint-status.yaml entry to 'done'. Commit with message: 'feat(story): complete $STORY_ID'.
4. Output the exact string <REVIEW_DONE> as your final line. If the implementation is fundamentally wrong and cannot be fixed, output <REVIEW_FAILED> instead and leave status as 'review'.
" "$REVIEW_LOG"
    set -e
    if grep -qiE "$LIMIT_RE" "$REVIEW_LOG"; then
      log "⛔ Credits exhausted during review of $STORY_ID. It stays in 'review' and will be picked up after reset."
      git checkout -- . 2>/dev/null || true
      COUNT=$((COUNT - 1))
      wait_for_credits
      continue
    fi
    if ! grep -q "<REVIEW_DONE>" "$REVIEW_LOG"; then
      FAILED=$((FAILED + 1))
      log "⚠️  Review of $STORY_ID failed. Marking blocked. Log: $REVIEW_LOG"
      mark_blocked "$STORY_ID"
      continue
    fi
  fi

  log "✅ Story $COUNT ($STORY_ID) finished, verified, and committed. Cooling down 3s..."
  sleep 3
done

log "🛑 Reached safety limit of $MAX_STORIES stories ($FAILED blocked)."
