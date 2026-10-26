use arena_store::{Store, Table};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::ui_auth::tests::{Hub, TEST_KEK, call, hub, token_for};

fn form(username: &str, phone: &str) -> Value {
    json!({
        "username": username,
        "password": "secret1",
        "phone": phone,
        "name": "Asha Rao",
        "email": "asha@example.com",
        "dob": "2000-01-02",
        "notes": "corner seat",
        "tags": ["vip"],
        "consent": true,
        "policy_version": "1",
    })
}

fn body(form: Value, ticket: Option<&str>) -> Value {
    let mut body = json!({"form": form, "idempotency_key": ulid::Ulid::generate().to_string()});
    if let Some(t) = ticket {
        body["elevation_ticket"] = json!(t);
    }
    body
}

async fn create(hub: &Hub, token: &str, body: Value) -> (StatusCode, Value) {
    call(hub, "POST", "/ui/players", Some(token), Some(body)).await
}

fn store(hub: &Hub) -> Store {
    Store::open_reader(&hub.path).unwrap()
}

fn written(hub: &Hub) -> (usize, usize, usize) {
    let s = store(hub);
    (
        s.rows(Table::Players).unwrap().len(),
        s.rows(Table::Consents).unwrap().len(),
        s.changes().unwrap().len(),
    )
}

#[tokio::test]
async fn a_counter_creates_a_profile_and_only_ciphertext_is_stored() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    let (status, reply) = create(&hub, &token, body(form("Asha_9", "+91 98765 43210"), None)).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let tables: Vec<&str> = reply["applied"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["tbl"].as_str().unwrap())
        .collect();
    assert_eq!(tables, ["players", "consents"]);

    let s = store(&hub);
    let players = s.rows(Table::Players).unwrap();
    let id = match &players[0]["player_id"] {
        arena_store::RowValue::Text(t) => t.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        players[0]["username"],
        arena_store::RowValue::Text("asha_9".into())
    );
    let wrapped = s.player_key(&id).unwrap().unwrap();
    let arena_store::RowValue::Text(name_enc) = &players[0]["name_enc"] else {
        panic!()
    };
    assert_eq!(
        arena_core::players::open_field(&TEST_KEK, &id, &wrapped, "name", name_enc).unwrap(),
        "Asha Rao"
    );
    let hash = match &players[0]["password_hash"] {
        arena_store::RowValue::Text(t) => t.clone(),
        other => panic!("{other:?}"),
    };
    assert!(hash.starts_with("$argon2id$"));

    let log: Vec<u8> = s
        .changes()
        .unwrap()
        .into_iter()
        .flat_map(|c| c.row)
        .collect();
    let log = String::from_utf8_lossy(&log).into_owned();
    for plain in [
        "Asha Rao",
        "9876543210",
        "asha@example.com",
        "2000-01-02",
        "corner seat",
        "secret1",
    ] {
        assert!(!log.contains(plain), "{plain}");
    }
}

#[tokio::test]
async fn validation_failures_name_their_field_and_write_nothing() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    let before = written(&hub);
    type Edit = fn(&mut Value);
    let cases: [(&str, Edit); 4] = [
        ("password", |f| f["password"] = json!("12345")),
        ("phone", |f| f["phone"] = json!("+91 12")),
        ("username", |f| f["username"] = json!("a")),
        ("dob", |f| f["dob"] = json!("2999-01-01")),
    ];
    for (field, edit) in cases {
        let mut f = form("asha", "+919876543210");
        edit(&mut f);
        let (status, err) = create(&hub, &token, body(f, None)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
        assert_eq!(
            (err["code"].as_str(), err["field"].as_str()),
            (Some("VALIDATION_FAILED"), Some(field))
        );
    }
    assert_eq!(written(&hub), before);
}

#[tokio::test]
async fn a_duplicate_username_or_phone_is_a_conflict_on_its_field() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    let (status, _) = create(&hub, &token, body(form("asha", "+919876543210"), None)).await;
    assert_eq!(status, StatusCode::OK);
    let before = written(&hub);
    for (f, code, field) in [
        (form("ASHA", "+919000000002"), "USERNAME_TAKEN", "username"),
        (form("other", "+91-98765 43210"), "PHONE_TAKEN", "phone"),
    ] {
        let (status, err) = create(&hub, &token, body(f, None)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{err}");
        assert_eq!(
            (err["code"].as_str(), err["field"].as_str()),
            (Some(code), Some(field))
        );
    }
    assert_eq!(written(&hub), before);
}

