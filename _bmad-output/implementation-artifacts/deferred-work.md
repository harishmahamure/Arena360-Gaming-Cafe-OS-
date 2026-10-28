- source_spec: `_bmad-output/implementation-artifacts/spec-1-3-generated-protocol-types-and-the-message-catalogue-lint.md`
  summary: Give `ErrorCode` (and other wire enums) a forward-compatible unknown-value fallback so older Kiosks/clients don't fail to deserialize codes added by a newer Hub.
  evidence: `ErrorCode` is a closed serde enum with no `#[serde(other)]` variant, and the Hub must accept Kiosks from the previous two releases (AD-8). There's no wire reader yet; decide this in Story 1.9 (LAN protocol envelope).
- source_spec: `_bmad-output/implementation-artifacts/spec-1-8-idempotent-intents-optimistic-row-versions-and-property-test.md`
  summary: Replicate `applied_intents` (and other non-change-log tables) to the standby, so idempotent retries survive failover.
  evidence: `applied_intents` sits outside `changes`, so replay and replicas don't have it. A client retrying across a failover would re-run its intent. Settle this in the standby/replication epic.
- source_spec: `_bmad-output/implementation-artifacts/spec-1-12-branch-locale-settings-currency-number-format-and-timezone.md`
  summary: Guard `branch.currency` changes that alter the ISO 4217 exponent once money rows exist (refuse, or require a conversion step), so stored minor units keep their meaning.
  evidence: No money tables exist in Epic 1. The first story that adds wallets, orders or prices must add the guard.
- source_spec: `_bmad-output/implementation-artifacts/spec-1-16-kiosk-privilege-split-locked-service-and-unprivileged-ui.md`
  summary: Add a retention policy for `device_events` (and other append-only operational tables) on the Hub.
  evidence: The table has no pruning; rate limits bound how fast it grows, but not its total size. Settle this with the data-lifecycle/retention epic.
- source_spec: `_bmad-output/implementation-artifacts/spec-1-16-kiosk-privilege-split-locked-service-and-unprivileged-ui.md`
  summary: Add a CI install/uninstall smoke test (silent install, assert the service is running or the app is present, silent uninstall) for both the Hub and Kiosk installers.
  evidence: Both installer jobs only build the NSIS exe; the hooks are checked only as source text.
- source_spec: `_bmad-output/implementation-artifacts/spec-1-18-kiosk-process-allow-block-list.md`
  summary: Harden kiosk signature checks. Verify through the running image's file handle (with a file-ID + hash cache key), pin the Arena signer by issuer/thumbprint with a code-signing EKU check, and require a valid Microsoft signature for the WebView2-descendant exemption.
  evidence: Today the check opens the file by path after start; signers match by CN only; any file named msedgewebview2.exe under the kiosk app is exempt.
- source_spec: `_bmad-output/implementation-artifacts/spec-1-18-kiosk-process-allow-block-list.md`
  summary: A Hub-side test that a failed kiosk-settings read drops paired sessions (`drop_paired`).
  evidence: DeviceDirectory has no seam to inject a read failure.
- source_spec: `spec-1-20-kiosk-admin-exit-with-manager-pin-offline-verifier-and-hub-r.md`
  summary: Bind hub-mode admin exits to the Hub's PIN check with a single-use ticket instead of trusting the kiosk's stated approver.
  evidence: `bind_to_session` keeps the kiosk's `elevated_by`, and `permission()` checks only that the approver currently holds `device.remote`.
- source_spec: `spec-1-20-kiosk-admin-exit-with-manager-pin-offline-verifier-and-hub-r.md`
  summary: Bound the kiosk-supplied `issued_at` on admin exits against the Hub's receive time, and use Hub-side time for the stale check where possible.
  evidence: `started_at`, `relock_at` and `stale()` all use the kiosk's `issued_at`.
- source_spec: `spec-1-20-kiosk-admin-exit-with-manager-pin-offline-verifier-and-hub-r.md`
  summary: Add a verifier epoch so kiosks accept a lower verifier version after a Hub restore or rebuild.
  evidence: `replaces()` accepts only a strictly higher version, so restored Hubs leave kiosks holding departed managers' hashes.
- source_spec: `spec-1-20-kiosk-admin-exit-with-manager-pin-offline-verifier-and-hub-r.md`
  summary: Reconcile admin exits whose `relock_at` passed without a relock, including remote exits whose push never reached the kiosk.
  evidence: `push_admin_unlock` returning 0 is only logged; nothing closes the row.
- source_spec: `spec-1-20-kiosk-admin-exit-with-manager-pin-offline-verifier-and-hub-r.md`
  summary: Manager PIN strength floor or rotation guidance, because every paired kiosk holds argon2 hashes of all `device.remote` holders.
  evidence: The offline verifier is pushed to all paired kiosks; short numeric PINs are brute-forceable from a compromised kiosk.

- source_spec: `_bmad-output/implementation-artifacts/spec-1-21-high-contrast-theme-as-a-per-device-branch-setting.md`
  summary: The kiosk's per-key rev guard (process lists and high contrast) ignores a Hub value with a lower rev, for example after a Hub database restore.
  evidence: maybe-false (medium if true). The guard seeds from the cached rev and rejects lower ones, even on the authoritative `settings.get` after hello. It would be settled by deciding whether hello-time reads may lower a rev.
- source_spec: `_bmad-output/implementation-artifacts/spec-1-21-high-contrast-theme-as-a-per-device-branch-setting.md`
  summary: `arena-hub-ui` test `sidecar::tests::a_sidecar_that_exits_stops_the_wait_early` fails locally (it took over 5 s) in code this story doesn't touch.
  evidence: It fails 3 out of 3 runs on this Mac (the stub sidecar takes about 6 s to be seen exiting). It is environmental or a timing budget, and predates story 1.21.

- source_spec: `_bmad-output/implementation-artifacts/spec-1-22-the-branch-runs-with-the-wan-down-and-shows-which-link-is-of.md`
  summary: The connectivity probe's default cloud host `api.arena360.in:443` is a placeholder, so every Hub with working internet shows "Cloud unreachable" until a real cloud `/healthz` exists.
  evidence: The frozen intent sets this default "as a placeholder until the cloud epic". Story 2-1 (cloud in India behind Cloudflare) should provide the real host, or the check should be disabled while no cloud is configured. The user decides.
