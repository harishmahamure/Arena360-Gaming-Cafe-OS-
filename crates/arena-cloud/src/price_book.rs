//! The price book service (Story 10.1): what `GET /v1/pricing` serves and what the audited
//! `arena-cloud ops price-book` commands do. A book is edited only as a draft; publishing it
//! mirrors every row to a Razorpay plan through the [`BillingProvider`] and only then, in one
//! transaction, makes it live and retires the previous book.

use arena_proto::{Band, BillingCycle, PriceItem, Pricing};
use arena_store::control::{
    BookAudit, ControlDb, DraftOutcome, MirroredPlan, NewOpsTask, PriceBookItem, PriceBookStatus,
    PublishOutcome, SetAmountOutcome,
};
use serde_json::json;

use crate::billing::{BillingError, BillingProvider, PlanResult, PlanSpec};

/// The `ops_tasks.kind` a manual provider leaves for the plans it did not create.
pub const TASK_CREATE_PLANS: &str = "razorpay_create_plans";

/// The largest monthly amount an operator may set, in minor units.
pub const MAX_MONTHLY_MINOR: i64 = 100_000_000;

/// Why pricing cannot be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PricingError {
    /// Paid plans are not on sale in the band yet (`BAND_NOT_AVAILABLE`).
    BandNotAvailable(Band),
    /// No book is live.
    NoLiveBook,
    Db(String),
}

/// The live book's lines for `band`, or why there are none.
pub async fn pricing(db: &ControlDb, band: Band) -> Result<Pricing, PricingError> {
    let db_err = |e: arena_store::control::ControlError| PricingError::Db(e.to_string());
    if !db.band_enabled(band).await.map_err(db_err)? {
        return Err(PricingError::BandNotAvailable(band));
    }
    let book = db
        .live_price_book()
        .await
        .map_err(db_err)?
        .ok_or(PricingError::NoLiveBook)?;
    let lines = db.price_lines(&book.id, band).await.map_err(db_err)?;
    Ok(Pricing {
        band,
        currency: band.currency().to_owned(),
        price_book_version: book.version,
        lines,
    })
}

/// What an ops command did, for its summary lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishSummary {
    pub book_id: String,
    pub version: u32,
    pub provider: &'static str,
    /// Plans the provider created now.
    pub plans_created: usize,
    /// Plans left to an operator (`manual`).
    pub plans_deferred: usize,
    /// A live book that only gained plan ids was not re-published.
    pub synced_only: bool,
    pub audit_id: String,
}

impl PublishSummary {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "{} price book {} (version {}) with provider {}: {} plan(s) created",
            if self.synced_only {
                "synced"
            } else {
                "published"
            },
            self.book_id,
            self.version,
            self.provider,
            self.plans_created
        )];
        if self.plans_deferred > 0 {
            lines.push(format!(
                "{} plan(s) left for an operator: see the open {TASK_CREATE_PLANS} ops task",
                self.plans_deferred
            ));
        }
        lines.push(format!("ops_audit {}", self.audit_id));
        lines
    }
}

/// Why an ops command on the book failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    UnknownBook(String),
    /// The book is live or retired: `PRICE_BOOK_IMMUTABLE`.
    Immutable(String),
    /// The book is retired, so it cannot be published.
    Retired(String),
    /// The live book already has every plan; there is nothing to publish.
    AlreadyPublished(String),
    /// A draft already exists.
    DraftExists(String),
    NoLiveBook,
    UnknownRow(String),
    BadAmount(i64),
    /// A draft must price every band, item and cycle before it can go live.
    Incomplete {
        have: usize,
        want: usize,
    },
    /// The provider failed; nothing was written.
    Provider(BillingError),
    /// Another publish got there first; nothing was written.
    Raced,
    Db(String),
}

