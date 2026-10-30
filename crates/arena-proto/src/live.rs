//! Live view (Story 8.40): a read-only floor for the owner, through the Cloudflare Tunnel.
//!
//! The cloud signs a short-lived [`LiveClaims`] token for one branch; the owner app presents it
//! in `hello` (class `live_view`) to the Hub's tunnel listener; the Hub pushes [`LiveFloor`]
//! frames. A live token is signed with the licence key but has its own claims (`aud` and no
//! tier), so neither kind of token can pass as the other.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::licence::{LICENCE_CLOCK_SKEW_S, LicenceError, VerifyKey, decode_signed};

/// The longest life of a live token.
pub const LIVE_TOKEN_TTL_S: i64 = 5 * 60;
/// The `aud` claim of a live token.
pub const LIVE_TOKEN_AUD: &str = "live";
/// The WebSocket path the cloud hands out; the Hub also answers `/`.
pub const LIVE_PATH: &str = "/live";
/// The Hub pushes the floor at least this often (milliseconds).
pub const LIVE_TICK_MS: u64 = 1_000;

/// What a live token says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LiveClaims {
    pub aud: String,
    pub tenant_id: String,
    pub branch_id: String,
    #[ts(type = "number")]
    pub iat: i64,
    #[ts(type = "number")]
    pub exp: i64,
}

impl LiveClaims {
    /// Claims for `branch_id` issued at `iat_s` (epoch seconds) with the longest life.
    pub fn new(tenant_id: &str, branch_id: &str, iat_s: i64) -> Self {
        Self {
            aud: LIVE_TOKEN_AUD.to_owned(),
            tenant_id: tenant_id.to_owned(),
            branch_id: branch_id.to_owned(),
            iat: iat_s,
            exp: iat_s + LIVE_TOKEN_TTL_S,
        }
    }
}

/// Why a live token was refused. Every kind is `TOKEN_INVALID` on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveTokenError {
    Licence(LicenceError),
    WrongAudience,
    WrongBranch,
    Expired,
    NotYetValid,
}

impl std::fmt::Display for LiveTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Licence(e) => write!(f, "{e}"),
            Self::WrongAudience => f.write_str("not a live view token"),
            Self::WrongBranch => f.write_str("the token is for another branch"),
            Self::Expired => f.write_str("the live view token has expired"),
            Self::NotYetValid => f.write_str("the live view token was issued in the future"),
        }
    }
}

impl std::error::Error for LiveTokenError {}

/// Verifies `token` for `branch_id` at `now_ms`: signature (by `keys`), audience, branch, a life
/// of at most [`LIVE_TOKEN_TTL_S`], and the time window (with the licence's clock skew).
pub fn verify_live_token(
    token: &str,
    keys: &[VerifyKey<'_>],
    branch_id: &str,
    now_ms: i64,
) -> Result<LiveClaims, LiveTokenError> {
    let claims: LiveClaims = decode_signed(token, keys).map_err(LiveTokenError::Licence)?;
    if claims.aud != LIVE_TOKEN_AUD {
        return Err(LiveTokenError::WrongAudience);
    }
    if claims.branch_id != branch_id {
        return Err(LiveTokenError::WrongBranch);
    }
    match claims.exp.checked_sub(claims.iat) {
        Some(life) if life > 0 && life <= LIVE_TOKEN_TTL_S => {}
        _ => return Err(LiveTokenError::Licence(LicenceError::Malformed)),
    }
    let now_s = now_ms.div_euclid(1000);
    if claims.iat > now_s.saturating_add(LICENCE_CLOCK_SKEW_S) {
        return Err(LiveTokenError::NotYetValid);
    }
    if now_s >= claims.exp {
        return Err(LiveTokenError::Expired);
    }
    Ok(claims)
}

/// The body of `live.token`: a fresh token for the same branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LiveToken {
    pub token: String,
}

/// The answer of `GET /v1/live/{branch}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LiveGrant {
    /// The Hub's WebSocket through the tunnel, `wss://<branch>.tunnel…/live`.
    pub url: String,
    /// The live token for `hello.credential`.
    pub token: String,
    /// When the token expires (epoch ms).
    #[ts(type = "number")]
    pub expires_at: i64,
}

/// What one tile of the live floor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum LiveTileState {
    Online,
    Offline,
    Active,
    Paused,
    Grace,
    Expired,
    Reserved,
    Maintenance,
}

