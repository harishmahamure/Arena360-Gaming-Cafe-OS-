#!/usr/bin/env bash
# Unattended BMAD sprint loop: pick a story, build it, review it, commit, push. Repeat until the sprint is done.
# Usage: scripts/bmad-sprint-runner.sh [max-stories]   (0 or omitted = no limit, run until <SPRINT_COMPLETE>)
# Env:   BMAD_CLAUDE_BIN (path to the claude binary) BMAD_MODEL BMAD_EFFORT BMAD_STORY_TIMEOUT BMAD_REVIEW BMAD_LIMIT_POLL BMAD_PUSH BMAD_REMOTE
#        BMAD_STORY_RETRIES  (0 = retry a story forever; N = mark it blocked after N failed attempts)
#        BMAD_RETRY_COOLDOWN (seconds to wait before retrying a failed attempt, default 30)
#
# Console / master log show only status lines (running / reviewing / completed / failed / blocked, with the
# story id). The agent's full output goes to .bmad-logs/run_<ts>_story_<n>[_review].log (raw stream: *.raw.jsonl).
#
# Design rules (learned the hard way):
#  * Never `set -e`: a single failing grep used to kill the whole loop silently. Every failure is handled explicitly.
#  * Never read claude through a pipe. On Windows the claude process (or a child that inherited its stdout, e.g. an
#    MCP server or a detached test run) can linger after the final `result` event, so `claude | tee` never saw EOF
#    and the runner sat after <STORY_DONE> without ever reviewing or starting the next story. claude writes to the
#    raw log; the runner polls that file for the `result` event (or the timeout) and then kills the process tree.
#    (`perl -e 'alarm; exec'` did not help: on Windows exec replaces perl, so the alarm never fired.)
#  * Quota / rate limits are detected from claude's structured `result` event and stderr, not from the agent's prose.
#    When hit, the runner keeps the half-finished work in the tree, waits for the quota to reset, and resumes the SAME story.
#  * A story is never skipped or marked blocked because the agent stopped early or hallucinated a marker; it is retried.
set -uo pipefail
export GIT_TERMINAL_PROMPT=0   # a push that needs a credential prompt must fail, not hang the loop

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
CHILD_PID=""   # the running claude process, killed on exit
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

log() { echo "$(date +"%H:%M:%S") $*" | tee -a "$MASTER_LOG"; sync_latest; }

# kill_tree PID: kill a process and every descendant. Walks the MSYS process table (children first), then
# taskkill //T on Windows for native descendants (claude.exe, node, python MCP servers), then the pid itself.
kill_tree() {
  local pid=${1:-} kid winpid
  [ -n "$pid" ] || return 0
  for kid in $(ps -ef 2>/dev/null | awk -v p="$pid" '$3 == p { print $2 }'); do
    kill_tree "$kid"
  done
  if command -v taskkill >/dev/null 2>&1; then
    winpid=$(ps -p "$pid" 2>/dev/null | awk -v p="$pid" '$1 == p { print $4 }')
    [ -n "$winpid" ] && taskkill //F //T //PID "$winpid" >/dev/null 2>&1
  fi
  kill -9 "$pid" 2>/dev/null
  return 0
}
# Whatever makes the runner exit, say so in the log instead of vanishing.
on_exit() {
  local rc=$?
  kill_tree "$CHILD_PID"
  log "🛑 exited (code $rc): $COUNT completed, $BLOCKED blocked, $ITER agent runs"
}
trap on_exit EXIT
trap 'log "🛑 interrupted; uncommitted work stays in the tree, rerun to resume"; exit 130' INT TERM

# Pretty-print claude's stream-json into the per-run story log. Perl (with core JSON::PP) ships with Git Bash.
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

