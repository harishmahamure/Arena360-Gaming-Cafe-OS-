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
