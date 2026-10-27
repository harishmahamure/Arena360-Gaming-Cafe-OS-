#!/usr/bin/env bash
# Unattended BMAD sprint loop: pick a story, build it, review it, commit, push. Repeat until the sprint is done.
# Usage: scripts/bmad-sprint-runner.sh [max-stories]   (0 or omitted = no limit, run until <SPRINT_COMPLETE>)
# Env:   BMAD_CLAUDE_BIN (path to the claude binary) BMAD_MODEL BMAD_EFFORT BMAD_STORY_TIMEOUT BMAD_REVIEW BMAD_LIMIT_POLL BMAD_PUSH BMAD_REMOTE
#        BMAD_STORY_RETRIES  (0 = retry a story forever; N = mark it blocked after N failed attempts)
#        BMAD_RETRY_COOLDOWN (seconds to wait before retrying a failed attempt, default 30)
#
# Design rules (learned the hard way):
#  * Never `set -e`: a single failing grep used to kill the whole loop silently. Every failure is handled explicitly.
#  * Quota / rate limits are detected from claude's structured `result` event and stderr, not from the agent's prose.
#    When hit, the runner keeps the half-finished work in the tree, waits for the quota to reset, and resumes the SAME story.
#  * A story is never skipped or marked blocked because the agent stopped early or hallucinated a marker; it is retried.
set -uo pipefail

MAX_STORIES=${1:-0}
CLAUDE_BIN=${BMAD_CLAUDE_BIN:-claude}
MODEL=${BMAD_MODEL:-sonnet}
EFFORT=${BMAD_EFFORT:-high}
STORY_TIMEOUT=${BMAD_STORY_TIMEOUT:-5400}
REVIEW=${BMAD_REVIEW:-1}
PUSH=${BMAD_PUSH:-1}
REMOTE=${BMAD_REMOTE:-origin}
LIMIT_POLL=${BMAD_LIMIT_POLL:-900}
STORY_RETRIES=${BMAD_STORY_RETRIES:-0}
RETRY_COOLDOWN=${BMAD_RETRY_COOLDOWN:-30}
COUNT=0        # stories finished (done, or sent back to build after a failed review)
ITER=0         # agent runs started (used for log file names)
BLOCKED=0
LOG_DIR=".bmad-logs"
# Matched only against claude's error/result text and stderr, never against the agent's normal output.
LIMIT_RE='hit your (usage |rate )?limit|usage limit|rate.?limit|limit reached|credit balance|out of (extra )?usage|insufficient credits|too many requests|overloaded|resets? (at|in) [0-9]|try again (later|in)|\b(429|529)\b'
mkdir -p "$LOG_DIR"

if [ -e ".git" ] && ! grep -qxF ".bmad-logs/" .git/info/exclude 2>/dev/null; then
  echo ".bmad-logs/" >> .git/info/exclude
fi

BRANCH=$(git rev-parse --abbrev-ref HEAD)
if ! git remote get-url "$REMOTE" >/dev/null 2>&1; then
  echo "Remote '$REMOTE' not found; disabling push."
  PUSH=0
fi

RUN_TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
MASTER_LOG="$LOG_DIR/sprint_${RUN_TIMESTAMP}.log"
# Symlinks need Developer Mode on Windows; fall back to copying the log after every write.
LATEST_IS_COPY=0
ln -sf "sprint_${RUN_TIMESTAMP}.log" "$LOG_DIR/latest.log" 2>/dev/null || LATEST_IS_COPY=1
sync_latest() { [ "$LATEST_IS_COPY" = "1" ] && cp "$MASTER_LOG" "$LOG_DIR/latest.log" 2>/dev/null || true; }

log() { echo "$@" | tee -a "$MASTER_LOG"; sync_latest; }

# Whatever makes the runner exit, say so in the log instead of vanishing.
on_exit() {
  local rc=$?
  log "🛑 Runner exiting (code $rc) at $(date +"%H:%M:%S"): $COUNT stories finished, $BLOCKED blocked, $ITER agent runs."
}
trap on_exit EXIT
trap 'log "🛑 Interrupted. Uncommitted work is left in the tree; rerun the script to resume the same story."; exit 130' INT TERM

