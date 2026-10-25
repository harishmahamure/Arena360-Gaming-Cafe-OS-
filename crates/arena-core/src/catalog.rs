use arena_proto::{
    AppError, CatalogEntry, ComponentView, Product, REASON_COMPONENT_INACTIVE, availability,
    bundle_quantity,
};
use arena_store::{ProductRow, Store};

struct Status {
    quantity: i64,
    avg_cost_minor: i64,
    available: bool,
    reason: Option<&'static str>,
    components: Vec<ComponentView>,
}

fn status(store: &Store, p: &ProductRow) -> Result<Status, AppError> {
    let deleted = p.deleted_at.is_some();
    if !p.bundle {
        let stock = store.stock(&p.product_id)?;
        let quantity = stock.as_ref().map_or(0, |s| s.qty);
        let (available, reason) = availability(p.active, deleted, p.track_stock, quantity);
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
            parts.push((store.stock(&id)?.map_or(0, |s| s.qty), qty));
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

pub(crate) fn products(store: &Store) -> Result<Vec<Product>, AppError> {
    store
        .products()?
        .into_iter()
        .map(|p| {
            let s = status(store, &p)?;
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

pub(crate) fn entries(store: &Store, ids: &[String]) -> Result<Vec<CatalogEntry>, AppError> {
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
                    let s = status(store, &p)?;
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
