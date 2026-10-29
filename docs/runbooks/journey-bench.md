# Player journey bench (Story 6.38)

The per-release proof that the player experience holds on a cheap PC. It is part of
`.github/workflows/release-bench.yml` (manual run and release tags). The last job, `journey-report`,
merges every result with `scripts/journey-report.mjs` and fails the release when a budget is
missed **or a result is missing**: a rig that is down is a failed release, not a skipped one.

## What is measured

| Measure                                   | Budget   | Where it comes from                                                                                          |
| ----------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------ |
| Login                                     | < 3 s    | Hub leg (`player.authenticate` over the LAN) + the UI leg (submit to the plan picker)                          |
| Shell opens after approval                | < 2 s    | Hub leg (start accepted to the PC told) + the UI leg (`pc-screen` to the shell)                                |
| Extend reaches the floor                  | ≤ 2 s    | Hub leg (`player.extend` to the PC told) + the UI leg (confirm to "More time added. GG.")                      |
| An order from the kiosk is on the Hub tile | ≤ 3 s    | Hub leg (`shop.place` to the open `kiosk_order` alert)                                                         |
| PC returns to attract after clean-up      | yes      | UI leg (grace card, then no session, then attract)                                                             |
| Kiosk memory, idle on attract and in shell | < 80 MB  | `scripts/bench/kiosk-idle.ps1 -Phase attract` and `-Phase shell` (service + app + WebView2, game excluded)    |
| 30 minutes without the Hub                | on time  | `a_real_thirty_minute_hub_outage_never_locks_early_and_the_lock_lands_on_time` (never early, ≤ 1 s after zero) |
| Launcher logout, 20 sessions per Windows  | 0 left   | `scripts/launcher-residue.mjs` on Windows 10 and on Windows 11                                                 |

The Hub and UI legs also run, with the same budgets, in the ordinary test suites
(`the_hub_side_of_journey_3_meets_its_timings` in `arena-api`, `journey.spec.ts` in the kiosk
Playwright suite); the release jobs run them on the reference i3, where the numbers mean something.
Budgets can be overridden for a trial with `JOURNEY_LOGIN_MS`, `JOURNEY_APPROVE_MS`,
`JOURNEY_EXTEND_MS`, `JOURNEY_ORDER_MS`, `JOURNEY_IDLE_MB` and `JOURNEY_DOWN_SECS`.

## The rig

- **Reference i3** (`self-hosted, windows, reference-i3`): the 2015 i3 + HDD used by the other
  release jobs. Builds and runs the Hub, UI and outage jobs.
- **Kiosk-idle PC**: the same runner with the Kiosk installed and the console signed in as the
  player (see `scripts/bench/kiosk-idle.ps1`). To measure the shell, set the repository variable
  `JOURNEY_START_SESSION_CMD` to a PowerShell command that starts a session on this PC from a Hub
  on the LAN (for example a curl to the lab Hub's staff API). Without it the shell result is
  missing and the report fails.
- **Two launcher lab PCs**, labelled `lab-win10` and `lab-win11`, each with Steam, Epic, Riot,
  Discord, Chrome and Edge installed, the Kiosk installed and paired to a lab Hub, and a real
  account for every launcher. These accounts are the lab's own and are never shared.

## Repository variables

| Variable                    | Value                                                                                                  |
| --------------------------- | ------------------------------------------------------------------------------------------------------ |
| `JOURNEY_START_SESSION_CMD` | PowerShell that starts a session on the idle-measured PC                                               |
| `LAUNCHER_LOGIN_CMD`        | Signs every launcher and browser in with the lab accounts (and exits non-zero if one would not sign in) |
| `LAUNCHER_END_CMDS`         | JSON array, one command per way a session ends: staff end, time-up, "I'm done", forced end             |
| `LAUNCHER_PROFILE`          | The player's profile folder, e.g. `C:\Users\player`                                                    |
| `LAUNCHER_STEAM_DIR`        | The Steam install folder                                                                               |

`{n}` in a command is the session number. The 20 sessions use the end paths in turn, so every path
is run at least five times when four are listed.

## What the residue scan looks for

Run after every ended session, before the next login: Steam `config/loginusers.vdf`,
`config/config.vdf`, `ssfn*` and the HTML cache; Epic `GameUserSettings.ini` and `webcache*`; the Riot
Client `Data` folder; Discord's `discord`, `discordptb` and `discordcanary` folders; any non-empty
cookie, saved-login, web-data or history file in a Chrome, Edge or Firefox profile; and any saved
Windows credential (`cmdkey /list`) naming one of them. A residual is recorded as `session N: <what>`
with the file kind and never its contents. The paths mirror the sign-out of Story 6.12
(`crates/arena-kiosk-svc/src/cleanup/signout.rs`); change one and change the other.

## By hand

```
cargo test --release -p arena-api --test lan the_hub_side_of_journey_3 -- --nocapture
pnpm --filter @arena/kiosk exec playwright test journey.spec.ts
$env:ARENA_HUB_DOWN_SECS = 60   # a short trial; the release uses 1800
cargo test --release -p arena-kiosk-svc --lib a_real_thirty_minute_hub_outage -- --ignored --nocapture
node scripts/launcher-residue.mjs --once --profile C:\Users\player --steam "C:\Program Files (x86)\Steam"
node scripts/journey-report.mjs --dir target/journey
```