# has_result RAW_LOG -> 0 once the stream contains claude's final top-level `result` event.
has_result() {
  tail -n 20 "$1" 2>/dev/null | perl -MJSON::PP -ne '
    my $e = eval { decode_json($_) }; next unless ref $e;
    $found = 1, last if ($e->{type} // "") eq "result";
    END { exit($found ? 0 : 1) }'
}

# run_claude PROMPT PRETTY_LOG RAW_LOG -> 0 when claude produced a result event, 1 on timeout / early death.
# Nothing from the agent reaches the console; see the header comment for why claude is not piped.
run_claude() {
  local prompt=$1 pretty=$2 raw=$3 start now status=1
  : > "$raw"
  "$CLAUDE_BIN" --dangerously-skip-permissions --verbose --output-format stream-json --model "$MODEL" --effort "$EFFORT" -p "$prompt" >"$raw" 2>&1 </dev/null &
  CHILD_PID=$!
  start=$(date +%s)
  while kill -0 "$CHILD_PID" 2>/dev/null; do
    if has_result "$raw"; then
      sleep 5   # let the stream flush; claude may still exit on its own
      status=0
      break
    fi
    now=$(date +%s)
    if [ $((now - start)) -ge "$STORY_TIMEOUT" ]; then
      log "⏱️  agent run exceeded ${STORY_TIMEOUT}s; killing it"
      break
    fi
    sleep 5
  done
  kill_tree "$CHILD_PID"
  wait "$CHILD_PID" 2>/dev/null
  CHILD_PID=""
  format_stream < "$raw" > "$pretty" 2>/dev/null
  has_result "$raw" && status=0
  return "$status"
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
    LIMIT_MSG=$(echo "$errs" | grep -aiE "$LIMIT_RE" | head -1 | cut -c1-200)
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

# first_story_with_status STATUS... -> key of the first story entry whose status is one of the given, in that order.
first_story_with_status() {
  local f st id
  f=$(sprint_file)
  [ -n "$f" ] || { echo ""; return; }
  for st in "$@"; do
    id=$(grep -E "^ +[0-9]+-[0-9]+-[^:]+: *$st[[:space:]]*$" "$f" | head -1 | sed -E 's/^ +([^:]+):.*/\1/')
    if [ -n "$id" ]; then echo "$id"; return; fi
  done
  echo ""
}

# The story the next build run will pick, mirroring the agent's own selection order.
next_story_id() {
  first_story_with_status in-progress review ready-for-dev backlog
}

# resolve_story_id PRETTY_LOG HEAD_BEFORE: marker in the log, else the commit message, else sprint-status.yaml.
resolve_story_id() {
  local id
  id=$(grep -oE "<STORY_ID: *[^>]+>" "$1" 2>/dev/null | head -1 | sed -E 's/<STORY_ID: *([^> ]*) *>/\1/')
  if [ -n "$id" ]; then echo "$id"; return; fi
  id=$(git log --format=%s "$2..HEAD" 2>/dev/null | sed -nE 's/^feat\(story\): (implement|complete) ([^ ]+).*/\2/p' | head -1)
  if [ -n "$id" ]; then echo "$id"; return; fi
  # Last resort: the story the agent was told to pick (same order as the build prompt), so a run that
  # never announced itself can still be retried or blocked instead of looping for ever.
  next_story_id
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
      return 0
    fi
    git fetch -q "$REMOTE" "$BRANCH" 2>>"$MASTER_LOG" || true
    if ! git rebase -q "$REMOTE/$BRANCH" >>"$MASTER_LOG" 2>&1; then
      git rebase --abort 2>/dev/null || true
      log "❌ push failed (rebase conflict); commits stay local: git pull --rebase $REMOTE $BRANCH && git push"
      return 1
    fi
  done
  log "❌ push failed after 3 attempts; commits stay local on $BRANCH"
  return 1
}

# Record a story as blocked in sprint-status.yaml, commit it, push it. Must run AFTER reset_tree.
mark_blocked() {
  local f
  f=$(sprint_file)
  [ -n "$f" ] && [ -n "$1" ] || return 0
  perl -pi -e "s/^( *\Q$1\E:).*/\$1 blocked/" "$f"
  BLOCKED=$((BLOCKED + 1))
  log "🚫 $1 blocked"
  if ! git diff --quiet -- "$f"; then
    git add -- "$f" && git commit -q -m "chore(sprint): mark $1 blocked"
    push_changes || true
  fi
}

# Commit anything the agent left uncommitted after reporting success.
commit_leftovers() {
  if [ -n "$(git status --porcelain)" ]; then
    git add -A && git commit -q -m "$2" || true
  fi
}

# Start from the remote tip so we never rebuild stories another machine already finished.
sync_repo() {
  [ "$PUSH" = "1" ] || return 0
  if ! git pull --rebase --autostash -q "$REMOTE" "$BRANCH" >>"$MASTER_LOG" 2>&1; then
    git rebase --abort 2>/dev/null || true
    log "⚠️  could not sync from $REMOTE/$BRANCH; continuing on local HEAD"
  fi
}

credits_ok() {
  local out rc
  out=$("$CLAUDE_BIN" -p "Reply with exactly OK" --model "$MODEL" --max-turns 1 --output-format json 2>&1 </dev/null)
  rc=$?
  [ $rc -eq 0 ] && ! echo "$out" | grep -aqiE "$LIMIT_RE"
}

# Block until a trivial claude call succeeds. Loops forever; the sprint resumes the moment the quota resets.
wait_for_credits() {
  local n=0
  until credits_ok; do
    n=$((n + 1))
    log "⏳ quota / rate limited (check #$n); retrying in $((LIMIT_POLL / 60)) min"
    sleep "$LIMIT_POLL"
  done
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
7. VERIFY GATE: Run the project's typechecker, linter, and unit tests. Fix all failures before proceeding. Never leave background processes running when you finish.
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
2. Fix every blocking finding directly in code, re-run the typechecker, linter and unit tests until green. Never leave background processes running when you finish.
3. Set the story file status and sprint-status.yaml entry to 'done'. Commit with message: 'feat(story): complete $1'. Do NOT push; the runner pushes.
4. Output the exact string <REVIEW_DONE> as your final line. If the implementation is fundamentally wrong and cannot be fixed, write the findings into the story file, commit that, output <REVIEW_FAILED> instead and leave status as 'review'.
PROMPT
}

RETRIES_LABEL=unlimited; [ "$STORY_RETRIES" -gt 0 ] && RETRIES_LABEL=$STORY_RETRIES
MAX_LABEL=unlimited;     [ "$MAX_STORIES" -gt 0 ] && MAX_LABEL=$MAX_STORIES
log "🚀 sprint runner started ($MODEL / effort $EFFORT / timeout ${STORY_TIMEOUT}s / review=$REVIEW / push=$PUSH / retries $RETRIES_LABEL / max-stories $MAX_LABEL)"
log "📂 agent logs: $LOG_DIR/run_${RUN_TIMESTAMP}_story_<n>.log"
wait_for_credits
sync_repo

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
  RETRY_NOTE=""; [ "$STORY_ATTEMPT" -gt 0 ] && RETRY_NOTE=" (retry #$STORY_ATTEMPT)"

  if [ "$(open_stories)" -eq 0 ]; then
    log "🎉 sprint complete: all stories done ($BLOCKED blocked)"
    exit 0
  fi
  log "▶️  running $(next_story_id)$RETRY_NOTE  [$(open_stories) open]"

  run_claude "$(build_prompt)" "$STORY_LOG" "$STORY_RAW"
  CLAUDE_RC=$?
  STORY_ID=$(resolve_story_id "$STORY_LOG" "$HEAD_BEFORE")

  # ---- Quota / rate limit: keep the work, wait, resume the same story. --------------------------------
  if hit_limit "$STORY_RAW"; then
    log "⛔ ${STORY_ID:-story} paused: ${LIMIT_MSG}"
    wait_for_credits
    continue
  fi

  # ---- Sprint complete: trust it only if sprint-status.yaml agrees. ----------------------------------
  if has_marker SPRINT_COMPLETE "$STORY_LOG" && [ "$(open_stories)" -eq 0 ]; then
    log "🎉 sprint complete: all stories done ($BLOCKED blocked)"
    exit 0
  fi

  # ---- Agent stopped early / timed out / hallucinated: retry the same story, never skip it. ------------
  if ! has_marker STORY_DONE "$STORY_LOG"; then
    STORY_ATTEMPT=$((STORY_ATTEMPT + 1))
    log "⚠️  ${STORY_ID:-?} failed (attempt $STORY_ATTEMPT, exit $CLAUDE_RC); see $STORY_LOG"
    if retries_left "$STORY_ATTEMPT"; then
      sleep "$RETRY_COOLDOWN"
      continue
    fi
    reset_tree
    mark_blocked "$STORY_ID"
    STORY_ATTEMPT=0; REVIEW_ATTEMPT=0
    continue
  fi

  commit_leftovers "$STORY_ID" "feat(story): implement ${STORY_ID:-story}"
  push_changes || true

  # ---- Review loop: retry on limit or early stop; <REVIEW_FAILED> sends the story back to build. -------
  if [ "$REVIEW" = "1" ] && [ -n "$STORY_ID" ]; then
    while :; do
      REVIEW_NOTE=""; [ "$REVIEW_ATTEMPT" -gt 0 ] && REVIEW_NOTE=" (retry #$REVIEW_ATTEMPT)"
      log "🧐 reviewing $STORY_ID$REVIEW_NOTE"
      run_claude "$(review_prompt "$STORY_ID")" "$REVIEW_LOG" "$REVIEW_RAW"
      CLAUDE_RC=$?
      if hit_limit "$REVIEW_RAW"; then
        log "⛔ $STORY_ID review paused: ${LIMIT_MSG}"
        wait_for_credits
        continue
      fi
      if has_marker REVIEW_DONE "$REVIEW_LOG"; then
        commit_leftovers "$STORY_ID" "feat(story): complete ${STORY_ID}"
        push_changes || true
        break
      fi
      if has_marker REVIEW_FAILED "$REVIEW_LOG"; then
        log "↩️  $STORY_ID sent back to build (review findings recorded)"
        commit_leftovers "$STORY_ID" "chore(story): review findings for ${STORY_ID}"
        push_changes || true
        break
      fi
      REVIEW_ATTEMPT=$((REVIEW_ATTEMPT + 1))
      log "⚠️  $STORY_ID review failed (attempt $REVIEW_ATTEMPT, exit $CLAUDE_RC); see $REVIEW_LOG"
      if retries_left "$REVIEW_ATTEMPT"; then
        sleep "$RETRY_COOLDOWN"
        continue
      fi
      reset_tree
      mark_blocked "$STORY_ID"
      break
    done
  fi

  COUNT=$((COUNT + 1))
  STORY_ATTEMPT=0; REVIEW_ATTEMPT=0
  log "✅ completed ${STORY_ID:-?}  [$COUNT done this run, $(open_stories) open]"
  sleep 3
done

log "🏁 reached the story limit of $MAX_STORIES ($BLOCKED blocked)"
