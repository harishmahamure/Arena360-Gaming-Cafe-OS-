# Android Hub: Google Play release and staged rollout

Story 4.16. The Android Hub is released only through Google Play, to the Free cafés. The cloud is
always released first: the Hub upload is gated on it.

## What runs by itself

A `vX.Y.Z` tag deploys the cloud (`deploy-cloud`). When that run succeeds, `release-android.yml`:

1. checks that `https://api.arena360.cloud/healthz` answers (cloud release N is live),
2. runs `node scripts/play-release.mjs check` (the listing declarations against the build),
3. builds the **signed** AAB with the SOPS upload key (a missing key fails the job; a Play release
   is never built unsigned), verifies it (`jarsigner`, 16 KB alignment, ABIs, production ad ids),
4. uploads it to the Play **internal testing** track with the Play Developer API, using the service
   account in `deploy/secrets/play-publisher.enc.yaml` (key `service_account_json`). Play App
   Signing re-signs the build for users; the CI key is only the upload key.

Nothing goes to production by itself. To run it by hand (for example after a failed run) use
**Actions → Release Android Hub → Run workflow** with the tag, after the cloud release is live.

## Before production: the pre-launch report

Open the Play Console pre-launch report for the build. It may leave internal testing only when it
shows **no crashes and no accessibility errors** on the tested devices. Record it:

```json
{ "version_code": 1000123, "crashes": 0, "accessibility_errors": 0, "devices": 6 }
```

Save that as `prelaunch.json` (not committed). `promote` refuses without it.

## Staged rollout: 1% → 10% → 50% → 100% over a week

The stages and holds are `deploy/play/rollout.json` (1% for 24 h, 10% for 48 h, 50% for 72 h, then
100%: six days of holds inside a seven-day week).

```sh
export PLAY_SERVICE_ACCOUNT_JSON="$(sops --decrypt --extract '["service_account_json"]' deploy/secrets/play-publisher.enc.yaml)"
node scripts/play-release.mjs promote <versionCode> --percent 1 --prelaunch prelaunch.json
# a day later, if the halt check below says continue:
node scripts/play-release.mjs promote <versionCode> --percent 10 --prelaunch prelaunch.json --since <ISO time 1% began>
```

`promote` refuses a stage out of order, before the previous stage's hold has passed, and without a
clean pre-launch report.

## Halt rule

Before each widening, and whenever the Play Console alerts, compare the version's numbers with the
thresholds recorded in `deploy/play/rollout.json` (`halt_when`):

| Metric                                               | Halt above |
| ---------------------------------------------------- | ---------- |
| Play user-perceived crash rate for the version       | 1.0 %      |
| Play ANR rate for the version                        | 0.47 %     |
| Fleet heartbeat-gap: Free Hubs on the version whose last heartbeat is more than 3 intervals old | 2.0 %      |

```sh
node scripts/play-release.mjs decide --crash <pct> --anr <pct> --gap <pct>   # exit 2 = halt
node scripts/play-release.mjs halt <versionCode>
```

Halting stops further users from receiving the build; Hubs that already have it keep running. Ship
a fix as a new version and start again at 1%. Change the thresholds only in a reviewed commit, never
during a rollout.

## What a Hub does with an update

The Hub asks Play (Play in-app updates, flexible) at start, every 6 hours and when System → Hub
service opens. The download is in the background and the Hub keeps serving. When it is ready,
System shows **Update ready**; **Install and restart** is enabled only while no session at the
branch is active, paused or in grace. If Play installs an update by itself while sessions run, the
package replacement restarts the Hub service (`BootReceiver`, `MY_PACKAGE_REPLACED`) and kiosks
continue offline until it is back.

## One-time Play Console setup (kept in `deploy/play/listing.json`)

- **Contains ads: Yes.**
- **Data safety:** the types in `data_safety.collected`, matching the Google Mobile Ads SDK version
  pinned in the ads plugin. `play-release.mjs check` fails when the SDK version changes without the
  listing's `ad_sdk.version` (redo the form, then update that field).
- **Foreground service:** type `connectedDevice`, with the declaration text from the listing.
- **Device catalogue:** exclude devices with less than 3 GB RAM; minSdk 28 excludes Android 8.1 and
  earlier.
- The Play app, the service account (release manager role on this app only) and
  `deploy/secrets/play-publisher.enc.yaml` are operator steps (`deploy/README.md`).
