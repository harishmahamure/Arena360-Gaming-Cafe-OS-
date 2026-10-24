//! The audit viewer's read path (Story 1.15): pages of the `changes` log with names resolved,
//! one change with its before/after row images (secrets redacted), and the filter options.
//! Read on its own read-only connection, never through the writer, each answer in one read
//! snapshot.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use arena_proto::{
    AUDIT_DEFAULT_LIMIT, AUDIT_MAX_LIMIT, ActorKind, AdminExitMode, AppError, AuditActor,
    AuditActorKind, AuditAdminExit, AuditDetail, AuditElevatedBy, AuditElevationKind,
    AuditElevationMode, AuditFacets, AuditImage, AuditItem, AuditPage, AuditStaffOption,
    AuditValue, ChangeOpKind, ElevationKind, ElevationMode, ErrorCode, IntentKind,
};
use arena_store::{AuditCursor, AuditFilter, AuditRow, ChangeOp, Row, RowValue, Store};

/// Columns whose values the viewer never shows: secrets (argon2id hashes) and, from later
/// epics, player personal data. Both images read [`REDACTED_MARKER`] instead; a change to one
/// is still listed in `changed`, so the viewer shows only that it changed.
pub const REDACTED: &[&str] = &["password_hash", "pin_hash", "token_hash"];

/// Columns that reference a player: the viewer shows them as ids only, never resolved to a
/// name or other personal data (no player tables exist yet; the rule is set here for them).
pub const ID_ONLY: &[&str] = &["player_id"];

/// What a redacted value reads as.
pub const REDACTED_MARKER: &str = "[redacted]";

/// The catalogue keys the UI shows for actors that are not a named staff member or device.
pub const ACTOR_SYSTEM_KEY: &str = "hub.audit.actor.system";
pub const ACTOR_OWNER_KEY: &str = "hub.audit.actor.owner";
pub const ACTOR_ADMIN_KEY: &str = "hub.audit.actor.admin";

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn internal(message: String) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Decodes unpadded base64url (`None` on any other character or a dangling sextet).
fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for b in text.bytes() {
        let v = B64URL.iter().position(|c| *c == b)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // Leftover bits must be zero padding (2 or 4 bits); 6 means a lone character.
    (bits < 6 && acc == 0).then_some(out)
}

/// The opaque cursor of a position: `at.seq.epoch`, base64url.
pub fn encode_cursor(c: &AuditCursor) -> String {
    crate::staff::base64url(format!("{}.{}.{}", c.at, c.seq, c.epoch).as_bytes())
}

/// The position behind a cursor from [`encode_cursor`] (`VALIDATION_FAILED` on `cursor`).
pub fn decode_cursor(text: &str) -> Result<AuditCursor, AppError> {
    let bad = || invalid("cursor", "the cursor is not one this hub issued");
    let bytes = base64url_decode(text).ok_or_else(bad)?;
    let s = String::from_utf8(bytes).map_err(|_| bad())?;
    let mut parts = s.splitn(3, '.');
    let at = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
    let seq = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
    let epoch = parts.next().filter(|e| !e.is_empty()).ok_or_else(bad)?;
    Ok(AuditCursor {
        at,
        seq,
        epoch: epoch.to_owned(),
    })
}

/// The intent domain of a kind (`settings.set` is `settings`).
pub fn module_of(intent_kind: &str) -> &str {
    intent_kind.split_once('.').map_or(intent_kind, |(d, _)| d)
}

/// Every known module (intent domain), in [`IntentKind::ALL`] order, each once.
pub fn known_modules() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for k in IntentKind::ALL {
        let m = module_of(k.as_str());
        if !out.iter().any(|o| o == m) {
            out.push(m.to_owned());
        }
    }
    out
}

/// A parsed `GET /ui/audit` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditQuery {
    pub filter: AuditFilter,
    pub limit: u32,
}

