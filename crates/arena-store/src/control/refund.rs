//! Full refund within 7 days of the first payment (Story 10.15, FR180, AD-20, AD-21). A
//! `refund_requests` row is the owner's ask (one per subscription); an operator approves or rejects
//! it. Approving refunds each payment at Razorpay (`refund_payments`, one row per payment as it is
//! refunded, so a retry skips what is done); Razorpay's `refund.processed` then marks the payment
//! processed and issues the credit note of its invoice, and the last one makes the subscription
//! `refunded` and the branch Free, all in the transaction that records the event.

use super::billing::{EventOutcome, EventRecord, SubscriptionChange, change_stmts, event_stmt};
use super::invoice::{NewInvoice, issue_stmt};
use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// `refund_requests.status`: waiting for an operator.
pub const REQUESTED: &str = "requested";
/// `refund_requests.status`: refunded at Razorpay, waiting for `refund.processed`.
pub const ISSUED: &str = "issued";
/// `refund_requests.status`: every payment's refund has been processed.
pub const PROCESSED: &str = "processed";
/// `refund_requests.status`: an operator turned it down.
pub const REJECTED: &str = "rejected";

/// A refund request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundRequest {
    pub id: String,
    pub subscription_id: String,
    pub tenant_id: String,
    pub branch_id: String,
    /// The first payment, the one the 7 days run from.
    pub payment_id: String,
    pub amount_minor: i64,
    pub currency: String,
    /// `requested`, `issued`, `processed` or `rejected`.
    pub status: String,
    pub requested_at: i64,
    pub decided_by: Option<String>,
    pub decided_reason: Option<String>,
    pub decided_at: Option<i64>,
    /// The provider's last error of an approval that did not complete.
    pub last_error: Option<String>,
    pub processed_at: Option<i64>,
}

/// One payment of a request, refunded in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundPayment {
    pub request_id: String,
    pub payment_id: String,
    pub amount_minor: i64,
    pub razorpay_refund_id: Option<String>,
    pub processed_at: Option<i64>,
}

/// A request to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRefundRequest<'a> {
    pub id: &'a str,
    pub subscription_id: &'a str,
    /// The `rev` the owner saw; the request is written only if the subscription is still at it.
    pub expect_rev: i64,
    pub tenant_id: &'a str,
    pub branch_id: &'a str,
    pub payment_id: &'a str,
    pub amount_minor: i64,
    pub currency: &'a str,
}

/// What writing a request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestOutcome {
    Created,
    /// The subscription is not `active` at that `rev` any more; nothing was written.
    Stale,
    /// The subscription already has a request.
    Exists,
}

/// An operator's decision, audited in the same transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision<'a> {
    pub request_id: &'a str,
    pub subscription_id: &'a str,
    pub operator: &'a str,
    pub reason: &'a str,
    pub audit_id: &'a str,
    /// `refund.approve` or `refund.reject`.
    pub action: &'a str,
    pub details_json: &'a str,
}

/// A `refund.processed` event to record against a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessedRefund<'a> {
    pub request_id: &'a str,
    pub payment_id: &'a str,
    /// How many payments are still unprocessed after this one; if another event changed that since
    /// it was read nothing is written (`Conflict`: read again).
    pub remaining: i64,
}

const COLUMNS: &str = "id, subscription_id, tenant_id, branch_id, payment_id, amount_minor, \
     currency, status, requested_at, decided_by, decided_reason, decided_at, last_error, \
     processed_at";

fn request_from(r: &[SqlValue]) -> Result<RefundRequest, ControlError> {
    Ok(RefundRequest {
        id: col_text(r, 0)?,
        subscription_id: col_text(r, 1)?,
        tenant_id: col_text(r, 2)?,
        branch_id: col_text(r, 3)?,
        payment_id: col_text(r, 4)?,
        amount_minor: col_int(r, 5)?,
        currency: col_text(r, 6)?,
        status: col_text(r, 7)?,
        requested_at: col_int(r, 8)?,
        decided_by: col_opt_text(r, 9)?,
        decided_reason: col_opt_text(r, 10)?,
        decided_at: col_opt_int(r, 11)?,
        last_error: col_opt_text(r, 12)?,
        processed_at: col_opt_int(r, 13)?,
    })
}