- source_spec: `_bmad-output/implementation-artifacts/spec-1-22-the-branch-runs-with-the-wan-down-and-shows-which-link-is-of.md`
  summary: Hub Playwright phone tests are flaky under full-suite load. These failed once and passed 6 out of 6 alone: `devices-display.spec.ts:292` (Counter PIN toggle, from 1.21) and `shell.spec.ts:382` (More sheet); earlier, `auth.spec.ts:214` (locked sign-in).
  evidence: Each failed only during a full `test:shell` run on a loaded machine and passed on every isolated rerun. This points to timing-sensitive waits, worth hardening before CI sees them.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-1-arena-360-cloud-runs-in-india-behind-cloudflare.md`
  summary: Operator setup before the first cloud deploy. Pin the sops 3.13.3 and age 1.3.2 sha256 values (`PIN-ME` in `deploy-cloud.yml`); generate the age keys and replace the placeholder recipients in `.sops.yaml`; encrypt `deploy/secrets/<env>.enc.yaml` (the Turso token, the origin certificate, `ghcr-pull`); install k3s and record its version; create the Turso DB in `aws-ap-south-1`; apply the Cloudflare zone settings and the Hostinger firewall; create the GitHub environments and their secrets.
  evidence: The loop can't fetch from GitHub or hold real credentials. The deploy job fails clearly until these are done. Steps are in `deploy/README.md`.

- source_spec: none (user decision, 2026-10-24)
  summary: Story 2.3's observability ACs — OpenTelemetry traces (role, route, status, tenant/branch ids) exported to Grafana Cloud, unhandled errors to Sentry, and the scrub test over exported telemetry — to be built as the last item of Epic 2, after 2.31.
  evidence: The user said "we can opt out from logging and sentry" and then "We will do at last".

- source_spec: `_bmad-output/implementation-artifacts/spec-2-2-cloud-error-envelope-local-dev-mode-and-single-sqlite-linkag.md`
  summary: Add tests for `--dev` failing to open its data dir (exit 1, log line) and for a local-backend migration under a held write lock (busy then retry).
  evidence: Verification-gap review found neither path exercised; both dev-only.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-3-control-data-is-backed-up-nightly-and-the-cloud-is-observabl.md`
  summary: Test the HTTPS paths of the alert webhook and S3 clients against a server whose certificate chains to a webpki root (or an injectable root store).
  evidence: Every test uses plain HTTP loopback; a broken root store would only show up when the first real alert or upload fails.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-3-control-data-is-backed-up-nightly-and-the-cloud-is-observabl.md`
  summary: Make the remote gz_control dump a point-in-time snapshot (Turso PITR/branch, or batched reads within the interactive-transaction timeout).
  evidence: The remote dump reads tables one after another without a transaction, so writes during the dump can leave cross-table drift (for example a child row without its parent).

- source_spec: `_bmad-output/implementation-artifacts/spec-2-6-owner-signs-in-on-a-new-device-with-totp-backup-code-or-sms.md`
  summary: Mitigate lock-out by strangers. Anyone who knows an owner's phone or email can trigger the 5-failure, 15-minute pause repeatedly. Options include exempting requests that carry a valid trusted-device cookie, or counting failures per client.
  evidence: Story 2.6 locks per account as specified; the reviewers found the denial-of-service path. Changing it alters the frozen intent, so it needs a decision.

- source_spec: none (observed during 2.6 verification)
  summary: `arena-kiosk-svc` test `wrong_and_counter_pins_are_refused_and_five_pause_across_an_app_restart` (`tests/runtime.rs:261`, "no entry found for key") flakes under heavy parallel load.
  evidence: Failed once in the full workspace run while three other builds ran, then passed 3/3 alone. It is Story 1.20 code that this work didn't touch.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-17-kiosk-finds-the-hub-on-the-lan-or-by-typed-ip.md`
  summary: Add Windows Defender Firewall rules (UDP 5353 mDNS for the Hub and the kiosk, plus TCP 7400/7401 inbound for the Hub) to the Hub and kiosk installers.
  evidence: Discovery uses multicast DNS, and the installers don't open the port. On a default Windows profile, browse replies may be blocked.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-26-branch-operating-settings-opening-hours-fulfilment-kiosk-vol.md`
  summary: Run the backup-directory writability probe for `system.backup_dir` values arriving by non-HTTP paths (the command mailbox in Story 2.15), not only `/ui/intent`.
  evidence: The probe lives in the HTTP handler; the core rule now only requires an absolute path.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-7-resumable-setup-wizard-and-step-1-café-and-first-branch.md`
  summary: Guard edits to a branch's country, currency and timezone once rates, products or invoices exist, or define how existing data migrates.
  evidence: Step 1 edits rewrite these fields unconditionally; later stories denominate data in the branch currency.

- source_spec: none (observed 2026-10-25 under load average ~19)
  summary: Hub Playwright phone tests `devices-display.spec.ts` "a Counter's toggle asks for a manager's PIN…" and `shell.spec.ts` "More sheet: … traps focus …" flake under heavy CPU load (2/6 failures on master).
  evidence: They pass when the machine is less loaded. The failures are timing-sensitive assertions (`toContainText`, `toBeFocused`) in Epic 1 code.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-27-printer-detection-and-test-print-at-the-hub.md`
  summary: A Windows CI or manual test that drives `local_printers` and `print_raw` against a real spooler queue (for example the built-in "Microsoft Print to PDF" with RAW, or a generic text driver).
  evidence: No automated test calls the spooler functions, and USB printing is the main production path.

- source_spec: none (observed 2026-10-25 at load average ~32)
  summary: Hub Playwright `auth.spec.ts` "a locked sign-in counts down, then lets you try again" flakes under extreme CPU load (1/6).
  evidence: The real-time countdown misses its 30 s test timeout when the machine is saturated.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-8-step-2-free-tier-issues-a-signed-licence.md`
  summary: Playwright cases for TierStep's 409 recovery (`ONE_FREE_BRANCH`/`ALREADY_SET` returned after a successful load).
  evidence: The stub can return them but no spec reaches the paths.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-8-step-2-free-tier-issues-a-signed-licence.md`
  summary: Review `jsonwebtoken`'s rust-crypto backend pulling in `rsa` (RUSTSEC Marvin timing advisory), even though only EdDSA is used. Consider a feature set or alternative that drops RSA.
  evidence: The implementer reported that `rsa`, `p256` and `p384` are now in the dependency tree.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-18-kiosk-pairs-with-a-6-digit-code-from-the-hub-or-a-manager-pi.md`
  summary: Idempotent kiosk pairing. When the Hub registered the device but the answer was lost, retries create orphan device rows (duplicate floor entries). Needs a request key or dedupe-by-name design, plus revoking the orphan.
  evidence: The kiosk retries with a new code; the first device row keeps a valid token.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-18-kiosk-pairs-with-a-6-digit-code-from-the-hub-or-a-manager-pi.md`
  summary: Pin the kiosk Tauri `generate_handler!` registration against `ipc-names.json` (for example by parsing `main.rs` in the existing COMMANDS test).
  evidence: Removing `pair`/`pair_info` from the handler list passes every test.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-10-hub-installer-pairs-with-the-code-and-retries-in-place.md`
  summary: Idempotent `/v1/pair`. When the cloud commits but the Hub loses the answer or fails its keystore or DB write, the code is burned and the old Hub revoked. Replay with the same `hub_instance_id` should re-mint; write the DB before the keystore or stage the token.
  evidence: Review findings from Blind Hunter and Edge Case Hunter; no test covers these paths.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-10-hub-installer-pairs-with-the-code-and-retries-in-place.md`
  summary: Per-IP limit behind Traefik. `client_ip` trusts `CF-Connecting-IP` only from Cloudflare peers, but the app's peer is the Traefik pod, so all clients share one bucket in the cluster. Needs a trusted-proxy setting.
  evidence: `deploy/k8s/base/ingress.yaml`; tests inject a Cloudflare peer.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-10-hub-installer-pairs-with-the-code-and-retries-in-place.md`
  summary: Hub hardening and tests. Re-check `hub_identity` and the writer epoch before the cloud call and after startup; Windows DPAPI user scope and ACLs; verify the licence before caching; Hub-side log scrub, Retry-After over HTTP, post-consume failure, dev-seed identity and IPv6 /64 tests.
  evidence: Review findings.
- source_spec: `_bmad-output/implementation-artifacts/spec-8-10-the-owner-app-shell-navigation-themes-install-and-free-limit.md`
  summary: Key the Owner app's IndexedDB cache by owner id, treat a 403 as a refused session, and add PNG and maskable manifest icons, a service worker update test, a ticking data age and a 768 px layout test.
  evidence: The cache is cleared on sign-out and a refused session, but not when a different owner signs in without a sign-out; the manifest has one SVG icon; the tablet breakpoint is untested.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-20-device-groups-with-six-swatches-held-at-the-hub.md`
  summary: Tests for 2.20: `groups.changed` SSE (`by_owner` true only for owner) and the 409 mapping over HTTP; axe in HC; keyboard select via key presses and drag-drop in Playwright; a snapshot-mapping test with real groups; Counter delete/assign cases.
  evidence: The Rust apply tests and the stubbed Playwright spec cover the logic, not the HTTP/SSE edge.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-20-device-groups-with-six-swatches-held-at-the-hub.md`
  summary: Map a unique-name constraint violation on `device_groups` (a race, or non-ASCII case folding) to `VALIDATION_FAILED name`.
  evidence: `COLLATE NOCASE` folds ASCII only; the pre-check and index disagree only for such names.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-18-kiosk-pairs-with-a-6-digit-code-from-the-hub-or-a-manager-pi.md`
  summary: `DeviceRegister.kind` has the same name as the `IntentPayload` tag (`kind`), so the JSON of a `device.register` intent carries a duplicate key. Rename it (as 2.20 did with `group_kind`) before any client submits it over the wire.
  evidence: Found while adding `group.upsert`; in-process tests do not serialise it.

