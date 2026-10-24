---
title: 'Story 2.27: Printer detection and test print at the Hub'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: '96b77c1ace63372574a196654f96394297bbd594'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-26-branch-operating-settings-opening-hours-fulfilment-kiosk-vol.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub can't find or test a receipt printer, so the go-live checklist has no printer status. Nothing confirms receipts will work before opening.

**Approach:**
- A printer adapter (USB through the Windows spooler, LAN raw `:9100`, and a recorded fake) behind one trait. Settings → Printer can find printers, add one by IP, pick a paper width, and print an ESC/POS test page.
- The result (`ok`, `failed` with the error, or `skipped`) goes into 2.26's `SetupStatus`.
- The printer & payments screen says UPI and card are recorded by reference at settle, and asks for no payment-provider keys.

## Boundaries & Constraints

**Always:**
- **Adapter** (`crates/arena-tasks/src/printer/` or the crate holding Hub adapters, per AD-28/AD-34): `trait Printer` with `discover() -> Vec<FoundPrinter{id, kind: usb|lan, name, addr?}>` and `send(target, bytes) -> Result<(), PrinterError>`.
  - **Windows USB:** enumerate local printers through the spooler (`EnumPrintersW`) and send RAW with `OpenPrinterW` / `StartDocPrinterW` (datatype `RAW`) / `WritePrinter`. Add the needed `windows` crate features to the workspace list.
  - **Other OSes, for dev:** no USB discovery (an empty list).
  - **LAN:** scan the Hub's own IPv4 /24 subnets (from `if-addrs`, already pinned by 2.17) for TCP `:9100`, connecting with a 300 ms timeout and at most 64 concurrent connections, all within 8 s.
  - **Manual IP:** `ip[:port]`, port default 9100. Send with a 5 s connect and write timeout.
  - **Fake:** records the sent bytes and has scripted discovery and failure. It's selected by `--printer fake` (dev) and by tests.
- **Test page (ESC/POS, text only, Font A):**
  - `ESC @` initialise, then `ESC M 0`.
  - The branch name centred at double width and height (`GS ! 0x11`), then back to normal (`GS ! 0x00`).
  - The date in the branch locale (1.12 formatting) and `Printer test OK`.
  - A width-specific ruler: 32 columns for 58 mm, 48 for 80 mm (digits `1234567890` repeated, cut to the width).
  - Feed 4 lines (`ESC d 4`) and a partial cut (`GS V 66 0`).
  - Non-ASCII in the branch name is transliterated to `?`, since the code page is PC437.
  - Golden byte tests cover both widths.
