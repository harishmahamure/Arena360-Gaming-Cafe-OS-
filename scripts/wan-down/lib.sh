# Shared helpers for scripts/wan-down/{run.sh,soak.sh} (Story 1.22). Sourced, not run.
#
# A pass starts a fresh dev-seeded Hub, provisions and starts a console-mode kiosk service
# (with the dev fake process table), then runs the e2e driver against both. "down" passes run
# every process inside the network namespace $NS, which has only `lo` (no route out).

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${WAN_DOWN_OUT:-$ROOT/target/wan-down}"
NS="${WAN_DOWN_NS:-arena-wan}"
BIN="${WAN_DOWN_BIN:-$ROOT/target/release}"
HUB="$BIN/arena-hub"
KIOSK="$BIN/arena-kiosk-svc"
E2E="$BIN/arena-e2e"
HEALTH_PORT=7409
LAN_PORT=7400
# The probe's endpoints, passed explicitly so the trace allow-list cannot drift from them.
PROBE_INTERNET="1.1.1.1:443,8.8.8.8:443"
CLOUD_HOST="api.arena360.cloud:443"
# The veth pair and subnet that reconnect the namespace (soak restore only).
VETH_HOST="arena-wan-h"
VETH_NS="arena-wan-n"
VETH_NET="10.213.0"

log() { printf 'wan-down: %s\n' "$*" >&2; }
die() {
  log "FAILED: $*"
  exit 1
}

# Builds the release binaries (dev fixtures on) and the Hub UI bundle.
build() {
  log "building release binaries (dev-fixtures) and the Hub UI"
  (cd "$ROOT" && cargo build --release --locked -p arena-hub -p arena-kiosk-svc -p arena-e2e \
    --features arena-hub/dev-fixtures,arena-kiosk-svc/dev-fixtures)
  (cd "$ROOT" && pnpm --filter @arena/hub build)
}

# Runs a command as the invoking user, inside the namespace for a "down" pass.
# usage: in_pass <mode> <cmd...>
in_pass() {
  local mode="$1"
  shift
  if [ "$mode" = down ]; then
    sudo ip netns exec "$NS" sudo -E -u "$(id -un)" env "PATH=$PATH" "HOME=$HOME" "$@"
  else
    "$@"
  fi
}

ns_create() {
  ns_destroy
  log "creating network namespace $NS (lo only, no route out)"
  sudo ip netns add "$NS"
  sudo ip -n "$NS" link set lo up
  # Name lookups inside go to loopback (nothing answers there), and only through files and
  # DNS: `ip netns exec` bind-mounts these over /etc, so no NSS module can reach the host's
  # resolver through a local socket.
  sudo mkdir -p "/etc/netns/$NS"
  echo "nameserver 127.0.0.1" | sudo tee "/etc/netns/$NS/resolv.conf" >/dev/null
  if [ -f /etc/nsswitch.conf ]; then
    sed 's/^hosts:.*/hosts: files dns/' /etc/nsswitch.conf |
      sudo tee "/etc/netns/$NS/nsswitch.conf" >/dev/null
  fi
}

ns_destroy() {
  ns_disconnect
  sudo ip netns del "$NS" 2>/dev/null || true
  sudo rm -rf "/etc/netns/$NS"
}

# Gives the namespace a route out through a veth pair and NAT (the soak's restore).
ns_connect() {
  log "restoring the network: veth $VETH_HOST <-> $VETH_NS, NAT to the host"
  sudo ip link add "$VETH_HOST" type veth peer name "$VETH_NS"
  sudo ip link set "$VETH_NS" netns "$NS"
  sudo ip addr add "$VETH_NET.1/24" dev "$VETH_HOST"
  sudo ip link set "$VETH_HOST" up
  sudo ip -n "$NS" addr add "$VETH_NET.2/24" dev "$VETH_NS"
  sudo ip -n "$NS" link set "$VETH_NS" up
  sudo ip -n "$NS" route add default via "$VETH_NET.1"
  sudo sysctl -qw net.ipv4.ip_forward=1
  sudo iptables -t nat -A POSTROUTING -s "$VETH_NET.0/24" -j MASQUERADE
  sudo iptables -I FORWARD -i "$VETH_HOST" -j ACCEPT
  sudo iptables -I FORWARD -o "$VETH_HOST" -j ACCEPT
}

ns_disconnect() {
  if ip link show "$VETH_HOST" >/dev/null 2>&1; then
    sudo iptables -t nat -D POSTROUTING -s "$VETH_NET.0/24" -j MASQUERADE 2>/dev/null || true
    sudo iptables -D FORWARD -i "$VETH_HOST" -j ACCEPT 2>/dev/null || true
    sudo iptables -D FORWARD -o "$VETH_HOST" -j ACCEPT 2>/dev/null || true
    sudo ip link del "$VETH_HOST" 2>/dev/null || true
  fi
}

# The pids of the process named <name> whose command line mentions <dir>.
pids_of() {
  local name="$1" dir="$2" pid
  for pid in $(pgrep -x "$name" 2>/dev/null || true); do
    if tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null | grep -qF -- "$dir"; then
      echo "$pid"
    fi
  done
}