- source_spec: `_bmad-output/implementation-artifacts/spec-2-20-device-groups-with-six-swatches-held-at-the-hub.md`
  summary: Hub shell Playwright is load-sensitive: under a saturated machine (load 40) different tests fail per run (the phone More-sheet focus test, audit paging, the devices-display copy row) and all pass in isolation.
  evidence: Full `test:shell` at load: 4 and then 3 failures, different each time; reruns with 1–2 workers pass.

- source_spec: `_bmad-output/implementation-artifacts/spec-7-1-kitchen-settings-at-the-hub-locked-on-free.md`
  summary: Server-side rule that `kitchen.kot_mode = another` needs a saved printer. The later KOT-printing story must fall back to the receipt printer or skip when it is missing.
  evidence: Review: the UI disables Save, but `settings.set` accepts the pair.
- source_spec: `_bmad-output/implementation-artifacts/spec-7-1-kitchen-settings-at-the-hub-locked-on-free.md`
  summary: Test that Settings → Kitchen unlocks and re-locks live on a `licence.flags` change. Needs the Story 2.12 licence event path.
  evidence: The page reloads on `licence.flags`; no test drives it.
- source_spec: `_bmad-output/implementation-artifacts/spec-7-1-kitchen-settings-at-the-hub-locked-on-free.md`
  summary: Tests for the audit before/after of kitchen keys, the `branch.seed_defaults` backfill of the four new rows, and the kiosk queue treating `TIER_LOCKED` as permanent.
  evidence: Generic paths; no kitchen-specific test.
- source_spec: `_bmad-output/implementation-artifacts/spec-7-1-kitchen-settings-at-the-hub-locked-on-free.md`
  summary: Counter and Kitchen roles keep editable fields with a PIN on Save (screen-lock precedent), not read-only. Revisit if the UX review insists.
  evidence: Story 7.1 AC says read-only fields; spec and decision log choose the precedent.

## Deferred from Story 8.11 review

- HC label assertions for variance flags, data-age warnings and command-row states: add when those Owner components land.
- Owner HC focus-ring test relies on a Tab round trip and a literal colour; read the token instead.
- No unit test for `packages/ui/src/theme.js`; no assertion that choosing HC makes no network call.

- source_spec: `_bmad-output/implementation-artifacts/spec-5-1-tax-settings-gst-registration-rates-inclusive-or-exclusive-a.md`
  summary: Story 5.2 must decide which rates an Indian branch without GST registration uses (`product_bp`/`play_bp` or the simple `rate_bp` and label); the Tax screen shows the GST pair for every Indian branch.
  evidence: Review finding (Blind Hunter).
- source_spec: `_bmad-output/implementation-artifacts/spec-5-1-tax-settings-gst-registration-rates-inclusive-or-exclusive-a.md`
  summary: When Story 2.13 lands, seed `branch.tax.country` from the cloud profile (bootstrap and `branch.seed_defaults`) instead of defaulting to `IN`, and show "Locked after the first invoice" beside the invoice prefix once `branch.code_locked_at` exists.
  evidence: Spec decisions; the profile and the column do not exist yet.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-11-hub-checks-in-by-heartbeat-and-the-wizard-advances-by-itself.md`
  summary: Heartbeat abuse and licence checks. An unauthenticated `/v1/heartbeat` costs an Argon2 verify with no per-IP limit; the Hub should also reject a licence whose tenant or branch differs from `hub_identity`, or that is older than the cached one.
  evidence: Review findings from Blind Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-11-hub-checks-in-by-heartbeat-and-the-wizard-advances-by-itself.md`
  summary: Trusted time and fencing. `cloud_time` is unsigned plain JSON; a same-Hub restart bumps the epoch counter so `confirmed` turns false; `live` has no setter until 2.31 (the 60 s interval is unreachable).
  evidence: Review findings; the spec defers fencing to 2.12.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-11-hub-checks-in-by-heartbeat-and-the-wizard-advances-by-itself.md`
  summary: Revoked Hub recovery. The 5-minute backoff cannot be interrupted by a re-pair, and a paired Hub with no keystore token shows nothing. Settle with the Hub unpair and re-pair flow.
  evidence: Review findings from Blind Hunter and Edge Case Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-11-hub-checks-in-by-heartbeat-and-the-wizard-advances-by-itself.md`
  summary: Wizard event stream hardening: per-owner connection cap, end the stream at session expiry, `X-Accel-Buffering: no`, and a client watchdog when the stream goes silent.
  evidence: Review findings.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-11-hub-checks-in-by-heartbeat-and-the-wizard-advances-by-itself.md`
  summary: Heartbeat test gaps: Hub end-to-end (`/healthz.tier` against a stub cloud), validation and per-Hub limit edges, SSE scoping to the branch, Playwright poll suppression while streaming, control migration 8 to 9 upgrade with existing hubs, `record_heartbeat` guards, and an intent submitted while an answer is being applied.
  evidence: Verification Gap findings.
## Deferred from Story 8.12 review

- `FEATURE_NOT_ENTITLED` for Free branches on owner read and command endpoints (AC 3): add the gate with each endpoint (8.13 and later), with a test per endpoint.
- Pass the branch into `Section` children for paid branches when the real section content lands.
- Empty branch list in `Section` renders unlocked content; decide a message when Branches (2.30) defines the empty tenant.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-21-register-consoles-vr-rigs-and-simulators-manually.md`
  summary: Story 2.19 must seed `licence.device_cap` with a non-settable rule, treat an unparseable value as an error rather than no cap, and extend the cap to PC pairing and kds registration.
  evidence: 2.21 reads the key (absent means no cap) and gates only manual devices; `settings.set` cannot write an unseeded key today.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-21-register-consoles-vr-rigs-and-simulators-manually.md`
  summary: Tests for 2.21: `DEVICE_LIMIT` maps to 403 over HTTP; a Counter removing a manual device gets the PIN prompt for `device.revoke`; the tier-lock tier follows the licence instead of a fixed Basic.
  evidence: Only the Playwright stub and the Rust apply tests cover them.


- source_spec: `_bmad-output/implementation-artifacts/spec-5-2-tax-is-computed-once-per-rate-on-every-bill-in-minor-units.md`
  summary: Settings → Tax still shows the GST product and play rate fields for an unregistered Indian branch, but 5.2 uses `rate_bp` there (default 0); expose "Default rate" when country is IN and GST is off and hide the GST rates.
  evidence: `TaxSettings::rates()` returns `(rate_bp, rate_bp)` unless `tax_mode` is `gst_in`; the Hub page (5.1) is unchanged.
- source_spec: `_bmad-output/implementation-artifacts/spec-5-2-tax-is-computed-once-per-rate-on-every-bill-in-minor-units.md`
  summary: Callers (sessions 3.x, plans 3.22, POS 5.x) must pass `gst = tax_mode() == GstIn` and `inclusive = settings.inclusive`, snapshot the play rate when a bill opens, store `subtotal_minor`, `tax_minor`, `total_minor`, `tax_breakup` on `orders` and `plans.tax_rate_bp` (default = play rate), and show "GST 5% (included)" and the branch label lines in bill-breakdown and the settle-bar.
  evidence: No `orders`, `plans` or bill UI exist yet, so 5.2 ships the pure `invoice::tax::compute` only; `Bill` is not `Serialize` (serialize `breakup` into `tax_breakup`).

## Deferred from Story 2.12 review

- source_spec: `_bmad-output/implementation-artifacts/spec-2-12-hub-verifies-its-licence-against-trusted-time-and-enforces-i.md`
  summary: Replay and time authority. `cloud_time` is unsigned and the Hub accepts a validly signed older licence (no `iat` floor against the cached one); plain HTTP to the cloud is allowed outside release. Sign `cloud_time` or add an `iat` floor, and require HTTPS in release.
  evidence: Review findings from Blind Hunter (also deferred from Story 2.11).
- source_spec: `_bmad-output/implementation-artifacts/spec-2-12-hub-verifies-its-licence-against-trusted-time-and-enforces-i.md`
  summary: Trusted clock after a restart is wall minus the saved delta, so a wall change made while the Hub was off shifts it once; `mono_ms` is stored but unused; no warning before the first cloud time (persist the last-seen wall time to catch an offline rollback); `Instant` may pause during suspend.
  evidence: Review findings from Blind Hunter and Edge Case Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-12-hub-verifies-its-licence-against-trusted-time-and-enforces-i.md`
  summary: A follower Hub (no writer handle) publishes no licence standing or clock warning; settle only runs where the heartbeat task has a writer. Settle with the standby story.
  evidence: Review finding from Edge Case Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-12-hub-verifies-its-licence-against-trusted-time-and-enforces-i.md`
  summary: `USER_LIMIT` is not enforced: `licence.user_cap` is written and `entitlement::limit("users")` answers it, but no `staff.upsert` check or error code exists. The staff story that counts active staff rows adds both. Pairing registrations (token set) stay exempt from `DEVICE_LIMIT` until Story 2.19. `ui_printer.rs` still reads `licence.flags` directly.
  evidence: Story AC wording; review findings.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-12-hub-verifies-its-licence-against-trusted-time-and-enforces-i.md`
  summary: Test that Settings → Kitchen unlocks and re-locks live on a `licence.flags` change in the Hub UI (the 7.1 deferral): the Hub now writes the flag, but no Playwright test drives the live reload.
  evidence: Story 7.1 deferral; this story covers the server side only.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-23-floor-zones-and-grid-layout-per-branch.md`
  summary: `devices_meta` place columns have no all-or-none CHECK (SQLite cannot add a table CHECK by ALTER); only `validate_place` enforces it. Add with the next `devices_meta` rebuild.
  evidence: Review finding from Blind Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-23-floor-zones-and-grid-layout-per-branch.md`
  summary: `device.revoke` keeps `zone_id`, `floor_x`, `floor_y` on the soft-deleted `devices_meta` row; clear them if a restore or re-register path for the same device id is added. `zone.upsert` `sort_order` has no server-side range (the UI caps input at nine digits).
  evidence: Review findings from Blind Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-23-floor-zones-and-grid-layout-per-branch.md`
  summary: Missing tests: `groups.changed` raised for `floor_zones` (Rust and Playwright), HTTP 409 mapping of `ZONE_NOT_EMPTY` and `CELL_TAKEN`, store-level checks of the 0016 indexes, the Counter PIN prompt on Settings → Layout, the floor list order below 840 px; the new intents are not in the Story 1.8 `proptest` generator (as for the group intents).
  evidence: Review findings from Verification Gap.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-23-floor-zones-and-grid-layout-per-branch.md`
  summary: Zone grid: no busy flag against rapid concurrent places (the second gets `STALE`); `free()` is quadratic per render; the PIN approval block sits above the group form, away from the group controls.
  evidence: Review findings from Blind Hunter.

