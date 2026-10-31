//! The price book (Story 10.1): versioned prices per band, item and cycle, the bands that are on
//! sale and the operator tasks a manual billing provider leaves behind. A book is edited only
//! while it is a `draft`; publishing makes it `live` and retires the previous one. Amounts are
//! minor units of the band's currency.

use arena_proto::{Band, BillingCycle, PriceItem, PriceLine};

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_text, text};

/// Where a price book is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceBookStatus {
    Draft,
    Live,
    Retired,
}

impl PriceBookStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Live => "live",
            Self::Retired => "retired",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Draft, Self::Live, Self::Retired]
            .into_iter()
            .find(|v| v.as_str() == s)
    }
}

/// One version of the price book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceBook {
    pub id: String,
    pub version: u32,
    pub effective_from: i64,
    pub status: PriceBookStatus,
    pub created_at: i64,
    pub published_at: Option<i64>,
}

/// One row of a book with the Razorpay plan mirrored from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceBookItem {
    pub band: Band,
    pub currency: String,
    pub item: PriceItem,
    pub cycle: BillingCycle,
    pub amount_minor: i64,
    pub gst_applicable: bool,
    pub razorpay_plan_id: Option<String>,
}

/// The audit row an ops change of the book writes in the same transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookAudit<'a> {
    pub id: &'a str,
    pub operator: &'a str,
    /// `ops_audit.action`.
    pub action: &'a str,
    /// `ops_audit.target`: the book or band the change touched.
    pub target: &'a str,
    pub reason: &'a str,
    pub details_json: &'a str,
}

/// A task left for an operator when the provider is `manual`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOpsTask<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub target: &'a str,
    pub details_json: &'a str,
}

/// An open operator task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsTaskRow {
    pub id: String,
    pub at: i64,
    pub kind: String,
    pub target: String,
    pub details_json: String,
}

/// A Razorpay plan created for one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirroredPlan {
    pub band: Band,
    pub item: PriceItem,
    pub cycle: BillingCycle,
    pub plan_id: String,
}

/// What publishing a book did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    /// A draft went live (and the previous live book was retired), or a live book was given its
    /// missing plan ids.
    Published,
    /// No such book.
    Unknown,
    /// The book is retired, so nothing can be published.
    Retired,
    /// Another publish changed the book first; nothing was written.
    Raced,
}

/// What an amount change did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetAmountOutcome {
    Set,
    /// The book is live or retired: `PRICE_BOOK_IMMUTABLE`.
    Immutable,
    /// No such book or no such row.
    Unknown,
}

/// What opening a new draft did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftOutcome {
    Created {
        id: String,
        version: u32,
    },
    /// A draft already exists.
    DraftExists(String),
    /// There is no live book to copy.
    NoLiveBook,
}

const BOOK_COLUMNS: &str = "id, version, effective_from, status, created_at, published_at";

fn book_from(r: &[SqlValue]) -> Result<PriceBook, ControlError> {
    let status = col_text(r, 3)?;
    Ok(PriceBook {
        id: col_text(r, 0)?,
        version: u32::try_from(col_int(r, 1)?)
            .map_err(|_| ControlError::Db("price book version out of range".to_owned()))?,
        effective_from: col_int(r, 2)?,
        status: PriceBookStatus::parse(&status)
            .ok_or_else(|| ControlError::Db(format!("unknown price book status {status:?}")))?,
        created_at: col_int(r, 4)?,
        published_at: col_opt_int(r, 5)?,
    })
}

const ITEM_COLUMNS: &str =
    "band, currency, item, cycle, amount_minor, gst_applicable, razorpay_plan_id";