#[tokio::test]
async fn the_raw_intent_route_refuses_player_create() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    let sealed = arena_core::players::seal(
        &TEST_KEK,
        &serde_json::from_value(form("asha", "+919876543210")).unwrap(),
        &ulid::Ulid::generate().to_string(),
        "$argon2id$stub".into(),
    )
    .unwrap();
    let intent = crate::ui_auth::tests::intent_as(
        arena_proto::Actor {
            kind: arena_proto::ActorKind::Staff,
            id: "x".into(),
        },
        arena_proto::IntentPayload::PlayerCreate(sealed),
        None,
    );
    let before = written(&hub);
    let (status, err) = call(
        &hub,
        "POST",
        "/ui/intent",
        Some(&token),
        Some(json!({"intent": intent})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");
    assert_eq!(err["code"], "UNSUPPORTED");
    assert_eq!(written(&hub), before);
}

#[tokio::test]
async fn a_member_without_the_permission_needs_a_managers_pin() {
    let hub = hub().await;
    let token = token_for(&hub, "kitchen", "k-pass").await;
    let (status, denied) = create(&hub, &token, body(form("asha", "+919876543210"), None)).await;
    assert_eq!(
        (status, denied["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
    );
    let item = json!({"intent_kind": "player.create"});
    let (status, _) = call(
        &hub,
        "POST",
        "/ui/elevate",
        Some(&token),
        Some(json!({"items": [item.clone()], "pin": "9999"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(written(&hub).0, 0);

    let (status, ok) = call(
        &hub,
        "POST",
        "/ui/elevate",
        Some(&token),
        Some(json!({"items": [item], "pin": "2222"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ok}");
    let ticket = ok["ticket"].as_str().unwrap().to_owned();
    let (status, reply) = create(
        &hub,
        &token,
        body(form("asha", "+919876543210"), Some(&ticket)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(written(&hub).0, 1);
    let last = store(&hub).changes().unwrap().pop().unwrap();
    assert_eq!(
        last.elevated_by_id.as_deref(),
        Some(hub.manager_id.as_str())
    );
    let (status, _) = create(
        &hub,
        &token,
        body(form("ravi", "+919000000003"), Some(&ticket)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(written(&hub).0, 1);
}

#[tokio::test]
async fn the_audit_viewer_shows_the_consent_and_hides_personal_data() {
    let hub = hub().await;
    let counter = token_for(&hub, "counter", "c-pass").await;
    let (status, _) = create(&hub, &counter, body(form("asha", "+919876543210"), None)).await;
    assert_eq!(status, StatusCode::OK);
    let manager = token_for(&hub, "manager", "m-pass").await;
    let changes = store(&hub).changes().unwrap();
    let detail = |tbl: &str| {
        let c = changes.iter().find(|c| c.tbl == tbl).unwrap().clone();
        format!("/ui/audit/item?epoch={}&seq={}", c.epoch, c.seq)
    };
    let (status, player) = call(&hub, "GET", &detail("players"), Some(&manager), None).await;
    assert_eq!(status, StatusCode::OK, "{player}");
    for column in [
        "name_enc",
        "phone_enc",
        "email_enc",
        "dob_enc",
        "notes_enc",
        "phone_lookup",
        "password_hash",
    ] {
        assert_eq!(player["after"][column], "[redacted]", "{column}");
    }
    assert!(!player.to_string().contains("Asha Rao"));
    let (status, consent) = call(&hub, "GET", &detail("consents"), Some(&manager), None).await;
    assert_eq!(status, StatusCode::OK, "{consent}");
    assert_eq!(consent["after"]["player_id"], player["after"]["player_id"]);
    assert_eq!(consent["after"]["purpose"], "transactional");
    assert_eq!(consent["after"]["policy_version"], "1");
    assert_eq!(consent["after"]["granted"], 1);
}

#[tokio::test]
async fn lookup_finds_by_digits_opens_a_profile_and_follows_new_players() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    let get = |uri: String| {
        let (hub, token) = (&hub, &token);
        async move { call(hub, "GET", &uri, Some(token), None).await }
    };
    let (status, none) = get("/ui/players?q=98765".into()).await;
    assert_eq!((status, none), (StatusCode::OK, json!([])));
    let (status, _) = create(&hub, &token, body(form("asha", "+919876543210"), None)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, hits) = get("/ui/players?q=%2B91%2098765-4".into()).await;
    assert_eq!(hits.as_array().unwrap().len(), 1, "{hits}");
    assert_eq!(hits[0]["name"], "Asha Rao");
    assert_eq!(hits[0]["phone"], "+919876543210");
    let (status, _) = create(&hub, &token, body(form("ravi", "+919876500000"), None)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, hits) = get("/ui/players?q=98765".into()).await;
    assert_eq!(hits.as_array().unwrap().len(), 2, "{hits}");
    let (_, empty) = get("/ui/players?q=abc".into()).await;
    assert_eq!(empty, json!([]));

    let id = hits[0]["player_id"].as_str().unwrap();
    let (status, profile) = get(format!("/ui/players/profile?id={id}")).await;
    assert_eq!(status, StatusCode::OK, "{profile}");
    assert_eq!(profile["player"]["tags"], json!(["vip"]));
    assert_eq!(
        profile["consent"],
        json!({"granted": true, "policy_version": "1"})
    );
    let (status, err) = get("/ui/players/profile?id=missing".into()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{err}");
    assert_eq!(err["code"], "NOT_FOUND");
}

#[tokio::test]
async fn lookup_needs_a_session_that_may_view_players() {
    let hub = hub().await;
    let (status, err) = call(&hub, "GET", "/ui/players?q=9", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{err}");
    let kitchen = token_for(&hub, "kitchen", "k-pass").await;
    let (status, err) = call(&hub, "GET", "/ui/players?q=9", Some(&kitchen), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{err}");
    assert_eq!(err["code"], "PERMISSION_DENIED");
    let counter = token_for(&hub, "counter", "c-pass").await;
    let (status, _) = call(&hub, "POST", "/ui/lock", Some(&counter), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, err) = call(&hub, "GET", "/ui/players?q=9", Some(&counter), None).await;
    assert_eq!(err["code"], "SESSION_LOCKED", "{status} {err}");
}

async fn profile_of(hub: &Hub, token: &str, phone_digits: &str) -> Value {
    let (_, hits) = call(
        hub,
        "GET",
        &format!("/ui/players?q={phone_digits}"),
        Some(token),
        None,
    )
    .await;
    let id = hits[0]["player_id"].as_str().unwrap();
    call(
        hub,
        "GET",
        &format!("/ui/players/profile?id={id}"),
        Some(token),
        None,
    )
    .await
    .1
}

fn edit_body(profile: &Value, phone: &str, consent: bool, ticket: Option<&str>) -> Value {
    let form = json!({
        "player_id": profile["player"]["player_id"],
        "base_rev": profile["rev"],
        "phone": phone,
        "name": "Asha Verma",
        "email": "",
        "dob": "",
        "notes": "new note",
        "tags": ["regular"],
        "consent": consent,
        "policy_version": "2",
    });
    body(form, ticket)
}

#[tokio::test]
async fn an_edit_updates_the_profile_and_the_lookup_follows_it() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    create(&hub, &token, body(form("asha", "+919876543210"), None)).await;
    let before = profile_of(&hub, &token, "98765").await;
    assert_eq!(before["email"], "asha@example.com");
    assert_eq!(before["dob"], "2000-01-02");
    assert_eq!(before["notes"], "corner seat");
    let (status, reply) = call(
        &hub,
        "POST",
        "/ui/players/update",
        Some(&token),
        Some(edit_body(&before, "+91 91111-11111", false, None)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let (_, old) = call(&hub, "GET", "/ui/players?q=9876543210", Some(&token), None).await;
    assert_eq!(old, json!([]));
    let after = profile_of(&hub, &token, "91111").await;
    assert_eq!(after["player"]["name"], "Asha Verma");
    assert_eq!(after["player"]["phone"], "+919111111111");
    assert_eq!(after["player"]["tags"], json!(["regular"]));
    assert_eq!(after["email"], "");
    assert_eq!(after["rev"], before["rev"].as_i64().unwrap() + 1);
    assert_eq!(
        after["consent"],
        json!({"granted": false, "policy_version": "2"})
    );
    let log: Vec<u8> = store(&hub)
        .changes()
        .unwrap()
        .into_iter()
        .flat_map(|c| c.row)
        .collect();
    let log = String::from_utf8_lossy(&log).into_owned();
    for plain in ["Asha Verma", "9111111111", "new note"] {
        assert!(!log.contains(plain), "{plain}");
    }
}

#[tokio::test]
async fn a_stale_or_invalid_edit_changes_nothing() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    create(&hub, &token, body(form("asha", "+919876543210"), None)).await;
    let profile = profile_of(&hub, &token, "98765").await;
    let go = |b: Value| {
        let (hub, token) = (&hub, &token);
        async move { call(hub, "POST", "/ui/players/update", Some(token), Some(b)).await }
    };
    let (status, _) = go(edit_body(&profile, "+919876543210", true, None)).await;
    assert_eq!(status, StatusCode::OK);
    let before = written(&hub);
    let (_, stale) = go(edit_body(&profile, "+919876543210", true, None)).await;
    assert_eq!(stale["code"], "STALE", "{stale}");
    let fresh = profile_of(&hub, &token, "98765").await;
    let (_, bad) = go(edit_body(&fresh, "+91 12", true, None)).await;
    assert_eq!(
        (bad["code"].as_str(), bad["field"].as_str()),
        (Some("VALIDATION_FAILED"), Some("phone"))
    );
    assert_eq!(written(&hub), before);
}

#[tokio::test]
async fn a_password_reset_stores_a_new_hash_and_refuses_mismatches() {
    let hub = hub().await;
    let token = token_for(&hub, "counter", "c-pass").await;
    create(&hub, &token, body(form("asha", "+919876543210"), None)).await;
    let id = profile_of(&hub, &token, "98765").await["player"]["player_id"].clone();
    let reset = |password: &str, confirm: &str| {
        body(
            json!({"player_id": id, "password": password, "confirm": confirm}),
            None,
        )
    };
    let go = |b: Value| {
        let (hub, token) = (&hub, &token);
        async move { call(hub, "POST", "/ui/players/password", Some(token), Some(b)).await }
    };
    let hash_of = || match &store(&hub).rows(Table::Players).unwrap()[0]["password_hash"] {
        arena_store::RowValue::Text(t) => t.clone(),
        other => panic!("{other:?}"),
    };
    let old = hash_of();
    let before = written(&hub);
    let (status, err) = go(reset("newpass1", "newpass2")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    assert_eq!(err["field"], "confirm");
    let (_, err) = go(reset("12345", "12345")).await;
    assert_eq!(err["field"], "password");
    assert_eq!((hash_of(), written(&hub)), (old.clone(), before));
    let (status, ok) = go(reset("newpass1", "newpass1")).await;
    assert_eq!(status, StatusCode::OK, "{ok}");
    let new = hash_of();
    assert!(new.starts_with("$argon2id$") && new != old);
    let log: Vec<u8> = store(&hub)
        .changes()
        .unwrap()
        .into_iter()
        .flat_map(|c| c.row)
        .collect();
    assert!(!String::from_utf8_lossy(&log).contains("newpass1"));
}

#[tokio::test]
async fn edit_and_reset_need_the_permission_or_a_managers_pin() {
    let hub = hub().await;
    let counter = token_for(&hub, "counter", "c-pass").await;
    create(&hub, &counter, body(form("asha", "+919876543210"), None)).await;
    let profile = profile_of(&hub, &counter, "98765").await;
    let kitchen = token_for(&hub, "kitchen", "k-pass").await;
    let (status, denied) = call(
        &hub,
        "POST",
        "/ui/players/update",
        Some(&kitchen),
        Some(edit_body(&profile, "+919876543210", true, None)),
    )
    .await;
    assert_eq!(
        (status, denied["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
    );
    let (_, ok) = call(
        &hub,
        "POST",
        "/ui/elevate",
        Some(&kitchen),
        Some(json!({"items": [{"intent_kind": "player.update"}], "pin": "2222"})),
    )
    .await;
    let ticket = ok["ticket"].as_str().unwrap().to_owned();
    let (status, reply) = call(
        &hub,
        "POST",
        "/ui/players/update",
        Some(&kitchen),
        Some(edit_body(&profile, "+919876543210", true, Some(&ticket))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let last = store(&hub).changes().unwrap().pop().unwrap();
    assert_eq!(
        last.elevated_by_id.as_deref(),
        Some(hub.manager_id.as_str())
    );
    let (status, denied) = call(
        &hub,
        "POST",
        "/ui/players/password",
        Some(&kitchen),
        Some(body(
            json!({"player_id": profile["player"]["player_id"], "password": "newpass1", "confirm": "newpass1"}),
            None,
        )),
    )
    .await;
    assert_eq!(
        (status, denied["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
    );
}