## Story 5.3 (products)

- Images are stored in `products.image` and so in every change-log row and in `GET /ui/products`; move to a thumbnail or side table if catalogs grow large.
- The Hub reports `SetupStatus.products_count`; forwarding it in the heartbeat `setup` block and completing wizard step 6 by itself is Story 2.28.
- `catalog.changed` reaches the Hub UI and paired kiosk sessions as pushed; a kiosk that was offline needs a catalog read on connect (Epic 6 Shop).
- Stock and expiry changes must push `catalog.changed` too (Stories 5.5 and later); `quantity` is a constant 0 until the `stock` table exists.
- The kitchen flag is stored plain; Epic 7 stories register kitchen-notes and ticket intents with the Story 7.1 gate.
- Categories are a product attribute; renaming or merging a category is a bulk edit not built yet. CSV import (FR87) is not built.
- JPEG validation checks signature, size and header dimensions, not a full decode; the Hub UI always re-encodes.
- No automated test for a malformed `branch.tax` row at apply; 44 px targets and axe on the error and PIN states are not asserted.
## Deferred from Story 4.1 review

- source_spec: `_bmad-output/implementation-artifacts/spec-4-1-android-hub-app-builds-from-the-shared-hub-code-and-checks-t.md`
  summary: The Android keystore is the existing stub (`put` is unsupported), so the in-process Hub cannot pair or keep a token until Story 4.3.
  evidence: Blind Hunter; by design, 4.3 owns it.
- source_spec: `_bmad-output/implementation-artifacts/spec-4-1-android-hub-app-builds-from-the-shared-hub-code-and-checks-t.md`
  summary: Tighten the generated `file_paths.xml` FileProvider (whole external and cache paths) or drop the provider; add a manifest check on the merged APK manifest (the debug `usesCleartextTraffic` placeholder) and `aapt2 dump badging` plus ABI listing of the debug APK in CI.
  evidence: Blind Hunter and Verification Gap findings.
- source_spec: `_bmad-output/implementation-artifacts/spec-4-1-android-hub-app-builds-from-the-shared-hub-code-and-checks-t.md`
  summary: A WebView older than Chrome 87 (the Vite target) may never run the bundle, so the update screen cannot show; a delayed-invoke Playwright test for the "Checking…" state; the first-run PASS path on a device image with 20 GB free and 4 GB RAM; RAM failure on a device; x86_64, NDK 28 and the signed AAB run in CI.
  evidence: Verification Gap findings; none could run on this machine.

## Deferred from Story 2.13 review

- source_spec: `_bmad-output/implementation-artifacts/spec-2-13-branch-bootstrap-applies-the-onboarding-profile-at-first-pai.md`
  summary: Settings → Branch has no editable branch code field yet (the `settings.set` path, the `BRANCH_CODE_LOCKED` rule and the Tax screen lock note exist). A code edited at the Hub is not reported to the cloud, whose `(tenant, branch_code)` is unique, and the `branch.branch_code` column keeps the bootstrap value.
  evidence: Spec accepted limits; Blind Hunter and Edge Case Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-13-branch-bootstrap-applies-the-onboarding-profile-at-first-pai.md`
  summary: `lock_branch_code()` has no production caller until the invoice engine (Story 5.x) calls it; the caller must add the returned `RowKey` to its written rows so the lock is stamped and audited. Add an integration test then.
  evidence: Blind Hunter, Edge Case Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-13-branch-bootstrap-applies-the-onboarding-profile-at-first-pai.md`
  summary: Owner setup code hardening. Validate `setup_expires_at` against time; plaintext window starts at pairing and a never-beating Hub keeps the sealed code until a reveal or heartbeat (no sweeper); a re-pair needs the new Hub's first beat before reveal; the 40-bit code is brute-forceable offline from a stolen change log; the sealing key is the MFA key without a purpose label; `owner_setup_codes` has no cascade on branch removal; a lost reveal response loses the code until staff setup codes can be reissued (Story 2.16).
  evidence: Review findings from Blind Hunter and Edge Case Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-13-branch-bootstrap-applies-the-onboarding-profile-at-first-pai.md`
  summary: Test gaps. Status-code mappings for `BRANCH_CODE_LOCKED` in the three crates, the `CloudPairer` mapping of `ALREADY_BOOTSTRAPPED`, onboarding Playwright for Copy success and denied-clipboard states and the 409 path, E.164 length boundaries, `branch.seed_defaults` cannot seed the tax country of an upgraded branch.
  evidence: Verification Gap findings.


## Story 5.4 (bundles)

- Bundle availability uses the same constant component quantity (0 for tracked components) as products; Story 5.5 replaces `UNRECORDED_STOCK` in `arena_core::catalog` and must never create a `stock` row for a bundle. `quantity` is 0 for a bundle with no tracked component (available, unlimited).
- Nesting is enforced in `product.upsert` only; a change-log row replayed or synced from elsewhere is not re-checked (no database trigger). Revisit with cloud sync of catalog rows.
- `ComponentView.active` is false for a deleted component as for an inactive one, so the list says "Component inactive: <name>" for both; a `deleted` flag could tell them apart.
- `catalog::status` reads each component with its own query (bounded by 30 components per bundle); batch it if catalogs grow.
- Replay of change-log `products` rows written before migration 0018 (no `bundle` key) is not exercised.
- Hub UI: no test of the 44 px targets or axe with the components error visible; a fractional quantity such as "1.5" is refused client-side by the same message as 0.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-24-device-status-and-maintenance.md`
  summary: Online and offline are derived from the LAN registry and `last_seen_at` is in memory only (null after a Hub restart until the device reconnects); persist `last_seen_at` if the owner app or reports need it. Returning from maintenance restores a stored `in_session` or `reserved` prior state even if that session or reservation ended meanwhile; the sessions and reservations stories must clear or re-derive it. Each disconnect spawns a 15 s timer task that bumps presence even if the device has reconnected (extra snapshot refetches on a flapping link; add a per-device generation if it matters).
  evidence: Review findings from Blind Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-24-device-status-and-maintenance.md`
  summary: Settings → Devices status list: focus is lost to the page when Return to service removes the clicked button and is not returned to the opener when the form closes; buttons are not disabled while a write is in flight. `online` (a live socket now) and `status` (15 s grace) differ for up to 15 s by design.
  evidence: Review findings from Blind Hunter.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-24-device-status-and-maintenance.md`
  summary: Missing tests: kiosk-to-snapshot-to-`devices.changed` end to end within 2 s (needs the LAN harness), `ui_snapshot` with a live registry, Counter permission refusal and elevation scope for `device.maintenance` at the Rust level, change-log row for a maintenance write, all five statuses and their state tokens in Playwright, STALE and rejected-PIN paths of the maintenance form, a 360 px layout check, and a full-string Last seen assertion.
  evidence: Review findings from Verification Gap.
