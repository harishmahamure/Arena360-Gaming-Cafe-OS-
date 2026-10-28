use arena_proto::{
    ActorKind, AppError, ErrorCode, GST_MAX_BP, ImportProductRow, Intent, IntentPayload, KEY_TAX,
    MAX_BARCODE_CHARS, MAX_BUNDLE_COMPONENTS, MAX_CATEGORY_CHARS, MAX_COMPONENT_QTY,
    MAX_PRICE_MINOR, ProductDelete, ProductUpsert, SIMPLE_MAX_BP, TaxMode, TaxSettings, hsn_valid,
    image_problem,
};
use arena_store::{
    ChangeOp, ProductRow, RowKey, StoreTx, Table, barcode_holder, delete_row, find_product,
    find_setting, insert_component, insert_product, list_components, live_bundles_with, read_row,
    set_low_stock_qty, set_product_deleted, update_product,
};

use super::mutate::Written;
use super::validate::validate_label;

#[derive(Default)]
pub(crate) struct ProductTarget {
    product: Option<ProductRow>,
    holder: Option<String>,
    tax: Option<TaxSettings>,
    parts: Vec<Option<ProductRow>>,
    old_components: Vec<(String, i64)>,
    nested: bool,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<ProductTarget, AppError> {
    let mut t = ProductTarget::default();
    let id = match p {
        IntentPayload::ProductUpsert(u) => {
            let barcode = u.barcode.trim();
            if !barcode.is_empty() {
                t.holder = barcode_holder(tx, barcode)?;
            }
            t.tax = find_setting(tx, KEY_TAX)?
                .map(|s| {
                    TaxSettings::parse(&s.value).ok_or_else(|| {
                        AppError::new(ErrorCode::Internal, "the tax settings cannot be read")
                    })
                })
                .transpose()?;
            t.parts = u
                .components
                .iter()
                .take(MAX_BUNDLE_COMPONENTS + 1)
                .map(|c| find_product(tx, &c.product_id))
                .collect::<Result<_, _>>()?;
            u.id.as_deref()
        }
        IntentPayload::ProductDelete(d) => Some(d.id.as_str()),
        _ => None,
    };
    if let Some(id) = id {
        t.product = find_product(tx, id)?.filter(|r| r.deleted_at.is_none());
        if t.product.is_some() {
            t.old_components = list_components(tx, id)?;
            t.nested = !live_bundles_with(tx, id)?.is_empty();
        }
    }
    Ok(t)
}

pub(super) fn base_rev(intent: &Intent, t: &ProductTarget) -> Result<(), AppError> {
    match (t.product.as_ref().map(|p| p.rev), intent.base_rev) {
        (Some(_), None) if intent.actor.kind != ActorKind::Owner => {
            Err(invalid("base_rev", "editing needs base_rev"))
        }
        (Some(rev), Some(base)) if rev != base => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "this changed since it was read",
        )),
        _ => Ok(()),
    }
}

fn gst(t: &ProductTarget) -> bool {
    t.tax
        .as_ref()
        .is_some_and(|x| x.tax_mode() == TaxMode::GstIn)
}

fn rate(u: &ProductUpsert, t: &ProductTarget) -> u16 {
    u.tax_rate_bp.unwrap_or_else(|| {
        t.product.as_ref().map_or_else(
            || t.tax.as_ref().map_or(0, |x| x.rates().0),
            |p| u16::try_from(p.tax_rate_bp).unwrap_or(0),
        )
    })
}