fn payment_from(r: &[SqlValue]) -> Result<RefundPayment, ControlError> {
    Ok(RefundPayment {
        request_id: col_text(r, 0)?,
        payment_id: col_text(r, 1)?,
        amount_minor: col_int(r, 2)?,
        razorpay_refund_id: col_opt_text(r, 3)?,
        processed_at: col_opt_int(r, 4)?,
    })
}

fn audit_stmt(d: &Decision<'_>, now: i64) -> Stmt {
    Stmt::new(
        "INSERT INTO ops_audit (id, at, operator, action, target, reason, details_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        vec![
            text(d.audit_id),
            SqlValue::Integer(now),
            text(d.operator),
            text(d.action),
            text(d.subscription_id),
            text(d.reason),
            text(d.details_json),
        ],
    )
    .expect(1)
}

impl ControlDb {
    /// The refund request of a subscription.
    pub async fn refund_request_of(
        &self,
        subscription_id: &str,
    ) -> Result<Option<RefundRequest>, ControlError> {
        let sql = format!("SELECT {COLUMNS} FROM refund_requests WHERE subscription_id = ?");
        let rows = self.query(&sql, vec![text(subscription_id)]).await?;
        rows.first().map(|r| request_from(r)).transpose()
    }

    /// The refund request with this id.
    pub async fn refund_request(&self, id: &str) -> Result<Option<RefundRequest>, ControlError> {
        let sql = format!("SELECT {COLUMNS} FROM refund_requests WHERE id = ?");
        let rows = self.query(&sql, vec![text(id)]).await?;
        rows.first().map(|r| request_from(r)).transpose()
    }

