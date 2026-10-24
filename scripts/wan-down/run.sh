#!/usr/bin/env bash
# The WAN-down integration run (Story 1.22). Linux with passwordless sudo, strace, curl and the
# Playwright Chromium installed (`pnpm --filter @arena/hub exec playwright install chromium`).
#
# 1. Builds the release binaries (dev fixtures) and the Hub UI bundle (skip with --no-build).
# 2. "up" pass, network up: a fresh dev-seeded Hub, a console kiosk, the e2e driver (every
#    Epic 1 flow once) and the Hub UI Playwright smoke; then the normalised change table.
# 3. "down" pass, the same inside a network namespace with only `lo`: the Hub, the kiosk, the
#    driver and the Hub UI smoke (vite preview and the browser) run under
#    `strace -f -e trace=connect`. The Hub must report `internet_offline`.
# 4. Asserts: every flow passed (the driver and the smoke exit 0), both tables are equal,
#    `seq` is gap-free, no intent inserted a row twice and the ledger matches (in
#    `arena-e2e check`), no connect() left loopback except the probe's endpoints from the
#    Hub's own trace (every other trace must stay on loopback; the UI's may also make
#    Chromium's built-in IPv6 reachability check, a UDP connect to [2001:4860:4860::8888]:443
#    that fails with ENETUNREACH here and sends nothing, CHROMIUM_IPV6_PROBE in
#    check-trace.mjs), and the Hub logged no errors.
# 5. With --soak-hours <h> (CI: 0.05), runs scripts/wan-down/soak.sh after it.
#
# Not covered here: Windows-only enforcement (the real lockdown and process killing) is out of
# a Linux job's reach; the Story 1.19 escape suite on Windows covers it. The smoke also checks
# its own requests (`page.on('request')`).
#
# usage: scripts/wan-down/run.sh [--no-build] [--soak-hours <h>]

set -euo pipefail
# shellcheck source=scripts/wan-down/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

BUILD=1
SOAK_HOURS=""
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --soak-hours)
      SOAK_HOURS="${2:?--soak-hours needs a value}"
      shift
      ;;
    *) die "unknown argument $1" ;;
  esac
  shift
done

[ "$(uname -s)" = Linux ] || die "needs Linux (network namespaces and strace)"
for tool in strace curl ip getent node pnpm; do
  command -v "$tool" >/dev/null || die "needs $tool"
done
sudo -n true 2>/dev/null || die "needs passwordless sudo"

cleanup() {
  stop_all "$OUT/up" 2>/dev/null || true
  stop_all "$OUT/down" 2>/dev/null || true
  ns_destroy 2>/dev/null || true
}
trap cleanup EXIT

[ "$BUILD" = 1 ] && build
rm -rf "$OUT"
mkdir -p "$OUT"

# One pass: <mode> is "up" (network up, untraced) or "down" (namespace, traced).
pass() {
  local mode="$1" dir="$OUT/$1" trace=""
  [ "$mode" = down ] && trace=1
  mkdir -p "$dir"
  start_hub "$mode" "$dir" "$trace"
  start_kiosk "$mode" "$dir" "$trace"
  local prefix=()
  [ -n "$trace" ] && prefix=(strace -f -qq -e trace=connect -o "$dir/trace.driver.log")
  log "[$mode] running the e2e driver"
  in_pass "$mode" "${prefix[@]}" "$E2E" run --hub-data "$dir/hub" --kiosk-data "$dir/kiosk" \
    --health-port "$HEALTH_PORT" --iterations 1 --out "$dir/result.json" ||
    die "[$mode] the e2e driver failed (logs in $dir)"
  log "[$mode] running the Hub UI smoke"
  local wan=0
  [ "$mode" = down ] && wan=1
  local ui_prefix=()
  [ -n "$trace" ] && ui_prefix=(strace -f -qq -e trace=connect -o "$dir/trace.ui.log")
  (cd "$ROOT/apps/hub" && in_pass "$mode" "${ui_prefix[@]}" env "ARENA_WAN_DOWN=$wan" \
    pnpm exec playwright test -c playwright.wan-down.config.ts) ||
    die "[$mode] the Hub UI smoke failed"
  healthz "$mode" >"$dir/healthz.end.json" || die "[$mode] /healthz did not answer at the end"
  log "[$mode] connectivity at the end: $(grep -o '"connectivity":{[^}]*}' "$dir/healthz.end.json")"
  if [ "$mode" = down ]; then
    grep -q '"state":"internet_offline"' "$dir/healthz.end.json" ||
      die "[down] the Hub did not report internet_offline"
  fi
  stop_all "$dir"
  "$E2E" check --hub-data "$dir/hub" --result "$dir/result.json" --table "$dir/table.jsonl" ||
    die "[$mode] the change log check failed"
  check_hub_log "$dir"
}

pass up
ns_create
pass down
ns_destroy

log "comparing the normalised change tables (up vs down)"
"$E2E" compare "$OUT/up/table.jsonl" "$OUT/down/table.jsonl" ||
  die "the WAN-down results differ from the WAN-up ones"

log "checking the connect() traces"
mapfile -t ALLOW < <(probe_allow_args)
node "$ROOT/scripts/wan-down/check-trace.mjs" "${ALLOW[@]}" "$OUT"/down/trace.*.log ||
  die "a component connected outside loopback"

if [ -n "$SOAK_HOURS" ]; then
  trap - EXIT
  cleanup
  "$ROOT/scripts/wan-down/soak.sh" --no-build --hours "$SOAK_HOURS"
fi

log "PASS"
