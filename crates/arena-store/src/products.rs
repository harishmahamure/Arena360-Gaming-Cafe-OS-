use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductRow {
    pub product_id: String,
    pub name: String,
    pub category: String,
    pub price_minor: i64,
    pub tax_rate_bp: i64,
    pub hsn: String,
    pub track_stock: bool,
    pub image: String,
    pub barcode: Option<String>,
    pub kitchen: bool,
    pub favourite: bool,
    pub active: bool,
    pub bundle: bool,
    pub rev: i64,
    pub deleted_at: Option<i64>,
}

const COLUMNS: &str = "product_id, name, category, price_minor, tax_rate_bp, hsn, track_stock, \
                       image, barcode, kitchen, favourite, active, bundle, rev, deleted_at";

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProductRow> {
    Ok(ProductRow {
        product_id: r.get(0)?,
        name: r.get(1)?,
        category: r.get(2)?,
        price_minor: r.get(3)?,
        tax_rate_bp: r.get(4)?,
        hsn: r.get(5)?,
        track_stock: r.get(6)?,
        image: r.get(7)?,
        barcode: r.get(8)?,
        kitchen: r.get(9)?,
        favourite: r.get(10)?,
        active: r.get(11)?,
        bundle: r.get(12)?,
        rev: r.get(13)?,
        deleted_at: r.get(14)?,
    })
}

pub fn find_product(conn: &Connection, id: &str) -> Result<Option<ProductRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM products WHERE product_id = ?1"),
        [id],
        from,
    )
    .optional()
    .map_err(db_error)
}

pub fn barcode_holder(conn: &Connection, barcode: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT product_id FROM products WHERE barcode = ?1 AND active = 1 AND deleted_at IS NULL",
        [barcode],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

pub fn list_products(conn: &Connection) -> Result<Vec<ProductRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM products WHERE deleted_at IS NULL
             ORDER BY category COLLATE NOCASE, name COLLATE NOCASE, product_id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// The listed (active, not deleted) products and bundles by units sold on orders that are not void
/// and were placed at or after `since` (Story 6.30): at most `limit`, most units first and ties by
/// product id. A line counts from when it was placed (a session order's lines are placed one
/// by one), else from the order's creation.
pub fn units_sold(
    conn: &Connection,
    since: i64,
    limit: usize,
) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT l.product_id, SUM(COALESCE(l.qty, 1)) AS units
             FROM order_lines l
             JOIN orders o ON o.id = l.order_id
             JOIN products p ON p.product_id = l.product_id
             WHERE o.status <> 'void' AND p.active = 1 AND p.deleted_at IS NULL
               AND COALESCE(l.placed_at, o.created_at) >= ?1
             GROUP BY l.product_id HAVING units > 0
             ORDER BY units DESC, l.product_id ASC LIMIT ?2",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map(params![since, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

/// The products and bundles the kiosk Shop lists (Story 6.30): active and not deleted, ordered
/// like [`list_products`], each with whether it has a photo. The photo itself (up to 200 KB per
/// product) is left in the table, so the menu is cheap to read once a second per kiosk; the
/// returned rows have an empty `image`.
pub fn list_menu_products(conn: &Connection) -> Result<Vec<(ProductRow, bool)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT product_id, name, category, price_minor, tax_rate_bp, hsn, track_stock,
                    '' AS image, barcode, kitchen, favourite, active, bundle, rev, deleted_at,
                    image <> '' AS has_image
             FROM products WHERE active = 1 AND deleted_at IS NULL
             ORDER BY category COLLATE NOCASE, name COLLATE NOCASE, product_id",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| Ok((from(r)?, r.get(15)?)))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn count_active_products(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM products WHERE active = 1 AND deleted_at IS NULL",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn insert_product(conn: &Connection, p: &ProductRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO products (product_id, name, category, price_minor, tax_rate_bp, hsn,
                               track_stock, image, barcode, kitchen, favourite, active, bundle,
                               rev, deleted_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, NULL, 0, 0)",
        params![
            p.product_id,
            p.name,
            p.category,
            p.price_minor,
            p.tax_rate_bp,
            p.hsn,
            p.track_stock,
            p.image,
            p.barcode,
            p.kitchen,
            p.favourite,
            p.active,
            p.bundle
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

fn expect_one(n: usize, id: &str) -> Result<(), AppError> {
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no products row {id}"),
        ))
    }
}

pub fn update_product(conn: &Connection, p: &ProductRow) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE products SET name = ?2, category = ?3, price_minor = ?4, tax_rate_bp = ?5,
                    hsn = ?6, track_stock = ?7, image = ?8, barcode = ?9, kitchen = ?10,
                    favourite = ?11, active = ?12, bundle = ?13
             WHERE product_id = ?1",
            params![
                p.product_id,
                p.name,
                p.category,
                p.price_minor,
                p.tax_rate_bp,
                p.hsn,
                p.track_stock,
                p.image,
                p.barcode,
                p.kitchen,
                p.favourite,
                p.active,
                p.bundle
            ],
        )
        .map_err(db_error)?;
    expect_one(n, &p.product_id)
}

pub fn set_product_deleted(conn: &Connection, id: &str, at_ms: i64) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE products SET deleted_at = ?2 WHERE product_id = ?1",
            params![id, at_ms],
        )
        .map_err(db_error)?;
    expect_one(n, id)
}

