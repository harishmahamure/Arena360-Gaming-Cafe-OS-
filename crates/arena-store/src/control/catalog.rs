//! The global game catalogue (Story 6.1): games, their executable signatures and media, the
//! `catalog_version` counter, the idempotent import and the rows the Hub manifest is built from.

use std::collections::BTreeMap;

use super::admin_audit::{NewAdminAudit, insert_stmt};
use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_text, text};

/// One executable signature of a game. `sha256` is empty when the signature has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSignatureRow {
    pub exe_name: String,
    pub product_name: String,
    pub publisher: String,
    pub sha256: String,
}

/// A game's cover or video: an immutable object `catalog/<sha256>.<ext>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogMediaRow {
    /// `cover` or `video`.
    pub kind: String,
    pub sha256: String,
    pub ext: String,
    pub bytes: i64,
    pub flash_screened: bool,
}

/// A game with its signatures and media.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    pub id: String,
    pub name: String,
    pub launcher: String,
    pub category: String,
    pub age_rating: String,
    pub published: bool,
    pub deleted: bool,
    pub rev: i64,
    pub signatures: Vec<CatalogSignatureRow>,
    pub media: Vec<CatalogMediaRow>,
}

impl CatalogRow {
    /// Does a Hub see this game as a live entry (not a tombstone)?
    pub fn is_live(&self) -> bool {
        self.published && !self.deleted
    }
}

/// One game to write in an import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameWrite {
    pub id: String,
    pub is_new: bool,
    pub name: String,
    pub launcher: String,
    pub category: String,
    pub age_rating: String,
    pub published: bool,
    /// The change is one a Hub must hear about (the game was or becomes published and live).
    pub bump: bool,
    /// `Some`: replaces the game's signatures.
    pub signatures: Option<Vec<CatalogSignatureRow>>,
    /// Upserted by kind; a kind not listed keeps its stored media.
    pub media: Vec<CatalogMediaRow>,
}

/// An import: the changed games and the audit row recorded with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogImport<'a> {
    pub games: Vec<GameWrite>,
    pub now: i64,
    pub audit: NewAdminAudit<'a>,
}

/// What an import left behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogImportOutcome {
    /// The `catalog_version` after the import.
    pub version: i64,
}

fn flag(b: bool) -> SqlValue {
    SqlValue::Integer(i64::from(b))
}

impl ControlDb {
    /// The current `catalog_version`.
    pub async fn catalog_version(&self) -> Result<i64, ControlError> {
        let rows = self
            .query("SELECT version FROM catalog_state WHERE id = 1", vec![])
            .await?;
        rows.first().map_or(Ok(0), |r| col_int(r, 0))
    }

    /// Every game, including unpublished and deleted ones (the import's view of what exists).
    pub async fn catalog_all(&self) -> Result<Vec<CatalogRow>, ControlError> {
        self.catalog_between(-1, i64::MAX).await
    }

    /// The games whose `changed_version` is beyond `after` and at most `upto`, by name.
    pub async fn catalog_between(
        &self,
        after: i64,
        upto: i64,
    ) -> Result<Vec<CatalogRow>, ControlError> {
        let range = || vec![SqlValue::Integer(after), SqlValue::Integer(upto)];
        let games = self
            .query(
                "SELECT id, name, launcher, category, age_rating, published, deleted_at, rev \
                 FROM catalog_games WHERE changed_version > ? AND changed_version <= ? \
                 ORDER BY lower(name), lower(launcher)",
                range(),
            )
            .await?;
        let sigs = self
            .query(
                "SELECT s.game_id, s.exe_name, s.product_name, s.publisher, s.sha256 \
                 FROM catalog_exe_signatures s JOIN catalog_games g ON g.id = s.game_id \
                 WHERE g.changed_version > ? AND g.changed_version <= ? \
                 ORDER BY s.game_id, lower(s.exe_name), s.sha256",
                range(),
            )
            .await?;
        let media = self
            .query(
                "SELECT m.game_id, m.kind, m.sha256, m.ext, m.bytes, m.flash_screened \
                 FROM catalog_media m JOIN catalog_games g ON g.id = m.game_id \
                 WHERE g.changed_version > ? AND g.changed_version <= ? \
                 ORDER BY m.game_id, m.kind",
                range(),
            )
            .await?;
        let mut sig_by: BTreeMap<String, Vec<CatalogSignatureRow>> = BTreeMap::new();
        for r in &sigs {
            sig_by
                .entry(col_text(r, 0)?)
                .or_default()
                .push(CatalogSignatureRow {
                    exe_name: col_text(r, 1)?,
                    product_name: col_text(r, 2)?,
                    publisher: col_text(r, 3)?,
                    sha256: col_text(r, 4)?,
                });
        }
        let mut media_by: BTreeMap<String, Vec<CatalogMediaRow>> = BTreeMap::new();
        for r in &media {
            media_by
                .entry(col_text(r, 0)?)
                .or_default()
                .push(CatalogMediaRow {
                    kind: col_text(r, 1)?,
                    sha256: col_text(r, 2)?,
                    ext: col_text(r, 3)?,
                    bytes: col_int(r, 4)?,
                    flash_screened: col_int(r, 5)? != 0,
                });
        }
        games
            .iter()
            .map(|r| {
                let id = col_text(r, 0)?;
                Ok(CatalogRow {
                    name: col_text(r, 1)?,
                    launcher: col_text(r, 2)?,
                    category: col_text(r, 3)?,
                    age_rating: col_text(r, 4)?,
                    published: col_int(r, 5)? != 0,
                    deleted: !matches!(r.get(6), Some(SqlValue::Null)),
                    rev: col_int(r, 7)?,
                    signatures: sig_by.remove(&id).unwrap_or_default(),
                    media: media_by.remove(&id).unwrap_or_default(),
                    id,
                })
            })
            .collect()
    }