- **Settings** (2.26's mechanism):
  - `printer.selected`: `{kind, id_or_addr, name}`, or empty.
  - `printer.paper_mm` ∈ {58, 80}, default 80.
  - `printer.status`: `{state: unset|ok|failed|skipped, error?, at}`. It's written only by the Hub's test and skip actions, never raw.
  - Writes need `settings.edit` (Manager or Owner); Counter is read-only.
- **Hub endpoints** (`/ui/*`, staff session):
  - `POST /ui/printer/find` → `{printers}` (USB plus a LAN scan).
  - `POST /ui/printer/test` → prints with the selected printer and width, and records `ok` or `failed` with the adapter's error message (≤ 200 characters), both through an intent.
  - `POST /ui/printer/skip` → records `skipped`.
  - Selection goes through `settings.set`.
- **`SetupStatus`:** gains `printer: unset|ok|failed|skipped` (2.26 struct), surfaced on `/ui/snapshot` and `/healthz`.
- **Hub UI (Settings → Printer):**
  - "Find printer" with a busy state (up to about 8 s), and the found list as radio options showing the name, kind, and IP for LAN printers.
  - A "Printer IP address" field with "Add".
  - Paper width 58 / 80 mm.
  - "Test print", with the result shown inline (ok, or the error text) and announced politely.
  - "Skip printer for now", which sets `skipped` with a note that nothing else on the Hub needs a printer.
  - A "Payments" section with the copy: UPI and card are recorded by reference when a bill is settled. No payment-provider keys are needed, and none are asked for.
  - Every string lives in `en.json`. axe passes in both themes and HC.
- **Tests:**
  - **Rust:**
    - golden bytes at 58 and 80 mm (including a long name, non-ASCII and the date);
    - LAN scan against local listeners on random ports, via a test-only port override and a /32 scan;
    - manual IP;
    - send timeout and refused;
    - fake discovery and failure;
    - status transitions through test and skip;
    - Counter denied;
    - `SetupStatus.printer`.
  - **Windows spooler code:** compiles behind `cfg(windows)`. It's unit-tested only for the RAW doc-info construction; a live print is a manual step.
  - **Playwright** (stubbed): find, select, test ok and failed, skip, the payments copy, axe.

**Never:**
- Payment-provider keys or fields.
- Graphics or raster printing.
- Making printing a dependency of any other Hub flow.
- Scanning beyond the Hub's own /24 subnets.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Find (fake) | fake with 1 USB + 1 LAN | both listed | N/A |
| LAN scan | listener on :9100-equivalent in the subnet | listed within 8 s | N/A |
| Manual IP | `192.168.1.50` | target `192.168.1.50:9100` | N/A |
| Bad IP | `abc` | `VALIDATION_FAILED` field `ip` | N/A |
| Test 80 mm | selected, fake | exact golden bytes; status `ok` | N/A |
| Test 58 mm | width 58 | 32-col ruler bytes | N/A |
| Test fails | refused / timeout | status `failed` + error | shown inline |
| Skip | skip | status `skipped` | N/A |
| Counter | Counter session test | `PERMISSION_DENIED` | N/A |
| Setup status | after skip | `printer: skipped` | N/A |

</frozen-after-approval>

## Code Map

Investigate: 2.26's settings rules / `SetupStatus` (`crates/arena-core/src/{setup.rs,settings_defaults.rs}`, `crates/arena-proto/src/operating.rs`), `/ui/intent` handling (`crates/arena-api/src/ui_*.rs`), the Hub Settings sub-navigation 2.26 added (`apps/hub/src/routes/settings/**`), 1.12 locale date formatting, `if-addrs` usage in `crates/arena-tasks/src/mdns.rs` (2.17), the `windows` workspace feature list in `Cargo.toml`. Hub migration number reserved **0018** only if one is needed.

## Tasks & Acceptance

**Execution:**
- [ ] adapter module (+ Windows spooler, LAN, fake), test-page builder with golden tests.
- [ ] settings rules, intents for status, `SetupStatus.printer`, `/ui/printer/*` endpoints, `--printer` dev flag.
- [ ] `apps/hub/src/routes/settings/printer/**`, `en.json`, tests.
- [ ] `README.md` (flags, Windows printing notes).

**Acceptance Criteria:**
- Given a dev Hub with `--printer fake`, when a Manager finds the fake printer, picks 58 mm and runs Test print, then the fake has recorded the 58 mm golden bytes and `/healthz` reports `setup.printer = "ok"`.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast`; `-p arena-hub --features dev-fixtures`; `-p arena-api --features dev-fixtures`; clippy incl. dev-fixtures (and `cargo check --target x86_64-pc-windows-msvc -p <adapter crate>` if the target is installed; else record it); fmt.
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell`; every `pnpm run check:*`; `pnpm run test:scripts`.

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter + Verification Gap on Sonnet). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/VG: status not reset when printer or width changes | medium | P | checklist reports an untested printer as ok |
| 2 | BH: arbitrary IPv4/port targets (loopback, metadata, public) | medium | P | `parse_printer_addr` rejects only unspecified, broadcast and multicast |
| 3 | BH: USB spooler send has no timeout | medium | P | `spawn_blocking` unbounded |
| 4 | BH: no single-flight for find/test; re-read status | medium | P | |
| 5 | BH: scans link-local adapters; /24 assumption; no ESC/POS probe; generic names | low | P (link-local skip) / R (the /24 rule and probe follow the spec) | |
| 6 | BH: printed-but-not-recorded reported as failed | low | P | |
| 7 | BH: focus lost after save | medium | P | a11y |
| 8 | BH: audit label for skip | low | P | |
| 9 | BH: whitespace error validation inconsistent | low | P | |
| 10 | BH: no `ESC t 0`; PC437 transliteration | low | P (ESC t 0) / R (`?` per spec) | |
| 11 | BH/VG: fragile timeout and deadline tests | medium | P | environment-dependent |
| 12 | BH: README service note; English adapter errors | low | P | |
| 13 | VG: follower refusal untested | medium | P | |
| 14 | VG: Windows spooler path has no behavioural test | medium | D | needs a real spooler queue; manual check per spec |
| 15 | VG: `PRINTER_INFO_4W` buffer alignment UB | medium | P | |
| 16 | VG: audit label render untested | low | R | the sibling labels aren't tested either |