fn item_from(r: &[SqlValue]) -> Result<PriceBookItem, ControlError> {
    let band = col_text(r, 0)?;
    let item = col_text(r, 2)?;
    let cycle = col_text(r, 3)?;
    Ok(PriceBookItem {
        band: Band::parse(&band)
            .ok_or_else(|| ControlError::Db(format!("unknown band {band:?}")))?,
        currency: col_text(r, 1)?,
        item: PriceItem::parse(&item)
            .ok_or_else(|| ControlError::Db(format!("unknown price item {item:?}")))?,
        cycle: BillingCycle::parse(&cycle)
            .ok_or_else(|| ControlError::Db(format!("unknown billing cycle {cycle:?}")))?,
        amount_minor: col_int(r, 4)?,
        gst_applicable: col_int(r, 5)? == 1,
        razorpay_plan_id: super::col_opt_text(r, 6)?,
    })
}

const AUDIT_INSERT: &str = "INSERT INTO ops_audit (id, at, operator, action, target, reason, \
     details_json) VALUES (?, ?, ?, ?, ?, ?, ?)";

impl ControlDb {
    /// The book with `id`.
    pub async fn price_book(&self, id: &str) -> Result<Option<PriceBook>, ControlError> {
        let sql = format!("SELECT {BOOK_COLUMNS} FROM price_books WHERE id = ?");
        let rows = self.query(&sql, vec![text(id)]).await?;
        rows.first().map(|r| book_from(r)).transpose()
    }

    /// The live book, if one is published.
    pub async fn live_price_book(&self) -> Result<Option<PriceBook>, ControlError> {
        let sql = format!("SELECT {BOOK_COLUMNS} FROM price_books WHERE status = 'live'");
        let rows = self.query(&sql, vec![]).await?;
        rows.first().map(|r| book_from(r)).transpose()
    }

    /// Every book, newest version first.
    pub async fn price_books(&self) -> Result<Vec<PriceBook>, ControlError> {
        let sql = format!("SELECT {BOOK_COLUMNS} FROM price_books ORDER BY version DESC");
        let rows = self.query(&sql, vec![]).await?;
        rows.iter().map(|r| book_from(r)).collect()
    }

    /// Every row of a book, in a stable order.
    pub async fn price_book_items(
        &self,
        book_id: &str,
    ) -> Result<Vec<PriceBookItem>, ControlError> {
        let sql = format!(
            "SELECT {ITEM_COLUMNS} FROM price_book_items WHERE price_book_id = ? \
             ORDER BY band, item, cycle"
        );
        let rows = self.query(&sql, vec![text(book_id)]).await?;
        rows.iter().map(|r| item_from(r)).collect()
    }

    /// The rows of `book_id` for one band, as the pricing API serves them.
    pub async fn price_lines(
        &self,
        book_id: &str,
        band: Band,
    ) -> Result<Vec<PriceLine>, ControlError> {
        Ok(self
            .price_book_items(book_id)
            .await?
            .into_iter()
            .filter(|i| i.band == band)
            .map(|i| PriceLine {
                item: i.item,
                cycle: i.cycle,
                amount_minor: i.amount_minor,
                gst_applicable: i.gst_applicable,
            })
            .collect())
    }

    /// Whether paid plans are on sale in `band`.
    pub async fn band_enabled(&self, band: Band) -> Result<bool, ControlError> {
        let rows = self
            .query(
                "SELECT enabled FROM billing_bands WHERE band = ?",
                vec![text(band.as_str())],
            )
            .await?;
        match rows.first() {
            Some(r) => Ok(col_int(r, 0)? == 1),
            None => Ok(false),
        }
    }

    /// Switches a band on or off and writes the audit row, in one transaction.
    pub async fn set_band_enabled(
        &self,
        band: Band,
        enabled: bool,
        audit: &BookAudit<'_>,
        now: i64,
    ) -> Result<(), ControlError> {
        let ok = self
            .transact(vec![
                Stmt::new(
                    "UPDATE billing_bands SET enabled = ?, updated_at = ? WHERE band = ?",
                    vec![
                        SqlValue::Integer(i64::from(enabled)),
                        SqlValue::Integer(now),
                        text(band.as_str()),
                    ],
                )
                .expect(1),
                audit_stmt(audit, now),
            ])
            .await?;
        if ok {
            Ok(())
        } else {
            Err(ControlError::Db(format!("no billing band {band:?}")))
        }
    }