    /// Writes `import.games`, moves the version once when any game `bump`s, and appends the
    /// audit row, all in one transaction. Answers the version afterwards.
    pub async fn apply_catalog_import(
        &self,
        import: &CatalogImport<'_>,
    ) -> Result<CatalogImportOutcome, ControlError> {
        let now = SqlValue::Integer(import.now);
        let mut stmts = Vec::new();
        if import.games.iter().any(|g| g.bump) {
            stmts.push(
                Stmt::new(
                    "UPDATE catalog_state SET version = version + 1 WHERE id = 1",
                    vec![],
                )
                .expect(1),
            );
        }
        for g in &import.games {
            let fields = vec![
                text(&g.name),
                text(&g.launcher),
                text(&g.category),
                text(&g.age_rating),
                flag(g.published),
            ];
            if g.is_new {
                let mut p = vec![text(&g.id)];
                p.extend(fields);
                p.push(flag(g.bump));
                p.push(now.clone());
                stmts.push(
                    Stmt::new(
                        "INSERT INTO catalog_games (id, name, launcher, category, age_rating, \
                         published, rev, changed_version, updated_at, deleted_at) \
                         VALUES (?, ?, ?, ?, ?, ?, 1, \
                         CASE WHEN ? THEN (SELECT version FROM catalog_state WHERE id = 1) \
                         ELSE 0 END, ?, NULL)",
                        p,
                    )
                    .expect(1),
                );
            } else {
                let mut p = fields;
                p.push(flag(g.bump));
                p.push(now.clone());
                p.push(text(&g.id));
                stmts.push(
                    Stmt::new(
                        "UPDATE catalog_games SET name = ?, launcher = ?, category = ?, \
                         age_rating = ?, published = ?, rev = rev + 1, \
                         changed_version = CASE WHEN ? \
                         THEN (SELECT version FROM catalog_state WHERE id = 1) \
                         ELSE changed_version END, updated_at = ?, deleted_at = NULL \
                         WHERE id = ?",
                        p,
                    )
                    .expect(1),
                );
            }
            if let Some(sigs) = &g.signatures {
                stmts.push(Stmt::new(
                    "DELETE FROM catalog_exe_signatures WHERE game_id = ?",
                    vec![text(&g.id)],
                ));
                for s in sigs {
                    stmts.push(
                        Stmt::new(
                            "INSERT INTO catalog_exe_signatures \
                             (game_id, exe_name, product_name, publisher, sha256) \
                             VALUES (?, ?, ?, ?, ?)",
                            vec![
                                text(&g.id),
                                text(&s.exe_name),
                                text(&s.product_name),
                                text(&s.publisher),
                                text(&s.sha256),
                            ],
                        )
                        .expect(1),
                    );
                }
            }
            for m in &g.media {
                stmts.push(
                    Stmt::new(
                        "INSERT INTO catalog_media (game_id, kind, sha256, ext, bytes, \
                         flash_screened) VALUES (?, ?, ?, ?, ?, ?) \
                         ON CONFLICT (game_id, kind) DO UPDATE SET sha256 = excluded.sha256, \
                         ext = excluded.ext, bytes = excluded.bytes, \
                         flash_screened = excluded.flash_screened",
                        vec![
                            text(&g.id),
                            text(&m.kind),
                            text(&m.sha256),
                            text(&m.ext),
                            SqlValue::Integer(m.bytes),
                            flag(m.flash_screened),
                        ],
                    )
                    .expect(1),
                );
            }
        }
        stmts.push(insert_stmt(&import.audit));
        if !self.transact(stmts).await? {
            return Err(ControlError::Db("catalogue import was refused".to_owned()));
        }
        Ok(CatalogImportOutcome {
            version: self.catalog_version().await?,
        })
    }
}
