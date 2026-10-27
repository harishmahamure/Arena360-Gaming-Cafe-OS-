//! A rule change mid-session: `session.rate_flip` closes a postpaid or wallet session's open rate
//! segment and opens the next at the rate and rule the group's pricing rules now give.
//!
//! [`pending`] is the one rule for whether a flip is due and when: the Hub's timer scans with it
//! and `apply()` runs it again, so the two cannot disagree. A flip happens at the instant the rule
//! begins or ends (a missed boundary is applied later, at its true time) or, for an edit or a
//! switch-off, at the moment it is applied.

use arena_proto::{AppError, ErrorCode, Intent, SessionMode, SessionRateFlip};
use arena_store::{
    ChangeOp, LiveSession, PricingRuleRow, RateRow, RowKey, SegmentRow, StoreTx, add_segment_pause,
    close_segments, find_rate, find_session, insert_segment, list_pricing_rules, list_segments,
    reprice_wallet_session,
};
use chrono_tz::Tz;

use super::mutate::Written;
use super::session::invalid;
use crate::pricing::{Flip, OpenSegment, due_flip};

const MIN_MS: i64 = 60_000;

#[derive(Default)]
pub(crate) struct FlipTarget {
    session: Option<LiveSession>,
    segments: Vec<SegmentRow>,
    flip: Option<Flip>,
}

/// The open segment of `segments`, if the session has one.
pub(super) fn open_of(segments: &[SegmentRow]) -> Option<&SegmentRow> {
    segments.iter().rev().find(|g| g.to_at.is_none())
}

/// Where a wallet session's own time stops: its end less the minutes extended since.
pub(super) fn own_end(s: &LiveSession) -> Option<i64> {
    s.ends_at.map(|end| end - s.extended_min * MIN_MS)
}

/// Whether a session's rate can change mid-session: a postpaid one, running or paused, or a
/// wallet one still running (a pack, pass or block is priced by its plan).
fn flippable(s: &LiveSession) -> bool {
    (s.mode == SessionMode::Postpaid.as_str() && matches!(s.state.as_str(), "active" | "paused"))
        || (s.mode == SessionMode::Wallet.as_str() && s.state == "active")
}

/// The rate change due on `s` at `now_ms`, if any. `base` is the rate row the session started on:
/// editing the group's rate never reprices a running session, only its pricing rules do.
pub(crate) fn pending(
    s: &LiveSession,
    segments: &[SegmentRow],
    base: &RateRow,
    rules: &[PricingRuleRow],
    tz: Tz,
    now_ms: i64,
) -> Option<Flip> {
    if !flippable(s) {
        return None;
    }
    let open = open_of(segments)?;
    let flip = due_flip(
        rules,
        &base.group_id,
        base.price_per_hour_minor,
        tz,
        &OpenSegment {
            from_at: open.from_at,
            rule_id: open.rule_id.as_deref(),
            rate_minor: open.rate_minor_per_hour,
        },
        now_ms,
    )?;
    if s.mode == SessionMode::Wallet.as_str() && own_end(s).is_none_or(|end| flip.at >= end) {
        return None;
    }
    Some(flip)
}

pub(super) fn load(
    tx: &StoreTx<'_>,
    f: &SessionRateFlip,
    now_ms: i64,
) -> Result<FlipTarget, AppError> {
    let Some(session) = find_session(tx, &f.session_id)? else {
        return Ok(FlipTarget::default());
    };
    let segments = list_segments(tx, Some(&session.id))?;
    let base = match session.rate_id.as_deref() {
        Some(id) => find_rate(tx, id)?,
        None => None,
    };
    let tz = crate::locale::tz_at(tx, now_ms)?;
    let flip = match (base, tz) {
        (Some(base), Some(tz)) => pending(
            &session,
            &segments,
            &base,
            &list_pricing_rules(tx)?,
            tz,
            now_ms,
        ),
        _ => None,
    };
    Ok(FlipTarget {
        session: Some(session),
        segments,
        flip,
    })
}

pub(super) fn validate(intent: &Intent, t: &FlipTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "session.rate_flip takes no base_rev"));
    }
    if t.session.is_none() {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    }
    if t.flip.is_none() {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "session_id",
            "no rate change is due for this session",
        ));
    }
    Ok(())
}

pub(super) fn mutate(tx: &StoreTx<'_>, t: &FlipTarget) -> Result<Vec<Written>, AppError> {
    let internal = || {
        AppError::new(
            ErrorCode::Internal,
            "a rate flip validated without its target",
        )
    };
    let (s, flip) = t
        .session
        .as_ref()
        .zip(t.flip.as_ref())
        .ok_or_else(internal)?;
    let open = open_of(&t.segments).ok_or_else(internal)?;
    let split = s.paused_at.map_or(0, |paused_at| {
        (flip.at - paused_at.max(open.from_at)).max(0)
    });
    if split > 0 {
        add_segment_pause(tx, &s.id, split)?;
    }
    close_segments(tx, &s.id, flip.at)?;
    let segment_id = ulid::Ulid::generate().to_string();
    insert_segment(
        tx,
        &segment_id,
        &s.id,
        flip.at,
        &open.device_id,
        flip.rate_minor,
        flip.rule_id.as_deref(),
    )?;
    if s.mode == SessionMode::Wallet.as_str() {
        resize_wallet(tx, s, &t.segments, open, split, flip.at, flip.rate_minor)?;
    }
    let mut written = Vec::new();
    if s.mode == SessionMode::Wallet.as_str() {
        written.push(Written {
            key: RowKey::Sessions { id: s.id.clone() },
            op: ChangeOp::Update,
            before: None,
        });
    }
    written.push(Written {
        key: RowKey::SessionSegments {
            id: open.id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    written.push(Written {
        key: RowKey::SessionSegments { id: segment_id },
        op: ChangeOp::Insert,
        before: None,
    });
    Ok(written)
}

/// Re-sizes a wallet session's end and set-aside for a new segment at `rate_minor` opening at
/// `at`. `open` is the segment that closes there with `split` ms of the current pause.
pub(super) fn resize_wallet(
    tx: &StoreTx<'_>,
    s: &LiveSession,
    segments: &[SegmentRow],
    open: &SegmentRow,
    split: i64,
    at: i64,
    rate_minor: i64,
) -> Result<(), AppError> {
    let closed: Vec<SegmentRow> = segments
        .iter()
        .map(|g| {
            if g.id == open.id {
                SegmentRow {
                    to_at: Some(at),
                    paused_ms: g.paused_ms + split,
                    ..g.clone()
                }
            } else {
                g.clone()
            }
        })
        .collect();
    let sized = crate::billing::wallet_end_after_flip(
        &closed,
        at,
        rate_minor,
        s.rounding_min,
        s.minimum_min,
        s.reserved_minor,
    );
    reprice_wallet_session(
        tx,
        &s.id,
        sized.ends_at + s.extended_min * MIN_MS,
        sized.reserved_minor,
    )
}
