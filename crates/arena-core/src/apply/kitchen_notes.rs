//! Kitchen notes (Story 7.2): the preset list on a kitchen product and the notes chosen on an
//! order line. Notes are text for the kitchen: they never change price, tax or stock.

use std::collections::BTreeSet;

use arena_proto::{
    ActorKind, AppError, ErrorCode, MAX_KITCHEN_PRESETS, MAX_LINE_NOTE_CHARS, MAX_LINE_NOTES,
    MAX_PRESET_NOTE_CHARS, PosSaleLine,
};

use super::pos_sale::Basket;
use super::session::invalid;

/// A note as it is stored: trimmed.
pub(super) fn clean(notes: &[String]) -> Vec<String> {
    notes.iter().map(|n| n.trim().to_owned()).collect()
}

/// One note's text rules: not empty, no control characters, at most `max` characters
/// (`FIELD_TOO_LONG` past it).
fn check_text(note: &str, max: usize, field: &str) -> Result<(), AppError> {
    let note = note.trim();
    if note.is_empty() || note.chars().any(char::is_control) {
        return Err(invalid(field, "a kitchen note is text without line breaks"));
    }
    if note.chars().count() > max {
        return Err(AppError::with_field(
            ErrorCode::FieldTooLong,
            field,
            format!("a kitchen note is up to {max} characters"),
        ));
    }
    Ok(())
}

/// The preset list of a product: up to 8 distinct notes of up to 40 characters, on a kitchen
/// product only.
pub(super) fn check_presets(notes: &[String], kitchen: bool) -> Result<(), AppError> {
    let field = "kitchen_notes";
    if notes.is_empty() {
        return Ok(());
    }
    if !kitchen {
        return Err(invalid(field, "only a kitchen product has kitchen notes"));
    }
    if notes.len() > MAX_KITCHEN_PRESETS {
        return Err(invalid(field, "a product holds up to 8 kitchen notes"));
    }
    let mut seen = BTreeSet::new();
    for n in notes {
        check_text(n, MAX_PRESET_NOTE_CHARS, field)?;
        if !seen.insert(n.trim()) {
            return Err(invalid(field, "a kitchen note is listed twice"));
        }
    }
    Ok(())
}

/// The notes on the lines of an order: each line's product must be a kitchen product, a note is
/// up to 60 characters, and a line holds distinct notes. Staff may add one free-text note beside
/// the product's presets; a device (the kiosk) may only toggle presets.
pub(super) fn check_lines(
    actor: ActorKind,
    lines: &[PosSaleLine],
    basket: &Basket,
) -> Result<(), AppError> {
    for (i, (line, item)) in lines.iter().zip(&basket.items).enumerate() {
        if line.kitchen_notes.is_empty() {
            continue;
        }
        let field = format!("lines[{i}].kitchen_notes");
        let Some(p) = item.product.as_ref().filter(|p| p.kitchen) else {
            return Err(invalid(&field, "this product takes no kitchen notes"));
        };
        if line.kitchen_notes.len() > MAX_LINE_NOTES {
            return Err(invalid(&field, "too many kitchen notes on one line"));
        }
        let mut seen = BTreeSet::new();
        let mut free = 0;
        for n in &line.kitchen_notes {
            check_text(n, MAX_LINE_NOTE_CHARS, &field)?;
            if !seen.insert(n.trim()) {
                return Err(invalid(&field, "a kitchen note is listed twice"));
            }
            if !p.kitchen_notes.iter().any(|preset| preset == n.trim()) {
                free += 1;
            }
        }
        if free > usize::from(actor != ActorKind::Device) {
            return Err(invalid(
                &field,
                if actor == ActorKind::Device {
                    "only the product's preset notes are offered here"
                } else {
                    "one free-text note per line"
                },
            ));
        }
    }
    Ok(())
}
