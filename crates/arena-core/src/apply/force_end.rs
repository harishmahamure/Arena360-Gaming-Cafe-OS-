//! `session.force_end` and its undo: ending a session early on a manager's PIN and a reason.
//!
//! The end itself is the ordinary end ([`super::session::mutate_end`]): a pack session deducts
//! only the minutes played, a wallet session pays only for the time used. On top of that the
//! session is marked `forced` with its reason, note, the minutes it had left and the pack minutes
//! the end took, so the audit shows them and the undo can give them back.

use arena_proto::{AppError, ErrorCode, Intent, SessionForceEnd, SessionMode};
use arena_store::{
    ChangeOp, ForcedEnd, LiveSession, RowKey, StoreTx, find_player_pack, forced_pack_minutes,
    reopen_segments, reopen_session, restore_pack_minutes, set_device_state,
};

use super::mutate::Written;
use super::session::{
    EndRevertTarget, EndTarget, check_rev, in_window, invalid, same_staff, undo_denied,
};

const MIN_MS: i64 = 60_000;

/// The longest note a forced end takes.
const NOTE_MAX_CHARS: usize = 140;

pub(crate) struct ForceEndTarget {
    end: EndTarget,
}

pub(super) fn load(tx: &StoreTx<'_>, f: &SessionForceEnd) -> Result<ForceEndTarget, AppError> {
    Ok(ForceEndTarget {
        end: super::session::load_end(
            tx,
            &arena_proto::SessionEnd {
                session_id: f.session_id.clone(),
            },
        )?,
    })
}

pub(super) fn base_rev(intent: &Intent, t: &ForceEndTarget) -> Result<(), AppError> {
    super::session::base_rev_end(intent, &t.end)
}

/// The session must still run, and \`other\` needs a note of 1 to 140 characters (any other reason
/// takes none).
pub(super) fn validate(
    intent: &Intent,
    f: &SessionForceEnd,
    t: &ForceEndTarget,
) -> Result<(), AppError> {
    super::session::validate_end(&t.end)?;
    let note = f.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
    match (intent.reason, note) {
        (Some(arena_proto::ReasonCode::Other), Some(n)) if n.chars().count() <= NOTE_MAX_CHARS => {
            Ok(())
        }
        (Some(arena_proto::ReasonCode::Other), _) => {
            Err(invalid("note", "say why in 1 to 140 characters"))
        }
        (_, Some(_)) => Err(invalid("note", "only the reason other takes a note")),
        _ => Ok(()),
    }
}

/// The pack minutes of a pack session's pack, 0 when it plays on none.
fn pack_minutes(tx: &StoreTx<'_>, s: &LiveSession) -> Result<i64, AppError> {
    let (Some(player_id), Some(pack_id), true) = (
        s.player_id.as_deref(),
        s.pack_id.as_deref(),
        s.mode == SessionMode::Pack.as_str(),
    ) else {
        return Ok(0);
    };
    Ok(find_player_pack(tx, player_id, pack_id)?
        .and_then(|p| p.minutes_left)
        .unwrap_or(0))
}

/// The whole minutes a timed session had left: its end less the moment it was paused (or now).
fn minutes_left(s: &LiveSession, now_ms: i64) -> i64 {
    s.ends_at.map_or(0, |end| {
        let from = s.paused_at.unwrap_or(now_ms);
        ((end - from).max(0) + MIN_MS - 1) / MIN_MS
    })
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    f: &SessionForceEnd,
    t: &ForceEndTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(s), Some(reason)) = (&t.end.session, intent.reason) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a forced end validated without its target",
        ));
    };
    let before = pack_minutes(tx, s)?;
    let written = super::session::mutate_end(tx, intent, &t.end, now_ms)?;
    let taken = (before - pack_minutes(tx, s)?).max(0);
    let note = f.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
    arena_store::mark_forced(
        tx,
        &s.id,
        &ForcedEnd {
            reason: reason.as_str(),
            note,
            left_min: minutes_left(s, now_ms),
            pack_min: taken,
        },
    )?;
    Ok(written)
}

pub(crate) struct ForceEndRevertTarget {
    base: EndRevertTarget,
    pack_min: Option<i64>,
}

pub(super) fn load_reverted(
    tx: &StoreTx<'_>,
    r: &arena_proto::SessionForceEndReverted,
    now: i64,
) -> Result<ForceEndRevertTarget, AppError> {
    Ok(ForceEndRevertTarget {
        base: super::session::load_end_reverted_of(tx, &r.session_id, now, "session.force_end")?,
        pack_min: forced_pack_minutes(tx, &r.session_id)?,
    })
}