- source_spec: `_bmad-output/implementation-artifacts/spec-2-15-owner-changes-reach-the-hub-through-the-command-mailbox-with.md`
  summary: Mailbox gaps left for Epic 8 and the wizard stories: `epoch` is required on fetch and result but not fenced; `commands` and `applied_commands` rows are never pruned and a `fetched` row whose Hub never answers stays open; the Hub rejects `setup.test_session` and `branch.go_live` as `UNSUPPORTED` until Stories 2.29 and 2.31; `settings.set` for `branch.name` is accepted by the cloud but refused by the Hub until Story 2.30 gives the name a setting; the "updated by owner" toast covers only groups, zones and device placement and fires on every owner write there, not only an overwrite; the command Deployment copies the control Deployment's secrets because the binary loads one auth configuration for every role; no per-Hub rate limit on the mailbox routes.
  evidence: Own notes while implementing; not yet reviewed.

## Deferred from: code review of spec-2-15 (2026-10-26)

- source_spec: `_bmad-output/implementation-artifacts/spec-2-15-owner-changes-reach-the-hub-through-the-command-mailbox-with.md`
  summary: A cloud restore can rewind `commands.id` below the Hub's cursor or onto old `applied_commands` ids and keys; a command that always fails `INTERNAL` blocks the queue behind it; a non-`NotFound` 4xx on a result post stalls the mailbox; `check-deploy` ignores the command Deployment, its Ingress path and rollout; missing tests for the 50-row page boundary and an `INTERNAL` failure inside the write transaction.
  evidence: Review findings from Edge Case Hunter, Blind Hunter and Verification Gap.

## Deferred from: Story 3.3

- `arena-hub dev seed` fails with `USER_LIMIT` on master (it upserts 4 staff before the licence is recorded, Free cap is 3). `release-bench` `hub-idle` seeding and `tests/dev_seed.rs` (2 tests) fail until the seed records its licence first or caps staff. A 60-device seed for the Hub service memory budget (Story 3.3 AC4) also needs it.

## Deferred from: code review of spec-3-10 (2026-10-27)

- Lock reason is not persisted across a kiosk service restart; the cover falls back to "Locked by staff".
- A `session.tick` does not unlock a PC that missed `session.started` (offline or restarted); only Retry recovers it.
- Paused and grace sessions are ticked as active (Story 3.15).
- No test drives `Registry::tick_sessions`, a same-key `session.start` replay, or the 24 h `lock_after_ms` fallback.
- `time_up_mode` is carried to the kiosk but unused.

## Deferred from: code review of spec-3-11 (2026-10-27)

- A paused session's in-progress pause is billed as play time (Story 3.15).
- `paused_total_ms` is taken off the last segment only; per-segment pause records come with 3.15 and 3.31.
- The pay-at-counter lock is fire-and-forget; an offline kiosk stays unlocked until a persisted lock exists.
- Open orders are unbounded in the snapshot, and the sheet shows no itemised bill for an unpaid order (Story 3.12).

## Deferred from: Story 3.12

- `session.settle` takes cash, UPI and card only; wallet and credit rows come with 3.16 and 3.18 (the `payments.method` column has no CHECK so they need no table rebuild).
- The settle bar and payment split UI is Story 3.13; Undo and `session.settle.reverted` are Story 3.14, which also needs the irreversible-effect guard. `SnapshotBill.paid_minor` stays 0 because only open orders are listed.
- The idle lock after a settle is fire-and-forget like the pay-at-counter lock: an offline kiosk stays on "Pay at counter" until it reconnects or a new session starts.
- The Windows-only `arena-kiosk-svc` tests (lockdown, DPAPI, pipes), `arena-tasks` timing tests and `arena-cloud` `binary` test fail in this environment independent of this story; `crates/arena-kiosk-svc/tests/discovery.rs` has unused-import lint errors on Windows.

## Deferred from: Story 3.13

- The bar offers cash, UPI and card only; wallet and credit rows come with 3.16 and 3.18. The POS screen reuses `SettleBar` in Epic 5.
- The 10 s "two-method split from opening the bar" run on reference hardware is not measured here (only the functional keyboard flow is tested).
- Inline reference and amount errors appear on a Settle attempt, not on blur of the field.
- `apps/hub/src/routes/players/+page.svelte` fails `prettier --check` on master independent of this story; `audit.spec.ts`, `players.spec.ts`, `shell.spec.ts` and `session-end.spec.ts` have intermittent failures under parallel load on master.
- The Playwright headless-shell download is blocked in this environment; the suite was run with the full Chromium channel.

## Deferred from: story 3-14-undo-for-end-and-settle
- `effect.recorded` Hub event has no sender until Epic 5 records receipt/drawer/invoice effects; the server already rejects Undo when an effect exists.
- Undoing a Settle does not re-lock the kiosk back to "Pay at counter".

## Deferred from: Story 4.3

- The Gradle/Kotlin build, the JNI call into `KeystoreBridge` and the device checks (key is non-exportable, Keystore reset shows "This Hub needs to be paired again", token and key absent in clear on a rooted image) were not run: no Android target or SDK here. `keystore_jni.rs` was type-checked on the host only.
- An Android Hub paired before 4.3 holds a plain token file; it fails to unwrap and asks to pair again.
- Data encrypted with a lost branch key stays unreadable after a re-pair until key escrow and restore (Story 9.2); the owner cannot yet replace a failed Hub from Account → Branches (Story 9.8).
- The key is not StrongBox-backed and not bound to a lock screen; a device with no secure lock screen still keeps it in the hardware or software Keystore.
- "branch.db untouched while lost" is tested on the branch's data (staff, settings, change log); the file bytes still change on start (lease and WAL bookkeeping).
- Code review (4.3): a cross-branch code given to a lost Hub is consumed by the cloud before the Hub's `record_repairing` refuses it (needs a cloud-side branch hint on `/v1/pair`); no Hub-level test for that refusal yet.
- Code review (4.3): a re-pair always mints a fresh branch key, even when only the token file was lost and the key still unwraps (spec says fresh KEK); the re-pair writes `hub_identity` before the wrapped secrets, so a Keystore write failure needs another code.
- Code review (4.3): no cloud test for `GET /v1/wizard/install-hub` with a paid or comp tier; the pair button focus fix is tested with a mouse only, not touch; no R8 keep rule for `KeystoreBridge` (verify on the first minified release build).