impl AuditQuery {
    /// Parses the query parameters `actor=<kind>[:<id>]`, `module=<domain>`, `from=<ms>`,
    /// `to=<ms>` (from inclusive, to exclusive), `cursor` and `limit` (1–100, default 50). An
    /// empty value counts as absent; anything else malformed is `VALIDATION_FAILED` naming
    /// the parameter. Unknown parameters are ignored.
    pub fn parse(params: &HashMap<String, String>) -> Result<Self, AppError> {
        let get = |k: &str| params.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let mut filter = AuditFilter::default();
        if let Some(actor) = get("actor") {
            let (kind, id) = match actor.split_once(':') {
                Some((k, id)) => (k, Some(id)),
                None => (actor, None),
            };
            let kind = ActorKind::parse(kind)
                .ok_or_else(|| invalid("actor", "actor is <kind> or <kind>:<id>"))?;
            if id.is_some_and(str::is_empty) {
                return Err(invalid("actor", "actor is <kind> or <kind>:<id>"));
            }
            filter.actor_kind = Some(kind.as_str().to_owned());
            filter.actor_id = id.map(str::to_owned);
        }
        if let Some(module) = get("module") {
            let plain = module.len() <= 32
                && module
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit());
            if !plain {
                return Err(invalid(
                    "module",
                    "module is an intent domain, e.g. settings",
                ));
            }
            filter.module = Some(module.to_owned());
        }
        let ms = |k: &str| -> Result<Option<i64>, AppError> {
            get(k)
                .map(|v| {
                    v.parse::<i64>()
                        .map_err(|_| invalid(k, "a time is UTC milliseconds"))
                })
                .transpose()
        };
        filter.from_ms = ms("from")?;
        filter.to_ms = ms("to")?;
        if let (Some(f), Some(t)) = (filter.from_ms, filter.to_ms)
            && f > t
        {
            return Err(invalid("to", "to must not be before from"));
        }
        if let Some(c) = get("cursor") {
            filter.after = Some(decode_cursor(c)?);
        }
        let limit = match get("limit") {
            None => AUDIT_DEFAULT_LIMIT,
            Some(v) => v
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=AUDIT_MAX_LIMIT).contains(n))
                .ok_or_else(|| invalid("limit", "limit is 1–100"))?,
        };
        Ok(Self { filter, limit })
    }
}

/// Names for actors and approvers, read in the same snapshot as the changes.
#[derive(Debug, Default, Clone)]
pub struct Names {
    staff: HashMap<String, String>,
    devices: HashMap<String, String>,
}

impl Names {
    pub fn new(staff: Vec<(String, String)>, devices: Vec<(String, String)>) -> Self {
        Self {
            staff: staff.into_iter().collect(),
            devices: devices.into_iter().collect(),
        }
    }

    fn read(store: &Store) -> Result<Self, AppError> {
        Ok(Self::new(store.staff_names()?, store.device_labels()?))
    }

    /// The name shown for an actor: a staff member's display name, a device's label, a
    /// catalogue key for `system`, `owner` and `admin`; the id itself when it is unknown.
    pub fn actor_name(&self, kind: ActorKind, id: &str) -> String {
        match kind {
            ActorKind::Staff => self.staff_name(id),
            ActorKind::Device => self.devices.get(id).cloned().unwrap_or_else(|| id.into()),
            ActorKind::System => ACTOR_SYSTEM_KEY.into(),
            ActorKind::Owner => ACTOR_OWNER_KEY.into(),
            ActorKind::Admin => ACTOR_ADMIN_KEY.into(),
        }
    }

    fn staff_name(&self, id: &str) -> String {
        self.staff.get(id).cloned().unwrap_or_else(|| id.into())
    }

