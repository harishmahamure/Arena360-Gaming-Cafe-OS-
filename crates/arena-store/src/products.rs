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
    pub rev: i64,
    pub deleted_at: Option<i64>,
}

const COLUMNS: &str = "product_id, name, category, price_minor, tax_rate_bp, hsn, track_stock, \
                       image, barcode, kitchen, favourite, active, rev, deleted_at";

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
        rev: r.get(12)?,
        deleted_at: r.get(13)?,
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
                               track_stock, image, barcode, kitchen, favourite, active, rev,
                               deleted_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, NULL, 0, 0)",
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
            p.active
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
                    favourite = ?11, active = ?12
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
                p.active
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
            rev: 0,
            deleted_at: None,
        }
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
