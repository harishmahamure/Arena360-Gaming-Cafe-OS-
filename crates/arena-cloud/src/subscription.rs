//! Branch subscriptions (Story 10.2): a branch is subscribed at the PPP band its billing country
//! fixes, on the live price book, and both are copied onto the subscription so a later publish
//! never reprices it. This module validates the request, quotes the price, reserves the branch,
//! creates the Razorpay subscription and hands back what Checkout needs; and it sweeps the
//! `pending` subscriptions nobody paid for. Webhooks, activation and entitlements are Story 10.3.

use std::ffi::OsString;
use std::time::Duration;

use arena_proto::{
    AddOns, Band, BillingCycle, CreateSubscription, MandateMethod, PriceItem, SubscriptionCheckout,
    SubscriptionState, Tier,
};
use arena_store::control::{
    Branch, ControlDb, NewOpsTask, NewSubscription, PriceBookItem, ReserveOutcome, Subscription,
};
use serde_json::json;

use crate::backup::{AlertTarget, JobError, code, open, report_failure};
use crate::billing::{BillingProvider, CancelResult, SubscriptionResult, SubscriptionSpec};
use crate::config::{Database, JobConfig, Task};

/// GST on band IN, in percent. Prices in the book are GST-exclusive.
pub const GST_PERCENT: i64 = 18;

/// The most packs of 20 devices a subscription may add.
pub const MAX_DEVICE_PACKS: u32 = 10;

/// An unpaid `pending` subscription is abandoned after this long.
pub const PENDING_TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// What the owner is told when UPI Autopay is not offered.
pub const RENEWAL_NOTICE: &str = "Renewals over Rs 15,000 need a card or bank mandate";

/// The `ops_tasks.kind` a manual provider leaves for a subscription it did not cancel.
pub const TASK_CANCEL_SUBSCRIPTION: &str = "razorpay_cancel_subscription";

/// A sweep gives up (and alerts) after this long, inside the 15-minute schedule.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How many stale subscriptions one sweep looks at.
const SWEEP_BATCH: u32 = 1000;

/// A renewal debit above this (GST included, minor units) cannot use UPI Autopay: Rs 15,000 on
/// band IN. The other bands are approximate conversions and only matter once they are on sale.
pub fn renewal_limit_minor(band: Band) -> i64 {
    match band {
        Band::In => 1_500_000,
        Band::My => 85_000,
        Band::Sg => 24_000,
        Band::Us => 18_000,
    }
}

/// The payment methods a renewal of `total_minor` may use, and the notice when UPI is left out
/// because the debit is over the limit. Outside band IN only a card is offered.
pub fn allowed_methods(band: Band, total_minor: i64) -> (Vec<MandateMethod>, Option<&'static str>) {
    if band != Band::In {
        return (vec![MandateMethod::Card], None);
    }
    if total_minor > renewal_limit_minor(band) {
        (
            vec![MandateMethod::Card, MandateMethod::Emandate],
            Some(RENEWAL_NOTICE),
        )
    } else {
        (
            vec![
                MandateMethod::Upi,
                MandateMethod::Card,
                MandateMethod::Emandate,
            ],
            None,
        )
    }
}

/// The band a billing country belongs to, or `None` when it has none.
pub fn band_for_country(country: &str) -> Option<Band> {
    Band::parse(&country.trim().to_ascii_uppercase())
}

/// Why a request is refused before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub field: &'static str,
    pub message: &'static str,
}

/// Checks the tier and add-ons and answers the price item of the tier. Free is never billed,
/// Owner login is not for sale yet, Tunnel is one at most and only on Basic (Premium includes it).
pub fn validate(tier: Tier, add_ons: &AddOns) -> Result<PriceItem, Invalid> {
    let item = match tier {
        Tier::Basic => PriceItem::Basic,
        Tier::Premium => PriceItem::Premium,
        Tier::Free => {
            return Err(Invalid {
                field: "tier",
                message: "Free is never billed. Choose Basic or Premium.",
            });
        }
    };
    if add_ons.devices_20 > MAX_DEVICE_PACKS {
        return Err(Invalid {
            field: "add_ons.devices_20",
            message: "Add 0 to 10 packs of 20 devices.",
        });
    }
    if add_ons.tunnel > 1 || (add_ons.tunnel == 1 && tier == Tier::Premium) {
        return Err(Invalid {
            field: "add_ons.tunnel",
            message: "Tunnel is one add-on on Basic and is included with Premium.",
        });
    }
    if add_ons.owner_login > 0 {
        return Err(Invalid {
            field: "add_ons.owner_login",
            message: "Owner login is not available yet.",
        });
    }
    Ok(item)
}

