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
