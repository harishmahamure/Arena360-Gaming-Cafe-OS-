---
title: 'Story 1.18: Kiosk process allow/block list'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '33c6f5d2503956e68cc72bbefb74d1d0cae917e4'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-17-kiosk-shell-replacement-and-windows-lockdown.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Even with the shell locked down, a player can launch tools (cmd, PowerShell, regedit, renamed copies) that break café rules or the lockdown, and nobody at the counter would know.

**Approach:** Store the process allow-list and block-list as branch settings. Push them to the kiosk service and cache them for offline use. The service watches process creation in the player session and kills disallowed processes within 200 ms, matching on the normalised path plus its Authenticode signature. It shows a persistent toast through the app and reports a `kiosk.process_blocked` intent. When offline, that intent goes through the 1.16 queue.

## Boundaries & Constraints

**Always:**
- **Settings (new migration, plus bootstrap seeding for new branches and an idempotent seed for an already-bootstrapped branch at Hub start through a system intent):**
  - `kiosk.process_allow` and `kiosk.process_block`: JSON arrays of rules `{path_glob, signer?}`. `path_glob` is matched case-insensitively against the normalised full path. `signer` is an optional Authenticode subject CN that must match a valid signature.
  - **Default allow:**
    - the kiosk app `{install}\arena-kiosk-ui.exe` and the service, both with signer = the Arena signer CN;
    - Windows essentials under `C:\Windows\System32\` — `conhost.exe`, `dllhost.exe`, `ctfmon.exe`, `fontdrvhost.exe`, `sihost.exe`, `RuntimeBroker.exe`, `smartscreen.exe`, `WerFault.exe`, `taskhostw.exe`, with signer "Microsoft Windows";
    - WebView2 `msedgewebview2.exe` under `C:\Program Files (x86)\Microsoft\EdgeWebView\Application\*\`, with signer "Microsoft Corporation".
  - **Default block (names, any path):** `cmd.exe`, `powershell.exe`, `pwsh.exe`, `regedit.exe`, `taskmgr.exe`, `mmc.exe`, `explorer.exe`, `control.exe`, `msconfig.exe`, `wscript.exe`, `cscript.exe`, `mshta.exe`.
  - The validator (1.8 `SETTING_RULES`) requires well-formed JSON and at most 500 rules.
  - Both settings are edited through `settings.set` (`settings.edit` permission).
- **Delivery:**
  - The kiosk service reads both settings after hello. A new LAN request `settings.get{keys}` is allowed in the paired scope for `kiosk.*` keys only.
  - It caches them in `queue.db` (a `kv` table) for offline use.
  - It applies a later `settings.changed` event, which pushes the `kiosk.*` key values to paired sessions, without a restart.
  - The 1.9 push-filter rule still holds: browser classes get nothing.
- **Decision (`arena-kiosk-core::procpolicy`, pure):** `decide(proc{path, signer?, valid_sig}, allow, block) -> Allow | Kill(reason)`.
  - A block match by file name or path glob kills.
  - Otherwise the process must match an allow rule whose path matches, and if the rule names a signer, `valid_sig && signer == rule.signer`.
  - Anything else kills.
  - A renamed blocked exe is killed because it isn't allowed. An allowed name at an unsigned or wrong path is killed for the same reason.
- **Enforcement (Windows, `arena-kiosk-svc`):**
  - Subscribe to process creation with WMI `Win32_ProcessStartTrace` through COM (`windows` crate), or ETW Kernel-Process, whichever reaches the 200 ms budget. Implementation Notes record the choice.
  - Filter to the player's session id, then resolve the path (`QueryFullProcessImageNameW`) and the signature (`WinVerifyTrust` plus signer CN from `CryptQueryObject`). Cache verification results by `(path, size, mtime)`.
  - Terminate (`TerminateProcess`) within 200 ms of the creation event; a debug log line records the elapsed time.
  - **At service start, and whenever the lists change:** enumerate existing player-session processes and kill the disallowed ones within 2 s.
  - Behind a `ProcessWatcher` / `ProcessKiller` trait pair, with fakes for tests.
- **Notification:**
  - After a kill, the service sends IPC `Toast{key: "player.app.blocked"}` to the app.
  - The app shows the catalogue line "That app isn't allowed on this PC." as a toast that persists until dismissed. It is non-focus-stealing: shown with `aria-live=polite`, focus never moves, and it is dismissed by its button.
  - Repeated kills collapse into the same toast with no stacking.
- **Audit:**
  - New intent `kiosk.process_blocked{exe_name, path, reason}`, requirement `Device`, paired scope. It writes into the 1.16 `device_events` table with `code=process_blocked`, through the 1.16 offline queue: a ULID key minted at kill time, applied exactly once.
  - The event appears in the Hub audit log (1.15) automatically, as a change row.
- **Tests:**
  - Pure `decide` for every matrix row.
  - Rule JSON validation.
  - Service flow with fakes: an event leads to a kill, a toast and a queued intent; offline then reconnect delivers it exactly once; a list change applies live; at start, already-running offenders are killed.
  - The paired `settings.get` scope only returns `kiosk.*` keys.
  - The kiosk UI toast through Playwright with a stubbed IPC event.

**Never:**
- Kernel drivers.
- Killing processes outside the player session.
- Killing based on the file name alone for allowed programs.
- Browser classes seeing these settings.
- A process GUI for staff (later; editing is through `settings.set`).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Blocked name | `C:\Windows\System32\cmd.exe` | Kill(blocked) | N/A |
| Renamed blocked | `C:\Users\player\Downloads\notepad.exe` (really cmd) | Kill(not allowed: path not in allow) | N/A |
| Allowed, signed | the kiosk exe at its install path, valid Arena signature | Allow | N/A |
| Allowed name, wrong path | `C:\Temp\arena-kiosk-ui.exe` | Kill(not allowed) | N/A |
| Allowed path, bad signature | the right path with an invalid signature | Kill(signature) | N/A |
| Unknown | `C:\Games\x.exe` | Kill(not allowed) | N/A |
| Live update | the allow-list gains `C:\Games\*.exe` | the next launch is allowed with no restart | N/A |
| Offline | killed while the Hub is down | the toast shows; the intent is queued and delivered once after reconnect | N/A |
| Startup sweep | a disallowed process already running at service start | killed within 2 s | N/A |
| Scope | a paired client asks for `settings.get` on `branch.currency` | `FORBIDDEN` (only `kiosk.*` keys) | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-kiosk-core` -- the 1.16 IPC types (add `Toast`) and 1.17's pure modules. Add `procpolicy.rs`.
- `crates/arena-kiosk-svc` -- the 1.16 queue.db, hub loop and traits. Add the watcher, killer and cache.
- `crates/arena-core/src/apply/*`, `settings_defaults.rs`/`SETTING_RULES` and the bootstrap seeding -- add the new settings and the `kiosk.process_blocked` intent.
- `crates/arena-proto/src/{intent.rs,scope.rs,frame.rs}` -- the paired scope and `settings.get`.
- `crates/arena-api/src/lan/{session.rs,registry.rs}` -- the request handling and the `settings.changed` push for paired sessions.
- `apps/kiosk/src/**` -- the toast UI.

