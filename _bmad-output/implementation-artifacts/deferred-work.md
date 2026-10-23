- source_spec: `_bmad-output/implementation-artifacts/spec-1-3-generated-protocol-types-and-the-message-catalogue-lint.md`
  summary: Give `ErrorCode` (and other wire enums) a forward-compatible unknown-value fallback so older Kiosks/clients don't fail to deserialize codes added by a newer Hub.
  evidence: `ErrorCode` is a closed serde enum with no `#[serde(other)]` variant, and the Hub must accept Kiosks from the previous two releases (AD-8). There's no wire reader yet; decide this in Story 1.9 (LAN protocol envelope).