/// What a subscription costs per cycle, in minor units of `currency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub band: Band,
    pub currency: String,
    /// Tier plus add-ons, GST excluded.
    pub amount_minor: i64,
    /// 18% of `amount_minor`, rounded half up, on band IN; else 0.
    pub tax_minor: i64,
    /// What each renewal debits.
    pub total_minor: i64,
}

/// Why a quote could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuoteError {
    /// The book has no price for the item in this band and cycle.
    NoPrice(String),
    Db(String),
}

/// Prices a tier and add-ons from the rows of one price book.
pub fn quote_items(
    items: &[PriceBookItem],
    band: Band,
    tier: PriceItem,
    cycle: BillingCycle,
    add_ons: &AddOns,
) -> Result<Quote, QuoteError> {
    let find = |item: PriceItem| {
        items
            .iter()
            .find(|i| i.band == band && i.item == item && i.cycle == cycle)
            .ok_or_else(|| {
                QuoteError::NoPrice(format!(
                    "{} {} {}",
                    band.as_str(),
                    item.as_str(),
                    cycle.as_str()
                ))
            })
    };
    let tier_row = find(tier)?;
    let mut amount = tier_row.amount_minor;
    let lines = [
        (PriceItem::AddonDevices20, add_ons.devices_20),
        (PriceItem::AddonTunnel, add_ons.tunnel),
        (PriceItem::AddonOwnerLogin, add_ons.owner_login),
    ];
    for (item, qty) in lines {
        if qty == 0 {
            continue;
        }
        let row = find(item)?;
        amount = row
            .amount_minor
            .checked_mul(i64::from(qty))
            .and_then(|n| amount.checked_add(n))
            .ok_or_else(|| QuoteError::NoPrice("amount out of range".to_owned()))?;
    }
    let tax = if tier_row.gst_applicable {
        (amount * GST_PERCENT + 50) / 100
    } else {
        0
    };
    Ok(Quote {
        band,
        currency: tier_row.currency.clone(),
        amount_minor: amount,
        tax_minor: tax,
        total_minor: amount + tax,
    })
}

/// Prices a tier and add-ons from the price book `book_id`.
pub async fn quote(
    db: &ControlDb,
    book_id: &str,
    band: Band,
    tier: PriceItem,
    cycle: BillingCycle,
    add_ons: &AddOns,
) -> Result<Quote, QuoteError> {
    let items = db
        .price_book_items(book_id)
        .await
        .map_err(|e| QuoteError::Db(e.to_string()))?;
    quote_items(&items, band, tier, cycle, add_ons)
}

/// What a subscription costs now: always priced from the subscription's own price book, never
/// the live one, so a later publish does not reprice it.
pub async fn quote_for(db: &ControlDb, sub: &Subscription) -> Result<Quote, QuoteError> {
    let tier = match sub.tier {
        Tier::Basic => PriceItem::Basic,
        Tier::Premium => PriceItem::Premium,
        Tier::Free => return Err(QuoteError::NoPrice("free".to_owned())),
    };
    let add_ons = AddOns {
        devices_20: sub.addon_devices_20,
        tunnel: sub.addon_tunnel,
        owner_login: sub.addon_owner_login,
    };
    quote(db, &sub.price_book_id, sub.band, tier, sub.cycle, &add_ons).await
}

/// Why a subscription could not be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionError {
    Invalid(Invalid),
    /// The billing country has no band, or its band is off sale (`BAND_NOT_AVAILABLE`).
    BandNotAvailable,
    /// The branch already has a subscription that is not over (`SUBSCRIPTION_EXISTS`).
    Exists(SubscriptionState),
    /// The provider could not create the subscription; the branch is free to retry (503).
    Provider(String),
    /// No book is live, or it has no price for the choice (503).
    Pricing(String),
    Db(String),
}

fn db_err(e: arena_store::control::ControlError) -> SubscriptionError {
    SubscriptionError::Db(e.to_string())
}

fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// Creates the subscription of `branch`: validates the request, quotes the live book, reserves
/// the branch (a `pending` row holding the band and book), creates the Razorpay subscription and
/// answers what Checkout needs. Validation, the band and the existing-subscription check all run
/// before the provider is called; a provider failure abandons the row so the branch can retry.
pub async fn create(
    db: &ControlDb,
    provider: &BillingProvider,
    branch: &Branch,
    req: &CreateSubscription,
    now: i64,
) -> Result<SubscriptionCheckout, SubscriptionError> {
    let item = validate(req.tier, &req.add_ons).map_err(SubscriptionError::Invalid)?;
    let band = band_for_country(&req.billing_country).ok_or(SubscriptionError::BandNotAvailable)?;
    if !db.band_enabled(band).await.map_err(db_err)? {
        return Err(SubscriptionError::BandNotAvailable);
    }
    if let Some(open) = db
        .subscription_of_branch(&branch.id)
        .await
        .map_err(db_err)?
    {
        return Err(SubscriptionError::Exists(open.state));
    }
    let book = db
        .live_price_book()
        .await
        .map_err(db_err)?
        .ok_or_else(|| SubscriptionError::Pricing("no live price book".to_owned()))?;
    let q = quote(db, &book.id, band, item, req.cycle, &req.add_ons)
        .await
        .map_err(|e| match e {
            QuoteError::NoPrice(m) => SubscriptionError::Pricing(m),
            QuoteError::Db(m) => SubscriptionError::Db(m),
        })?;
    let (methods, notice) = allowed_methods(band, q.total_minor);

    let id = format!("sb_{}", new_id().to_lowercase());
    let new = NewSubscription {
        id: id.clone(),
        tenant_id: branch.tenant_id.clone(),
        branch_id: branch.id.clone(),
        tier: req.tier,
        cycle: req.cycle,
        band,
        currency: q.currency.clone(),
        price_book_id: book.id.clone(),
        addon_devices_20: req.add_ons.devices_20,
        addon_tunnel: req.add_ons.tunnel,
        addon_owner_login: req.add_ons.owner_login,
    };
    match db.reserve_subscription(&new, now).await.map_err(db_err)? {
        ReserveOutcome::Reserved => {}
        ReserveOutcome::Exists(state) => return Err(SubscriptionError::Exists(state)),
    }

    let spec = SubscriptionSpec {
        subscription_id: id.clone(),
        branch_id: branch.id.clone(),
        book_id: book.id.clone(),
        band,
        item,
        cycle: req.cycle,
        currency: q.currency.clone(),
        total_minor: q.total_minor,
        methods: methods.clone(),
    };
    let razorpay_id = match provider.create_subscription(&spec).await {
        Ok(SubscriptionResult::Created(rid)) => rid,
        Ok(SubscriptionResult::Deferred) => {
            release(db, &id, now).await;
            return Err(SubscriptionError::Provider(
                "the billing provider is manual".to_owned(),
            ));
        }
        Err(e) => {
            release(db, &id, now).await;
            return Err(SubscriptionError::Provider(e.0));
        }
    };
    let Some(key_id) = provider.key_id() else {
        release(db, &id, now).await;
        return Err(SubscriptionError::Provider("no checkout key".to_owned()));
    };
    if !db
        .set_razorpay_subscription(&id, &razorpay_id, now)
        .await
        .map_err(db_err)?
    {
        return Err(SubscriptionError::Db(
            "the subscription changed while it was being created".to_owned(),
        ));
    }
    tracing::info!(
        branch_id = %branch.id,
        subscription_id = %id,
        band = band.as_str(),
        price_book = %book.id,
        "subscription created, awaiting payment"
    );
    Ok(SubscriptionCheckout {
        subscription_id: id,
        state: SubscriptionState::Pending,
        key_id: key_id.to_owned(),
        razorpay_subscription_id: razorpay_id,
        band,
        currency: q.currency,
        tier: req.tier,
        cycle: req.cycle,
        methods,
        notice: notice.map(str::to_owned),
        amount_minor: q.amount_minor,
        tax_minor: q.tax_minor,
        total_minor: q.total_minor,
    })
}

/// Abandons the row of a call that failed before it had a Razorpay subscription, so the branch
/// can try again. A database failure here is logged: the sweep abandons the row within 24 hours.
async fn release(db: &ControlDb, id: &str, now: i64) {
    match db.abandon_subscription(id, None, now).await {
        Ok(true) => {}
        Ok(false) => tracing::warn!(subscription_id = %id, "subscription already left pending"),
        Err(e) => {
            tracing::error!(subscription_id = %id, error = %e, "could not abandon the subscription");
        }
    }
}

/// What one sweep did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SweepOutcome {
    /// Stale subscriptions cancelled at the provider (or left to an operator) and abandoned.
    pub abandoned: u64,
    /// Subscriptions the provider failed to cancel; the next run tries again.
    pub failed: u64,
}