# Pretty-print claude's stream-json. Perl (with core JSON::PP) ships with Git Bash; python does not.
format_stream() {
  perl -MJSON::PP -ne '
    BEGIN { $| = 1; binmode STDOUT, ":utf8"; }
    my $e = eval { decode_json($_) };
    unless (ref $e) { print; next }
    my $t = $e->{type} // "";
    if ($t eq "result") {
      my $r = $e->{result} // "";
      $r = encode_json($r) if ref $r;
      print $r, "\n";
      print "\x{26D4} claude result: is_error=1 subtype=", ($e->{subtype} // ""), "\n" if $e->{is_error};
      next;
    }
    next unless $t eq "assistant";
    for my $b (@{ $e->{message}{content} // [] }) {
      my $bt = $b->{type} // "";
      if ($bt eq "text") { print $b->{text}, "\n" }
      elsif ($bt eq "tool_use") { print "\x{1F527} $b->{name} ", substr(encode_json($b->{input} // {}), 0, 160), "\n" }
    }'
}

# run_claude PROMPT PRETTY_LOG RAW_LOG  -> exit status of claude
run_claude() {
  perl -e 'alarm shift; exec @ARGV' "$STORY_TIMEOUT" \
    "$CLAUDE_BIN" --dangerously-skip-permissions --verbose --output-format stream-json --model "$MODEL" --effort "$EFFORT" -p "$1" 2>&1 \
    | tee "$3" | format_stream | tee "$2" | tee -a "$MASTER_LOG"
  local rc=${PIPESTATUS[0]}
  sync_latest
  return "$rc"
}

# Error-ish text from a raw stream-json log: the final result event when it is an error, plus any non-JSON (stderr) lines.
stream_errors() {
  [ -f "$1" ] || return 0
  perl -MJSON::PP -ne '
    my $e = eval { decode_json($_) };
    unless (ref $e) { print; next }
    next unless ($e->{type} // "") eq "result";
    my $r = $e->{result} // $e->{error} // "";
    $r = encode_json($r) if ref $r;
    $r =~ s/\s+/ /g;
    print "RESULT is_error=", ($e->{is_error} ? 1 : 0), " subtype=", ($e->{subtype} // ""), " $r\n"
      if $e->{is_error} || (($e->{subtype} // "success") ne "success");
  ' "$1" 2>/dev/null
}

# hit_limit RAW_LOG -> 0 when the run died on quota / rate limit / overload. Sets LIMIT_MSG.
hit_limit() {
  local errs
  errs=$(stream_errors "$1")
  if echo "$errs" | grep -aqiE "$LIMIT_RE"; then
    LIMIT_MSG=$(echo "$errs" | grep -aiE "$LIMIT_RE" | head -1 | cut -c1-300)
    return 0
  fi
  return 1
}

# has_marker NAME FILE -> 0 when the marker is on a line of its own (prose like "not emitting `<STORY_DONE>`" does not count).
has_marker() {
  grep -qE "^[[:space:]]*<$1>[[:space:]]*$" "$2" 2>/dev/null
}

sprint_file() {
  find . -name sprint-status.yaml -not -path "*/node_modules/*" -not -path "*/.claude/*" -not -path "*/.agents/*" 2>/dev/null | head -1
}

# Number of story entries (keys like 3-17-...) whose status is neither done nor blocked.
open_stories() {
  local f
  f=$(sprint_file)
  [ -n "$f" ] || { echo 0; return; }
  awk '{ sub(/[[:cntrl:]]+$/, "") } /^ +[0-9]+-[0-9]+-[^:]+: */ { s=$2; if (s != "done" && s != "blocked") n++ } END { print n+0 }' "$f"
}

# resolve_story_id PRETTY_LOG HEAD_BEFORE: marker in the log, else the commit message, else sprint-status.yaml.
resolve_story_id() {
  local id f st
  id=$(grep -oE "<STORY_ID: *[^>]+>" "$1" 2>/dev/null | head -1 | sed -E 's/<STORY_ID: *([^> ]*) *>/\1/')
  if [ -n "$id" ]; then echo "$id"; return; fi
  id=$(git log --format=%s "$2..HEAD" 2>/dev/null | sed -nE 's/^feat\(story\): (implement|complete) ([^ ]+).*/\2/p' | head -1)
  if [ -n "$id" ]; then echo "$id"; return; fi
  f=$(sprint_file)
  [ -n "$f" ] || { echo ""; return; }
  for st in in-progress review ready-for-dev; do
    id=$(grep -E "^ +[0-9]+-[0-9]+-[^:]+: *$st[[:space:]]*$" "$f" | head -1 | sed -E 's/^ +([^:]+):.*/\1/')
    if [ -n "$id" ]; then echo "$id"; return; fi
  done
  echo ""
}

# Throw away whatever an agent run left behind. Only used when a story is given up on (finite BMAD_STORY_RETRIES).
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
  BLOCKED=$((BLOCKED + 1))
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

credits_ok() {
  local out rc
  out=$("$CLAUDE_BIN" -p "Reply with exactly OK" --model "$MODEL" --max-turns 1 --output-format json 2>&1)
  rc=$?
  [ $rc -eq 0 ] && ! echo "$out" | grep -aqiE "$LIMIT_RE"
}

# Block until a trivial claude call succeeds. Loops forever; the sprint resumes the moment the quota resets.
wait_for_credits() {
  local n=0
  until credits_ok; do
    n=$((n + 1))
    log "⏳ $(date +"%H:%M:%S") Quota exhausted / rate limited (check #$n). Retrying in $((LIMIT_POLL / 60)) min..."
    sleep "$LIMIT_POLL"
  done
  log "✅ $(date +"%H:%M:%S") Credits available, continuing."
}

retries_left() {  # retries_left ATTEMPTS -> 0 when another attempt is allowed
  [ "$STORY_RETRIES" -eq 0 ] || [ "$1" -lt "$STORY_RETRIES" ]
}

build_prompt() {
  cat <<'PROMPT'
You are an autonomous BMAD Sprint Execution Agent running in an unattended loop.

Follow these exact steps in order:
1. INSPECT SPRINT: Read the sprint status file (sprint-status.yaml, usually under docs/ or the BMAD output folder; fall back to docs/stories/). A story is OPEN if its status is anything other than 'done' or 'blocked'. Pick ONE open story, preferring in this order: 'in-progress', 'review', 'ready-for-dev', then 'backlog'. Use the lowest-numbered story within the chosen status.
2. CHECK IF DONE: If there is no open story, output the exact string <SPRINT_COMPLETE> and exit immediately without making changes.
3. ANNOUNCE: You MUST output a line of the exact form <STORY_ID:the-story-key-from-sprint-status> (the key exactly as written in sprint-status.yaml, no spaces) before changing anything. The runner parses this line.
4. RESUME CHECK: Run 'git status'. If there are uncommitted changes, they are a previous interrupted attempt at this same story (the runner was stopped by a quota limit or timeout). Inspect them and CONTINUE from them; do NOT discard or revert them.
5. PREPARE STORY: If the story is 'backlog' (no story file yet), run the bmad-create-story workflow to draft it and mark it 'ready-for-dev'.
6. BUILD STORY: Run the bmad-dev-story workflow for that story (read docs/ARCHITECTURE-SPINE.md first if present). Implement every task and acceptance criterion using Serena LSP and Context7. If the story is in 'review', address any open review findings in the story file instead.
7. VERIFY GATE: Run the project's typechecker, linter, and unit tests. Fix all failures before proceeding.
8. UPDATE SPRINT & COMMIT:
   - Check off all completed tasks in the story file and set its status to 'review'.
   - Update sprint-status.yaml so this story is 'review'.
   - Stage and commit all changes with message: 'feat(story): implement <story-id>'. Do NOT push; the runner pushes.
9. Output the exact string <STORY_DONE> as your final line. Never output <STORY_DONE> or <SPRINT_COMPLETE> unless it is literally true.
PROMPT
}

review_prompt() {
  cat <<PROMPT
You are an autonomous BMAD reviewer in an unattended loop. Story key: $1.
1. Run the bmad-code-review workflow for this story (status 'review'). Read the story file and the diff of the last commit.
2. Fix every blocking finding directly in code, re-run the typechecker, linter and unit tests until green.
3. Set the story file status and sprint-status.yaml entry to 'done'. Commit with message: 'feat(story): complete $1'. Do NOT push; the runner pushes.
4. Output the exact string <REVIEW_DONE> as your final line. If the implementation is fundamentally wrong and cannot be fixed, write the findings into the story file, commit that, output <REVIEW_FAILED> instead and leave status as 'review'.
PROMPT
}

RETRIES_LABEL=unlimited; [ "$STORY_RETRIES" -gt 0 ] && RETRIES_LABEL=$STORY_RETRIES
MAX_LABEL=unlimited;     [ "$MAX_STORIES" -gt 0 ] && MAX_LABEL=$MAX_STORIES
log "🚀 Starting Autonomous BMAD Sprint Runner ($MODEL / effort $EFFORT / timeout ${STORY_TIMEOUT}s / review=$REVIEW / push=$PUSH to $REMOTE/$BRANCH / limit-poll ${LIMIT_POLL}s / story-retries $RETRIES_LABEL / max-stories $MAX_LABEL)"
log "📂 Live logs saving to: $LOG_DIR/latest.log"
log "🔎 Preflight: checking credits / rate limits..."
wait_for_credits
sync_repo
if [ -n "$(git status --porcelain)" ]; then
  log "📎 Uncommitted changes found from an earlier interrupted attempt; the agent will resume from them."
fi
log "---------------------------------------------------------------------"

STORY_ATTEMPT=0   # failed build attempts on the current story
REVIEW_ATTEMPT=0  # failed review attempts on the current story
LIMIT_MSG=""

while [ "$MAX_STORIES" -eq 0 ] || [ "$COUNT" -lt "$MAX_STORIES" ]; do
  ITER=$((ITER + 1))
  STORY_LOG="$LOG_DIR/run_${RUN_TIMESTAMP}_story_${ITER}.log"
  STORY_RAW="$LOG_DIR/run_${RUN_TIMESTAMP}_story_${ITER}.raw.jsonl"
  REVIEW_LOG="$LOG_DIR/run_${RUN_TIMESTAMP}_story_${ITER}_review.log"
  REVIEW_RAW="$LOG_DIR/run_${RUN_TIMESTAMP}_story_${ITER}_review.raw.jsonl"
  HEAD_BEFORE=$(git rev-parse HEAD)
  RETRY_NOTE=""; [ "$STORY_ATTEMPT" -gt 0 ] && RETRY_NOTE=" (retry #$STORY_ATTEMPT of current story)"

  log ""
  log "🔄 [Run $ITER | finished $COUNT / $MAX_LABEL | open stories: $(open_stories)] Started at $(date +"%H:%M:%S")$RETRY_NOTE"
  log "📄 Story log: $STORY_LOG"

  run_claude "$(build_prompt)" "$STORY_LOG" "$STORY_RAW"
  CLAUDE_RC=$?
  STORY_ID=$(resolve_story_id "$STORY_LOG" "$HEAD_BEFORE")

  # ---- Quota / rate limit: keep the work, wait, resume the same story. --------------------------------
  if hit_limit "$STORY_RAW"; then
    log ""
    log "⛔ $(date +"%H:%M:%S") Quota exhausted or rate limited mid-run: ${LIMIT_MSG}"
    log "   Keeping uncommitted work for ${STORY_ID:-the current story}; will resume it when the quota resets."
    wait_for_credits
    continue
  fi

  # ---- Sprint complete: trust it only if sprint-status.yaml agrees. ----------------------------------
  if has_marker SPRINT_COMPLETE "$STORY_LOG"; then
    OPEN=$(open_stories)
    if [ "$OPEN" -eq 0 ]; then
      log ""
      log "🎉 All stories are COMPLETE ($BLOCKED blocked). Exiting loop cleanly."
      exit 0
    fi
    log "⚠️  Agent claimed <SPRINT_COMPLETE> but $OPEN stories are still open in sprint-status.yaml. Ignoring and retrying."
  fi

  # ---- Agent stopped early / timed out / hallucinated: retry the same story, never skip it. ------------
  if ! has_marker STORY_DONE "$STORY_LOG"; then
    STORY_ATTEMPT=$((STORY_ATTEMPT + 1))
    log ""
    log "⚠️  Story ${STORY_ID:-?} did not finish (claude exit $CLAUDE_RC; timeout or no <STORY_DONE>). Attempt $STORY_ATTEMPT failed. Log: $STORY_LOG"
    ERRS=$(stream_errors "$STORY_RAW" | head -3 | cut -c1-300)
    [ -n "$ERRS" ] && log "   claude reported: $ERRS"
    if retries_left "$STORY_ATTEMPT"; then
      log "🔁 Retrying ${STORY_ID:-the story} in ${RETRY_COOLDOWN}s, continuing from the work left in the tree."
      sleep "$RETRY_COOLDOWN"
      continue
    fi
    log "🚫 Gave up on ${STORY_ID:-?} after $STORY_ATTEMPT attempts (BMAD_STORY_RETRIES=$STORY_RETRIES)."
    reset_tree
    mark_blocked "$STORY_ID"
    STORY_ATTEMPT=0; REVIEW_ATTEMPT=0
    continue
  fi

  commit_leftovers "$STORY_ID" "feat(story): implement ${STORY_ID:-story}"
  push_changes || true

  # ---- Review loop: retry on limit or early stop; <REVIEW_FAILED> sends the story back to build. -------
  if [ "$REVIEW" = "1" ]; then
    if [ -z "$STORY_ID" ]; then
      log "⚠️  Could not determine the story key for review; the next build run will pick up the 'review' story."
    else
      while :; do
        REVIEW_NOTE=""; [ "$REVIEW_ATTEMPT" -gt 0 ] && REVIEW_NOTE=" (retry #$REVIEW_ATTEMPT)"
        log "🧐 Reviewing $STORY_ID...$REVIEW_NOTE"
        run_claude "$(review_prompt "$STORY_ID")" "$REVIEW_LOG" "$REVIEW_RAW"
        CLAUDE_RC=$?
        if hit_limit "$REVIEW_RAW"; then
          log "⛔ $(date +"%H:%M:%S") Quota exhausted during review of $STORY_ID: ${LIMIT_MSG}"
          log "   Keeping review work in the tree; will resume the review when the quota resets."
          wait_for_credits
          continue
        fi
        if has_marker REVIEW_DONE "$REVIEW_LOG"; then
          commit_leftovers "$STORY_ID" "feat(story): complete ${STORY_ID}"
          push_changes || true
          break
        fi
        if has_marker REVIEW_FAILED "$REVIEW_LOG"; then
          log "↩️  Reviewer rejected $STORY_ID; it stays in 'review' and the next build run will address the findings."
          commit_leftovers "$STORY_ID" "chore(story): review findings for ${STORY_ID}"
          push_changes || true
          break
        fi
        REVIEW_ATTEMPT=$((REVIEW_ATTEMPT + 1))
        log "⚠️  Review of $STORY_ID did not finish (claude exit $CLAUDE_RC; no <REVIEW_DONE>). Attempt $REVIEW_ATTEMPT failed. Log: $REVIEW_LOG"
        if retries_left "$REVIEW_ATTEMPT"; then
          log "🔁 Retrying the review in ${RETRY_COOLDOWN}s."
          sleep "$RETRY_COOLDOWN"
          continue
        fi
        log "🚫 Gave up reviewing $STORY_ID after $REVIEW_ATTEMPT attempts (BMAD_STORY_RETRIES=$STORY_RETRIES)."
        reset_tree
        mark_blocked "$STORY_ID"
        break
      done
    fi
  fi

  COUNT=$((COUNT + 1))
  STORY_ATTEMPT=0; REVIEW_ATTEMPT=0
  log "✅ Story $COUNT (${STORY_ID:-?}) finished at $(date +"%H:%M:%S"); $(open_stories) stories still open. Cooling down 3s..."
  sleep 3
done

log "🏁 Reached the story limit of $MAX_STORIES ($BLOCKED blocked)."
