//! Group bills: several postpaid sessions on several PCs billed together to one payer.
//!
//! A `group_bills` row links the sessions (through `sessions.group_bill_id`) and names the payer.
//! Each member ends and frees its PC on its own; the one bill is created when the last running
//! member ends, with every member's segments as lines, and it settles once like any other bill.

use arena_proto::{AppError, ErrorCode, GroupPayer, Intent, SessionGroup, SessionMode};
use arena_store::{
    ChangeOp, GroupBillRow, LiveSession, OrderRow, RowKey, SegmentRow, StoreTx, find_group_bill,
    find_group_order, find_order_for_session, find_player, find_session, insert_group_bill,
    list_group_sessions, list_segments, set_group_payer, set_order_payer, set_session_group,
};

use super::mutate::Written;
use super::session::{check_rev, invalid};

const MEMBERS_MAX: usize = 32;

pub(super) const SHARE_REFUSAL: &str = "Only postpaid sessions can share a bill";

/// The other members of a group bill, loaded when one of them ends.
pub(crate) struct GroupEnd {
    pub(super) group: GroupBillRow,
    pub(super) others: Vec<(LiveSession, Vec<SegmentRow>)>,
}

pub(super) fn is_running(s: &LiveSession) -> bool {
    matches!(s.state.as_str(), "active" | "paused" | "grace")
        || (s.state == "expired" && s.ended_at.is_none())
}

impl GroupEnd {
    /// Whether the session ending now is the last running member.
    pub(super) fn is_last(&self) -> bool {
        !self.others.iter().any(|(m, _)| is_running(m))
    }
}

pub(super) fn load_end(
    tx: &StoreTx<'_>,
    session: &LiveSession,
) -> Result<Option<GroupEnd>, AppError> {
    let Some(group_id) = session.group_bill_id.as_deref() else {
        return Ok(None);
    };
    let Some(group) = find_group_bill(tx, group_id)? else {
        return Ok(None);
    };
    let mut others = Vec::new();
    for m in list_group_sessions(tx, group_id)? {
        if m.id != session.id {
            let segments = list_segments(tx, Some(&m.id))?;
            others.push((m, segments));
        }
    }
    Ok(Some(GroupEnd { group, others }))
}

#[derive(Default)]
pub(crate) struct GroupTarget {
    host: Option<LiveSession>,
    members: Vec<Option<LiveSession>>,
    /// Whether the host / each member already has a bill order (products placed on it).
    host_ordered: bool,
    ordered: Vec<bool>,
}

pub(super) fn load(tx: &StoreTx<'_>, g: &SessionGroup) -> Result<GroupTarget, AppError> {
    let Some(host) = find_session(tx, &g.session_id)? else {
        return Ok(GroupTarget::default());
    };
    let members: Vec<Option<LiveSession>> = g
        .member_ids
        .iter()
        .map(|id| find_session(tx, id))
        .collect::<Result<_, _>>()?;
    let ordered = members
        .iter()
        .map(|m| match m {
            Some(m) => Ok(find_order_for_session(tx, &m.id)?.is_some()),
            None => Ok(false),
        })
        .collect::<Result<_, AppError>>()?;
    Ok(GroupTarget {
        host_ordered: find_order_for_session(tx, &host.id)?.is_some(),
        host: Some(host),
        members,
        ordered,
    })
}

pub(super) fn base_rev(intent: &Intent, t: &GroupTarget) -> Result<(), AppError> {
    let Some(h) = &t.host else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    check_rev(intent, h.rev, "session.group")
}

fn joinable(s: &LiveSession, ordered: bool, field: &str) -> Result<(), AppError> {
    if s.mode != SessionMode::Postpaid.as_str() {
        return Err(invalid(field, SHARE_REFUSAL));
    }
    if ordered {
        return Err(invalid(
            field,
            "A session with an order on its bill cannot share a bill",
        ));
    }
    if !matches!(s.state.as_str(), "active" | "paused") {
        return Err(invalid(field, "only a running session can share a bill"));
    }
    Ok(())
}

pub(super) fn validate(g: &SessionGroup, t: &GroupTarget) -> Result<(), AppError> {
    let Some(host) = &t.host else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    joinable(host, t.host_ordered, "session_id")?;
    if g.member_ids.is_empty() || g.member_ids.len() > MEMBERS_MAX {
        return Err(invalid("member_ids", "pick 1 to 32 sessions to add"));
    }
    for (i, ((id, member), ordered)) in g
        .member_ids
        .iter()
        .zip(&t.members)
        .zip(&t.ordered)
        .enumerate()
    {
        if *id == g.session_id || g.member_ids[..i].contains(id) {
            return Err(invalid("member_ids", "each session can be added once"));
        }
        let Some(m) = member else {
            return Err(AppError::with_field(
                ErrorCode::NotFound,
                "member_ids",
                "no such session",
            ));
        };
        joinable(m, *ordered, "member_ids")?;
        if m.group_bill_id.is_some() && m.group_bill_id != host.group_bill_id {
            return Err(invalid(
                "member_ids",
                "That session is already in another group bill",
            ));
        }
    }
    Ok(())
}