pub(super) fn base_rev_reverted(intent: &Intent, t: &ForceEndRevertTarget) -> Result<(), AppError> {
    let Some(s) = &t.base.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    check_rev(intent, s.rev, "session.force_end.reverted")
}

/// Only a session ended by \`session.force_end\` comes back, by the staff member who ended it
/// within the undo window, with its PC free and no receipt, drawer kick or invoice recorded. A
/// postpaid one needs its bill still open and unpaid.
pub(super) fn validate_reverted(intent: &Intent, t: &ForceEndRevertTarget) -> Result<(), AppError> {
    let b = &t.base;
    let Some(s) = &b.session else {
        return Err(invalid("session_id", "no such session"));
    };
    if s.state != "ended" || t.pack_min.is_none() {
        return Err(undo_denied("this session was not force-ended"));
    }
    let postpaid = s.mode == SessionMode::Postpaid.as_str();
    if postpaid {
        if b.order.is_none() && s.group_bill_id.is_some() {
            return Err(undo_denied(
                "only the last session of a group to end can be undone",
            ));
        }
        if b.order.as_ref().is_none_or(|o| o.status != "open") || b.paid {
            return Err(undo_denied("the bill is no longer open and unpaid"));
        }
    } else if s.mode != SessionMode::Block.as_str() && b.order.is_none() {
        return Err(undo_denied("this session has no bill to undo"));
    }
    if b.busy {
        return Err(undo_denied("This PC already has a session."));
    }
    if !b
        .device
        .as_ref()
        .is_some_and(|d| d.deleted_at.is_none() && d.state != "maintenance")
    {
        return Err(undo_denied("This PC is not available."));
    }
    if !same_staff(intent, s.ended_by.as_deref()) {
        return Err(undo_denied("only the staff member who ended it can undo"));
    }
    if !in_window(b.now, s.ended_at) {
        return Err(undo_denied("the undo window has closed"));
    }
    if b.effect {
        return Err(undo_denied("a receipt, drawer or invoice already happened"));
    }
    Ok(())
}

/// Reopens the session and its segments and puts its PC back in session. A postpaid session drops
/// its open bill; a pack, pass or wallet session also drops its settled bill, payment and wallet
/// debit and gets its pack minutes back; a block session keeps the bill it was sold with.
pub(super) fn mutate_reverted(
    tx: &StoreTx<'_>,
    t: &ForceEndRevertTarget,
) -> Result<Vec<Written>, AppError> {
    let b = &t.base;
    let (Some(s), Some(d), Some(ended_at)) = (
        &b.session,
        &b.device,
        b.session.as_ref().and_then(|s| s.ended_at),
    ) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "an undo validated without its target",
        ));
    };
    if s.mode == SessionMode::Block.as_str() {
        return reopen_block(tx, s, d, ended_at);
    }
    let mut written = Vec::new();
    if s.mode != SessionMode::Postpaid.as_str() {
        let settle = super::session::SettleRevertTarget::of(tx, b)?;
        let order = settle.order.as_ref().map(|o| o.id.clone());
        written.extend(super::session::mutate_settle_reverted(tx, &settle)?);
        written
            .retain(|w| !matches!((&w.key, &order), (RowKey::Orders { id }, Some(o)) if id == o));
    }
    written.extend(super::session::mutate_end_reverted(tx, b)?);
    if let (Some(pack_id), Some(minutes)) = (&s.pack_id, t.pack_min.filter(|m| *m > 0)) {
        restore_pack_minutes(tx, pack_id, minutes)?;
        written.push(Written {
            key: RowKey::PlayerPacks {
                id: pack_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}

fn reopen_block(
    tx: &StoreTx<'_>,
    s: &LiveSession,
    d: &arena_store::DeviceMetaRow,
    ended_at: i64,
) -> Result<Vec<Written>, AppError> {
    reopen_segments(tx, &s.id, ended_at)?;
    reopen_session(tx, &s.id)?;
    set_device_state(tx, &d.device_id, "in_session", Some(&d.state), None)?;
    Ok(vec![
        Written {
            key: RowKey::Sessions { id: s.id.clone() },
            op: ChangeOp::Update,
            before: None,
        },
        Written {
            key: RowKey::DevicesMeta {
                device_id: d.device_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        },
    ])
}