/// The running session on a tile: times and who, nothing about money.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LiveTileSession {
    #[ts(type = "number")]
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub paused_at: Option<i64>,
    #[ts(type = "number")]
    pub paused_total_ms: i64,
    /// When a timed session ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub ends_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub grace_ends_at: Option<i64>,
    /// The player's id; absent for a guest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub guest_no: Option<i64>,
}

/// One device on the live floor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LiveTile {
    pub device_id: String,
    pub label: String,
    /// The zone id, or absent when Unassigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub zone: Option<String>,
    pub state: LiveTileState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub session: Option<LiveTileSession>,
}

/// A floor zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LiveZone {
    pub id: String,
    pub name: String,
}

/// The whole floor, pushed as the `live.floor` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LiveFloor {
    /// The Hub's clock when this was read (epoch ms): the data age is measured from it.
    #[ts(type = "number")]
    pub generated_at: i64,
    pub zones: Vec<LiveZone>,
    pub tiles: Vec<LiveTile>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::licence::{DEV_LICENCE_VERIFY_KEYS, LicenceClaims, dev_sign_any};
    use crate::tier::Tier;

    const NOW_S: i64 = 1_800_000_000;
    const NOW_MS: i64 = NOW_S * 1000;

    fn token(claims: &LiveClaims) -> String {
        dev_sign_any(claims).unwrap()
    }

    #[test]
    fn a_fresh_token_for_this_branch_passes() {
        let c = LiveClaims::new("t", "b1", NOW_S);
        let got = verify_live_token(&token(&c), DEV_LICENCE_VERIFY_KEYS, "b1", NOW_MS).unwrap();
        assert_eq!(got, c);
    }

    #[test]
    fn another_branch_an_expired_token_and_a_long_life_are_refused() {
        let c = LiveClaims::new("t", "b1", NOW_S);
        let keys = DEV_LICENCE_VERIFY_KEYS;
        assert_eq!(
            verify_live_token(&token(&c), keys, "b2", NOW_MS).unwrap_err(),
            LiveTokenError::WrongBranch
        );
        assert_eq!(
            verify_live_token(&token(&c), keys, "b1", NOW_MS + 301_000).unwrap_err(),
            LiveTokenError::Expired
        );
        let mut long = c.clone();
        long.exp = long.iat + LIVE_TOKEN_TTL_S + 1;
        assert!(matches!(
            verify_live_token(&token(&long), keys, "b1", NOW_MS),
            Err(LiveTokenError::Licence(LicenceError::Malformed))
        ));
        let mut future = c;
        future.iat += 3600;
        future.exp += 3600;
        assert_eq!(
            verify_live_token(&token(&future), keys, "b1", NOW_MS).unwrap_err(),
            LiveTokenError::NotYetValid
        );
    }

    #[test]
    fn a_licence_token_is_not_a_live_token_and_the_reverse() {
        let licence = LicenceClaims::for_tier("t", "b1", Tier::Premium, NOW_S);
        let licence_token = dev_sign_any(&licence).unwrap();
        assert!(matches!(
            verify_live_token(&licence_token, DEV_LICENCE_VERIFY_KEYS, "b1", NOW_MS),
            Err(LiveTokenError::Licence(LicenceError::Malformed))
        ));
        let live = token(&LiveClaims::new("t", "b1", NOW_S));
        assert!(crate::licence::verify_claims(&live, DEV_LICENCE_VERIFY_KEYS).is_err());
        let mut wrong_aud = LiveClaims::new("t", "b1", NOW_S);
        wrong_aud.aud = "licence".into();
        assert_eq!(
            verify_live_token(&token(&wrong_aud), DEV_LICENCE_VERIFY_KEYS, "b1", NOW_MS)
                .unwrap_err(),
            LiveTokenError::WrongAudience
        );
    }

    #[test]
    fn no_keys_refuse_everything() {
        let t = token(&LiveClaims::new("t", "b1", NOW_S));
        assert_eq!(
            verify_live_token(&t, &[], "b1", NOW_MS).unwrap_err(),
            LiveTokenError::Licence(LicenceError::NoKeys)
        );
    }

    #[test]
    fn the_floor_carries_no_money_or_names() {
        let json = serde_json::to_string(&LiveTile {
            device_id: "d".into(),
            label: "PC 1".into(),
            zone: None,
            state: LiveTileState::Active,
            session: Some(LiveTileSession {
                started_at: 1,
                paused_at: None,
                paused_total_ms: 0,
                ends_at: None,
                grace_ends_at: None,
                player_id: None,
                guest_no: Some(3),
            }),
        })
        .unwrap();
        assert!(!json.contains("username") && !json.contains("balance"));
        assert!(json.contains("\"guest_no\":3"));
    }
}