    /// Opens a new draft as a copy of the live book (plan ids empty), with the next version.
    pub async fn open_price_book_draft(
        &self,
        id: &str,
        audit: &BookAudit<'_>,
        now: i64,
    ) -> Result<DraftOutcome, ControlError> {
        let books = self.price_books().await?;
        if let Some(d) = books.iter().find(|b| b.status == PriceBookStatus::Draft) {
            return Ok(DraftOutcome::DraftExists(d.id.clone()));
        }
        if !books.iter().any(|b| b.status == PriceBookStatus::Live) {
            return Ok(DraftOutcome::NoLiveBook);
        }
        let version = books.iter().map(|b| b.version).max().unwrap_or(0) + 1;
        let done = self
            .transact(vec![
                Stmt::new(
                    "INSERT INTO price_books (id, version, effective_from, status, created_at, \
                     published_at) VALUES (?, ?, ?, 'draft', ?, NULL)",
                    vec![
                        text(id),
                        SqlValue::Integer(i64::from(version)),
                        SqlValue::Integer(now),
                        SqlValue::Integer(now),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "INSERT INTO price_book_items (price_book_id, band, currency, item, cycle, \
                     amount_minor, gst_applicable, razorpay_plan_id) \
                     SELECT ?, band, currency, item, cycle, amount_minor, gst_applicable, NULL \
                     FROM price_book_items WHERE price_book_id = \
                     (SELECT id FROM price_books WHERE status = 'live')",
                    vec![text(id)],
                ),
                audit_stmt(audit, now),
            ])
            .await;
        match done {
            Ok(true) => Ok(DraftOutcome::Created {
                id: id.to_owned(),
                version,
            }),
            Ok(false) => Err(ControlError::Db("the draft was not created".to_owned())),
            Err(ControlError::Db(m)) if m.contains("UNIQUE") => {
                let draft = self
                    .price_books()
                    .await?
                    .into_iter()
                    .find(|b| b.status == PriceBookStatus::Draft);
                Ok(DraftOutcome::DraftExists(
                    draft.map(|d| d.id).unwrap_or_default(),
                ))
            }
            Err(e) => Err(e),
        }
    }

    /// Sets the monthly amount of an item in a draft; the yearly amount follows at exactly ten
    /// times. A book that is not a draft is refused ([`SetAmountOutcome::Immutable`]).
    pub async fn set_draft_amount(
        &self,
        book_id: &str,
        band: Band,
        item: PriceItem,
        monthly_minor: i64,
        audit: &BookAudit<'_>,
        now: i64,
    ) -> Result<SetAmountOutcome, ControlError> {
        let Some(book) = self.price_book(book_id).await? else {
            return Ok(SetAmountOutcome::Unknown);
        };
        if book.status != PriceBookStatus::Draft {
            return Ok(SetAmountOutcome::Immutable);
        }
        let target = |cycle: &str, amount: i64| {
            vec![
                SqlValue::Integer(amount),
                text(book_id),
                text(band.as_str()),
                text(item.as_str()),
                text(cycle),
            ]
        };
        let ok = self
            .transact(vec![
                Stmt::new(
                    "UPDATE price_book_items SET amount_minor = ? WHERE price_book_id = ? \
                     AND band = ? AND item = ? AND cycle = ? AND (SELECT status FROM price_books \
                     WHERE id = price_book_id) = 'draft'",
                    target("monthly", monthly_minor),
                )
                .expect(1),
                Stmt::new(
                    "UPDATE price_book_items SET amount_minor = ? WHERE price_book_id = ? \
                     AND band = ? AND item = ? AND cycle = ? AND (SELECT status FROM price_books \
                     WHERE id = price_book_id) = 'draft'",
                    target("yearly", monthly_minor * 10),
                )
                .expect(1),
                audit_stmt(audit, now),
            ])
            .await?;
        if ok {
            return Ok(SetAmountOutcome::Set);
        }
        // Rolled back: the book was published meanwhile, or the row does not exist.
        Ok(match self.price_book(book_id).await? {
            Some(b) if b.status != PriceBookStatus::Draft => SetAmountOutcome::Immutable,
            _ => SetAmountOutcome::Unknown,
        })
    }

    /// Publishes `book_id`: records the Razorpay plan ids in `plans`, makes a draft live and the
    /// previous live book retired, and writes the audit row (and an operator task when the
    /// provider left work for a person). All of it or none of it. A live book is only given its
    /// missing plan ids.
    pub async fn publish_price_book(
        &self,
        book_id: &str,
        plans: &[MirroredPlan],
        task: Option<&NewOpsTask<'_>>,
        audit: &BookAudit<'_>,
        now: i64,
    ) -> Result<PublishOutcome, ControlError> {
        let Some(book) = self.price_book(book_id).await? else {
            return Ok(PublishOutcome::Unknown);
        };
        if book.status == PriceBookStatus::Retired {
            return Ok(PublishOutcome::Retired);
        }
        let mut stmts = Vec::with_capacity(plans.len() + 4);
        for p in plans {
            stmts.push(
                Stmt::new(
                    "UPDATE price_book_items SET razorpay_plan_id = ? WHERE price_book_id = ? \
                     AND band = ? AND item = ? AND cycle = ? AND razorpay_plan_id IS NULL",
                    vec![
                        text(&p.plan_id),
                        text(book_id),
                        text(p.band.as_str()),
                        text(p.item.as_str()),
                        text(p.cycle.as_str()),
                    ],
                )
                .expect(1),
            );
        }
        if book.status == PriceBookStatus::Draft {
            stmts.push(Stmt::new(
                "UPDATE price_books SET status = 'retired' WHERE status = 'live'",
                vec![],
            ));
            stmts.push(
                Stmt::new(
                    "UPDATE price_books SET status = 'live', published_at = ?, \
                     effective_from = ? WHERE id = ? AND status = 'draft'",
                    vec![
                        SqlValue::Integer(now),
                        SqlValue::Integer(now),
                        text(book_id),
                    ],
                )
                .expect(1),
            );
        }
        if let Some(t) = task {
            stmts.push(
                Stmt::new(
                    "INSERT INTO ops_tasks (id, at, kind, target, details_json, done_at) \
                     VALUES (?, ?, ?, ?, ?, NULL)",
                    vec![
                        text(t.id),
                        SqlValue::Integer(now),
                        text(t.kind),
                        text(t.target),
                        text(t.details_json),
                    ],
                )
                .expect(1),
            );
        }
        stmts.push(audit_stmt(audit, now));
        Ok(if self.transact(stmts).await? {
            PublishOutcome::Published
        } else {
            PublishOutcome::Raced
        })
    }

    /// Open operator tasks of `kind`, oldest first.
    pub async fn open_ops_tasks(&self, kind: &str) -> Result<Vec<OpsTaskRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, at, kind, target, details_json FROM ops_tasks \
                 WHERE kind = ? AND done_at IS NULL ORDER BY at, id",
                vec![text(kind)],
            )
            .await?;
        rows.iter()
            .map(|r| {
                Ok(OpsTaskRow {
                    id: col_text(r, 0)?,
                    at: col_int(r, 1)?,
                    kind: col_text(r, 2)?,
                    target: col_text(r, 3)?,
                    details_json: col_text(r, 4)?,
                })
            })
            .collect()
    }
}

/// The audit insert for an ops change of the book.
fn audit_stmt(audit: &BookAudit<'_>, now: i64) -> Stmt {
    Stmt::new(
        AUDIT_INSERT,
        vec![
            text(audit.id),
            SqlValue::Integer(now),
            text(audit.operator),
            text(audit.action),
            text(audit.target),
            text(audit.reason),
            text(audit.details_json),
        ],
    )
    .expect(1)
}
