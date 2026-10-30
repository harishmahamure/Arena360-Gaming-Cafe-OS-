//! What the Hub's notification drain does with each due `notify_outbox` row (Story 8.29).
//!
//! [`plan`] reads the rows whose `not_before` has passed and decides each one: send it, wait for
//! something (a receipt waits for its invoice number), skip it for good (the action was reverted,
//! the player no longer agrees), or expire it (still unsent after 7 days). The decision of a row
//! runs through [`RULES`], one small check each, so a later story adds its rule (the erasure rule
//! of Epic 9, the "session no longer near its end" rule of Story 8.30) without touching the
//! drain. Nothing here writes: the task applies the result as `notify.*` intents.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, NOTIFY_EXPIRY_MS, NOTIFY_KIND_RECEIPT, NotifyRequest, SnapshotLocale, valid_e164,
};
use arena_store::{InvoiceOrder, NotifyRow, PlayerRow, Store};

use crate::SnapshotReader;
use crate::locale::format_minor;
use crate::players::{KEK_LEN, open_field};

/// How many due rows one pass takes.
pub const DRAIN_BATCH: i64 = 20;

/// What one rule says about a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// This rule has no objection.
    Pass,
    /// Not yet: look again on the next pass.
    Wait,
    /// Never: the row is marked `skipped` with this reason.
    Skip(&'static str),
}

/// Everything a rule may look at about one row.
pub struct RowContext<'a> {
    pub row: &'a NotifyRow,
    /// The player, `None` once they are gone (erased or deleted).
    pub player: Option<&'a PlayerRow>,
    /// The player's current consent to the row's purpose: (granted, policy version).
    pub consent: Option<(bool, String)>,
    pub order: Option<&'a InvoiceOrder>,
}

type Rule = fn(&RowContext<'_>) -> Verdict;

/// The per-row eligibility check, in the order the rules run. The first rule that does not pass
/// decides the row.
pub const RULES: &[Rule] = &[player_exists, consent_current, receipt_ready];

fn player_exists(c: &RowContext<'_>) -> Verdict {
    if c.player.is_some() {
        Verdict::Pass
    } else {
        Verdict::Skip("player_gone")
    }
}

fn consent_current(c: &RowContext<'_>) -> Verdict {
    match c.consent {
        Some((true, _)) => Verdict::Pass,
        _ => Verdict::Skip("consent_withdrawn"),
    }
}

/// A receipt goes out for a settled sale, and once its invoice number exists.
fn receipt_ready(c: &RowContext<'_>) -> Verdict {
    if c.row.kind != NOTIFY_KIND_RECEIPT {
        return Verdict::Pass;
    }
    match c.order {
        Some(o) if o.status == "settled" => {
            if o.invoice_no.is_some() {
                Verdict::Pass
            } else {
                Verdict::Wait
            }
        }
        _ => Verdict::Skip("order_reverted"),
    }
}

/// What the drain does with one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrainAction {
    /// Post this to `/v1/notify`.
    Send(Box<NotifyRequest>),
    /// Leave the row queued.
    Wait,
    Skip(String),
    Expire,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainItem {
    pub id: String,
    pub action: DrainAction,
    /// The total of the order a receipt is for, to write as the `amount` parameter.
    total_minor: i64,
}

fn decide(c: &RowContext<'_>) -> Verdict {
    RULES
        .iter()
        .map(|rule| rule(c))
        .find(|v| *v != Verdict::Pass)
        .unwrap_or(Verdict::Pass)
}

/// The message of a row. The `amount` and `cafe` parameters of a receipt are added by the reader.
fn request(
    row: &NotifyRow,
    phone: String,
    name: String,
    order: Option<&InvoiceOrder>,
) -> NotifyRequest {
    let mut params = BTreeMap::new();
    params.insert("name".to_owned(), name);
    if let Some(o) = order {
        params.insert(
            "receipt_no".to_owned(),
            o.invoice_no.clone().unwrap_or_default(),
        );
    }
    NotifyRequest {
        outbox_id: row.id.clone(),
        kind: row.kind.clone(),
        purpose: row.purpose.clone(),
        consent_policy_version: row.consent_policy_version.clone(),
        recipient: phone,
        params,
    }
}

/// Decides every due row (see the module docs). A receipt's `Send` request lacks its `amount` and
/// `cafe` parameters; [`SnapshotReader::notify_plan`] adds them.
pub fn plan(
    store: &Store,
    kek: &[u8; KEK_LEN],
    now_ms: i64,
    limit: i64,
) -> Result<Vec<DrainItem>, AppError> {
    let mut items = Vec::new();
    for row in store.due_notify(now_ms, limit)? {
        let mut total_minor = 0;
        let action = if now_ms - row.not_before > NOTIFY_EXPIRY_MS {
            DrainAction::Expire
        } else {
            let player = store.player(&row.player_id)?;
            let consent = store.latest_consent(&row.player_id, &row.purpose)?;
            let order = store.invoice_order(&row.order_id)?;
            total_minor = order.as_ref().map_or(0, |o| o.total_minor);
            let ctx = RowContext {
                row: &row,
                player: player.as_ref().map(|(p, _, _)| p),
                consent,
                order: order.as_ref(),
            };
            match (decide(&ctx), &player) {
                (Verdict::Wait, _) => DrainAction::Wait,
                (Verdict::Skip(reason), _) => DrainAction::Skip(reason.to_owned()),
                (Verdict::Pass, None) => DrainAction::Skip("player_gone".to_owned()),
                (Verdict::Pass, Some((p, _, wrapped))) => {
                    let open =
                        |field, enc: &str| open_field(kek, &p.player_id, wrapped, field, enc);
                    let phone = open("phone", &p.phone_enc)?;
                    if valid_e164(&phone) {
                        let name = open("name", &p.name_enc)?;
                        DrainAction::Send(Box::new(request(&row, phone, name, order.as_ref())))
                    } else {
                        DrainAction::Skip("phone_invalid".to_owned())
                    }
                }
            }
        };
        items.push(DrainItem {
            id: row.id,
            action,
            total_minor,
        });
    }
    Ok(items)
}

/// Adds the `amount` (the order's total as the branch writes money) and `cafe` parameters a
/// receipt template needs.
fn complete(item: &mut DrainItem, cafe: &str, locale: Option<&SnapshotLocale>) {
    let DrainAction::Send(req) = &mut item.action else {
        return;
    };
    if req.kind != NOTIFY_KIND_RECEIPT {
        return;
    }
    let amount = match locale {
        Some(l) => format_minor(
            item.total_minor,
            usize::from(l.exponent),
            &l.symbol,
            l.symbol_position,
            l.number_format,
        ),
        None => item.total_minor.to_string(),
    };
    req.params.insert("amount".to_owned(), amount);
    req.params.insert("cafe".to_owned(), cafe.to_owned());
}

impl SnapshotReader {
    /// The drain's decision for every row due at `now_ms`, receipts complete with their amount
    /// and café name.
    pub fn notify_plan(
        &self,
        kek: &[u8; KEK_LEN],
        now_ms: i64,
    ) -> Result<Vec<DrainItem>, AppError> {
        let mut items = self.with_store(|store| plan(store, kek, now_ms, DRAIN_BATCH))?;
        if items
            .iter()
            .any(|i| matches!(i.action, DrainAction::Send(_)))
        {
            let snapshot = self.snapshot(now_ms)?;
            let cafe = snapshot.branch.as_ref().map_or("", |b| b.name.as_str());
            for item in &mut items {
                complete(item, cafe, snapshot.locale.as_ref());
            }
        }
        Ok(items)
    }
}