    /// One change as the list shows it. A stored actor kind, elevation kind or mode this
    /// build does not know shows as `unknown` (with a warning) rather than failing the page;
    /// an unknown actor kind shows its id as its name.
    pub fn item(&self, row: &AuditRow) -> Result<AuditItem, AppError> {
        let warn = |what: &str, value: &str| {
            tracing::warn!(epoch = %row.epoch, seq = row.seq, what, value, "audit: unknown value");
        };
        let (kind, name) = match ActorKind::parse(&row.actor_kind) {
            Some(k) => (AuditActorKind::from(k), self.actor_name(k, &row.actor_id)),
            None => {
                warn("actor_kind", &row.actor_kind);
                (AuditActorKind::Unknown, row.actor_id.clone())
            }
        };
        let elevated_by = match (
            &row.elevated_by_kind,
            &row.elevated_by_id,
            &row.elevated_by_mode,
        ) {
            (Some(k), Some(id), Some(m)) => Some(AuditElevatedBy {
                kind: ElevationKind::parse(k).map_or_else(
                    || {
                        warn("elevated_by_kind", k);
                        AuditElevationKind::Unknown
                    },
                    AuditElevationKind::from,
                ),
                id: id.clone(),
                name: self.staff_name(id),
                mode: ElevationMode::parse(m).map_or_else(
                    || {
                        warn("elevated_by_mode", m);
                        AuditElevationMode::Unknown
                    },
                    AuditElevationMode::from,
                ),
            }),
            _ => None,
        };
        Ok(AuditItem {
            epoch: row.epoch.clone(),
            seq: row.seq,
            at: row.at,
            business_day: row.business_day.clone(),
            actor: AuditActor {
                kind,
                id: row.actor_id.clone(),
                name,
            },
            elevated_by,
            module: module_of(&row.intent_kind).to_owned(),
            intent_kind: row.intent_kind.clone(),
            intent_id: row.intent_id.clone(),
            tbl: row.tbl.clone(),
            row_id: row.row_id.clone(),
            op: match row.op {
                ChangeOp::Insert => ChangeOpKind::Insert,
                ChangeOp::Update => ChangeOpKind::Update,
                ChangeOp::Delete => ChangeOpKind::Delete,
            },
            flagged: row.flagged,
        })
    }

    /// The summary of a `device_admin_exits` row image: the kiosk and the approving manager by
    /// name, how it was granted, its times and its review flag. `None` for other tables or an
    /// image that lacks the columns.
    pub fn admin_exit(&self, tbl: &str, image: &Row) -> Option<AuditAdminExit> {
        if tbl != arena_store::Table::DeviceAdminExits.name() {
            return None;
        }
        let text = |c: &str| match image.get(c) {
            Some(RowValue::Text(t)) => Some(t.clone()),
            _ => None,
        };
        let int = |c: &str| match image.get(c) {
            Some(RowValue::Integer(n)) => Some(*n),
            _ => None,
        };
        let device_id = text("device_id")?;
        let staff_id = text("staff_id")?;
        let mode = text("mode")?;
        Some(AuditAdminExit {
            device_label: self
                .devices
                .get(&device_id)
                .cloned()
                .unwrap_or_else(|| device_id.clone()),
            device_id,
            staff_name: self.staff_name(&staff_id),
            staff_id,
            mode: AdminExitMode::parse(&mode)
                .map_or("unknown", AdminExitMode::as_str)
                .to_owned(),
            started_at: int("started_at")?,
            relock_at: int("relock_at")?,
            relocked_at: int("relocked_at"),
            flagged: int("flagged").is_some_and(|f| f != 0),
            flag_reason: text("flag_reason"),
        })
    }
}

/// Decodes a change's MessagePack row image.
pub fn decode_image(bytes: &[u8]) -> Result<Row, AppError> {
    rmp_serde::from_slice(bytes).map_err(|e| internal(format!("decode row image: {e}")))
}