/// Abandons the unpaid `pending` subscriptions older than 24 hours: each is cancelled at the
/// provider first and only then abandoned, so a provider failure leaves it for the next run. A
/// row that never got a Razorpay subscription is simply abandoned; under a `manual` provider the
/// cancel is left to an operator as an ops task.
pub async fn sweep_pending(
    db: &ControlDb,
    provider: &BillingProvider,
    now: i64,
) -> Result<SweepOutcome, JobError> {
    let failed = |e: arena_store::control::ControlError| {
        JobError::new(code::SUBSCRIPTION_SWEEP_FAILED, e.to_string())
    };
    let stale = db
        .stale_pending_subscriptions(now - PENDING_TTL_MS, SWEEP_BATCH)
        .await
        .map_err(failed)?;
    let mut out = SweepOutcome::default();
    for sub in stale {
        let mut task_details = None;
        if let Some(rid) = &sub.razorpay_subscription_id {
            match provider.cancel_subscription(rid).await {
                Ok(CancelResult::Cancelled) => {}
                Ok(CancelResult::Deferred) => {
                    task_details = Some(
                        json!({
                            "razorpay_subscription_id": rid,
                            "branch_id": sub.branch_id,
                        })
                        .to_string(),
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        subscription_id = %sub.id,
                        code = %e,
                        "could not cancel an unpaid subscription at the provider; retrying next run"
                    );
                    out.failed += 1;
                    continue;
                }
            }
        }
        let task_id = new_id();
        let task = task_details.as_deref().map(|d| NewOpsTask {
            id: &task_id,
            kind: TASK_CANCEL_SUBSCRIPTION,
            target: &sub.id,
            details_json: d,
        });
        if db
            .abandon_subscription(&sub.id, task.as_ref(), now)
            .await
            .map_err(failed)?
        {
            out.abandoned += 1;
        }
    }
    tracing::info!(
        task = Task::SubscriptionSweep.as_str(),
        abandoned = out.abandoned,
        failed = out.failed,
        "unpaid subscriptions swept"
    );
    Ok(out)
}

