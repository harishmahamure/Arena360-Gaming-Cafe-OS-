#!/usr/bin/env bash
# The long WAN outage (Story 1.22): the Hub and a console kiosk run inside a network namespace
# with no route out while the e2e driver loops every Epic 1 flow for <h> hours (CI: 0.05 = 3
# minutes; release bench: 8). Then the network is restored (a veth pair and NAT into the
# namespace, with the processes still running) and it asserts:
#
# - the Hub's probe sees the internet again (`/healthz` leaves `internet_offline`);
# - `seq` is gap-free and no change row was lost or duplicated (applied intents per kind
#   versus the driver's ledger, `arena-e2e check`);
# - the Hub's log has no error lines.
#
# Linux with passwordless sudo, curl and iptables.
#
# usage: scripts/wan-down/soak.sh --hours <h> [--no-build]

set -euo pipefail
# shellcheck source=scripts/wan-down/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

HOURS=""
BUILD=1
while [ $# -gt 0 ]; do
  case "$1" in
    --hours)
      HOURS="${2:?--hours needs a value}"
      shift
      ;;
    --no-build) BUILD=0 ;;
    *) die "unknown argument $1" ;;
  esac
  shift
done
[ -n "$HOURS" ] || die "usage: soak.sh --hours <h> [--no-build]"
[ "$(uname -s)" = Linux ] || die "needs Linux (network namespaces)"
for tool in curl ip iptables; do
  command -v "$tool" >/dev/null || die "needs $tool"
done
sudo -n true 2>/dev/null || die "needs passwordless sudo"

DIR="$OUT/soak"
cleanup() {
  stop_all "$DIR" 2>/dev/null || true
  ns_destroy 2>/dev/null || true
}
trap cleanup EXIT

[ "$BUILD" = 1 ] && build
rm -rf "$DIR"
mkdir -p "$DIR"
ns_create
start_hub down "$DIR"
start_kiosk down "$DIR"

log "soak: looping the flows for $HOURS h with the WAN down"
in_pass down "$E2E" run --hub-data "$DIR/hub" --kiosk-data "$DIR/kiosk" \
  --health-port "$HEALTH_PORT" --hours "$HOURS" --out "$DIR/result.json" ||
  die "soak: the e2e driver failed (logs in $DIR)"
log "soak: $(grep -o '"iterations": [0-9]*' "$DIR/result.json")"

ns_connect
log "soak: waiting for the probe to see the internet (at most 40 s)"
seen=""
for _ in $(seq 1 80); do
  body="$(healthz down 2>/dev/null || true)"
  if printf '%s' "$body" | grep -q '"connectivity":{"state":"\(online\|cloud_unreachable\)"'; then
    seen="$(printf '%s' "$body" | grep -o '"connectivity":{[^}]*}')"
    break
  fi
  sleep 0.5
done
[ -n "$seen" ] || die "soak: the probe still reports the internet offline after the restore"
log "soak: after the restore $seen"

stop_all "$DIR"
"$E2E" check --hub-data "$DIR/hub" --result "$DIR/result.json" ||
  die "soak: the change log check failed"
check_hub_log "$DIR"
log "soak: PASS"