## Deferred from: Story 4.4

- The Gradle/Kotlin build, `NsdBridge.kt`, `nsd_jni.rs` and `android.rs` on the Android target, a kiosk listing the tablet within 10 s, the 5 s re-register after a real IP change, and the Story 1.9 client-class tests on an emulator were not run: no Android target, SDK or emulator here. `nsd_jni.rs` was type-checked on the host in a scratch crate only.
- The network name is the transport ("Wi-Fi", "Ethernet") plus the SSID only when Android reveals it; the Hub requests no location permission, so the SSID is normally absent.
- The "No LAN connection" badge is shown only on the Android Hub (`lan.no_lan`); the Windows Hub keeps showing "No network address yet" in Settings → Devices.
- No Hub-level test injects a fake advertiser through `StartOptions`; `MdnsTask` is covered by unit tests with fakes. `arena-e2e` fails to compile on Windows (unix-only imports) and `test:scripts` has 12 kubectl-dependent failures, both independent of this story.
- No R8 keep rule for `NsdBridge` (its JNI-called static methods); verify on the first minified release build.

## Deferred from: Story 4.5

- The Gradle/Kotlin build and every device-only criterion were not run (no Android SDK, emulator or device here): the real Back gesture and 3-button Back through `MainActivity`'s callback (including that it stays on top of the Tauri `AppPlugin` callback), system bars, display cutouts and the gesture bar as padding in portrait and landscape on a phone and a tablet, the on-screen keyboard resizing the WebView and revealing the focused field, the system font scale (130% cap through `WebSettings.textZoom`, a live `fontScale` change) and rotation on hardware. The browser tests simulate Back with the `arena:back` event, the keyboard with a short window, and rotation with a viewport resize.
- On a phone the settle region is last in the sheet visually but stays in the middle of the markup (CSS `order`), so Tab reaches the settle bar before the actions drawn above it; a keyboard-only phone user is rare, so the markup was left alone.
- During a transfer or grouping pick on a phone the sheet is a bottom card (Story 3.5 behaviour), not full height, so the floor targets stay reachable.
- The Epic 5 phone POS (`phone-cart-bar`, full-height cart sheet) must register its own Back handler with `pushBack` and set `--sticky-bottom` on `.hub-main` for its sticky bar.
- The 44 px audit covers Floor, the tile sheets, Players, the Shift placeholder and the More sheet only; other screens are not audited yet.

## Deferred from: Story 4.6

- The Gradle/Kotlin build, `BluetoothBridge.kt`, `bt_jni.rs` on the Android target, the Android 12+ permission prompt (including the second refusal, where Android stops showing it), a real SPP printer (connect, a page, a printer switched off, out of range or dropped mid-print, and a reconnect on the next print) and the 10 s failure bound on a device were not run: no Android target, SDK, emulator or printer here. `bt_jni.rs` was not type-checked on the host.
- The permission prompt is wired through `HubBridge` (`bluetoothState`, `requestBluetooth`, `openAppSettings`) and a window event, not a Tauri plugin; AD-28 calls the Bluetooth piece a plugin, but Stories 4.3 and 4.4 set the JNI bridge pattern and this follows it. If a later story moves the bridges into Tauri plugins this one moves with them.
- Only a failed job to a Bluetooth printer raises `printer_offline`; a failed LAN job on Android is kept as the last job failure but raises no alert, so the Windows Hub's behaviour is unchanged. The alert has no sound (it has no device) and is cleared by a staff acknowledgement, not when the printer comes back.
- The last job failure is held in memory and is lost on a Hub restart; there is no job queue yet (receipts and KOTs arrive in Epics 5 and 7), so "the next print retries the connection" is a fresh connection per job, not a queued retry.
- A Bluetooth printer is matched by its device address; renaming it in Android settings changes the listed name, not the saved selection.
- `dev_seed`, `lan` and `printer` integration tests in `arena-hub` fail on master with `USER_LIMIT: the staff limit for this branch is reached` (checked with the work stashed), and `pnpm run check:tokens-used` fails on `--sticky-bottom` in `+layout.svelte` (Story 4.5); both are independent of this story.

## Deferred from: code review of Story 4.6

- `AndroidPrinter` is chosen only when the JNI bridge connects; if `bt_jni::connect` fails the Hub falls back to `SystemPrinter` and lists USB. A degraded Android start should still use `AndroidPrinter` with an `Unavailable` link.
- A `printer_offline` alert stays open after the printer recovers until staff acknowledge it; no per-printer job lock yet (jobs are single test prints until Epics 5 and 7).
- No `arena-hub` test starts the Hub with `StartOptions.bluetooth = Some(FakeBluetooth)`; blocked by the `USER_LIMIT` integration test failures on master.
- `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out` fail on master too (checked at the commit before this story).
- Bluetooth list is not filtered by device class; discovery and `access` are read separately in `find`.

## Deferred from: Story 4.7

- Nothing writes backups yet; the folder is only chosen, checked and watched. The export (Story 9.4/9.5) must write through `DocumentsContract` for a `content://` location (the bridge has the probe only), and raise the header `alert-badge` "Backup folder not available" (Story 9.16) from the same availability flag.
- The availability watch runs only when the shell supplies a `BackupFolder` (Android); a Windows path that goes missing after it was saved is not rechecked.
- A `PIN_REQUIRED` answer to a Manager-less save after the picker closed drops the picked folder (the page asks for the PIN, then opens the picker again); the Counter path asks for the PIN first, so this only happens if permissions change mid-flow.
- `backup_jni.rs`, `BackupFolderBridge.kt`, the folder picker, a USB OTG drive and an unplug are not exercised here (no Android build or device).

## Deferred from: code review of Story 4.7

- A picked folder the Hub refuses (probe failed) keeps its persisted grant until the next accepted save calls `keepBackupFolder`; repeated failed picks could accumulate grants (Android caps them). The page could call `keepBackupFolder(current)` after a refusal.

## Deferred from: Story 4.8

- The first AC asks for an Android emulator or device-farm run with a Windows kiosk VM on the same virtual LAN. Nothing here can drive the Android UI or a Windows VM, so the full path runs on the host: the real cloud, the real `arena-hub` binary with `--platform android` and a kiosk that speaks the LAN protocol (`free_android_golive.rs`). The `android-e2e.yml` emulator job only installs the debug APK and checks that it starts. A UI-driven run (Appium or Maestro through the WebView, plus a Windows kiosk VM that locks at zero for real) is still to be built. The workflow itself and the emulator job were not run here.
- The kiosk "locks at zero" step is the protocol client reporting `locked` after the `test_session` event, not a real lock screen (the kiosk lock is covered by Story 2.29).
- The test waits on the Hub's 10 s boot heartbeat between steps, so it takes about a minute; a `--heartbeat-interval` override would shorten it but is a product change.
- The ad-consent item is checked on the Free tier only; nothing here checks the banner (Stories 4.10 to 4.15).

## Deferred from: Story 4.9

- `staff.deactivate` is Owner-only and takes the Owner's own PIN at `POST /ui/staff/deactivate`, not the ticket flow of Story 1.13 (that flow refuses a member who already holds the permission). A Manager sees the list but has no button. The cloud's owner command mailbox cannot deactivate staff yet (Story 8.26).
- Nothing reactivates a deactivated member; `staff.upsert` with `active: true` would, and counts against the limit, but no screen offers it. Deactivated members are not listed.
- The Staff screen does not invite members: invites come from the wizard (Story 2.16) and the owner app (Story 8.26). The "Upgrade" link stays hidden behind `billingEnabled` until Story 10.5.
- The `DEVICE_LIMIT` check on Android runs against the in-process Hub on the host (the cap lowered in `branch.db` to the one paired kiosk); no Android device or emulator ran it.
- A deactivated member's open `/ui/events` stream ends with their next request or heartbeat, not by a push.

