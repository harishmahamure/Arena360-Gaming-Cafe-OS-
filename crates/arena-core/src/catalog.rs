use arena_proto::{AppError, CatalogEntry, Product, availability};
use arena_store::Store;

const UNRECORDED_STOCK: i64 = 0;

pub(crate) fn products(store: &Store) -> Result<Vec<Product>, AppError> {
    Ok(store
        .products()?
        .into_iter()
        .map(|p| {
            let (available, reason) =
                availability(p.active, false, p.track_stock, UNRECORDED_STOCK);
            Product {
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
                rev: p.rev,
                quantity: UNRECORDED_STOCK,
                available,
                reason: reason.map(str::to_owned),
            }
        })
        .collect())
}

pub(crate) fn entries(store: &Store, ids: &[String]) -> Result<Vec<CatalogEntry>, AppError> {
    ids.iter()
        .map(|id| {
            let (available, reason) = match store.product(id)? {
                Some(p) => availability(
                    p.active,
                    p.deleted_at.is_some(),
                    p.track_stock,
                    UNRECORDED_STOCK,
                ),
                None => availability(false, true, false, 0),
            };
            Ok(CatalogEntry {
                product_id: id.clone(),
                available,
                reason: reason.map(str::to_owned),
            })
        })
        .collect()
}
