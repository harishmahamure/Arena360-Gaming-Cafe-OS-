// Generated from the tier table in crates/arena-proto/src/tier.rs by
// `UPDATE_TIERS_SNAPSHOT=1 cargo test -p arena-proto tier_snapshot` (Story 11.3).
// Do not edit: CI fails when it differs from the table. `null` is no cap.
export const TIERS_SNAPSHOT = [
  {
    "branches": 1,
    "devices": 20,
    "flags": [],
    "retention_days": 0,
    "tier": "free",
    "upload_interval_s": 0,
    "users": 3
  },
  {
    "branches": null,
    "devices": 60,
    "flags": [
      "owner_app",
      "kiosk_orders",
      "kitchen",
      "standby"
    ],
    "retention_days": 365,
    "tier": "basic",
    "upload_interval_s": 900,
    "users": null
  },
  {
    "branches": null,
    "devices": 200,
    "flags": [
      "owner_app",
      "kiosk_orders",
      "kitchen",
      "standby",
      "tunnel",
      "live_view"
    ],
    "retention_days": 1095,
    "tier": "premium",
    "upload_interval_s": 300,
    "users": null
  }
] as const;
