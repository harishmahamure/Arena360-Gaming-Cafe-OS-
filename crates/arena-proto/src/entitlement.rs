use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::licence::{self, LicenceError, VerifyKey};
use crate::tier::{Tier, TierLimits};

pub const CLOCK_SKEW_LIMIT_MS: i64 = 5 * 60 * 1000;
const MAX_FLAGS_CHARS: usize = 256;

pub fn clock_wrong(wall_ms: i64, trusted_ms: i64) -> bool {
    wall_ms.saturating_sub(trusted_ms).unsigned_abs() > CLOCK_SKEW_LIMIT_MS as u64
}

fn flag_name(f: &str) -> bool {
    !f.is_empty()
        && f.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum LicenceStanding {
    Verified,
    Expired,
    Unverified,
}

impl LicenceStanding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Expired => "expired",
            Self::Unverified => "unverified",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entitlement {
    pub standing: LicenceStanding,
    pub tier: Tier,
    pub flags: Vec<String>,
    pub limits: TierLimits,
}

impl Entitlement {
    pub fn free(standing: LicenceStanding) -> Self {
        let d = Tier::Free.defaults();
        Self {
            standing,
            tier: Tier::Free,
            flags: d.flags.iter().map(|f| (*f).to_owned()).collect(),
            limits: d.limits,
        }
    }

    pub fn evaluate(
        jwt: Option<&str>,
        keys: &[VerifyKey<'_>],
        tenant_id: &str,
        branch_id: &str,
        trusted_now_ms: i64,
    ) -> Self {
        let Some(jwt) = jwt else {
            return Self::free(LicenceStanding::Unverified);
        };
        let Ok(claims) = licence::verify_claims(jwt, keys) else {
            return Self::free(LicenceStanding::Unverified);
        };
        if claims.tenant_id != tenant_id || claims.branch_id != branch_id {
            return Self::free(LicenceStanding::Unverified);
        }
        if !claims.flags.iter().all(|f| flag_name(f))
            || claims.flags.join(",").len() > MAX_FLAGS_CHARS
        {
            return Self::free(LicenceStanding::Unverified);
        }
        match licence::check_time(&claims, trusted_now_ms) {
            Ok(()) => Self {
                standing: LicenceStanding::Verified,
                tier: claims.tier,
                flags: claims.flags,
                limits: claims.limits,
            },
            Err(LicenceError::Expired) if claims.tier == Tier::Free => {
                Self::free(LicenceStanding::Verified)
            }
            Err(LicenceError::Expired) => Self::free(LicenceStanding::Expired),
            Err(_) => Self::free(LicenceStanding::Unverified),
        }
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    pub fn limit(&self, name: &str) -> Option<u32> {
        match name {
            "devices" => Some(self.limits.devices),
            "users" => Some(self.limits.users),
            "branches" => Some(self.limits.branches),
            _ => None,
        }
    }

    pub fn apply_payload(&self, clock_wrong: bool) -> LicenceApply {
        LicenceApply {
            standing: self.standing,
            clock_wrong,
            flags: self.flags.join(","),
            device_cap: self.limits.devices,
            user_cap: self.limits.users,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LicenceApply {
    pub standing: LicenceStanding,
    pub clock_wrong: bool,
    pub flags: String,
    pub device_cap: u32,
    pub user_cap: u32,
}

impl LicenceApply {
    pub fn flags_valid(&self) -> bool {
        self.flags.chars().count() <= MAX_FLAGS_CHARS
            && (self.flags.is_empty() || self.flags.split(',').all(flag_name))
    }

    pub fn rows(&self) -> [(&'static str, String); 5] {
        [
            (crate::KEY_LICENCE_FLAGS, self.flags.clone()),
            (crate::KEY_DEVICE_CAP, self.device_cap.to_string()),
            (crate::KEY_USER_CAP, self.user_cap.to_string()),
            (crate::KEY_LICENCE_STATUS, self.standing.as_str().to_owned()),
            (
                crate::KEY_LICENCE_CLOCK,
                if self.clock_wrong { "wrong" } else { "ok" }.to_owned(),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::licence::{DEV_LICENCE_VERIFY_KEYS, LicenceClaims, dev_sign};

    const T0: i64 = 1_800_000_000;

    fn token(tier: Tier, tenant: &str, branch: &str) -> String {
        dev_sign(&LicenceClaims::for_tier(tenant, branch, tier, T0)).unwrap()
    }

    fn eval(jwt: Option<&str>, now_ms: i64) -> Entitlement {
        Entitlement::evaluate(jwt, DEV_LICENCE_VERIFY_KEYS, "T", "B", now_ms)
    }

    #[test]
    fn a_genuine_current_licence_carries_its_tier_flags_and_limits() {
        let mut c = LicenceClaims::for_tier("T", "B", Tier::Basic, T0);
        c.flags = vec!["kitchen".into()];
        let e = eval(Some(&dev_sign(&c).unwrap()), T0 * 1000);
        assert_eq!(e.standing, LicenceStanding::Verified);
        assert_eq!((e.tier, e.limit("devices")), (Tier::Basic, Some(60)));
        assert!(e.has_flag("kitchen") && !e.has_flag("tunnel"));
        assert_eq!(e.limit("nope"), None);
    }

    #[test]
    fn a_missing_garbled_foreign_or_other_branch_licence_is_free_and_unverified() {
        let ok = token(Tier::Premium, "T", "B");
        let tampered = ok.replacen('.', ".x", 1);
        let cases = [
            None,
            Some("junk".to_owned()),
            Some(tampered),
            Some(token(Tier::Premium, "T", "OTHER")),
            Some(token(Tier::Premium, "OTHER", "B")),
        ];
        for jwt in cases {
            let e = eval(jwt.as_deref(), T0 * 1000);
            assert_eq!(e.standing, LicenceStanding::Unverified, "{jwt:?}");
            assert_eq!(e.tier, Tier::Free, "{jwt:?}");
            assert_eq!(
                e.limits,
                TierLimits {
                    devices: 20,
                    users: 3,
                    branches: 1
                },
                "{jwt:?}"
            );
            assert!(e.flags.is_empty());
        }
        let no_keys = Entitlement::evaluate(Some(&ok), &[], "T", "B", T0 * 1000);
        assert_eq!(no_keys.standing, LicenceStanding::Unverified);
    }

    #[test]
    fn a_licence_without_a_limits_claim_does_not_verify() {
        let mut claims =
            serde_json::to_value(LicenceClaims::for_tier("T", "B", Tier::Premium, T0)).unwrap();
        claims.as_object_mut().unwrap().remove("limits");
        assert!(serde_json::from_value::<LicenceClaims>(claims).is_err());
    }

    #[test]
    fn an_expired_paid_licence_falls_back_to_free_and_a_free_one_never_warns() {
        let after = (T0 + licence::LICENCE_TTL_S + 1) * 1000;
        let paid = eval(Some(&token(Tier::Basic, "T", "B")), after);
        assert_eq!(paid.standing, LicenceStanding::Expired);
        assert_eq!((paid.tier, paid.limits.devices), (Tier::Free, 20));
        let free = eval(Some(&token(Tier::Free, "T", "B")), after);
        assert_eq!(
            (free.standing, free.tier),
            (LicenceStanding::Verified, Tier::Free)
        );
    }

    #[test]
    fn a_licence_from_the_future_is_unverified() {
        let e = eval(Some(&token(Tier::Basic, "T", "B")), (T0 - 3600) * 1000);
        assert_eq!(e.standing, LicenceStanding::Unverified);
    }

    #[test]
    fn the_apply_payload_round_trips_the_five_setting_rows() {
        let e = eval(Some(&token(Tier::Basic, "T", "B")), T0 * 1000);
        let p = e.apply_payload(true);
        assert!(p.flags_valid());
        let rows = p.rows();
        assert_eq!(rows[1], (crate::KEY_DEVICE_CAP, "60".to_owned()));
        assert_eq!(rows[3].1, "verified");
        assert_eq!(rows[4].1, "wrong");
        let bad = LicenceApply {
            flags: "Kitchen".into(),
            ..p
        };
        assert!(!bad.flags_valid());
    }

    #[test]
    fn an_unknown_key_a_bad_signature_and_bad_flags_are_unverified() {
        let ok = token(Tier::Basic, "T", "B");
        let other = [VerifyKey {
            kid: "someone-else",
            public_key: DEV_LICENCE_VERIFY_KEYS[0].public_key,
        }];
        let e = Entitlement::evaluate(Some(&ok), &other, "T", "B", T0 * 1000);
        assert_eq!(e.standing, LicenceStanding::Unverified);
        let mut forged = ok.clone();
        let last = forged.pop().unwrap();
        forged.push(if last == 'A' { 'B' } else { 'A' });
        assert_eq!(
            eval(Some(&forged), T0 * 1000).standing,
            LicenceStanding::Unverified
        );
        for bad in ["Kitchen", "kit-chen", ""] {
            let mut c = LicenceClaims::for_tier("T", "B", Tier::Basic, T0);
            c.flags = vec![bad.into()];
            let e = eval(Some(&dev_sign(&c).unwrap()), T0 * 1000);
            assert_eq!(e.standing, LicenceStanding::Unverified, "{bad:?}");
        }
        let mut c = LicenceClaims::for_tier("T", "B", Tier::Basic, T0);
        c.flags = vec!["kitchen2".into()];
        assert!(eval(Some(&dev_sign(&c).unwrap()), T0 * 1000).has_flag("kitchen2"));
    }

    #[test]
    fn the_expiry_instant_is_expired_and_the_second_before_is_not() {
        let basic = token(Tier::Basic, "T", "B");
        let exp = T0 + licence::LICENCE_TTL_S;
        assert_eq!(
            eval(Some(&basic), (exp - 1) * 1000 + 999).standing,
            LicenceStanding::Verified
        );
        assert_eq!(
            eval(Some(&basic), exp * 1000).standing,
            LicenceStanding::Expired
        );
    }

    #[test]
    fn every_paid_tier_carries_the_kitchen_flag_and_free_does_not() {
        for (tier, kitchen) in [
            (Tier::Free, false),
            (Tier::Basic, true),
            (Tier::Premium, true),
        ] {
            let e = eval(Some(&token(tier, "T", "B")), T0 * 1000);
            assert_eq!(e.has_flag(crate::FLAG_KITCHEN), kitchen, "{tier:?}");
            assert!(e.apply_payload(false).flags_valid());
        }
    }

    #[test]
    fn the_skew_limit_is_exclusive_in_both_directions() {
        let limit = CLOCK_SKEW_LIMIT_MS;
        assert!(!clock_wrong(1_000 + limit, 1_000));
        assert!(!clock_wrong(1_000 - limit, 1_000));
        assert!(clock_wrong(1_001 + limit, 1_000));
        assert!(clock_wrong(999 - limit, 1_000));
        assert!(clock_wrong(i64::MIN, i64::MAX));
    }

    #[test]
    fn flag_lists_follow_the_grammar() {
        let p = |flags: &str| LicenceApply {
            standing: LicenceStanding::Verified,
            clock_wrong: false,
            flags: flags.into(),
            device_cap: 1,
            user_cap: 1,
        };
        for ok in ["", "a", "a,b_2"] {
            assert!(p(ok).flags_valid(), "{ok:?}");
        }
        for bad in ["A", "a,,b", "a b", ",a", "a,", &"a".repeat(257)] {
            assert!(!p(bad).flags_valid(), "{bad:?}");
        }
    }
}
