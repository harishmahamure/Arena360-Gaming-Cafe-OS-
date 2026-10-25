---
title: 'Story 1.19: Automated 30-minute kiosk escape suite on Windows 10 and 11'
type: 'feature'
created: '2026-10-23'
status: 'blocked'
baseline_commit: '7809586199583b3d8c7c8944fd524580ffdc359b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-18-kiosk-process-allow-block-list.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Nothing proves the kiosk lockdown holds on real Windows. A regression in the shell replacement, policies, keyboard hook, process killer or watchdog would reach café PCs unnoticed.

**Approach:** Two pieces work together.
- **Harness:** `arena-escape`, a data-driven Rust binary that runs inside the player session on a VM. It drives scripted escape cases through `SendInput` and other Win32 calls, watches for forbidden outcomes, takes screenshots and writes a JSON report.
- **Orchestrator:** a GitHub workflow on a self-hosted Hyper-V runner. It restores clean Windows 10 and Windows 11 checkpoints, installs the release-candidate Kiosk, runs the harness for at least 30 minutes per OS, and fails the release on any escape.

## Boundaries & Constraints

**Always:**
- **Cases file:** `tests/escape/cases.yaml` is the single list of cases. Every case runs on every OS image in every run.
- **Case schema:** `{id, title, steps: [Step], expect: no_escape}`. The `Step` types are:
  - `keys{chord: "Win+R" | "Alt+Tab" | "Alt+F4" | "Ctrl+Shift+Esc" | "Win" | "Win+E" | "Win+I" | "Win+X" | "Ctrl+Esc" | "Alt+Esc" | "Shift×5"}`
  - `sas_options` (Ctrl+Alt+Del screen, run through the SAS test path or the policy check, documented as such)
  - `wait{ms}`
  - `launch{path}` (attempt `CreateProcess` as the player)
  - `file_dialog_spawn` (open a common file dialog from the kiosk's test hook if one exists; otherwise the step records `skipped: no dialog surface`)
  - `usb_insert{vhd}` (attach a prepared VHD with `autorun.inf` through the orchestrator's host-side hook)
  - `kill_app` (terminate `arena-kiosk-ui`)
  - `accessibility{kind: sticky|filter|toggle|magnifier|narrator|osk}`
- **Seed cases:** at least one case for each kind the epic names:
  - every key combo listed above;
  - Task Manager (via Ctrl+Shift+Esc and launching `taskmgr`);
  - Run (Win+R);
  - Settings (Win+I, and `ms-settings:` via launch);
  - USB with autorun;
  - process spawn through a file dialog;
  - Sticky Keys (Shift×5) and the other accessibility shortcuts;
  - killing the kiosk app, which must be relaunched by the watchdog with the screen covered.
- **Detector:** after every step and every 250 ms while waiting, it fails the case if any of these holds:
  - **Desktop or Explorer:** an `explorer.exe` process exists in the player session, or the foreground window class is `Progman`, `WorkerW` or `CabinetWClass`.
  - **Forbidden process:** a `cmd.exe`, `powershell.exe`, `pwsh.exe`, `taskmgr.exe` or `regedit.exe` process, or any process outside the 1.18 allow-list, has a visible top-level window for more than 300 ms (longer than the killer's 200 ms budget plus margin).
  - **Settings app:** the foreground app is `SystemSettings.exe`.
  - **Uncovered screen:** the kiosk window doesn't cover the virtual screen after a kill or restart for more than 2 s.

  The rules live in a pure, testable `detect(snapshot) -> Option<Escape>`.
- **On failure:** record the case id, the step index, a PNG screenshot of the full virtual screen (GDI `BitBlt`), the OS build (`RtlGetVersion`) and the time. The run then continues, so every failure in a run is collected.
- **Duration:** the harness loops through the case list, randomising the order each pass with a seed recorded in the report, until at least 30 minutes have passed (`--min-minutes 30`), and always finishes the pass it is in.
- **Report:** `escape-report.json` holds `{os_build, os_edition, kiosk_version, started_at, finished_at, duration_s, seed, passes, attempts, cases: [{id, runs, failures: [{step, screenshot, at}]}], result: pass|fail}`. Exit code 0 means pass and 1 means fail.
- **Crate:** `crates/arena-escape` is a binary and a workspace member. Its Windows code is `cfg(windows)`. On other OSes it exits 2 with "escape suite runs on Windows only". The pure modules compile and are tested on every OS: case parsing and validation, `detect` rules on synthetic snapshots, report building, chord parsing and duration/pass logic with an injected clock.
- **Orchestrator:**
  - `.github/workflows/escape-suite.yml` runs on `workflow_dispatch` and on `v*` tags, `runs-on: [self-hosted, windows, hyperv]`, with a matrix over the `win10` and `win11` VM names.
  - It calls `scripts/escape/run-vm.ps1`. Per VM, that script:
    - `Restore-VMCheckpoint clean`;
    - starts the VM and waits for its heartbeat;
    - copies the installer and harness in over PowerShell Direct (`Copy-VMFile` / `New-PSSession -VMName`);
    - installs silently;
    - signs in as `player` through the VM's auto-logon in the clean image (prepared by `scripts/escape/README.md`);
    - starts the harness in the player session through a scheduled task;
    - handles `usb_insert` by attaching the VHD;
    - collects the report and screenshots;
    - stops the VM and reverts it to the checkpoint.
  - Every run starts from the `clean` checkpoint.
  - It uploads `escape-report-<os>.json` and the screenshots as release artefacts, and fails the job when `result=fail`. The release workflow gates on it (documented; wired into the 1.10 `release-bench.yml` trigger set).
- **Docs:** `scripts/escape/README.md` covers how to build the Windows 10 1803+ and Windows 11 images, create the `clean` checkpoint and register the runner. It also explains how to add a case: append it to `cases.yaml`, and it then runs on both images.

**Never:**
- Running the suite in normal PR CI (it needs the Hyper-V runner).
- Marking a case pass when the detector didn't run.
- Leaving a VM in a modified state after a run.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Parse | `cases.yaml` with an unknown step type | parse error naming the case and step | exit 2 |
| Detect explorer | a snapshot with `explorer.exe` in the player session | `Escape::Desktop` | N/A |
| Detect brief cmd | cmd visible for 150 ms, then gone | no escape (within the budget) | N/A |
| Detect lingering cmd | cmd visible for 400 ms | `Escape::ForbiddenProcess` | N/A |
| Uncovered | the kiosk window rect is smaller than the virtual screen for 2.5 s after `kill_app` | `Escape::Uncovered` | N/A |
| Duration | an injected clock with 31 minutes elapsed in the middle of a pass | finishes the pass, then stops | N/A |
| Report | 2 failures in the run | `result=fail`, exit 1, both failures listed | N/A |
| Other OS | run on macOS | message, exit 2 | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-kiosk-core` -- 1.17's `keys`/`monitors` and 1.18's `procpolicy`. Reuse the monitor union and the allow-list decision for the detector. Mind the layer table: `arena-escape` is unrestricted.
- `crates/arena-kiosk-svc` -- the 1.16–1.18 Windows code patterns (`windows` crate pin, cfg gating).
- `.github/workflows/release-bench.yml` (1.10/1.16) -- the self-hosted runner and artefact patterns.
- `apps/kiosk` -- add a test hook to open a file dialog, only if it is cheap and gated to a test build. Otherwise the step records `skipped: no dialog surface` per the schema.

## Tasks & Acceptance

**Execution:**
- [ ] `crates/arena-escape/**` -- cases, detect, report, chords, runner (Windows), screenshots; unit tests for every matrix row.
- [ ] `tests/escape/cases.yaml` -- the seed cases.
- [ ] `.github/workflows/escape-suite.yml`, `scripts/escape/{run-vm.ps1,README.md}`.
- [ ] `README.md` -- a pointer to the suite.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` on Linux and macOS, then the pure modules pass every non-Windows matrix row.
- Given the CI Windows job, then `arena-escape` compiles.
- Given the self-hosted runner, the suite produces a report. This can't be run here; it is noted as unverified.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test --workspace --locked` -- expected: pass
- `cargo run -p arena-escape -- --cases tests/escape/cases.yaml --validate-only` -- expected: the cases parse (works on any OS)
- `pnpm check` -- expected: pass