/// `--task subscription-sweep`: opens `gz_control` (no migration), resolves the billing provider
/// from the environment and sweeps once; a failure (including a timeout) goes through the alert
/// path. Returns whether it succeeded.
pub async fn run_job(
    job: &JobConfig,
    env: &impl Fn(&str) -> Option<OsString>,
    target: &AlertTarget,
) -> bool {
    let run = async {
        let database = match &job.dev_data_dir {
            Some(dir) => Database::Local {
                data_dir: dir.clone(),
            },
            None => crate::config::remote_database(env)
                .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?,
        };
        let provider = crate::billing::provider_from_env(job.dev_data_dir.is_some(), env)
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
        let db = open(&database)
            .await
            .map_err(|e| JobError::new(code::SUBSCRIPTION_SWEEP_FAILED, e.to_string()))?;
        sweep_pending(&db, &provider, crate::now_ms()).await
    };
    let res = match tokio::time::timeout(RUN_TIMEOUT, run).await {
        Ok(r) => r,
        Err(_) => Err(JobError::new(
            code::SUBSCRIPTION_SWEEP_TIMEOUT,
            format!(
                "the sweep did not finish within {} s",
                RUN_TIMEOUT.as_secs()
            ),
        )),
    };
    match res {
        Ok(_) => true,
        Err(e) => {
            report_failure(Task::SubscriptionSweep, &e, target).await;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(band: Band, item: PriceItem, cycle: BillingCycle, amount: i64) -> PriceBookItem {
        PriceBookItem {
            band,
            currency: band.currency().to_owned(),
            item,
            cycle,
            amount_minor: amount,
            gst_applicable: band.gst_applicable(),
            razorpay_plan_id: None,
        }
    }

    fn book() -> Vec<PriceBookItem> {
        let mut rows = Vec::new();
        for (band, basic, premium, pack, tunnel) in [
            (Band::In, 20_000, 199_900, 5_000, 20_000),
            (Band::Us, 600, 5_900, 150, 600),
        ] {
            for (cycle, mul) in [(BillingCycle::Monthly, 1), (BillingCycle::Yearly, 10)] {
                rows.push(row(band, PriceItem::Basic, cycle, basic * mul));
                rows.push(row(band, PriceItem::Premium, cycle, premium * mul));
                rows.push(row(band, PriceItem::AddonDevices20, cycle, pack * mul));
                rows.push(row(band, PriceItem::AddonTunnel, cycle, tunnel * mul));
            }
        }
        rows
    }

    #[test]
    fn countries_map_to_bands_and_nothing_else_does() {
        assert_eq!(band_for_country("IN"), Some(Band::In));
        assert_eq!(band_for_country(" my "), Some(Band::My));
        assert_eq!(band_for_country("SG"), Some(Band::Sg));
        assert_eq!(band_for_country("US"), Some(Band::Us));
        assert_eq!(band_for_country("GB"), None);
        assert_eq!(band_for_country(""), None);
        assert_eq!(band_for_country("INR"), None);
    }

    #[test]
    fn validation_refuses_free_owner_login_and_a_second_or_included_tunnel() {
        let none = AddOns::default();
        assert_eq!(validate(Tier::Basic, &none), Ok(PriceItem::Basic));
        assert_eq!(validate(Tier::Premium, &none), Ok(PriceItem::Premium));
        assert_eq!(validate(Tier::Free, &none).unwrap_err().field, "tier");
        let with = |devices_20, tunnel, owner_login| AddOns {
            devices_20,
            tunnel,
            owner_login,
        };
        assert!(validate(Tier::Basic, &with(10, 1, 0)).is_ok());
        for (tier, add_ons, field) in [
            (Tier::Basic, with(11, 0, 0), "add_ons.devices_20"),
            (Tier::Basic, with(0, 2, 0), "add_ons.tunnel"),
            (Tier::Premium, with(0, 1, 0), "add_ons.tunnel"),
            (Tier::Basic, with(0, 0, 1), "add_ons.owner_login"),
        ] {
            assert_eq!(validate(tier, &add_ons).unwrap_err().field, field);
        }
    }

    #[test]
    fn gst_applies_to_band_in_only_and_rounds_half_up() {
        let rows = book();
        let q = quote_items(
            &rows,
            Band::In,
            PriceItem::Basic,
            BillingCycle::Yearly,
            &AddOns::default(),
        )
        .unwrap();
        assert_eq!(
            (q.amount_minor, q.tax_minor, q.total_minor),
            (200_000, 36_000, 236_000)
        );
        let q = quote_items(
            &rows,
            Band::Us,
            PriceItem::Basic,
            BillingCycle::Monthly,
            &AddOns::default(),
        )
        .unwrap();
        assert_eq!(
            (q.currency.as_str(), q.tax_minor, q.total_minor),
            ("USD", 0, 600)
        );
        // 18% of 5,025 paise is 904.5: rounds half up to 905.
        let mut odd = book();
        odd.iter_mut()
            .find(|r| {
                r.band == Band::In && r.item == PriceItem::Basic && r.cycle == BillingCycle::Monthly
            })
            .unwrap()
            .amount_minor = 5_025;
        let q = quote_items(
            &odd,
            Band::In,
            PriceItem::Basic,
            BillingCycle::Monthly,
            &AddOns::default(),
        )
        .unwrap();
        assert_eq!(q.tax_minor, 905);
    }

    #[test]
    fn add_ons_are_priced_by_quantity_and_premium_yearly_is_the_documented_total() {
        let rows = book();
        let q = quote_items(
            &rows,
            Band::In,
            PriceItem::Premium,
            BillingCycle::Yearly,
            &AddOns::default(),
        )
        .unwrap();
        assert_eq!(q.total_minor, 2_358_820);
        let q = quote_items(
            &rows,
            Band::In,
            PriceItem::Basic,
            BillingCycle::Monthly,
            &AddOns {
                devices_20: 2,
                tunnel: 1,
                owner_login: 0,
            },
        )
        .unwrap();
        // 20,000 + 2 x 5,000 + 20,000
        assert_eq!(q.amount_minor, 50_000);
        assert!(
            quote_items(
                &rows,
                Band::My,
                PriceItem::Basic,
                BillingCycle::Monthly,
                &AddOns::default()
            )
            .is_err()
        );
    }

    #[test]
    fn a_renewal_over_the_limit_drops_upi_and_says_why() {
        let all = vec![
            MandateMethod::Upi,
            MandateMethod::Card,
            MandateMethod::Emandate,
        ];
        assert_eq!(allowed_methods(Band::In, 236_000), (all.clone(), None));
        assert_eq!(allowed_methods(Band::In, 1_500_000), (all, None));
        assert_eq!(
            allowed_methods(Band::In, 1_500_001),
            (
                vec![MandateMethod::Card, MandateMethod::Emandate],
                Some(RENEWAL_NOTICE)
            )
        );
        assert_eq!(
            allowed_methods(Band::Us, 600),
            (vec![MandateMethod::Card], None)
        );
        assert_eq!(renewal_limit_minor(Band::In), 1_500_000);
    }
}