pub(super) fn validate_upsert(u: &ProductUpsert, t: &ProductTarget) -> Result<(), AppError> {
    validate_label(&u.name).map_err(|m| invalid("name", &m))?;
    let category = u.category.trim();
    if category.chars().count() > MAX_CATEGORY_CHARS || category.chars().any(char::is_control) {
        return Err(invalid("category", "category must be up to 40 characters"));
    }
    if !(0..=MAX_PRICE_MINOR).contains(&u.price_minor) {
        return Err(invalid("price_minor", "price is out of range"));
    }
    let cap = if gst(t) { GST_MAX_BP } else { SIMPLE_MAX_BP };
    if rate(u, t) > cap {
        return Err(invalid("tax_rate_bp", "tax rate is out of range"));
    }
    let hsn = u.hsn.trim();
    if gst(t) && !hsn_valid(hsn) {
        return Err(AppError::with_field(
            ErrorCode::HsnRequired,
            "hsn",
            "HSN must be 4, 6 or 8 digits",
        ));
    }
    if !hsn.is_empty() && !hsn_valid(hsn) {
        return Err(invalid("hsn", "HSN must be 4, 6 or 8 digits"));
    }
    let barcode = u.barcode.trim();
    if barcode.chars().count() > MAX_BARCODE_CHARS || !barcode.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(invalid(
            "barcode",
            "barcode must be up to 48 visible characters",
        ));
    }
    if u.active && t.holder.as_ref().is_some_and(|h| Some(h) != u.id.as_ref()) {
        return Err(AppError::with_field(
            ErrorCode::BarcodeTaken,
            "barcode",
            "another active product uses this barcode",
        ));
    }
    if let Some(m) = image_problem(&u.image) {
        return Err(invalid("image", m));
    }
    if u.id.is_some() && t.product.is_none() {
        return Err(invalid("id", "no such product"));
    }
    validate_components(u, t)
}

fn validate_components(u: &ProductUpsert, t: &ProductTarget) -> Result<(), AppError> {
    let bad = |m: &str| invalid("components", m);
    if !u.bundle {
        return if u.components.is_empty() {
            Ok(())
        } else {
            Err(bad("only a bundle has components"))
        };
    }
    if t.nested {
        return Err(invalid("bundle", "this product is a component of a bundle"));
    }
    if u.components.is_empty() || u.components.len() > MAX_BUNDLE_COMPONENTS {
        return Err(bad("a bundle needs 1 to 30 components"));
    }
    let mut seen = std::collections::HashSet::new();
    for (c, part) in u.components.iter().zip(&t.parts) {
        if !(1..=MAX_COMPONENT_QTY).contains(&c.qty) {
            return Err(bad("quantity must be 1 to 1000"));
        }
        if Some(&c.product_id) == u.id.as_ref() {
            return Err(bad("a bundle cannot contain itself"));
        }
        if !seen.insert(&c.product_id) {
            return Err(bad("a component is listed twice"));
        }
        match part {
            Some(p) if p.deleted_at.is_none() && p.bundle => {
                return Err(bad("a bundle cannot contain a bundle"));
            }
            Some(p) if p.deleted_at.is_none() => {}
            _ => return Err(bad("no such product")),
        }
    }
    Ok(())
}

pub(super) fn validate_delete(t: &ProductTarget) -> Result<(), AppError> {
    t.product
        .as_ref()
        .map(|_| ())
        .ok_or_else(|| invalid("id", "no such product"))
}

fn written(id: String, op: ChangeOp) -> Vec<Written> {
    vec![Written {
        key: RowKey::Products { product_id: id },
        op,
        before: None,
    }]
}

pub(super) fn mutate_upsert(
    tx: &StoreTx<'_>,
    intent: &Intent,
    u: &ProductUpsert,
    t: &ProductTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (id, op) = match &t.product {
        Some(old) => (old.product_id.clone(), ChangeOp::Update),
        None => (ulid::Ulid::generate().to_string(), ChangeOp::Insert),
    };
    let barcode = u.barcode.trim();
    let row = ProductRow {
        product_id: id.clone(),
        name: u.name.trim().to_owned(),
        category: u.category.trim().to_owned(),
        price_minor: u.price_minor,
        tax_rate_bp: rate(u, t).into(),
        hsn: u.hsn.trim().to_owned(),
        track_stock: u.track_stock && !u.bundle,
        image: u.image.clone(),
        barcode: (!barcode.is_empty()).then(|| barcode.to_owned()),
        kitchen: u.kitchen,
        favourite: u.favourite,
        active: u.active,
        bundle: u.bundle,
        rev: 0,
        deleted_at: None,
    };
    match op {
        ChangeOp::Update => update_product(tx, &row)?,
        _ => insert_product(tx, &row)?,
    }
    let mut out = written(id.clone(), op);
    if !row.track_stock || row.bundle {
        set_low_stock_qty(tx, &id, None)?;
        out.extend(super::stock::ack_low_for(
            tx,
            &id,
            &intent.actor.id,
            now_ms,
        )?);
    }
    let kept = |id: &str, qty: i64| {
        u.components
            .iter()
            .any(|c| c.product_id == id && c.qty == qty)
    };
    let stale: Vec<_> = t
        .old_components
        .iter()
        .filter(|(c, q)| !kept(c, *q))
        .cloned()
        .collect();
    drop_components(tx, &id, &stale, &mut out)?;
    for c in u.components.iter().filter(|c| {
        !t.old_components
            .iter()
            .any(|(o, q)| *o == c.product_id && *q == c.qty)
    }) {
        insert_component(tx, &id, &c.product_id, c.qty)?;
        out.push(component_write(&id, &c.product_id, ChangeOp::Insert, None));
    }
    Ok(out)
}