/// The columns whose values differ between `before` and `after` (a column missing on one side
/// counts as changed), in column-name order. Empty for a created row (`before` is `None`).
pub fn changed_fields(before: Option<&Row>, after: &Row) -> Vec<String> {
    let Some(before) = before else {
        return Vec::new();
    };
    let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| before.get(*k) != after.get(*k))
        .cloned()
        .collect()
}

/// Whether the viewer must never show `column`'s value.
pub fn is_redacted(column: &str) -> bool {
    REDACTED.contains(&column)
}

/// Whether `column` holds a player reference shown as an id only ([`ID_ONLY`]).
pub fn is_id_only(column: &str) -> bool {
    ID_ONLY.contains(&column)
}

/// An [`ID_ONLY`] value as the viewer shows it: the id alone. A text value keeps only its
/// leading id (letters, digits, `-`, `_`, `:`), dropping anything joined to it (a name, a
/// phone); an integer id stays; anything else, or text with no id in front, is redacted.
fn id_only(value: &RowValue) -> AuditValue {
    let is_id = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':');
    match value {
        RowValue::Null => AuditValue::Null,
        RowValue::Integer(n) => AuditValue::Integer(*n),
        RowValue::Text(t) => {
            let id: String = t.trim_start().chars().take_while(|c| is_id(*c)).collect();
            if id.is_empty() {
                AuditValue::Text(REDACTED_MARKER.into())
            } else {
                AuditValue::Text(id)
            }
        }
        RowValue::Real(_) => AuditValue::Text(REDACTED_MARKER.into()),
    }
}

/// A row image as JSON values, with every [`REDACTED`] column replaced by the marker (a null
/// stays null: "no PIN" is not a secret) and every [`ID_ONLY`] column reduced to its id.
pub fn redact(row: &Row) -> AuditImage {
    row.iter()
        .map(|(k, v)| {
            let value = match v {
                RowValue::Null => AuditValue::Null,
                _ if is_redacted(k) => AuditValue::Text(REDACTED_MARKER.into()),
                _ if is_id_only(k) => id_only(v),
                RowValue::Integer(n) => AuditValue::Integer(*n),
                RowValue::Real(f) => AuditValue::Real(*f),
                RowValue::Text(t) => AuditValue::Text(t.clone()),
            };
            (k.clone(), value)
        })
        .collect()
}

/// Reads one page: the rows, their names and whether more follow, in one snapshot.
pub fn read_page(store: &Store, query: &AuditQuery) -> Result<AuditPage, AppError> {
    let (rows, names) = store.read_consistent(|s| {
        Ok((
            s.audit_page(&query.filter, query.limit + 1)?,
            Names::read(s)?,
        ))
    })?;
    let more = rows.len() > query.limit as usize;
    let rows = &rows[..rows.len().min(query.limit as usize)];
    let next_cursor = if more {
        rows.last().map(|r| {
            encode_cursor(&AuditCursor {
                at: r.at,
                epoch: r.epoch.clone(),
                seq: r.seq,
            })
        })
    } else {
        None
    };
    let items = rows
        .iter()
        .map(|r| names.item(r))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AuditPage { items, next_cursor })
}

/// Reads the change `(epoch, seq)` with its images (`None` when there is no such change).
pub fn read_detail(store: &Store, epoch: &str, seq: i64) -> Result<Option<AuditDetail>, AppError> {
    let found = store.read_consistent(|s| {
        let Some(change) = s.change(epoch, seq)? else {
            return Ok(None);
        };
        let before = s.previous_row_image(&change.tbl, &change.row_id, epoch, seq)?;
        Ok(Some((change, before, Names::read(s)?)))
    })?;
    let Some((change, before, names)) = found else {
        return Ok(None);
    };
    let after = decode_image(&change.row)?;
    let before = before.as_deref().map(decode_image).transpose()?;
    let changed = changed_fields(before.as_ref(), &after);
    let admin_exit = names.admin_exit(&change.tbl, &after);
    let mut row = AuditRow::from(&change);
    row.flagged = admin_exit.as_ref().map(|e| e.flagged);
    Ok(Some(AuditDetail {
        admin_exit,
        item: names.item(&row)?,
        after: redact(&after),
        before: before.as_ref().map(redact),
        changed,
    }))
}