impl std::fmt::Display for BookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownBook(id) => write!(f, "UNKNOWN_PRICE_BOOK: no price book {id:?}"),
            Self::Immutable(id) => write!(
                f,
                "PRICE_BOOK_IMMUTABLE: price book {id:?} is published; prices change only by publishing a new version"
            ),
            Self::Retired(id) => write!(f, "PRICE_BOOK_RETIRED: price book {id:?} is retired"),
            Self::AlreadyPublished(id) => write!(
                f,
                "ALREADY_PUBLISHED: price book {id:?} is live and every plan exists"
            ),
            Self::DraftExists(id) => {
                write!(f, "DRAFT_EXISTS: price book {id:?} is already a draft")
            }
            Self::Incomplete { have, want } => write!(
                f,
                "PRICE_BOOK_INCOMPLETE: the draft has {have} prices, a book needs {want} (every band, item and cycle)"
            ),
            Self::NoLiveBook => write!(f, "NO_LIVE_PRICE_BOOK: there is no live book to copy"),
            Self::UnknownRow(what) => write!(f, "UNKNOWN_PRICE_ROW: {what}"),
            Self::BadAmount(n) => write!(
                f,
                "BAD_AMOUNT: {n} is not between 1 and {MAX_MONTHLY_MINOR} minor units"
            ),
            Self::Provider(e) => write!(
                f,
                "BILLING_PROVIDER_FAILED: {e}; nothing was published, run the command again"
            ),
            Self::Raced => write!(
                f,
                "PUBLISH_RACED: the book changed while publishing; nothing was written"
            ),
            Self::Db(m) => write!(f, "CONTROL_DB_FAILED: {m}"),
        }
    }
}

impl std::error::Error for BookError {}

fn db_err(e: arena_store::control::ControlError) -> BookError {
    BookError::Db(e.to_string())
}

fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// `price-book draft`: a new draft copied from the live book.
pub async fn open_draft(
    db: &ControlDb,
    operator: &str,
    reason: &str,
    now: i64,
) -> Result<(String, u32), BookError> {
    let book_id = format!("pb_{}", new_id().to_lowercase());
    let audit_id = new_id();
    let details = json!({ "from": "live" }).to_string();
    let audit = BookAudit {
        id: &audit_id,
        operator,
        action: "price_book_draft",
        target: &book_id,
        reason,
        details_json: &details,
    };
    match db
        .open_price_book_draft(&book_id, &audit, now)
        .await
        .map_err(db_err)?
    {
        DraftOutcome::Created { id, version } => Ok((id, version)),
        DraftOutcome::DraftExists(id) => Err(BookError::DraftExists(id)),
        DraftOutcome::NoLiveBook => Err(BookError::NoLiveBook),
    }
}

/// `price-book set`: the monthly amount of one item in a draft (the yearly amount follows at
/// exactly ten times).
#[allow(clippy::too_many_arguments)]
pub async fn set_amount(
    db: &ControlDb,
    book_id: &str,
    band: Band,
    item: PriceItem,
    monthly_minor: i64,
    operator: &str,
    reason: &str,
    now: i64,
) -> Result<(), BookError> {
    if !(1..=MAX_MONTHLY_MINOR).contains(&monthly_minor) {
        return Err(BookError::BadAmount(monthly_minor));
    }
    let audit_id = new_id();
    let details = json!({
        "band": band.as_str(),
        "item": item.as_str(),
        "monthly_minor": monthly_minor,
        "yearly_minor": monthly_minor * 10,
    })
    .to_string();
    let audit = BookAudit {
        id: &audit_id,
        operator,
        action: "price_book_set",
        target: book_id,
        reason,
        details_json: &details,
    };
    match db
        .set_draft_amount(book_id, band, item, monthly_minor, &audit, now)
        .await
        .map_err(db_err)?
    {
        SetAmountOutcome::Set => Ok(()),
        SetAmountOutcome::Immutable => Err(BookError::Immutable(book_id.to_owned())),
        SetAmountOutcome::Unknown => Err(BookError::UnknownRow(format!(
            "{book_id} has no {} {} row",
            band.as_str(),
            item.as_str()
        ))),
    }
}

/// `price-book band`: switch a band on or off.
pub async fn set_band(
    db: &ControlDb,
    band: Band,
    enabled: bool,
    operator: &str,
    reason: &str,
    now: i64,
) -> Result<(), BookError> {
    let audit_id = new_id();
    let details = json!({ "enabled": enabled }).to_string();
    let audit = BookAudit {
        id: &audit_id,
        operator,
        action: "billing_band",
        target: band.as_str(),
        reason,
        details_json: &details,
    };
    db.set_band_enabled(band, enabled, &audit, now)
        .await
        .map_err(db_err)
}

fn spec_of(book_id: &str, i: &PriceBookItem) -> PlanSpec {
    PlanSpec {
        book_id: book_id.to_owned(),
        band: i.band,
        item: i.item,
        cycle: i.cycle,
        currency: i.currency.clone(),
        amount_minor: i.amount_minor,
    }
}