pub fn list_components(conn: &Connection, bundle_id: &str) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT component_id, qty FROM bundle_components WHERE bundle_id = ?1 ORDER BY component_id")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([bundle_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn live_bundles_with(conn: &Connection, component_id: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT c.bundle_id FROM bundle_components c
             JOIN products b ON b.product_id = c.bundle_id
             WHERE c.component_id = ?1 AND b.deleted_at IS NULL ORDER BY c.bundle_id",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([component_id], |r| r.get(0))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn insert_component(
    conn: &Connection,
    bundle_id: &str,
    component_id: &str,
    qty: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO bundle_components (bundle_id, component_id, qty) VALUES (?1, ?2, ?3)",
        params![bundle_id, component_id, qty],
    )
    .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn row(id: &str, barcode: Option<&str>, active: bool) -> ProductRow {
        ProductRow {
            product_id: id.into(),
            name: id.into(),
            category: String::new(),
            price_minor: 1,
            tax_rate_bp: 0,
            hsn: String::new(),
            track_stock: true,
            image: String::new(),
            barcode: barcode.map(Into::into),
            kitchen: false,
            favourite: false,
            active,
            bundle: false,
            rev: 0,
            deleted_at: None,
        }
    }

    fn sell(
        conn: &Connection,
        order: &str,
        status: &str,
        created: i64,
        lines: &[(&str, i64, Option<i64>)],
    ) {
        conn.execute(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, kind)
             VALUES (?1, ?2, 0, 0, '2026-10-29', '2026-27', 0, ?3, ?3, 'pos_sale')",
            params![order, status, created],
        )
        .unwrap();
        for (i, (product, qty, placed)) in lines.iter().enumerate() {
            conn.execute(
                "INSERT INTO order_lines (id, order_id, from_at, to_at, minutes,
                                          rate_minor_per_hour, amount_minor, position, rev,
                                          created_at, updated_at, product_id, qty, placed_at)
                 VALUES (?1, ?2, 0, 0, 0, 0, 0, ?3, 0, 0, 0, ?4, ?5, ?6)",
                params![
                    format!("{order}-{i}"),
                    order,
                    i as i64,
                    product,
                    qty,
                    placed
                ],
            )
            .unwrap();
        }
    }

    #[test]
    fn the_menu_lists_active_live_products_without_their_photos() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db"), 0).unwrap();
        store
            .write_tx(|tx| {
                let mut with_photo = row("A", None, true);
                with_photo.image = "data:image/jpeg;base64,AAAA".into();
                insert_product(tx, &with_photo)?;
                insert_product(tx, &row("B", None, true))?;
                insert_product(tx, &row("C", None, false))?;
                insert_product(tx, &row("D", None, true))?;
                set_product_deleted(tx, "D", 1)?;
                let menu = list_menu_products(tx)?;
                let seen: Vec<_> = menu
                    .iter()
                    .map(|(p, photo)| (p.product_id.as_str(), p.image.as_str(), *photo))
                    .collect();
                assert_eq!(seen, [("A", "", true), ("B", "", false)]);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn units_sold_counts_listed_products_on_live_orders_in_the_window() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db"), 0).unwrap();
        store
            .write_tx(|tx| {
                for (id, active) in [("A", true), ("B", true), ("C", true), ("D", false)] {
                    insert_product(tx, &row(id, None, active))?;
                }
                insert_product(tx, &row("E", None, true))?;
                set_product_deleted(tx, "E", 1)?;
                // Settled and open orders count, a void one does not.
                sell(
                    tx,
                    "O1",
                    "settled",
                    1_000,
                    &[("A", 3, None), ("B", 2, None)],
                );
                sell(
                    tx,
                    "O2",
                    "open",
                    1_000,
                    &[("B", 2, Some(1_500)), ("C", 4, None)],
                );
                sell(tx, "O3", "void", 1_000, &[("C", 50, None)]);
                // Before the window: the line's placement wins over the order's creation.
                sell(tx, "O4", "settled", 5_000, &[("A", 10, Some(400))]);
                sell(tx, "O5", "settled", 100, &[("A", 10, None)]);
                // Inactive and deleted products are never listed.
                sell(
                    tx,
                    "O6",
                    "settled",
                    1_000,
                    &[("D", 9, None), ("E", 9, None)],
                );
                let top = units_sold(tx, 1_000, 3)?;
                assert_eq!(
                    top,
                    [
                        ("B".to_owned(), 4),
                        ("C".to_owned(), 4),
                        ("A".to_owned(), 3)
                    ]
                );
                assert_eq!(units_sold(tx, 1_000, 1)?.len(), 1);
                assert!(units_sold(tx, 9_999, 3)?.is_empty());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn the_index_holds_one_active_live_product_per_barcode() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db"), 0).unwrap();
        store
            .write_tx(|tx| {
                insert_product(tx, &row("A", Some("111"), true))?;
                assert!(insert_product(tx, &row("B", Some("111"), true)).is_err());
                insert_product(tx, &row("C", Some("111"), false))?;
                insert_product(tx, &row("D", None, true))?;
                insert_product(tx, &row("E", None, true))?;
                assert_eq!(barcode_holder(tx, "111")?.as_deref(), Some("A"));
                assert_eq!(count_active_products(tx)?, 3);
                set_product_deleted(tx, "A", 5)?;
                assert_eq!(barcode_holder(tx, "111")?, None);
                assert_eq!(count_active_products(tx)?, 2);
                let mut c = row("C", Some("111"), true);
                update_product(tx, &c)?;
                c.product_id = "F".into();
                assert!(insert_product(tx, &c).is_err());
                assert_eq!(list_products(tx)?.len(), 3);
                Ok(())
            })
            .unwrap();
    }
}
