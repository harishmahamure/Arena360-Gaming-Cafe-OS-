//! The Players → Overdue list (Story 8.31): everyone with credit outstanding, oldest unpaid spend
//! first, with the aging bucket, whether the player agreed to messages and, on a paid branch, the
//! day of the last credit reminder that went out. A read only: it never queues a reminder.

use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, CREDIT_BUCKET_0_7, CREDIT_BUCKET_8_14, CREDIT_BUCKET_15_PLUS,
    CREDIT_OVERDUE_DAYS, KEY_LICENCE_TIER, OverdueRow, OverdueView, tier_is_paid,
};
use arena_store::Store;

use crate::SnapshotReader;
use crate::locale::current_business_day;
use crate::players::{KEK_LEN, open_field};

fn bucket(age_days: i64) -> &'static str {
    if age_days >= CREDIT_OVERDUE_DAYS {
        CREDIT_BUCKET_15_PLUS
    } else if age_days >= 8 {
        CREDIT_BUCKET_8_14
    } else {
        CREDIT_BUCKET_0_7
    }
}

/// The overdue list as of `now_ms`. Names are opened with `kek`; a player whose key is gone is
/// `erased` with an empty name.
pub fn overdue_view(
    store: &Store,
    kek: &[u8; KEK_LEN],
    now_ms: i64,
) -> Result<OverdueView, AppError> {
    {
        let Some(today) = current_business_day(store, now_ms)? else {
            return Ok(OverdueView {
                as_of_day: String::new(),
                reminders: false,
                rows: Vec::new(),
            });
        };
        let reminders = store
            .settings()?
            .iter()
            .find(|s| s.key == KEY_LICENCE_TIER)
            .is_some_and(|s| tier_is_paid(&s.value));
        let mut rows = Vec::new();
        for d in store.debtors(&today)? {
            let name = match (&d.name_enc, &d.dek_wrapped) {
                (Some(enc), Some(wrapped)) => {
                    open_field(kek, &d.player_id, wrapped, "name", enc).ok()
                }
                _ => None,
            };
            rows.push(OverdueRow {
                consent: store
                    .latest_consent(&d.player_id, CONSENT_TRANSACTIONAL)?
                    .map(|c| c.0),
                last_reminder_day: if reminders {
                    store.last_credit_reminder_sent(&d.player_id)?
                } else {
                    None
                },
                erased: name.is_none(),
                name: name.unwrap_or_default(),
                outstanding_minor: d.outstanding_minor,
                oldest_unpaid_day: d.aging.oldest_unpaid_day.unwrap_or_default(),
                age_days: d.aging.oldest_age_days,
                bucket: bucket(d.aging.oldest_age_days).to_owned(),
                player_id: d.player_id,
            });
        }
        rows.sort_by(|a, b| {
            // Rows with no spend on record (a limit-only balance) sort last.
            let key =
                |r: &OverdueRow| (r.oldest_unpaid_day.is_empty(), r.oldest_unpaid_day.clone());
            key(a)
                .cmp(&key(b))
                .then_with(|| a.player_id.cmp(&b.player_id))
        });
        Ok(OverdueView {
            as_of_day: today,
            reminders,
            rows,
        })
    }
}

impl SnapshotReader {
    /// The overdue list as of `now_ms` (see [`overdue_view`]).
    pub fn overdue(&self, kek: &[u8; KEK_LEN], now_ms: i64) -> Result<OverdueView, AppError> {
        self.with_store(|store| overdue_view(store, kek, now_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bucket_follows_the_age_of_the_oldest_spend() {
        assert_eq!(bucket(0), "0_7");
        assert_eq!(bucket(7), "0_7");
        assert_eq!(bucket(8), "8_14");
        assert_eq!(bucket(14), "8_14");
        assert_eq!(bucket(15), "15_plus");
    }
}