## Deferred from: Story 4.10

- The Kotlin module, its Gradle wiring, `ArenaAds.kt` and the Kotlin unit test were written without an Android toolchain and have not been compiled. Check the SDK 25.5.0 API names (`getCurrentOrientationAnchoredAdaptiveBannerAdSize`, `RequestConfiguration.toBuilder`, `InterstitialAd.load`) and AGP 9's handling of `org.jetbrains.kotlin.android` in a library module on first build.
- "Tauri plugin" here is a Kotlin-only Gradle module reached through a WebView interface (like `ArenaHub`), not a Tauri plugin with a Rust crate: a Rust plugin crate would put ad symbols in Rust, which the story forbids.
- `loadAd`, `InterstitialAd.load` and `show` run on the main thread because the SDK requires it; only SDK start-up runs on the plugin's worker thread. StrictMode (debug) logs main-thread network or disk access from the plugin; it does not fail the build.
- `check-ads.mjs ids` reads `BuildConfig.java` and the merged manifest under `build/`; the Gradle output paths were not verified. The release checklist (Story 4.16) should run it with `--require-production` and set `ARENA_ADMOB_*` from repository variables; CI currently builds release with the all-zero placeholder.
- The Windows executables are checked for plain strings only; Tauri compresses embedded assets, so the Hub and Kiosk web build folders are checked too.
- `entitlement.changed` is not sent as the first event of a new stream (the snapshot carries `tier`; the layout re-evaluates on every snapshot). A tier change between a snapshot load and the stream opening is picked up by the next snapshot.
- The gate does not render a banner slot, lifecycle pausing, the content rating (the plugin takes `rating`, the UI sends PG) or the interstitial rules: Stories 4.11 to 4.13.

## Deferred from: code review of Story 4.10

- The Kotlin unit test (`AdsConfigTest`) is not run by CI: add `./gradlew :tauri-plugin-arena-ads:testDebugUnitTest` once the module has been built on a real toolchain.
- Gate-off clears the banner only; a preloaded interstitial survives until the activity is destroyed (the page cannot show it once the gate is off). The SDK's manifest components and `APPLICATION_ID` meta-data ship in paid Android builds; `DELAY_APP_MEASUREMENT_INIT` is worth checking on the first device run.
- `check-ads.mjs` skips any directory named `build`, `dist` or `target`, also nested ones; no consent (UMP) or child-directed treatment yet (Story 4.16 release checklist).


## Deferred from: Story 4.11

- `ArenaAds.pause()`, `resume()` and the deferred banner request in `ArenaAds.kt`, and the `MainActivity` hooks, are uncompiled and unrun (no Android toolchain). Check on a device that the banner stops requesting with the screen off, that a banner tap returns to the same screen with its state kept (the activity must not be recreated), and that the 60px strip fits the anchored adaptive banner at 412 and 800 dp (a taller adaptive size is not clamped yet).
- The layout test runs in desktop Chromium with a fake `ArenaAds`: it checks geometry, not the native banner view over the strip.
- The phone More sheet, lock screen and full-height tile sheet now end at the slot, so the bottom bar stays visible (inert) beneath them instead of being covered.
- The strip is drawn on the sign-in screen and the unreachable state of a Free Android Hub, not on the pairing or device-check screens (the gate is off there).
- The bottom-bar items were 1px taller than the bar, which let the page scroll 1px; they are now 63px (`min-height: calc(var(--space-9) - 1px)`).

## Deferred from: code review of Story 4.11

- Layout test gaps: no case for the Undo dock, the phone picking card, the sign-in gate strip or the wide tile-sheet `--ad-shrink` height; `ArenaAds` pause/resume logic has no JVM unit test (extract the paused/requested state when a toolchain exists).
- The native `AdView` is not covered by `inert`, so a locked Hub's banner can still be tapped; `onPause` also fires for multi-window and system dialogs (consider `onStop`); a failed banner load is not retried until the width changes; the lock overlay on the phone sign-in gate stops 64px above the slot (`--ad-reserved` counts a bottom bar that gate does not draw).

## Deferred from: Story 4.12

