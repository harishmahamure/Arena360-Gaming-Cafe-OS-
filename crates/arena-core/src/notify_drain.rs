//! What the Hub's notification drain does with each due `notify_outbox` row (Story 8.29).
//!
//! [`plan`] reads the rows whose `not_before` has passed and decides each one: send it, wait for
//! something (a receipt waits for its invoice number), skip it for good (the action was reverted,
//! the player no longer agrees), or expire it (still unsent after 7 days). The decision of a row
//! runs through [`RULES`], one small check each, so a later story adds its rule without touching
//! the drain (Story 9.1 added the first: a message for an erased player is skipped with reason
//! `erased`, also one queued before the erasure). Story 8.30 added the "session no longer near its end"
//! rule for the session and console messages, and Story 8.31 the "credit still owed" rule for the
//! credit reminder. Story 8.34 added the owner-alert rows: they name no player, so the player and
//! consent rules pass them, a `low_stock` alert is skipped once its Hub alert is no longer open, and
//! the request carries the alert's parameters with the amount in the branch money format. Nothing
//! here writes: the task applies the result as `notify.*` intents.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, NOTIFY_EXPIRY_MS, NOTIFY_KIND_CONSOLE_5MIN, NOTIFY_KIND_CREDIT_REMINDER,
    NOTIFY_KIND_LOW_BALANCE, NOTIFY_KIND_RECEIPT, NOTIFY_KIND_SESSION_ENDING,
    NOTIFY_PURPOSE_OWNER_ALERT, NotifyRequest, OwnerAlertKind, SESSION_NEAR_END_MS, SnapshotLocale,
    valid_e164,
};
use arena_store::{InvoiceOrder, LiveSession, NotifyRow, PlayerRow, Store};

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
    /// Whether the row's player was erased (Story 9.1), also a message queued before the erasure.
    pub erased: bool,
    /// The player's current consent to the row's purpose: (granted, policy version).
    pub consent: Option<(bool, String)>,
    pub order: Option<&'a InvoiceOrder>,
    /// The session a session message is about, while it is still running.
    pub session: Option<&'a LiveSession>,
    /// What the player owes on credit now (0 when nothing, or no credit).
    pub owed_minor: i64,
    /// For a `low_stock` alert: whether its Hub alert is still open.
    pub alert_open: bool,
    pub now_ms: i64,
}

