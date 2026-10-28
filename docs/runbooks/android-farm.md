# Android Hub device-farm proof (Story 4.17)

Every release of the Android Hub is proven on two attached devices before it is promoted in Play
(see [android-play-rollout.md](android-play-rollout.md)): an **Android 9 device with 3 GB RAM**
(the cheapest supported device, minSdk 28) and one on the **latest Android**. The job is
`.github/workflows/android-farm.yml`; it runs on `v*` tags and by hand, and a release is not
promoted past internal testing while either device's run is red.

## What one device run does

`scripts/android-farm/run.sh` installs the debug APK, seeds the branch database inside the app's
private directory with the `arena-hub dev seed` tool, starts the app (its foreground service is the
Hub) and then, from the runner on the same Wi-Fi:

| Step | How | Fails the run when |
|------|-----|--------------------|
| 60-minute load | `crates/arena-hub/tests/android_fleet.rs`: 20 fake kiosks on pinned TLS, a Hub UI stream, start / pause / resume / extend / transfer / end / settle at 5/s, every 7th intent resent with its key | apply p95 > 100 ms, a floor or kiosk update > 2 s, an intent lost or delivered twice, a kiosk drops |
| Doze | `android-farm.mjs doze`: unplug, screen off, `deviceidle force-idle` for 30 min starting at minute 15; once a minute checks deep idle, the foreground service and `/healthz` | the Hub stops answering, the service is not foreground, the platform leaves Doze |
| Timers | a 2-minute wallet session on a reserved PC every 5 minutes; lateness is read the moment it leaves `active` | a time-up is over 5 s late, or fewer than 2 fire inside the Doze window |
| Ads | the same load runs with the banner and interstitial active; `dumpsys gfxinfo framestats` | any UI frame takes over 100 ms (`android-farm.mjs frames`) |
| No WAN | the lab network has no route out; the gateway's SYN capture and DNS log are checked | a connection other than the connectivity probe (`1.1.1.1:443`, `8.8.8.8:443`) or an ad domain |
| Kill | `am force-stop`, restart, then `FLEET_PHASE=verify` | an acknowledged intent is missing (resent with its key), the live sessions differ, a kiosk cannot sync, or a probe session in flight never times out |

`android-farm.mjs verify` reads `report.json`, `restart.json` and `doze.json` and checks them
against `deploy/android-farm/farm.json`.

## Budgets and the measurement hooks

The budgets are data in `deploy/android-farm/farm.json`, with floors in `scripts/android-farm.mjs`
that `pnpm run check:farm` enforces (a config that asks for less than the story fails). To time a
later operation on the 3 GB device (the daily rollup, NFR5), add one object under `budgets`
(for example `"daily_rollup": { "p95_ms": 5000 }`) and have the operation's test write
`{ p95_ms, max_ms, count }` under that name in the report's `ops`. Nothing else changes.

## Farm setup (once)

- A self-hosted runner per device, labelled `android-farm` and the device id (`android9-3gb`,
  `android-latest`), with `adb`, Node, Rust and `curl`, on the lab Wi-Fi. Repository variables
  `FARM_SERIAL_<id>` and `FARM_IP_<id>` (id with `-` as `_`) hold each device's adb serial and Wi-Fi
  address.
- The lab gateway has no route to the internet. It writes `tcpdump -n 'tcp[tcpflags] & tcp-syn != 0'`
  for the device's address and the `dnsmasq` query log to files; set the variables
  `FARM_GATEWAY_SYN` and `FARM_GATEWAY_DNS` to their paths on the runner. Without them the egress
  step is skipped, and the run says so.
- The device is not rooted; the APK is a debug build so `run-as` can seed its private directory.

## Run the same load without a device

`cargo test -p arena-hub --features dev-fixtures --test android_fleet` starts a local Hub with
`--platform android`, runs a 6-second load, kills it, restarts it and runs the verify phase. It runs
in CI on every push. `FLEET_SECONDS`, `FLEET_KIOSKS`, `FLEET_DEVICES`, `FLEET_RATE`,
`FLEET_TIMER_EVERY_S`, `FLEET_TIMER_MINUTES` and `FLEET_OUT` set the run; `ARENA_FLEET_UI` (a
forwarded health port) and `ARENA_FLEET_LAN` (`host:port`) point it at a Hub on a device.

## Reading a failure

Download the `android-farm-<device>` artifact: `load.log` and `restart.log` are the test output,
`report.json` the measurements, `gfxinfo.txt` the frame data, `doze.json` the Doze window and its
problems. A red run on the 3 GB device that is green on the latest one is the finding the job exists
to produce: do not promote.