fn session_written(id: &str) -> Written {
    Written {
        key: RowKey::Sessions { id: id.to_owned() },
        op: ChangeOp::Update,
        before: None,
    }
}

/// The group bill of `host`, made (paid by the host) when it has none.
fn ensure_group(
    tx: &StoreTx<'_>,
    host: &LiveSession,
    written: &mut Vec<Written>,
) -> Result<String, AppError> {
    if let Some(id) = &host.group_bill_id {
        return Ok(id.clone());
    }
    let id = ulid::Ulid::generate().to_string();
    let guest_no = host.player_id.is_none().then_some(host.guest_no).flatten();
    insert_group_bill(tx, &id, host.player_id.as_deref(), guest_no)?;
    written.push(Written {
        key: RowKey::GroupBills { id: id.clone() },
        op: ChangeOp::Insert,
        before: None,
    });
    set_session_group(tx, &host.id, &id)?;
    written.push(session_written(&host.id));
    Ok(id)
}

/// Puts the session `id`, inserted by this same intent, into the group bill of `host`.
pub(super) fn join_new(
    tx: &StoreTx<'_>,
    host: &LiveSession,
    id: &str,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    let group_id = ensure_group(tx, host, written)?;
    set_session_group(tx, id, &group_id)
}

/// Whether a session started now may join the group bill of `host`.
pub(super) fn check_start(
    mode: SessionMode,
    host: Option<&LiveSession>,
    host_ordered: bool,
) -> Result<(), AppError> {
    if mode != SessionMode::Postpaid {
        return Err(invalid("group_with", SHARE_REFUSAL));
    }
    match host {
        None => Err(AppError::with_field(
            ErrorCode::NotFound,
            "group_with",
            "no such session",
        )),
        Some(h) => joinable(h, host_ordered, "group_with"),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    g: &SessionGroup,
    t: &GroupTarget,
) -> Result<Vec<Written>, AppError> {
    let host = t.host.as_ref().ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "a group validated without its target")
    })?;
    let mut written = Vec::new();
    let group_id = ensure_group(tx, host, &mut written)?;
    for (id, member) in g.member_ids.iter().zip(&t.members) {
        let in_group = member
            .as_ref()
            .is_some_and(|m| m.group_bill_id.as_deref() == Some(group_id.as_str()));
        if !in_group {
            set_session_group(tx, id, &group_id)?;
            written.push(session_written(id));
        }
    }
    Ok(written)
}

#[derive(Default)]
pub(crate) struct PayerTarget {
    group: Option<GroupBillRow>,
    order: Option<OrderRow>,
    members: Vec<LiveSession>,
    player: bool,
}

pub(super) fn load_payer(tx: &StoreTx<'_>, p: &GroupPayer) -> Result<PayerTarget, AppError> {
    let Some(group) = find_group_bill(tx, &p.group_bill_id)? else {
        return Ok(PayerTarget::default());
    };
    let player = match &p.player_id {
        Some(id) => find_player(tx, id)?.is_some(),
        None => false,
    };
    Ok(PayerTarget {
        order: find_group_order(tx, &group.id)?,
        members: list_group_sessions(tx, &group.id)?,
        group: Some(group),
        player,
    })
}

pub(super) fn base_rev_payer(intent: &Intent, t: &PayerTarget) -> Result<(), AppError> {
    let Some(g) = &t.group else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "group_bill_id",
            "no such group bill",
        ));
    };
    check_rev(intent, g.rev, "group.payer")
}

pub(super) fn validate_payer(p: &GroupPayer, t: &PayerTarget) -> Result<(), AppError> {
    if t.group.is_none() {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "group_bill_id",
            "no such group bill",
        ));
    }
    if t.order.as_ref().is_some_and(|o| o.status != "open") {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "group_bill_id",
            "This group bill is already settled",
        ));
    }
    match (&p.player_id, p.guest_no) {
        (Some(_), None) => {
            if !t.player {
                return Err(AppError::with_field(
                    ErrorCode::NotFound,
                    "player_id",
                    "no such player",
                ));
            }
            Ok(())
        }
        (None, Some(n)) => {
            if t.members
                .iter()
                .any(|m| m.player_id.is_none() && m.guest_no == Some(n))
            {
                Ok(())
            } else {
                Err(invalid("guest_no", "that guest is not on this bill"))
            }
        }
        _ => Err(invalid(
            "player_id",
            "name either a player or a guest as the payer",
        )),
    }
}

pub(super) fn mutate_payer(
    tx: &StoreTx<'_>,
    p: &GroupPayer,
    t: &PayerTarget,
) -> Result<Vec<Written>, AppError> {
    let group = t.group.as_ref().ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "a payer validated without its target")
    })?;
    set_group_payer(tx, &group.id, p.player_id.as_deref(), p.guest_no)?;
    let mut written = vec![Written {
        key: RowKey::GroupBills {
            id: group.id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }];
    if let Some(o) = &t.order {
        set_order_payer(tx, &o.id, p.player_id.as_deref(), p.guest_no)?;
        written.push(Written {
            key: RowKey::Orders { id: o.id.clone() },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}