/// The filter options: every staff member by name, and the known modules.
pub fn read_facets(store: &Store) -> Result<AuditFacets, AppError> {
    let staff = store
        .read_consistent(|s| s.staff_names())?
        .into_iter()
        .map(|(id, name)| AuditStaffOption { id, name })
        .collect();
    Ok(AuditFacets {
        staff,
        modules: known_modules(),
    })
}

/// A read-only connection that serves the audit viewer, on a primary or a follower.
pub struct AuditReader {
    reader: Mutex<Store>,
}

impl std::fmt::Debug for AuditReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditReader").finish_non_exhaustive()
    }
}

impl AuditReader {
    /// Opens a read-only connection to the branch database at `db_path`.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, AppError> {
        Ok(Self {
            reader: Mutex::new(Store::open_reader(db_path)?),
        })
    }

    fn store(&self) -> Result<std::sync::MutexGuard<'_, Store>, AppError> {
        self.reader
            .lock()
            .map_err(|_| internal("audit reader lock poisoned".into()))
    }

    /// One page. Blocking (SQLite).
    pub fn page(&self, query: &AuditQuery) -> Result<AuditPage, AppError> {
        let store = self.store()?;
        read_page(&store, query)
    }

    /// One change with its images. Blocking (SQLite).
    pub fn detail(&self, epoch: &str, seq: i64) -> Result<Option<AuditDetail>, AppError> {
        let store = self.store()?;
        read_detail(&store, epoch, seq)
    }

    /// The filter options. Blocking (SQLite).
    pub fn facets(&self) -> Result<AuditFacets, AppError> {
        let store = self.store()?;
        read_facets(&store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, RowValue)]) -> Row {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn cursors_round_trip_and_reject_junk() {
        let c = AuditCursor {
            at: 1_800_000_000_123,
            epoch: "12:01JHUB.X".into(),
            seq: 42,
        };
        let text = encode_cursor(&c);
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert_eq!(decode_cursor(&text).unwrap(), c);
        for junk in ["", "!!", "a", "bm9wZQ", "MS4y"] {
            let e = decode_cursor(junk).unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{junk}");
            assert_eq!(e.field.as_deref(), Some("cursor"));
        }
    }

    #[test]
    fn base64url_decoding_inverts_encoding() {
        for input in [&b""[..], b"f", b"fo", b"foo", b"foob", b"\xfb\xff\xfe"] {
            let text = crate::staff::base64url(input);
            assert_eq!(base64url_decode(&text).unwrap(), input);
        }
        assert!(base64url_decode("Zh").is_none(), "non-zero padding bits");
    }

    #[test]
    fn the_query_parses_filters_and_bounds_the_limit() {
        let q = AuditQuery::parse(&params(&[
            ("actor", "staff:01JCOUNTER"),
            ("module", "settings"),
            ("from", "100"),
            ("to", "200"),
        ]))
        .unwrap();
        assert_eq!(q.limit, 50);
        assert_eq!(q.filter.actor_kind.as_deref(), Some("staff"));
        assert_eq!(q.filter.actor_id.as_deref(), Some("01JCOUNTER"));
        assert_eq!(q.filter.module.as_deref(), Some("settings"));
        assert_eq!((q.filter.from_ms, q.filter.to_ms), (Some(100), Some(200)));
        let kind_only =
            AuditQuery::parse(&params(&[("actor", "device"), ("limit", "100")])).unwrap();
        assert_eq!(kind_only.filter.actor_kind.as_deref(), Some("device"));
        assert_eq!(kind_only.filter.actor_id, None);
        assert_eq!(kind_only.limit, 100);
        assert_eq!(
            AuditQuery::parse(&params(&[("module", "")])).unwrap(),
            AuditQuery::parse(&HashMap::new()).unwrap()
        );
        for (k, v) in [
            ("actor", "robot:x"),
            ("actor", "staff:"),
            ("module", "settings%"),
            ("module", "Settings"),
            ("from", "yesterday"),
            ("limit", "0"),
            ("limit", "101"),
            ("cursor", "@@"),
        ] {
            let e = AuditQuery::parse(&params(&[(k, v)])).unwrap_err();
            assert_eq!(e.field.as_deref(), Some(k), "{k}={v}");
        }
        let e = AuditQuery::parse(&params(&[("from", "5"), ("to", "4")])).unwrap_err();
        assert_eq!(e.field.as_deref(), Some("to"));
    }

    #[test]
    fn modules_are_the_intent_domains() {
        assert_eq!(module_of("settings.set"), "settings");
        assert_eq!(module_of("odd"), "odd");
        let m = known_modules();
        for want in ["branch", "settings", "device", "staff", "role"] {
            assert!(m.iter().any(|x| x == want), "{want}");
        }
        let mut dedup = m.clone();
        dedup.dedup();
        assert_eq!(dedup.len(), m.len());
    }

    #[test]
    fn diff_lists_changed_columns_and_none_for_a_create() {
        let before = row(&[
            ("key", RowValue::Text("k".into())),
            ("value", RowValue::Text("INR".into())),
            ("rev", RowValue::Integer(1)),
            ("updated_at", RowValue::Integer(5)),
            ("prior_value", RowValue::Null),
        ]);
        let after = row(&[
            ("key", RowValue::Text("k".into())),
            ("value", RowValue::Text("USD".into())),
            ("rev", RowValue::Integer(2)),
            ("updated_at", RowValue::Integer(9)),
            ("prior_value", RowValue::Null),
        ]);
        assert_eq!(
            changed_fields(Some(&before), &after),
            vec!["rev", "updated_at", "value"]
        );
        assert!(changed_fields(None, &after).is_empty());
        let mut grown = after.clone();
        grown.insert("extra".into(), RowValue::Integer(1));
        assert_eq!(changed_fields(Some(&after), &grown), vec!["extra"]);
    }

    #[test]
    fn secrets_are_redacted_but_their_change_is_listed() {
        let before = row(&[
            ("login", RowValue::Text("counter".into())),
            ("pin_hash", RowValue::Text("$argon2id$old".into())),
            ("password_hash", RowValue::Text("$argon2id$pw".into())),
        ]);
        let after = row(&[
            ("login", RowValue::Text("counter".into())),
            ("pin_hash", RowValue::Text("$argon2id$new".into())),
            ("password_hash", RowValue::Null),
        ]);
        assert_eq!(
            changed_fields(Some(&before), &after),
            vec!["password_hash", "pin_hash"]
        );
        let shown = redact(&before);
        assert_eq!(shown["pin_hash"], AuditValue::Text(REDACTED_MARKER.into()));
        assert_eq!(
            shown["password_hash"],
            AuditValue::Text(REDACTED_MARKER.into())
        );
        assert_eq!(shown["login"], AuditValue::Text("counter".into()));
        assert_eq!(redact(&after)["password_hash"], AuditValue::Null);
        for secret in ["password_hash", "pin_hash", "token_hash"] {
            assert!(is_redacted(secret));
        }
        assert!(!ID_ONLY.iter().any(|c| is_redacted(c)));
    }

    #[test]
    fn player_references_show_as_ids_only() {
        let image = row(&[
            (
                "player_id",
                RowValue::Text("01JPLAYER Asha Rao +91 98765".into()),
            ),
            ("other", RowValue::Text("01JPLAYER Asha Rao".into())),
        ]);
        let shown = redact(&image);
        assert_eq!(shown["player_id"], AuditValue::Text("01JPLAYER".into()));
        assert_eq!(
            shown["other"],
            AuditValue::Text("01JPLAYER Asha Rao".into())
        );
        let odd = row(&[
            ("player_id", RowValue::Text(" (Asha)".into())),
            ("x", RowValue::Null),
        ]);
        assert_eq!(
            redact(&odd)["player_id"],
            AuditValue::Text(REDACTED_MARKER.into())
        );
        assert_eq!(
            redact(&row(&[("player_id", RowValue::Integer(7))]))["player_id"],
            AuditValue::Integer(7)
        );
        assert!(is_id_only("player_id"));
    }

    #[test]
    fn an_admin_exit_image_is_summarised_with_names() {
        let names = Names::new(
            vec![("S".into(), "Asha".into())],
            vec![("D".into(), "Kiosk 3".into())],
        );
        let image = row(&[
            ("exit_id", RowValue::Text("E".into())),
            ("device_id", RowValue::Text("D".into())),
            ("staff_id", RowValue::Text("S".into())),
            ("mode", RowValue::Text("cached_verifier".into())),
            ("started_at", RowValue::Integer(10)),
            ("relock_at", RowValue::Integer(20)),
            ("relocked_at", RowValue::Null),
            ("flagged", RowValue::Integer(1)),
            ("flag_reason", RowValue::Text("stale_verifier".into())),
        ]);
        let e = names.admin_exit("device_admin_exits", &image).unwrap();
        assert_eq!(
            (
                e.device_label.as_str(),
                e.staff_name.as_str(),
                e.mode.as_str()
            ),
            ("Kiosk 3", "Asha", "cached_verifier")
        );
        assert_eq!((e.started_at, e.relock_at, e.relocked_at), (10, 20, None));
        assert!(e.flagged);
        assert_eq!(e.flag_reason.as_deref(), Some("stale_verifier"));
        assert!(names.admin_exit("settings", &image).is_none());
    }

    #[test]
    fn unknown_stored_kinds_show_as_unknown() {
        let names = Names::default();
        let row = AuditRow {
            epoch: "1:H".into(),
            seq: 1,
            at: 0,
            business_day: None,
            actor_kind: "robot".into(),
            actor_id: "R2".into(),
            elevated_by_kind: Some("retina".into()),
            elevated_by_id: Some("S1".into()),
            elevated_by_mode: Some("orbit".into()),
            intent_kind: "odd".into(),
            intent_id: "I".into(),
            tbl: "settings".into(),
            row_id: "x".into(),
            op: ChangeOp::Update,
            flagged: None,
        };
        let item = names.item(&row).unwrap();
        assert_eq!(item.actor.kind, AuditActorKind::Unknown);
        assert_eq!(item.actor.name, "R2");
        let e = item.elevated_by.unwrap();
        assert_eq!(e.kind, AuditElevationKind::Unknown);
        assert_eq!(e.mode, AuditElevationMode::Unknown);
        assert_eq!(item.module, "odd");
    }

    #[test]
    fn names_resolve_with_fixed_keys_and_fall_back_to_the_id() {
        let names = Names::new(
            vec![("S1".into(), "Asha".into())],
            vec![("D1".into(), "Dev Kiosk".into())],
        );
        assert_eq!(names.actor_name(ActorKind::Staff, "S1"), "Asha");
        assert_eq!(names.actor_name(ActorKind::Staff, "S9"), "S9");
        assert_eq!(names.actor_name(ActorKind::Device, "D1"), "Dev Kiosk");
        assert_eq!(names.actor_name(ActorKind::Device, "D9"), "D9");
        assert_eq!(names.actor_name(ActorKind::System, "hub"), ACTOR_SYSTEM_KEY);
        assert_eq!(names.actor_name(ActorKind::Owner, "o"), ACTOR_OWNER_KEY);
        assert_eq!(names.actor_name(ActorKind::Admin, "a"), ACTOR_ADMIN_KEY);
    }
}
