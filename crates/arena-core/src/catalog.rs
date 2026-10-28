use arena_proto::{
    AppError, CatalogEntry, ComponentView, DEFAULT_STOCK_EXPIRY_WARN_DAYS, ExpiringLot, ExpiryView,
    KEY_STOCK_EXPIRY_WARN_DAYS, Product, REASON_COMPONENT_INACTIVE, REASON_EXPIRED,
    STOCK_EXPIRY_WARN_RANGE, add_days, availability, bundle_quantity, lot_expired,
};
use arena_store::{ProductRow, Store};

struct Status {
    quantity: i64,
    avg_cost_minor: i64,
    available: bool,
    reason: Option<&'static str>,
    components: Vec<ComponentView>,
}

/// The units of a tracked product that may be sold: its stock less what sits in lots past their
/// expiry date on `today`. Also answers whether any of the missing units are expired.
fn sellable(
    store: &Store,
    product_id: &str,
    qty: i64,
    today: Option<&str>,
) -> Result<(i64, bool), AppError> {
    let expired = match today {
        Some(day) => store.expired_qty(product_id, day)?,
        None => 0,
    };
    Ok(((qty - expired).max(0), expired > 0))
}

fn status(store: &Store, p: &ProductRow, today: Option<&str>) -> Result<Status, AppError> {
    let deleted = p.deleted_at.is_some();
    if !p.bundle {
        let stock = store.stock(&p.product_id)?;
        let quantity = stock.as_ref().map_or(0, |s| s.qty);
        let (usable, has_expired) = if p.track_stock {
            sellable(store, &p.product_id, quantity, today)?
        } else {
            (quantity, false)
        };
        let (available, reason) = match availability(p.active, deleted, p.track_stock, usable) {
            (false, Some("out_of_stock")) if has_expired => (false, Some(REASON_EXPIRED)),
            other => other,
        };
        return Ok(Status {
            quantity,
            avg_cost_minor: stock.map_or(0, |s| s.avg_cost_minor),
            available,
            reason,
            components: Vec::new(),
        });
    }
    let mut components = Vec::new();
    let mut parts = Vec::new();
    for (id, qty) in store.components(&p.product_id)? {
        let row = store.product(&id)?;
        let live = row.as_ref().filter(|c| c.deleted_at.is_none());
        if live.is_some_and(|c| c.track_stock) {
            let held = store.stock(&id)?.map_or(0, |s| s.qty);
            parts.push((sellable(store, &id, held, today)?.0, qty));
        }
        components.push(ComponentView {
            name: row.as_ref().map_or_else(|| id.clone(), |c| c.name.clone()),
            active: live.is_some_and(|c| c.active),
            product_id: id,
            qty,
        });
    }
    let quantity = bundle_quantity(parts);
    let (available, reason) = match availability(p.active, deleted, false, 0) {
        (true, _) if components.iter().any(|c| !c.active) => {
            (false, Some(REASON_COMPONENT_INACTIVE))
        }
        (true, _) => availability(true, false, quantity.is_some(), quantity.unwrap_or(0)),
        gone => gone,
    };
    Ok(Status {
        quantity: quantity.unwrap_or(0),
        avg_cost_minor: 0,
        available,
        reason,
        components,
    })
}

pub(crate) fn products(store: &Store, now_ms: i64) -> Result<Vec<Product>, AppError> {
    let today = crate::locale::current_business_day(store, now_ms)?;
    store
        .products()?
        .into_iter()
        .map(|p| {
            let s = status(store, &p, today.as_deref())?;
            Ok(Product {
                id: p.product_id,
                name: p.name,
                category: p.category,
                price_minor: p.price_minor,
                tax_rate_bp: u16::try_from(p.tax_rate_bp).unwrap_or(0),
                hsn: p.hsn,
                track_stock: p.track_stock,
                image: p.image,
                barcode: p.barcode.unwrap_or_default(),
                kitchen: p.kitchen,
                favourite: p.favourite,
                active: p.active,
                bundle: p.bundle,
                components: s.components,
                rev: p.rev,
                quantity: s.quantity,
                avg_cost_minor: s.avg_cost_minor,
                available: s.available,
                reason: s.reason.map(str::to_owned),
            })
        })
        .collect()
}

pub(crate) fn entries(
    store: &Store,
    ids: &[String],
    now_ms: i64,
) -> Result<Vec<CatalogEntry>, AppError> {
    let today = crate::locale::current_business_day(store, now_ms)?;
    let mut all = ids.to_vec();
    for id in ids {
        for bundle in store.bundles_with(id)? {
            if !all.contains(&bundle) {
                all.push(bundle);
            }
        }
    }
    all.into_iter()
        .map(|id| {
            let (available, reason) = match store.product(&id)? {
                Some(p) => {
                    let s = status(store, &p, today.as_deref())?;
                    (s.available, s.reason)
                }
                None => availability(false, true, false, 0),
            };
            Ok(CatalogEntry {
                product_id: id,
                available,
                reason: reason.map(str::to_owned),
            })
        })
        .collect()
}

/// Inventory → Expiring and Expired: the lots with stock left that expire within the branch's
/// warning window, and those already past their expiry date, as of the business day at `now_ms`.
pub(crate) fn expiry_view(store: &Store, now_ms: i64) -> Result<ExpiryView, AppError> {
    let warn_days = store
        .settings()?
        .into_iter()
        .find(|s| s.key == KEY_STOCK_EXPIRY_WARN_DAYS)
        .and_then(|s| s.value.parse::<i64>().ok())
        .filter(|d| (STOCK_EXPIRY_WARN_RANGE.0..=STOCK_EXPIRY_WARN_RANGE.1).contains(d))
        .unwrap_or_else(|| DEFAULT_STOCK_EXPIRY_WARN_DAYS.parse().unwrap_or(3));
    let today = crate::locale::current_business_day(store, now_ms)?.unwrap_or_default();
    let mut view = ExpiryView {
        warn_days,
        expiring: Vec::new(),
        expired: Vec::new(),
        today: today.clone(),
    };
    let Some(last) = add_days(&today, warn_days) else {
        return Ok(view);
    };
    for (lot, product) in store.expiring_lots(&last)? {
        let Some(expiry) = lot.expiry.clone() else {
            continue;
        };
        let row = ExpiringLot {
            lot_id: lot.lot_id,
            product_id: lot.product_id,
            product,
            qty: lot.qty_remaining,
            value_minor: lot.qty_remaining.saturating_mul(lot.unit_cost_minor),
            expiry,
        };
        if lot_expired(&row.expiry, &today) {
            view.expired.push(row);
        } else {
            view.expiring.push(row);
        }
    }
    Ok(view)
}
