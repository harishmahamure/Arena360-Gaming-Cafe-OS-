//! `gst_archive.publish` (Story 9.14, AD-36): the system intent that records a yearly GST archive
//! PUT to the cloud. It upserts only that year's `gst_archive_log` row, so the upload appears in
//! the audit, and never touches a business row.

use arena_proto::{
    AppError, GST_ARCHIVE_KEY_MAX_CHARS, GST_ARCHIVE_ORPHANS_CAP, GstArchivePublish,
    gst_archive_object_key, valid_fy,
};
use arena_store::{
    ChangeOp, GstArchiveLogRow, NewGstArchiveLog, RowKey, StoreTx, find_gst_archive_log,
    insert_gst_archive_log, update_gst_archive_log,
};

use super::mutate::Written;
use super::session::invalid;

pub(crate) struct GstArchiveTarget {
    existing: Option<GstArchiveLogRow>,
}

/// The `gst_archive_log` row of the payload's year, if it exists.
pub(super) fn load(tx: &StoreTx<'_>, p: &GstArchivePublish) -> Result<GstArchiveTarget, AppError> {
    Ok(GstArchiveTarget {
        existing: find_gst_archive_log(tx, &p.fy)?,
    })
}

pub(super) fn validate(
    intent: &arena_proto::Intent,
    p: &GstArchivePublish,
    _t: &GstArchiveTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "a publish takes no base_rev"));
    }
    if !valid_fy(&p.fy) {
        return Err(invalid(
            "fy",
            "fy must be a financial year such as FY2026-27",
        ));
    }
    if p.key != gst_archive_object_key(&p.fy) || p.key.chars().count() > GST_ARCHIVE_KEY_MAX_CHARS {
        return Err(invalid("key", "key must be gst-archive/<fy>.zst"));
    }
    if p.sha256.len() != 64
        || !p
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("sha256", "sha256 must be 64 lower-case hex digits"));
    }
    if p.bytes < 0 || p.invoices < 0 || p.credit_notes < 0 || p.built_at < 0 {
        return Err(invalid(
            "bytes",
            "sizes, counts and built_at must not be negative",
        ));
    }
    if p.orphaned.len() > GST_ARCHIVE_ORPHANS_CAP {
        return Err(invalid("orphaned", "too many orphaned invoice numbers"));
    }
    if p.orphaned
        .iter()
        .any(|n| n.is_empty() || n.chars().count() > 128 || n.chars().any(char::is_control))
    {
        return Err(invalid(
            "orphaned",
            "an orphaned invoice number is 1 to 128 printable characters",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &GstArchivePublish,
    t: &GstArchiveTarget,
) -> Result<Vec<Written>, AppError> {
    let new = NewGstArchiveLog {
        fy: &p.fy,
        key: &p.key,
        sha256: &p.sha256,
        bytes: p.bytes,
        invoices: p.invoices,
        credit_notes: p.credit_notes,
        orphaned: &p.orphaned,
        built_at: p.built_at,
    };
    let op = if t.existing.is_some() {
        update_gst_archive_log(tx, &new)?;
        ChangeOp::Update
    } else {
        insert_gst_archive_log(tx, &new)?;
        ChangeOp::Insert
    };
    Ok(vec![Written {
        key: RowKey::GstArchiveLog { fy: p.fy.clone() },
        op,
        before: None,
    }])
}