fn component_write(
    bundle_id: &str,
    component_id: &str,
    op: ChangeOp,
    before: Option<arena_store::Row>,
) -> Written {
    Written {
        key: RowKey::BundleComponents {
            bundle_id: bundle_id.to_owned(),
            component_id: component_id.to_owned(),
        },
        op,
        before,
    }
}

fn drop_components(
    tx: &StoreTx<'_>,
    bundle_id: &str,
    old: &[(String, i64)],
    out: &mut Vec<Written>,
) -> Result<(), AppError> {
    for (component_id, _) in old {
        let key = RowKey::BundleComponents {
            bundle_id: bundle_id.to_owned(),
            component_id: component_id.clone(),
        };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::BundleComponents, &before)?;
        out.push(component_write(
            bundle_id,
            component_id,
            ChangeOp::Delete,
            Some(before),
        ));
    }
    Ok(())
}

pub(super) fn mutate_delete(
    tx: &StoreTx<'_>,
    intent: &Intent,
    d: &ProductDelete,
    t: &ProductTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    set_product_deleted(tx, &d.id, now_ms)?;
    let mut out = written(d.id.clone(), ChangeOp::Update);
    out.extend(super::stock::ack_low_for(
        tx,
        &d.id,
        &intent.actor.id,
        now_ms,
    )?);
    drop_components(tx, &d.id, &t.old_components, &mut out)?;
    Ok(out)
}

/// The product an import row writes: a new active product, or the matched one with only the
/// columns of the file changed (its image, favourite flag and active flag stay).
pub(crate) fn upsert_from_import(
    row: &ImportProductRow,
    old: Option<&ProductRow>,
) -> ProductUpsert {
    ProductUpsert {
        id: old.map(|o| o.product_id.clone()),
        name: row.name.trim().to_owned(),
        category: row.category.trim().to_owned(),
        price_minor: row.price_minor,
        tax_rate_bp: row.tax_rate_bp,
        hsn: row.hsn.trim().to_owned(),
        track_stock: row.track_stock,
        image: old.map(|o| o.image.clone()).unwrap_or_default(),
        barcode: row.barcode.trim().to_owned(),
        kitchen: row.kitchen,
        favourite: old.is_some_and(|o| o.favourite),
        active: old.is_none_or(|o| o.active),
        bundle: false,
        components: Vec::new(),
    }
}

/// The Story 5.3 rules for one import row: the same checks a `product.upsert` of it would
/// meet. `old` is the product the row updates, `holder` the active product that holds the row's
/// barcode now, `tax` the branch's tax settings.
pub(crate) fn check_import_row(
    row: &ImportProductRow,
    old: Option<&ProductRow>,
    holder: Option<&str>,
    tax: Option<&TaxSettings>,
) -> Result<(), AppError> {
    let t = import_target(old, holder, tax);
    validate_upsert(&upsert_from_import(row, old), &t)
}

/// The `product.upsert` rules for a new bundle of a setup file (Story 5.36): `parts` are the
/// products its components name (`None` for one that does not exist), `holder` the active
/// product that holds its barcode now.
pub(crate) fn check_bundle(
    u: &ProductUpsert,
    parts: Vec<Option<ProductRow>>,
    holder: Option<&str>,
    tax: Option<&TaxSettings>,
) -> Result<(), AppError> {
    let target = ProductTarget {
        parts,
        ..import_target(None, holder, tax)
    };
    validate_upsert(u, &target)
}

pub(crate) fn import_target(
    old: Option<&ProductRow>,
    holder: Option<&str>,
    tax: Option<&TaxSettings>,
) -> ProductTarget {
    ProductTarget {
        product: old.cloned(),
        holder: holder.map(str::to_owned),
        tax: tax.cloned(),
        ..ProductTarget::default()
    }
}