/// `price-book publish`: mirrors every row without a plan to the provider, then in one
/// transaction records the plan ids, makes a draft live, retires the previous book and writes
/// the audit row. A provider error leaves the draft a draft and writes nothing. A live book
/// that lacks plan ids (the seed book, or one published under `manual`) is only given them.
pub async fn publish(
    db: &ControlDb,
    provider: &BillingProvider,
    book_id: &str,
    operator: &str,
    reason: &str,
    now: i64,
) -> Result<PublishSummary, BookError> {
    let book = db
        .price_book(book_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| BookError::UnknownBook(book_id.to_owned()))?;
    if book.status == PriceBookStatus::Retired {
        return Err(BookError::Retired(book_id.to_owned()));
    }
    let items = db.price_book_items(book_id).await.map_err(db_err)?;
    let want = Band::ALL.len() * PriceItem::ALL.len() * BillingCycle::ALL.len();
    if book.status == PriceBookStatus::Draft && items.len() != want {
        return Err(BookError::Incomplete {
            have: items.len(),
            want,
        });
    }
    let missing: Vec<&PriceBookItem> = items
        .iter()
        .filter(|i| i.razorpay_plan_id.is_none())
        .collect();
    let synced_only = book.status == PriceBookStatus::Live;
    if synced_only && missing.is_empty() {
        return Err(BookError::AlreadyPublished(book_id.to_owned()));
    }
    let mut plans = Vec::with_capacity(missing.len());
    let mut deferred = Vec::new();
    for item in missing {
        match provider
            .create_plan(&spec_of(book_id, item))
            .await
            .map_err(BookError::Provider)?
        {
            PlanResult::Created(plan_id) => plans.push(MirroredPlan {
                band: item.band,
                item: item.item,
                cycle: item.cycle,
                plan_id,
            }),
            PlanResult::Deferred => deferred.push(json!({
                "band": item.band.as_str(),
                "item": item.item.as_str(),
                "cycle": item.cycle.as_str(),
                "currency": item.currency,
                "amount_minor": item.amount_minor,
            })),
        }
    }
    let audit_id = new_id();
    let task_id = new_id();
    let task_details = json!({ "book": book_id, "plans": deferred }).to_string();
    let task = (!deferred.is_empty()).then_some(NewOpsTask {
        id: &task_id,
        kind: TASK_CREATE_PLANS,
        target: book_id,
        details_json: &task_details,
    });
    let details = json!({
        "version": book.version,
        "provider": provider.name(),
        "plans_created": plans.len(),
        "plans_deferred": deferred.len(),
    })
    .to_string();
    let audit = BookAudit {
        id: &audit_id,
        operator,
        action: if synced_only {
            "price_book_sync"
        } else {
            "price_book_publish"
        },
        target: book_id,
        reason,
        details_json: &details,
    };
    match db
        .publish_price_book(book_id, &plans, task.as_ref(), &audit, now)
        .await
        .map_err(db_err)?
    {
        PublishOutcome::Published => Ok(PublishSummary {
            book_id: book_id.to_owned(),
            version: book.version,
            provider: provider.name(),
            plans_created: plans.len(),
            plans_deferred: deferred.len(),
            synced_only,
            audit_id,
        }),
        PublishOutcome::Unknown => Err(BookError::UnknownBook(book_id.to_owned())),
        PublishOutcome::Retired => Err(BookError::Retired(book_id.to_owned())),
        PublishOutcome::Raced => Err(BookError::Raced),
    }
}

/// `price-book list`: one line per book, then the live book's monthly prices per band.
pub async fn list_lines(db: &ControlDb) -> Result<Vec<String>, BookError> {
    let mut lines = Vec::new();
    for b in db.price_books().await.map_err(db_err)? {
        lines.push(format!(
            "{}  version {}  {}",
            b.id,
            b.version,
            b.status.as_str()
        ));
    }
    if let Some(live) = db.live_price_book().await.map_err(db_err)? {
        for i in db.price_book_items(&live.id).await.map_err(db_err)? {
            if i.cycle == BillingCycle::Monthly {
                lines.push(format!(
                    "  {} {} {} {} minor{}",
                    i.band.as_str(),
                    i.item.as_str(),
                    i.currency,
                    i.amount_minor,
                    if i.razorpay_plan_id.is_some() {
                        "  (plan)"
                    } else {
                        ""
                    }
                ));
            }
        }
    }
    Ok(lines)
}
