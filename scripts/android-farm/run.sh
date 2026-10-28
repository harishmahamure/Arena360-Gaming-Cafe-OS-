#!/usr/bin/env bash
# Story 4.17: one farm device, start to finish. Runs on the farm runner the device is attached to
# (adb), on the same Wi-Fi network as the device and with the gateway's captures available.
#
#   scripts/android-farm/run.sh <serial> <device-ip> <debug-apk> <hub-seed-bin> <out-dir>
#
# <hub-seed-bin> is `arena-hub` built for the device's ABI with --features dev-fixtures; it only
# seeds the branch database inside the app's private directory (the APK must be a debug build).
# The app's own Hub (the foreground service) then serves it; kiosks are the fleet test on this
# runner, over Wi-Fi. Exit 1 when any budget is missed.
#
# FARM_GATEWAY_SYN / FARM_GATEWAY_DNS: files the lab gateway fills while the device is attached
# (tcpdump -n 'tcp[tcpflags] & tcp-syn != 0' and the dnsmasq query log); the egress check runs
# only when both are set. The lab network has no route to the internet.
set -euo pipefail

serial=$1 ip=$2 apk=$3 seed_bin=$4 out=$5
pkg=in.arena360.hub
health=7409 lan=7400
farm=deploy/android-farm/farm.json
minutes=$(node -p "require('./$farm').load.minutes")
doze_after=$(node -p "require('./$farm').doze.start_after_min")
doze_for=$(node -p "require('./$farm').doze.minutes")
adb_() { adb -s "$serial" "$@"; }
mkdir -p "$out"

echo "== install and seed"
adb_ uninstall "$pkg" >/dev/null 2>&1 || true
adb_ install -r "$apk"
adb_ push "$seed_bin" /data/local/tmp/arena-hub-seed
adb_ shell chmod 755 /data/local/tmp/arena-hub-seed
adb_ shell run-as "$pkg" mkdir -p files
adb_ shell run-as "$pkg" /data/local/tmp/arena-hub-seed dev seed --data-dir "/data/data/$pkg/files"
adb_ forward "tcp:$health" "tcp:$health"
adb_ shell dumpsys battery set ac 1 || true
adb_ shell svc power stayon true
adb_ shell monkey -p "$pkg" -c android.intent.category.LAUNCHER 1 >/dev/null
for _ in $(seq 1 60); do
  curl -fsS "http://127.0.0.1:$health/healthz" | grep -q '"role":"primary"' && break
  sleep 2
done
curl -fsS "http://127.0.0.1:$health/healthz" | grep -q '"role":"primary"'

echo "== load: ${minutes} min, Doze from minute ${doze_after} for ${doze_for} min"
adb_ shell dumpsys gfxinfo "$pkg" reset >/dev/null
load_start_ms=$(node -p "Date.now()")
export ARENA_FLEET_UI=$health ARENA_FLEET_LAN="$ip:$lan" FLEET_OUT="$out"
export FLEET_PHASE=load FLEET_SECONDS=$((minutes * 60))
export FLEET_TIMER_EVERY_S=$(node -p "require('./$farm').load.timer_probe_every_s")
export FLEET_TIMER_MINUTES=$(node -p "require('./$farm').load.timer_probe_minutes")
cargo test --locked --release -p arena-hub --features dev-fixtures --test android_fleet -- --nocapture \
  >"$out/load.log" 2>&1 &
load_pid=$!
sleep $((doze_after * 60))
doze_status=0
node scripts/android-farm.mjs doze "$serial" --health-port "$health" --minutes "$doze_for" \
  --load-start-ms "$load_start_ms" --out "$out/doze.json" || doze_status=$?
load_status=0
wait "$load_pid" || load_status=$?

echo "== frames and egress"
adb_ shell dumpsys gfxinfo "$pkg" framestats >"$out/gfxinfo.txt"
status=0
node scripts/android-farm.mjs frames "$out/gfxinfo.txt" || status=1
if [ -n "${FARM_GATEWAY_SYN:-}" ] && [ -n "${FARM_GATEWAY_DNS:-}" ]; then
  node scripts/android-farm.mjs egress "$FARM_GATEWAY_SYN" "$FARM_GATEWAY_DNS" || status=1
fi

echo "== kill during the run's end state, restart, verify"
node scripts/android-farm.mjs kill "$serial"
for _ in $(seq 1 60); do
  curl -fsS "http://127.0.0.1:$health/healthz" 2>/dev/null | grep -q '"role":"primary"' && break
  sleep 2
done
FLEET_PHASE=verify cargo test --locked --release -p arena-hub --features dev-fixtures \
  --test android_fleet -- --nocapture >"$out/restart.log" 2>&1 || status=1

echo "== verdict"
node scripts/android-farm.mjs verify "$out/report.json" "$out/restart.json" "$out/doze.json" || status=1
[ "$doze_status" -eq 0 ] || status=1
[ "$load_status" -eq 0 ] || status=1
adb_ forward --remove "tcp:$health" || true
exit "$status"