## Tasks & Acceptance

**Execution:**
- [x] Proto, core and store: settings and seeding, the new intent, `settings.get` with scope and push, with tests.
- [x] kiosk-core `procpolicy` with tests; kiosk-svc watcher, killer, cache and flow with tests on fakes; Windows WMI/ETW and verify-trust code behind `cfg(windows)`.
- [x] Kiosk app toast and its Playwright test; catalogue keys; README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` and the dev-fixtures runs on Linux and macOS, then every non-Windows matrix row passes. The Windows code compiles in CI.
- Given `pnpm check` and the kiosk UI tests, then both pass.

## Implementation Notes

- **Default allow-list addition:** `C:\Windows\System32\userinit.exe` (signer "Microsoft Windows") is in `DEFAULT_PROCESS_ALLOW`. Winlogon runs it as the player at every sign-in, before it starts the kiosk shell. No schema migration: the settings are seeded through bootstrap and `branch.seed_defaults` (change-logged).
- **Creation watch: WMI `Win32_ProcessStartTrace`, not ETW.** COM, `ROOT\CIMV2`, semi-synchronous `ExecNotificationQuery` read on a dedicated MTA thread (`Next` with a 500 ms timeout). The WMI kernel-trace provider normally delivers start events within a few tens of ms.
  - **Latency budget** (from the event's arrival to `TerminateProcess`, logged as `debug elapsed_ms`): session id plus token plus image path is under 5 ms. A cached signature takes about 0 ms. A first-time `WinVerifyTrust` with a catalog lookup takes about 10–80 ms, with no revocation or network retrieval. Kills that need no signature (the block-list, or no allow path matched) skip verification.
  - **Not measured:** the 200 ms figure has not been measured on real Windows here. The Story 1.19 escape suite has to confirm it on Windows 10 and 11, and switch to ETW Kernel-Process if WMI misses the budget.
  - **Safety net:** a full sweep runs every 10 s, and every 500 ms while the WMI watch is down. The watch is started again every 30 s.
- **Safety rules (required, enforced before any decision):**
  - Only processes in the player's session are considered: the active console session, whose user token SID equals the configured `player_user`'s SID. The process's own token user must be that SID. SYSTEM, LocalService, NetworkService, DWM/UMFD and other accounts are never touched, so `csrss`, `winlogon` and `dwm` are out of reach.
  - A process whose session, token or path cannot be read is left alone.
  - The configured kiosk app (`configured_app_path`) and the service image (`current_exe`) are never killed; they are matched by normalised full path. Debug builds skip their signature check entirely. Release builds log a failed check once and still leave them running.
  - Critical `%SystemRoot%\System32` images are never killed, matched by full path (`PROTECTED`: csrss, winlogon, wininit, smss, services, lsass, dwm, LogonUI, fontdrvhost, svchost, consent, userinit, sihost, ctfmon). This matters because user-service `svchost` instances run as the player.
  - The kill reopens the pid with `PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION` and re-checks the session, token user and path on that handle. A reused pid is never hit.
- **Signatures:**
  - `WinVerifyTrust(WINTRUST_ACTION_GENERIC_VERIFY_V2)` checks the embedded signature first. On `TRUST_E_NOSIGNATURE` it checks catalog signatures: `CryptCATAdminAcquireContext2` with SHA-256, then SHA-1, then `CalcHashFromFileHandle2`, `EnumCatalogFromHash`, and `WinVerifyTrust` on `WTD_CHOICE_CATALOG`. Most inbox Windows binaries are catalog-signed only, so this check keeps the essentials alive.
  - The signer CN comes from `WTHelperProvDataFromStateData`, then `WTHelperGetProvSignerFromChain`, then `CertGetNameStringW(CN)`. This replaces `CryptQueryObject`, so embedded and catalog signatures go through one path.
  - Results are cached by (path, size, mtime), at most 4096 entries.
  - A signature is verified only when it can change the outcome.
- **Platform traits:** `ProcessWatcher` (subscribe, list, player session, inspect), `SignatureVerifier` (stamp, verify) and `ProcessKiller` (kill with re-check). The verifier is a third trait beside the spec's pair. `procs::fake::FakeSystem` plays all three in tests. `Runtime::enforce(api, opts)` starts enforcement. Only the Windows host calls it (with `WinProcesses`), so console mode and other platforms never kill anything.
- **Delivery:**
  - `settings.get` runs after every hello. `settings.changed` pushes apply live.
  - Each valid list is cached in `queue.db` `kv(key, value, rev)` and published on `Ctx::lists`, which triggers a sweep. An invalid value is refused with an `error` log, and the previous list stays.
  - Within one connection, an older `rev` never overwrites a newer one, so a `settings.get` reply overtaken by a push cannot roll back. Across connections the Hub is authoritative, so a restored Hub can lower revs.
  - Defaults apply until the Hub has answered once.
- **Notification and audit:**
  - Toasts go over a broadcast channel to the app supervisor, then as IPC `toast`, and are dropped when no app is connected. The service collapses toasts within 1 s. The UI shows one toast that later toasts replace.
  - `kiosk.process_blocked` gets its ULID key at kill time and goes through the 1.16 queue.
  - The same (path, reason) within 5 s is killed but not reported again, so a respawn loop cannot flood the queue or the Hub's shared 60/min limit.
- **Kiosk UI:**
  - The Tauri shell forwards `toast` as the `toast` event, pinned in `ipc-names.json`.
  - The webview accepts only known keys (`player.app.blocked`). It renders the toast in an `aria-live=polite` region that never moves focus and lets clicks through outside the toast, with a Dismiss button (new catalogue key `common.dismiss`).
- **Windows code** is type-checked locally with the msvc target (`cargo clippy --target x86_64-pc-windows-msvc -p arena-kiosk-svc --all-targets --features dev-fixtures -- -D warnings`) and in CI. It has not been run on real Windows; Story 1.19 covers that.
- **Known risk:** the default WebView2 rule expects the machine-wide Evergreen runtime under `Program Files (x86)`. A fixed-version or per-user WebView2 elsewhere would get its `msedgewebview2.exe` killed, which breaks the kiosk UI, until the branch adds an allow rule.
- **Review fixes (safety and reliability hardening; the enforcer kills less, fails safe and is observable; no new termination capability):**
  - **Rollout mode:** new setting `kiosk.process_mode` (`enforce` | `report`, `SettingRule::OneOf`, paired-readable as `kiosk.*`, pushed and cached in `kv` like the lists). `branch.bootstrap` seeds `enforce`; the `branch.seed_defaults` backfill seeds `report` (`settings_defaults::backfill_value`). In report mode nothing is terminated and no toast is shown; each disallowed process is reported once per (pid, path) as `kiosk.process_blocked{mode: "report"}` (optional payload field, default `enforce`, omitted on the wire; stored detail `<exe> (<reason>, report only): <path>`). A kiosk with no mode from the Hub (or a corrupt cached one) reports only. README says how to switch to enforce.
  - **Never break the kiosk UI:** a descendant of the running kiosk app whose image file name is `msedgewebview2.exe` (case-insensitive) is exempt; every other descendant (the app is the player's shell) goes through the normal allow/block decision (parent pids from WMI `ParentProcessID` and Toolhelp snapshots, chain walked up to 32 levels; the app ancestor must be running now, re-inspected by path, session and user). New default allow rules: WebView2 in `C:\Program Files\…`, per-user `C:\Users\*\AppData\Local\…`, fixed-version `{install}\**\msedgewebview2.exe` (signer "Microsoft Corporation"); `{system}\SystemApps\*\TextInputHost.exe` and `{system}\System32\backgroundTaskHost.exe` (signer "Microsoft Windows"). `**` (across folders) was added to the glob, now a DP matcher. This supersedes the Known risk above.
  - **`{system}`** expands from `%SystemRoot%` on the kiosk (`procpolicy::expand_dirs`, applied to both lists); every default Windows rule uses it. Placeholders are accepted only as the root prefix.
  - **Validation:** refuses catch-all block rules (only wildcards, optionally `.exe`), an empty allow-list, a signer with leading/trailing space, and unsigned allow rules under `X:\Users\`, `X:\ProgramData\`, `X:\Temp\`, `{system}\Temp\` or a wildcard first folder.
  - **Observability caps:** at most 30 `kiosk.process_blocked` per minute (`ReportCap`), then one `kiosk.log{warn, process_blocked_summary, "N more"}` per minute (flushed by the sweeper). The stored detail is capped at `MAX_KIOSK_LOG_DETAIL_CHARS` with the path cut from the left (`…`), on the kiosk and again in `detail()`. Unreadable process details (`Inspect::Unreadable{reason}`) warn once per (pid, reason) when in or possibly in the player session and the process is left alone; `Inspect::Gone` is silent. A failed kill warns once per (pid, path) and queues one `kiosk.log{error, kill_failed}`; a vanished/changed process (`NotFound`) is silent.
  - **Reliability:** bounded creation channel (`EVENT_QUEUE` 1024, `EventSink` requests a sweep on overflow); `MAX_IN_FLIGHT` 8 checks (semaphore); single-flight signature verification per cache key (`OnceLock` per key); `AppliedRevs::from_cache` seeds revs from `kv` at every connect; a sweep follows every (re)subscribe; the enforcer returns at once if stop is already set; `settings.get` is bounded by a 5 s timeout and by shutdown; the configured app path is re-read on each sweep (`EnforceOptions::app_config`).
  - **Toast:** only for a kill after a creation event whose process had a visible top-level window (`ProcessWatcher::has_visible_window`, default false) or was started within 5 s by a player-session process; sweeps never toast. The LocalSystem service runs in session 0 and cannot enumerate the player's windows, so on Windows the 5 s rule decides.
  - **Docs:** the README latency claim is corrected (WMI path aims at ≤200 ms, polling fallback 500 ms, to be measured in Story 1.19; WMI only, no ETW).
  - **Tests:** `procs::tests` (report mode never terminates; WebView2-descendant exemption, with other app children such as `cmd.exe` decided normally; signer-scoped block through `Inner::decision`; older rev and invalid value keep the previous list in ctx and kv; corrupt kv falls back to defaults/report; report cap and summary; single-flight; unreadable/kill-failed warn-once; toast rules; queue overflow; app path re-read); `tests/runtime.rs` `report_mode_from_the_hub_kills_nothing_and_reports_once`; `arena-api` `kiosk_events_share_one_rate_limit`; proto and core validation cases.
  - **Trade-offs to know:** (a) seeding revs from `kv` means a Hub restored from an older backup cannot lower a kiosk's lists until the setting's rev passes the cached one (re-provisioning or clearing `queue.db` resets it); (b) the descendant exemption is narrowed to `msedgewebview2.exe`, so a file of that name started under the app runs unchecked (its path and signer are not checked); everything else the app launches is enforced; (c) the mode defaults to `report` on a kiosk that never heard from its Hub.
  - **Follow-ups (not done here):** verify the signature through the running image's file handle (and key the cache on file ID + hash) instead of by path after start (triage #5); pin the Arena signer by issuer/thumbprint and check the EKU instead of the CN only (triage #6); the long-path `\\?\` verify fix (#15) and the `drop_paired` seam (#12) stay open.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Upgraded branches go straight to deny-by-default, so every game is killed; no report-only mode | high | `seed_defaults` backfills the lists with no rollout guard. | patch (`kiosk.process_mode` enforce/report: backfill seeds report, bootstrap seeds enforce) |
| 2 | WebView2 at a non-default path (per-user, fixed-version, Program Files), or its children, killed; `TextInputHost` etc. missing (blind, edge) | high | Takes down the kiosk UI. | patch (exempt descendants of the own UI pid; more default rules) |
| 3 | Default allow rules hard-code `C:\Windows` (blind, verify-gap) | medium | Breaks when SystemRoot is elsewhere. | patch (`{system}` placeholder) |
| 4 | Validation accepts a wildcard block-all, an empty allow-list, `{install}` mid-path, wide unsigned allows over user-writable roots, untrimmed signer (blind, edge) | medium | Operator foot-guns. | patch |
| 5 | Signature checked by path after start; cache key owner-controlled | medium | Signed-file swap. | patch (verify via the image's file handle; key the cache on file ID + hash) |
| 6 | Signer matched by CN only | medium | Any CA-issued "Arena 360" cert passes. | patch (pin the Arena issuer/thumbprint via config; EKU check) |
| 7 | Unreadable process details are skipped silently | low | Skipping is the safe choice (system processes share the session); log at warn. | patch (warn only) |
| 8 | Kill reports flood the queue and evict history; no global cap (blind, edge) | medium | Distinct paths bypass dedup. | patch (per-minute cap plus a summary event) |
| 9 | Creation events unbounded; duplicate verifies on bursts (blind, edge) | medium | Starves the service. | patch (bounded channel, semaphore, single-flight) |
| 10 | Stale list from a lagging follower; AppliedRevs not seeded from kv (blind, edge) | medium | Rollback of operator changes. | patch |
| 11 | Signer-scoped block path, the stale-rev guard, the invalid-list fallback and the shared rate limit are untested (verify-gap) | low | Pre-verified gaps. | patch |
| 12 | `drop_paired` on a failed settings read untested (verify-gap, deferred by reviewer) | low | Needs a seam in DeviceDirectory. | defer |
| 13 | Gaps between the startup sweep and the subscription, and on resubscribe | low | Up to 10 s uncovered. | patch (sweep after subscribe) |
| 14 | Kill failures warn every poll | low | Log flood. | patch (warn once, report to the Hub) |
| 15 | Long-path verify drops the `\\?\` prefix | low | Signed long-path apps killed. | patch |
| 16 | Stop already set; `fetch_settings` blocks shutdown | low | Direct fixes. | patch |
| 17 | `app_path` change not picked up by the exemption | low | Re-read the config. | patch |
| 18 | `process_blocked` detail not capped like `kiosk.log` | low | Two limits on one column. | patch |
| 19 | Toast shown for background kills the player didn't cause | low | Confusing message. | patch (toast only for processes with a visible window or a recent player launch) |
| 20 | The 200 ms claim isn't guaranteed in fallback; no ETW path despite the README (edge) | low | Doc fix; measure in 1.19. | patch (doc) |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-kiosk-svc --features dev-fixtures && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check` -- expected: pass after commit