    /// The request that refunded Razorpay payment `payment_id`.
    pub async fn refund_request_of_payment(
        &self,
        payment_id: &str,
    ) -> Result<Option<RefundRequest>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM refund_requests WHERE id IN \
             (SELECT request_id FROM refund_payments WHERE payment_id = ?)"
        );
        let rows = self.query(&sql, vec![text(payment_id)]).await?;
        rows.first().map(|r| request_from(r)).transpose()
    }

    /// The payments refunded under a request.
    pub async fn refund_payments(
        &self,
        request_id: &str,
    ) -> Result<Vec<RefundPayment>, ControlError> {
        let rows = self
            .query(
                "SELECT request_id, payment_id, amount_minor, razorpay_refund_id, processed_at \
                 FROM refund_payments WHERE request_id = ? ORDER BY payment_id",
                vec![text(request_id)],
            )
            .await?;
        rows.iter().map(|r| payment_from(r)).collect()
    }

    /// Writes the owner's request if the subscription is still `active` at the `rev` they saw and
    /// has no request yet.
    pub async fn create_refund_request(
        &self,
        r: &NewRefundRequest<'_>,
        now: i64,
    ) -> Result<RequestOutcome, ControlError> {
        let stmts = vec![
            Stmt::new(
                "UPDATE subscriptions SET updated_at = updated_at \
                 WHERE id = ? AND rev = ? AND state = 'active'",
                vec![text(r.subscription_id), SqlValue::Integer(r.expect_rev)],
            )
            .expect(1),
            Stmt::new(
                "INSERT INTO refund_requests (id, subscription_id, tenant_id, branch_id, \
                 payment_id, amount_minor, currency, status, requested_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, 'requested', ?)",
                vec![
                    text(r.id),
                    text(r.subscription_id),
                    text(r.tenant_id),
                    text(r.branch_id),
                    text(r.payment_id),
                    SqlValue::Integer(r.amount_minor),
                    text(r.currency),
                    SqlValue::Integer(now),
                ],
            )
            .expect(1),
        ];
        match self.transact(stmts).await {
            Ok(true) => Ok(RequestOutcome::Created),
            Ok(false) => Ok(RequestOutcome::Stale),
            Err(ControlError::Db(m)) if m.contains("refund_requests") && m.contains("UNIQUE") => {
                Ok(RequestOutcome::Exists)
            }
            Err(e) => Err(e),
        }
    }

    /// Records that `payment_id` was refunded at Razorpay as `razorpay_refund_id`. A repeat is a
    /// no-op.
    pub async fn record_payment_refunded(
        &self,
        request_id: &str,
        payment_id: &str,
        amount_minor: i64,
        razorpay_refund_id: &str,
    ) -> Result<(), ControlError> {
        self.transact(vec![Stmt::new(
            "INSERT OR IGNORE INTO refund_payments (request_id, payment_id, amount_minor, \
             razorpay_refund_id) VALUES (?, ?, ?, ?)",
            vec![
                text(request_id),
                text(payment_id),
                SqlValue::Integer(amount_minor),
                text(razorpay_refund_id),
            ],
        )])
        .await
        .map(|_| ())
    }

    /// Keeps the provider's error on a request whose approval did not complete (`None` clears it).
    pub async fn set_refund_error(
        &self,
        request_id: &str,
        error: Option<&str>,
    ) -> Result<(), ControlError> {
        self.transact(vec![Stmt::new(
            "UPDATE refund_requests SET last_error = ? WHERE id = ? AND status = 'requested'",
            vec![error.map_or(SqlValue::Null, text), text(request_id)],
        )])
        .await
        .map(|_| ())
    }

    /// The operator approved: the request is `issued` and the decision audited. `false`: it was
    /// no longer `requested`.
    pub async fn mark_refund_issued(
        &self,
        d: &Decision<'_>,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE refund_requests SET status = 'issued', decided_by = ?, decided_reason = ?, \
                 decided_at = ?, last_error = NULL WHERE id = ? AND status = 'requested'",
                vec![
                    text(d.operator),
                    text(d.reason),
                    SqlValue::Integer(now),
                    text(d.request_id),
                ],
            )
            .expect(1),
            audit_stmt(d, now),
        ])
        .await
    }

    /// The operator rejected: the request is `rejected` with the reason the owner reads, and the
    /// decision audited. `false`: it was no longer `requested`, or some of its payments were
    /// already refunded.
    pub async fn mark_refund_rejected(
        &self,
        d: &Decision<'_>,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE refund_requests SET status = 'rejected', decided_by = ?, decided_reason = ?, \
                 decided_at = ?, last_error = NULL WHERE id = ? AND status = 'requested' \
                 AND NOT EXISTS (SELECT 1 FROM refund_payments WHERE request_id = ?)",
                vec![
                    text(d.operator),
                    text(d.reason),
                    SqlValue::Integer(now),
                    text(d.request_id),
                    text(d.request_id),
                ],
            )
            .expect(1),
            audit_stmt(d, now),
        ])
        .await
    }

    /// Records a `refund.processed` event: the payment is marked processed, `credit_note` (the
    /// reversal of its invoice) is issued, and, when this was the last unprocessed payment,
    /// `complete` makes the subscription `refunded` and the request `processed`.
    pub async fn apply_refund_processed(
        &self,
        event: &EventRecord<'_>,
        refund: &ProcessedRefund<'_>,
        credit_note: Option<&NewInvoice>,
        complete: Option<&SubscriptionChange<'_>>,
        now: i64,
    ) -> Result<EventOutcome, ControlError> {
        let mut stmts = vec![
            event_stmt(event, true, now),
            Stmt::new(
                "UPDATE refund_payments SET processed_at = ? \
                 WHERE request_id = ? AND payment_id = ? AND processed_at IS NULL",
                vec![
                    SqlValue::Integer(now),
                    text(refund.request_id),
                    text(refund.payment_id),
                ],
            )
            .expect(1),
            // What is still waiting is what the caller read.
            Stmt::new(
                "UPDATE refund_requests SET status = status WHERE id = ? AND \
                 (SELECT COUNT(*) FROM refund_payments WHERE request_id = ? \
                 AND processed_at IS NULL) = ?",
                vec![
                    text(refund.request_id),
                    text(refund.request_id),
                    SqlValue::Integer(refund.remaining),
                ],
            )
            .expect(1),
        ];
        if let Some(note) = credit_note {
            stmts.push(issue_stmt(note, event.id));
        }
        if let Some(change) = complete {
            stmts.extend(change_stmts(change, now));
            stmts.push(
                Stmt::new(
                    "UPDATE refund_requests SET status = 'processed', processed_at = ? \
                     WHERE id = ? AND status IN ('issued', 'requested')",
                    vec![SqlValue::Integer(now), text(refund.request_id)],
                )
                .expect(1),
            );
        }
        match self.transact(stmts).await {
            Ok(true) => Ok(EventOutcome::Recorded),
            Ok(false) => Ok(EventOutcome::Conflict),
            Err(ControlError::Db(m)) if m.contains("billing_events") && m.contains("UNIQUE") => {
                Ok(EventOutcome::Duplicate)
            }
            Err(ControlError::Db(m)) if m.contains("billing_invoices") && m.contains("UNIQUE") => {
                Ok(EventOutcome::Conflict)
            }
            Err(e) => Err(e),
        }
    }
}
