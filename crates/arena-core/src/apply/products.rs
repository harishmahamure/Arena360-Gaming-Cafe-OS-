use arena_proto::{
    ActorKind, AppError, ErrorCode, GST_MAX_BP, Intent, IntentPayload, KEY_TAX, MAX_BARCODE_CHARS,
    MAX_CATEGORY_CHARS, MAX_PRICE_MINOR, ProductDelete, ProductUpsert, SIMPLE_MAX_BP, TaxMode,
    TaxSettings, hsn_valid, image_problem,
};
use arena_store::{
    ChangeOp, ProductRow, RowKey, StoreTx, barcode_holder, find_product, find_setting,
    insert_product, set_product_deleted, update_product,
};

use super::mutate::Written;
use super::validate::validate_label;

#[derive(Default)]
pub(crate) struct ProductTarget {
    product: Option<ProductRow>,
    holder: Option<String>,
    tax: Option<TaxSettings>,
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
            u.id.as_deref()
        }
        IntentPayload::ProductDelete(d) => Some(d.id.as_str()),
        _ => None,
    };
    if let Some(id) = id {
        t.product = find_product(tx, id)?.filter(|r| r.deleted_at.is_none());
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
    match (&u.id, &t.product) {
        (Some(_), None) => Err(invalid("id", "no such product")),
        _ => Ok(()),
    }
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
    u: &ProductUpsert,
    t: &ProductTarget,
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
        track_stock: u.track_stock,
        image: u.image.clone(),
        barcode: (!barcode.is_empty()).then(|| barcode.to_owned()),
        kitchen: u.kitchen,
        favourite: u.favourite,
        active: u.active,
        rev: 0,
        deleted_at: None,
    };
    match op {
        ChangeOp::Update => update_product(tx, &row)?,
        _ => insert_product(tx, &row)?,
    }
    Ok(written(id, op))
}

pub(super) fn mutate_delete(
    tx: &StoreTx<'_>,
    d: &ProductDelete,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    set_product_deleted(tx, &d.id, now_ms)?;
    Ok(written(d.id.clone(), ChangeOp::Update))
}
