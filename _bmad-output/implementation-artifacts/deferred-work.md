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