- `ArenaAds.configure`, the `npa=1` request extras (`AdMobAdapter`) and the `personalised` flag in `AdsConfig` are uncompiled and unrun (no Android toolchain); check on a device with the SDK's ad inspector that a branch with no recorded consent sends non-personalised requests and that a stricter rating reaches the next request. The rating takes effect through the SDK's global request configuration, so an ad already loaded in the banner stays until its next refresh.
- The real AdMob blocking controls cannot be set or verified from the repo: `AD-CATEGORIES.md` says what must be blocked, and `check-ads.mjs note --release` fails until a named person fills in `Checked by`, `Checked on` and `Checked for release`. Nobody has signed it yet; the Story 4.16 release checklist must run it.
- Consent is a plain tick (Story 2.26's `ads.consent`), not a Google UMP consent form: `npa=1` is the only personalisation switch. EEA/UK users would need UMP; the product is India-first, so it is not built here.
- The consent line shows the staff id as stored, not the staff member's name (the Ads screen does not load the staff list).
- `branch.ads_rating` is a branch setting seeded by `branch.bootstrap` and `branch.seed_defaults`; a Windows or paid Hub carries it but nothing reads it there.

## Deferred from: Story 4.13

- The Kotlin side needed no change (`interstitialPreload`/`interstitialShow` and their events already existed) and, like the rest of the plugin, has not been run on a device: whether the Hub's alert sound and `devices.changed` handling keep running in the WebView while the SDK's full-screen activity is on top is only covered by a Playwright test that plays the plugin's events.
- Leaving through the ad is detected by `ad.dismissed`/`ad.failed`, a `visibilitychange` back to visible, or a 2-minute safety limit; the real behaviour of the SDK's close callback after a click-through is unverified on a device.
- The `phone-cart-bar` and POS cart (Epic 5) must call `registerAdExclusion` when they are built.

## Deferred from: Story 4.14

- The Grafana Cloud export of daily impressions and requests per slot is not built: it waits for Story 2.3's exporter. The cloud holds the totals (`gz_control.ad_impression_daily`, `ad_request_daily`) and `ControlDb::ad_fill` for fill rate; nothing calls the query yet.
- A replacement Hub restarts its row ids at 1 while the cloud cursor for the branch stays where it was: the cloud answers its older cursor, the Hub holds its cursor to the ids it has given, and counts are not added until the new ids pass the stored cursor (they are lost, never doubled). Needs an epoch or Hub id on the cursor.
- `after_id` is an addition to the block shape in the story (`{through_id, counts}`): without it a lost answer makes the Hub resend rows the cloud already added under a larger `through_id`, and they would count twice. A block without `after_id` is still accepted, held only to `through_id` beyond the stored cursor.
- The event post needs no staff session and the loopback guard lets a request with no `Origin` header through, so another app on the device could add counts to `ad_requests` / `ad_impressions`. The counts are advisory; nothing is billed on them.
- A post that cannot reach the Hub after five tries (about 3 minutes), or a page reload while one waits, loses that event: the waiting list is in memory.
- `hub_tier()` in `arena-core` is still a stub that answers Free, so the Hub does not itself refuse to record an ad on a paid tier; the UI gate keeps a paid or Windows Hub from posting, and the heartbeat sends the blocks only while the effective tier is Free.
- The cloud `record_ads` guard (an `UPDATE ... WHERE` that must change one row) was run against the local SQLite backend only; the remote (sqld) variant of the test skips without a server.
- Kotlin (`EventIds`, the `id` in the event script, `emit`) is uncompiled and unrun (no Android toolchain): check on a device that every `arena:ad` event carries a 26-character `id`.
- `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` fails on a Windows machine that booted less than 8 days ago (`Instant::checked_sub`), and `arena-cloud` `binary`/`backup` timeout tests and `printer::lan` `a_printer_that_stops_reading_times_out` fail in this sandbox; none touch this story.
- Review of Story 4.14 (deferred): the Hub event endpoint has no rate limit or row cap and the Hub does not refuse ads on a paid tier (see the `hub_tier()` stub above); the cloud does not tie a block's total to `through_id - after_id`, check the day is near cloud time, or log a refused block; `ad_fill` has no caller yet and its rate is only meaningful for closed days; `ArenaAds.kt` counts one request per banner width, not per SDK refresh.
- Story 4.15: the first `licence.apply` of a Hub inserts `licence.tier` (no previous image), so its first audit row has no "old" tier; a Hub whose writer is not primary keeps an unknown tier (gate off) until it is. The Kotlin `ArenaAds.stop()` has not been compiled or run on a device.

## Deferred from: Story 4.16

- Nothing here has run against real services: the Gradle/Kotlin build (`UpdateBridge.kt`, `app-update:2.1.0`, the `startUpdateFlowForResult` launcher), the `release-android.yml` workflow, and the Play Developer API calls (edits, bundles, tracks) are written from the documented API and tested only against a fake `fetch`.
- Operator steps remain: create the Play app and service account, `deploy/secrets/play-publisher.enc.yaml` and `android-upload.enc.yaml`, the `play-release` environment, the real age recipient and the `SOPS_SHA256` pin in `release-android.yml`; the Console forms (Contains ads, data safety, foreground-service declaration, device catalogue) are entered by hand from `deploy/play/listing.json`, which `check:play` only compares with the build.
- The Play pre-launch report cannot be read through the API: `promote` trusts a hand-written `prelaunch.json`. The heartbeat-gap metric for a version has no query yet (the halt rule takes the number as an argument); `promote --since` is typed by the release manager because the current stage's start time is not stored.
- The install rule is enforced by the page (open sessions from the snapshot); the Kotlin `install()` does not check sessions itself. The Play prompt for a flexible update appears when the check finds one, which can be mid-shift.
- `release-android.yml` repeats the AAB build steps of the `android` job in `ci.yml`; fold them into one reusable workflow when both are next touched. The tag is not compared with the app version (both are `0.1.0`, workspace version).

## Deferred from: Story 4.17

- Nothing here has run on a device or a farm: `run.sh`, the `android-farm.yml` workflow (APK build, arm64 seeding tool, runner labels and `FARM_*` variables), `adb` Doze control, `dumpsys gfxinfo` framestats and the gateway captures are written from the documented tools and tested only against fakes and fixtures. The `android_fleet` load and verify phases ran against a local Hub (6 to 8 s load), not for 60 minutes.
- Apply latency is measured as the round trip of `POST /ui/intent` (over `adb forward` on a device), an upper bound on server-side apply; the Hub exposes no apply timing of its own.
- The kill is at the end of the load, not during it (the test needs its Hub for the whole load); the in-flight timer probes and the resend of every acknowledged intent stand in for "during". The core-level kill-during-burst check stays in `arena-core` `floor_load` (Story 3.37).
- Frame blocking is read from `gfxinfo` for the whole run, not only while an ad loads, and `gfxinfo` sees the native window, not WebView compositing; an ad-load marker from `ArenaAds.kt` would narrow it.
- Only the reserved probe device's time-up is timed. Time-up locks on kiosks are not asserted (the fake kiosks do not draw), and a kiosk receiving the `pc_command` lock is not checked.
- `dev seed` now applies a roomy licence (100 devices, 10 users) before the fixture staff: the four fixture staff already broke `dev seed` and `dev_seed`/`lan` tests under the Free user cap (Story 4.9), and the fleet needs 20 devices. The pairing licence recorded at the end puts the Free caps back, so the fleet revokes the seeded "Dev Kiosk" to stay under the cap of 20.
- The egress check needs gateway captures the lab must provide (`FARM_GATEWAY_SYN`, `FARM_GATEWAY_DNS`); without them the step is skipped. Ad destinations are matched by DNS name from the resolver's log, so a CDN IP shared with a non-ad name fails the check by design.

## Deferred from: Story 5.7

- The expiry check rides the hourly housekeeping sweep, so the "Expiring soon" alert and the `catalog.changed` push for newly expired products land within an hour of the business day turning over, not at the exact start. Availability itself is computed from the business day at read time, so an expired lot is held from sale at once; only the push to already-open kiosk/POS views waits for the sweep.
- Expiry state is branch-wide: one alert for all lots, no per-lot alert rows, no undo of a write-off, and the Counter-with-PIN path for `stock.write_off` is covered by a mocked Playwright run only.
- The price of expired stock sitting in waste/stock reports (Story 5.31) will need `stock_moves.reason = 'expired'` grouped with waste.
- (Review 5.7) If a `stock.in` raises today's alert after the business day turns but before the first sweep, `raised_today` makes the check a no-op, so newly expired products miss that day's `catalog.changed` touch (availability itself is still right at read time). `stock.waste` / `stock.in.reverted` also leave a stale alert until the next hourly check, and changing the warn days does not re-evaluate the alert until then.

## Deferred from: Story 5.8

- The low-stock check compares the quantity on hand (`stock.qty`), which still counts expired lots that Story 5.7 holds from sale. A product whose remaining stock is all expired does not alert until the expired lot is written off (that decrement crosses the threshold); a lot passing its date writes nothing, so it neither raises nor clears the alert. Comparing against sellable quantity would need the daily expiry check to re-evaluate thresholds.
- "Clears" is an acknowledgement by the actor of the stock-in or revert, so the record cannot tell "restocked" from "dismissed by staff"; Epic 8's owner alert reads the alert row's `product_id`, `qty`, `threshold` and `raised_at`.
- A system `alert.raise` with kind `stock_low` is not refused and would create a product-less alert.
- Story 5.11 (sale) must call `decrement()` for the alert to fire on sales; today only waste, write-off and stock-in revert reach it in production.

## Deferred from: code review of story-5-18 (2026-10-28)

- Drawer kick has apply-level tests through `pos.sale` only; `session.settle` and `plan.sell` share `queue_on_settle` but lack a drawer test.
- `drawer.kicked` applied after the sale's shift is closed appends a `drawer_open` event to that closed shift; Story 5.26 must ignore zero-amount `drawer_open` rows in variance and decide whether late kicks are allowed.
- A failed kick leaves the sale non-undoable and there is no retry or manual "open drawer" action.

## Deferred from: Story 5.24

- The last partial refund of a tax-exclusive counter sale recomputes tax once per rate per refund, so the credit notes of one order may differ from the original tax by a minor unit; a final refund that clears every line can then be refused `REFUND_EXCEEDS_PAID` by that unit.
- A refund of a session order that carried play time cannot give the minutes back to a wallet or pack; play-time lines are refunded as money only.
- Refund approvals above a threshold (Epic 14) and large-refund owner alerts (Epic 8) are not wired; the credit note has no WhatsApp channel.
- `drawer.kicked` is not queued for a cash refund; the drawer opens only for sales.
- The Sessions screen has no Refund entry; a refund starts from POS.
- Credit not yet collected is cut using the player's current outstanding; a refund of a sale whose credit was partly collected through other orders cuts only what is still owed.

## Deferred from: Story 5.25

- An Undo of a cash sale after a deposit or drawer expense can take the shift's expected cash below zero (the compensating `sale_reverted` row is not checked against the drawer); Story 5.26 must read a negative expectation as is.
- The expense list, deposit slips and receipt photos are stored but have no viewer yet (Z-report, Story 5.28, and Reports, Story 5.27, will read them); the Shift screen shows only a confirmation.
- Owner-paid expenses are recorded against the open shift for the audit trail but never move the drawer; they are not synced to a petty-cash ledger (Epic 14).
- `expected_cash` ignores zero-amount `drawer_open` rows by construction; late kicks after close are still undecided (see Story 5.18 notes).
