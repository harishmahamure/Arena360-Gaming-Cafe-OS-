---
title: 'Story 6.12: Session-end cleanup signs out launchers, Discord and browsers'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '4c1a738'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-11-session-end-cleanup-ends-the-player-s-programs-and-clears-th.md'
---

## Intent

**Problem:** After Story 6.11 the next player finds no programs, clipboard or temp files, but the last player's Steam, Epic, Riot and Discord logins, browser profiles and saved Windows credentials are still on the PC.

**Approach:** Extend the 6.11 pipeline (`Pipeline::standard`) with one step per target and a credentials step. No protocol, Hub or UI change: a failed step uses `kiosk.cleanup_failed` with the new step name.

## Design

- Steps, after `programs`: `signout_steam`, `signout_epic`, `signout_riot`, `signout_discord`, `reset_chrome`, `reset_edge`, `reset_firefox`, then `credentials`, `clipboard`, `temp_files`, `volume`.
- Each target step first ends that target's programs through the process enforcer's kill path (`CloseScope::Images`, the same app/service/protected-image safety rules as `programs`) and fails with "<Launcher> is still running" if any remain; **files are removed only after that**.
- Removals are in `cleanup/signout.rs` (`Target::removals`): Steam `config\loginusers.vdf`, `config\config.vdf`, `ssfn*`, `htmlcache` and the player's `AutoLoginUser`/`RememberPassword` registry values; Epic `GameUserSettings.ini` and `webcache*`; Riot `Riot Client\Data`; Discord (stable, PTB, Canary) roaming folders; Chrome, Edge and Firefox profile folders. A target that is not installed has nothing to remove, so its step passes.
- Browsers are reset by removing the profile folder; if the branch keeps `browser-templates\<chrome|edge|firefox>\{Local,Roaming}` next to the service config, it is copied back in.
- Nothing follows a link (SYSTEM acting in the player's profile): a link on the way is refused, a link at the end loses only the link.
- Registry (`--steam-login`) and Credential Manager (`--credentials`) work as `cleanup-helper` tasks in the player's session, like clipboard and volume.

## Tasks & Acceptance

- [x] `procs` -- `Closer` takes a `CloseScope` (all, or named images)
- [x] `cleanup` -- host trait gains profile/Steam/template folders, Steam login, credentials; sign-out steps and pipeline order; fake host
- [x] `cleanup_windows` -- Steam install dir, `--steam-login` and `--credentials` helper tasks; `Win32_Security_Credentials` feature
- [x] Tests: portable removal/copy/link tests, pipeline tests on the fake host and fake process table

**Acceptance:**

- Session end signs out Steam, Epic, Riot and Discord, resets Chrome, Edge, Firefox and clears Credential Manager before the original lock shows. (tested on the fake host)
- A running launcher is ended first; its files stay if it will not exit. (tested)
- A failed sign-out is retried once, then reported by step name; the others still run. (tested)
- A launcher or browser that is not installed is skipped. (tested)

## Verification

- `cargo test -p arena-kiosk-svc --lib cleanup::` passes (the 14 other lib failures — config/lockdown/pipe/dpapi/procs app path — fail the same way without this change on this non-admin host).
- `cargo clippy -p arena-kiosk-svc --lib --bins -- -D warnings` and `cargo fmt --check` clean. `tests/discovery.rs` does not build on Windows (as before).
- Not run here: `tests/runtime.rs` (unix only; its cleanup call list was updated), the Windows registry/Credential Manager code against real Steam, Epic, Riot, Discord, Chrome and Edge logins on Windows 10/11 images (the AC's image test needs real images).
- Registry paths and file locations for each launcher were taken from the known layouts, not verified against a live install on this host.

## Review Triage Log (code review, 2026-10-29)

Fixed: none needed. Checked: removals refuse a link on the way and remove a link leaf as a link only; a target's files are removed only after its programs are gone; `msedgewebview2.exe` is not in Edge's image list; the registry and Credential Manager helpers treat "nothing there" as success and free the system list; step names fit the Hub's rules.

Deferred (not blocking): the Windows registry/Credential Manager/launcher paths still need a run on real Windows 10/11 images with live logins (as listed under Verification).
