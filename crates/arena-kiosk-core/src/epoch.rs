//! The highest Hub epoch a device has seen (Story 9.21, AD-24).
//!
//! After a promote the standby is the main Hub with a higher epoch. A device that connects to it
//! raises its floor to that epoch and from then on refuses any Hub with a lower one, so a
//! returning old main Hub (or a stale mDNS answer) can never take the floor back. The device
//! keeps the floor on disk and presents it again on its next start.

use arena_proto::{AppError, ErrorCode, epoch_parts};

/// The epoch of a Hub as an ordered pair; `(counter, instance)` orders lexicographically, as on
/// the Hub.
fn rank(epoch: &str) -> Option<(i64, &str)> {
    epoch_parts(epoch)
}

/// The highest epoch seen so far.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EpochFloor(Option<String>);

impl EpochFloor {
    /// A floor restored from storage. Text that is not an epoch is forgotten rather than trusted.
    pub fn restored(stored: Option<&str>) -> Self {
        Self(
            stored
                .map(str::trim)
                .filter(|e| rank(e).is_some())
                .map(str::to_owned),
        )
    }

    /// The highest epoch seen, `<counter>:<instance>`.
    pub fn highest(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Whether a Hub holding `epoch` may be connected to. A Hub that states none (no lease yet,
    /// not the primary) is let through and does not move the floor. A lower epoch than the floor
    /// is refused `FENCED`; an equal or higher one is let through and raises the floor. Returns
    /// whether the floor rose, so the caller knows to store it.
    pub fn admit(&mut self, epoch: Option<&str>) -> Result<bool, AppError> {
        let Some(epoch) = epoch else {
            return Ok(false);
        };
        let Some(seen) = rank(epoch) else {
            return Err(AppError::new(
                ErrorCode::BadFrame,
                "the Hub stated an epoch that is not one",
            ));
        };
        if let Some(floor) = self.0.as_deref().and_then(rank)
            && seen < floor
        {
            return Err(AppError::new(
                ErrorCode::Fenced,
                format!(
                    "This Hub is no longer the main Hub (its epoch {epoch} is below {}).",
                    self.0.as_deref().unwrap_or_default()
                ),
            ));
        }
        let rises = self.0.as_deref().and_then(rank).is_none_or(|f| seen > f);
        if rises {
            self.0 = Some(epoch.to_owned());
        }
        Ok(rises)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_hub_sets_the_floor_and_a_higher_one_raises_it() {
        let mut floor = EpochFloor::default();
        assert_eq!(floor.admit(Some("3:MAIN")), Ok(true));
        assert_eq!(floor.highest(), Some("3:MAIN"));
        // The same Hub again changes nothing.
        assert_eq!(floor.admit(Some("3:MAIN")), Ok(false));
        // The promoted standby: a higher counter.
        assert_eq!(floor.admit(Some("4:STBY")), Ok(true));
        assert_eq!(floor.highest(), Some("4:STBY"));
    }

    #[test]
    fn a_lower_epoch_is_refused_and_the_floor_stays() {
        let mut floor = EpochFloor::restored(Some("4:STBY"));
        let err = floor.admit(Some("3:MAIN")).unwrap_err();
        assert_eq!(err.code, ErrorCode::Fenced);
        assert!(err.message.contains("3:MAIN") && err.message.contains("4:STBY"));
        assert_eq!(floor.highest(), Some("4:STBY"));
        // Counters compare as numbers, not text: 10 is above 9.
        let mut floor = EpochFloor::restored(Some("9:A"));
        assert_eq!(floor.admit(Some("10:A")), Ok(true));
        assert!(floor.admit(Some("9:Z")).is_err());
    }

    #[test]
    fn the_same_counter_orders_by_instance() {
        let mut floor = EpochFloor::restored(Some("4:B"));
        assert!(floor.admit(Some("4:A")).is_err());
        assert_eq!(floor.admit(Some("4:C")), Ok(true));
    }

    #[test]
    fn a_hub_with_no_epoch_is_let_through_and_junk_is_not() {
        let mut floor = EpochFloor::restored(Some("4:STBY"));
        assert_eq!(floor.admit(None), Ok(false));
        assert_eq!(floor.highest(), Some("4:STBY"));
        assert_eq!(
            floor.admit(Some("junk")).unwrap_err().code,
            ErrorCode::BadFrame
        );
        assert_eq!(EpochFloor::restored(Some("junk")).highest(), None);
        assert_eq!(EpochFloor::restored(None).highest(), None);
    }
}