type Rule = fn(&RowContext<'_>) -> Verdict;

/// The per-row eligibility check, in the order the rules run. The first rule that does not pass
/// decides the row.
pub const RULES: &[Rule] = &[
    player_erased,
    player_exists,
    consent_current,
    receipt_ready,
    session_near_end,
    credit_still_owed,
    stock_still_low,
];

/// Whether the row raises an owner alert (it names no player and needs no consent).
fn is_alert(row: &NotifyRow) -> bool {
    row.purpose == NOTIFY_PURPOSE_OWNER_ALERT
}

/// A message for an erased player is never sent, whenever it was queued.
fn player_erased(c: &RowContext<'_>) -> Verdict {
    if !is_alert(c.row) && c.erased {
        Verdict::Skip("erased")
    } else {
        Verdict::Pass
    }
}

fn player_exists(c: &RowContext<'_>) -> Verdict {
    if is_alert(c.row) || c.player.is_some() {
        Verdict::Pass
    } else {
        Verdict::Skip("player_gone")
    }
}

fn consent_current(c: &RowContext<'_>) -> Verdict {
    if is_alert(c.row) {
        return Verdict::Pass;
    }
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

/// A "session ending" or "console 5 minutes" message goes out only while its session is still
/// running and five minutes or less from its end. A session that ended, or was extended past that
/// (say within the Undo window of the moment that raised the message), is skipped.
fn session_near_end(c: &RowContext<'_>) -> Verdict {
    if c.row.kind != NOTIFY_KIND_SESSION_ENDING && c.row.kind != NOTIFY_KIND_CONSOLE_5MIN {
        return Verdict::Pass;
    }
    let near = c.session.is_some_and(|s| {
        matches!(s.state.as_str(), "active" | "paused")
            && s.ends_at.is_some_and(|at| {
                (1..=SESSION_NEAR_END_MS).contains(&(at - s.paused_at.unwrap_or(c.now_ms)))
            })
    });
    if near {
        Verdict::Pass
    } else {
        Verdict::Skip("session_changed")
    }
}

/// A credit reminder goes out only while the player still owes credit: one who paid within the
/// Undo window, or since, is skipped.
fn credit_still_owed(c: &RowContext<'_>) -> Verdict {
    if c.row.kind == NOTIFY_KIND_CREDIT_REMINDER && c.owed_minor <= 0 {
        Verdict::Skip("credit_cleared")
    } else {
        Verdict::Pass
    }
}

/// A `low_stock` alert goes out only while the Hub's own "Low stock" alert is still open: stock that
/// was refilled within the Undo window, or since, is skipped.
fn stock_still_low(c: &RowContext<'_>) -> Verdict {
    if is_alert(c.row) && c.row.kind == OwnerAlertKind::LowStock.as_str() && !c.alert_open {
        Verdict::Skip("stock_recovered")
    } else {
        Verdict::Pass
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
    /// The money a message names: the total of the order a receipt is for, or what a credit
    /// reminder says is owed now (the `amount` parameter), or the wallet balance of a low-balance
    /// message (the `balance` parameter).
    total_minor: i64,
}

fn decide(c: &RowContext<'_>) -> Verdict {
    RULES
        .iter()
        .map(|rule| rule(c))
        .find(|v| *v != Verdict::Pass)
        .unwrap_or(Verdict::Pass)
}

/// The raw values a session message was queued with.
fn stored_params(row: &NotifyRow) -> BTreeMap<String, String> {
    row.params
        .as_deref()
        .and_then(|p| serde_json::from_str(p).ok())
        .unwrap_or_default()
}

/// The message of a row. The money and `cafe` parameters are added by the reader.
fn request(
    row: &NotifyRow,
    phone: String,
    name: String,
    order: Option<&InvoiceOrder>,
) -> NotifyRequest {
    let mut params = stored_params(row);
    params.remove("amount_minor");
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

/// The request of an owner alert: its stored parameters (the amount is added by the reader) and no
/// recipient.
fn alert_request(row: &NotifyRow) -> NotifyRequest {
    let mut params = stored_params(row);
    params.remove("amount_minor");
    NotifyRequest {
        outbox_id: row.id.clone(),
        kind: row.kind.clone(),
        purpose: NOTIFY_PURPOSE_OWNER_ALERT.to_owned(),
        consent_policy_version: "owner".to_owned(),
        recipient: String::new(),
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
    let due = store.due_notify(now_ms, limit)?;
    let live = if due.iter().any(|r| r.session_id.is_some()) {
        store.live_sessions()?
    } else {
        Vec::new()
    };
    for row in due {
        let mut total_minor = 0;
        let action = if now_ms - row.not_before > NOTIFY_EXPIRY_MS {
            DrainAction::Expire
        } else {
            let alert = is_alert(&row);
            let player = if alert {
                None
            } else {
                store.player(&row.player_id)?
            };
            let erased = !alert && store.player_erased(&row.player_id)?;
            let consent = if alert {
                None
            } else {
                store.latest_consent(&row.player_id, &row.purpose)?
            };
            let alert_open = match (&row.source_ref, row.kind.as_str()) {
                (Some(id), "low_stock") if alert => store.alert_is_open(id)?,
                _ => false,
            };
            let order = match &row.order_id {
                Some(id) => store.invoice_order(id)?,
                None => None,
            };
            let owed_minor = if row.kind == NOTIFY_KIND_CREDIT_REMINDER {
                store
                    .credit(&row.player_id)?
                    .map_or(0, |c| c.outstanding_minor)
            } else {
                0
            };
            total_minor = match row.kind.as_str() {
                NOTIFY_KIND_CREDIT_REMINDER => owed_minor,
                _ if alert => stored_params(&row)
                    .get("amount_minor")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                NOTIFY_KIND_LOW_BALANCE => stored_params(&row)
                    .get("amount_minor")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                _ => order.as_ref().map_or(0, |o| o.total_minor),
            };
            let session = row
                .session_id
                .as_ref()
                .and_then(|id| live.iter().find(|s| &s.id == id));
            let ctx = RowContext {
                row: &row,
                player: player.as_ref().map(|(p, _, _)| p),
                erased,
                consent,
                order: order.as_ref(),
                session,
                owed_minor,
                alert_open,
                now_ms,
            };
            match (decide(&ctx), &player) {
                (Verdict::Wait, _) => DrainAction::Wait,
                (Verdict::Skip(reason), _) => DrainAction::Skip(reason.to_owned()),
                (Verdict::Pass, _) if alert => DrainAction::Send(Box::new(alert_request(&row))),
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

/// `minor` as the branch writes money (raw minor units when the branch has no locale yet).
fn money_text(minor: i64, locale: Option<&SnapshotLocale>) -> String {
    match locale {
        Some(l) => format_minor(
            minor,
            usize::from(l.exponent),
            &l.symbol,
            l.symbol_position,
            l.number_format,
        ),
        None => minor.to_string(),
    }
}

/// Adds the money (as the branch writes it) and `cafe` parameters the templates need: `amount`
/// for a receipt, `balance` for a low-balance message, `cafe` for all.
fn complete(item: &mut DrainItem, cafe: &str, locale: Option<&SnapshotLocale>) {
    let DrainAction::Send(req) = &mut item.action else {
        return;
    };
    if req.purpose == NOTIFY_PURPOSE_OWNER_ALERT {
        // An alert names the branch itself, so it has no `cafe`; its money is `amount`.
        let has_amount = OwnerAlertKind::parse(&req.kind)
            .is_some_and(|k| k.required_params().contains(&"amount"));
        if has_amount {
            req.params
                .insert("amount".to_owned(), money_text(item.total_minor, locale));
        }
        return;
    }
    let money = match req.kind.as_str() {
        NOTIFY_KIND_RECEIPT | NOTIFY_KIND_CREDIT_REMINDER => Some("amount"),
        NOTIFY_KIND_LOW_BALANCE => Some("balance"),
        _ => None,
    };
    if let Some(name) = money {
        req.params
            .insert(name.to_owned(), money_text(item.total_minor, locale));
    }
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