# Sends <signal> to <name> for <dir> and waits up to 15 s for it to exit.
stop_proc() {
  local name="$1" dir="$2" sig="$3" pids i
  pids="$(pids_of "$name" "$dir")"
  [ -n "$pids" ] || return 0
  # shellcheck disable=SC2086
  kill "-$sig" $pids 2>/dev/null || true
  for i in $(seq 1 150); do
    [ -n "$(pids_of "$name" "$dir")" ] || return 0
    sleep 0.1
  done
  log "$name did not stop on SIG$sig; killing it"
  # shellcheck disable=SC2086
  kill -KILL $pids 2>/dev/null || true
}

# GET /healthz from inside the pass.
healthz() {
  in_pass "$1" curl -sf --max-time 2 "http://127.0.0.1:$HEALTH_PORT/healthz"
}

# The value after <prefix> on the line of <file> that has it.
after() {
  grep -F -- "$2" "$1" | head -n1 | sed "s/.*$2//" | tr -d '[:space:]'
}

# Starts the Hub for <mode> in <dir>: dev seed first (a one-shot writer on the fresh data dir),
# then the service. <trace> (optional) runs it under strace.
start_hub() {
  local mode="$1" dir="$2" trace="${3:-}"
  mkdir -p "$dir/hub"
  log "[$mode] dev seed"
  in_pass "$mode" "$HUB" dev seed --data-dir "$dir/hub" >"$dir/seed.out" 2>&1 ||
    die "dev seed: $(cat "$dir/seed.out")"
  local prefix=()
  [ -n "$trace" ] && prefix=(strace -f -qq -e trace=connect -o "$dir/trace.hub.log")
  log "[$mode] starting the Hub"
  in_pass "$mode" "${prefix[@]}" "$HUB" --data-dir "$dir/hub" \
    --cloud-host "$CLOUD_HOST" --probe-internet "$PROBE_INTERNET" >"$dir/hub.out" 2>&1 &
  local i body
  for i in $(seq 1 300); do
    body="$(healthz "$mode" 2>/dev/null || true)"
    if printf '%s' "$body" | grep -q '"role":"primary"'; then
      printf '%s' "$body" >"$dir/healthz.start.json"
      return 0
    fi
    sleep 0.1
  done
  die "[$mode] the Hub never became primary: $(cat "$dir/hub.out")"
}

# Provisions and starts the console kiosk for <mode> in <dir> against the Hub.
start_kiosk() {
  local mode="$1" dir="$2" trace="${3:-}"
  local token fingerprint
  token="$(after "$dir/seed.out" "device token (shown once): ")"
  fingerprint="$(grep -o '"tls_fingerprint":"[0-9a-f]*"' "$dir/healthz.start.json" | cut -d'"' -f4)"
  [ -n "$token" ] && [ -n "$fingerprint" ] || die "[$mode] no device token or fingerprint"
  in_pass "$mode" "$KIOSK" dev provision --hub "127.0.0.1:$LAN_PORT" --fingerprint "$fingerprint" \
    --token "$token" --data-dir "$dir/kiosk" >"$dir/provision.out" 2>&1 ||
    die "[$mode] dev provision: $(cat "$dir/provision.out")"
  local prefix=()
  [ -n "$trace" ] && prefix=(strace -f -qq -e trace=connect -o "$dir/trace.kiosk.log")
  log "[$mode] starting the kiosk service (console, fake process table)"
  in_pass "$mode" "${prefix[@]}" "$KIOSK" --console --dev-fake-procs --data-dir "$dir/kiosk" \
    >"$dir/kiosk.out" 2>&1 &
}

stop_all() {
  local dir="$1"
  stop_proc arena-kiosk-svc "$dir/kiosk" INT
  stop_proc arena-hub "$dir/hub" TERM
}

# Fails when the Hub's log ring has an error line.
check_hub_log() {
  local dir="$1"
  if grep -rh '"level":"ERROR"' "$dir/hub/logs" >"$dir/hub-errors.log" 2>/dev/null; then
    die "the Hub logged errors: $(head -n 5 "$dir/hub-errors.log")"
  fi
  log "the Hub log has no error lines"
}

# The probe's allow-list for check-trace: the internet endpoints, plus whatever the cloud
# host resolves to on this machine (inside the namespace the lookup goes to loopback and
# fails, so normally the probe never connects to the cloud there; these only keep a lookup
# that did succeed from counting the probe's own connection as a violation).
probe_allow_args() {
  local ep host addr
  IFS=',' read -ra eps <<<"$PROBE_INTERNET"
  for ep in "${eps[@]}"; do printf -- '--allow\n%s\n' "$ep"; done
  host="${CLOUD_HOST%:*}"
  for addr in $(getent ahosts "$host" 2>/dev/null | awk '{print $1}' | sort -u); do
    if [[ "$addr" == *:* ]]; then
      printf -- '--allow\n[%s]:%s\n' "$addr" "${CLOUD_HOST##*:}"
    else
      printf -- '--allow\n%s:%s\n' "$addr" "${CLOUD_HOST##*:}"
    fi
  done
}
