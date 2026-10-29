use super::stock::{intent, owner, staff};
use super::*;
use crate::players::{KEK_LEN, open_field, seal};
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::PlayerForm;

pub(super) const KEK: [u8; KEK_LEN] = [9; KEK_LEN];

pub(super) fn form(username: &str, phone: &str) -> PlayerForm {
    PlayerForm {
        username: username.into(),
        password: "secret1".into(),
        phone: phone.into(),
        name: "Asha Rao".into(),
        email: "asha@example.com".into(),
        dob: "2000-01-02".into(),
        notes: "prefers the corner seat".into(),
        tags: vec!["vip".into()],
        consent: true,
        policy_version: "1".into(),
    }
}

pub(super) fn create(f: &PlayerForm) -> (String, IntentPayload) {
    let id = ulid::Ulid::generate().to_string();
    let sealed = seal(&KEK, f, &id, "$argon2id$stub".into()).unwrap();
    (id, IntentPayload::PlayerCreate(sealed))
}

pub(super) async fn add(
    f: &Fixture,
    actor: Actor,
    form: &PlayerForm,
) -> Result<(String, Applied), AppError> {
    let (id, payload) = create(form);
    f.writer
        .submit_intent(intent(actor, payload))
        .await
        .map(|a| (id, a))
}

fn counts(f: &Fixture) -> (usize, usize, usize) {
    let s = reopen(f);
    (
        s.rows(Table::Players).unwrap().len(),
        s.rows(Table::Consents).unwrap().len(),
        s.changes().unwrap().len(),
    )
}

#[tokio::test]
async fn a_profile_writes_player_consent_and_key_with_no_plaintext_in_the_log() {
    let f = bootstrapped().await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let before = counts(&f);
    let (id, applied) = add(&f, counter.clone(), &form("Asha_9", "+91 98765-43210"))
        .await
        .unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["players", "consents", "wallets"]
    );
    assert_eq!(counts(&f), (1, 1, before.2 + 3));

    let store = reopen(&f);
    let player = &store.rows(Table::Players).unwrap()[0];
    assert_eq!(player["player_id"], RowValue::Text(id.clone()));
    assert_eq!(player["username"], RowValue::Text("asha_9".into()));
    assert_eq!(player["tags"], RowValue::Text("[\"vip\"]".into()));
    assert_eq!(player["rev"], RowValue::Integer(1));
    let consent = &store.rows(Table::Consents).unwrap()[0];
    assert_eq!(consent["player_id"], RowValue::Text(id.clone()));
    assert_eq!(consent["purpose"], RowValue::Text("transactional".into()));
    assert_eq!(consent["policy_version"], RowValue::Text("1".into()));
    assert_eq!(consent["granted"], RowValue::Integer(1));
    assert_eq!(consent["at"], RowValue::Integer(T0));

    let wrapped = store.player_key(&id).unwrap().unwrap();
    let RowValue::Text(name_enc) = &player["name_enc"] else {
        panic!("{player:?}")
    };
    assert_eq!(
        open_field(&KEK, &id, &wrapped, "name", name_enc).unwrap(),
        "Asha Rao"
    );

    let log: Vec<u8> = store
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
    assert!(log.contains("$argon2id$stub"));
}

#[tokio::test]
async fn an_unticked_consent_is_recorded_as_not_granted() {
    let f = bootstrapped().await;
    let mut form = form("ravi", "+919000000001");
    form.consent = false;
    add(&f, owner(), &form).await.unwrap();
    let consent = &reopen(&f).rows(Table::Consents).unwrap()[0];
    assert_eq!(consent["granted"], RowValue::Integer(0));
}

#[tokio::test]
async fn a_taken_username_or_phone_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    add(&f, owner(), &form("asha", "+91 98765 43210"))
        .await
        .unwrap();
    let before = counts(&f);

    let e = add(&f, owner(), &form("ASHA", "+919000000002"))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::UsernameTaken, Some("username"))
    );
    let e = add(&f, owner(), &form("other", "+91-98765-43210"))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::PhoneTaken, Some("phone"))
    );
    assert_eq!(counts(&f), before);
    assert_eq!(reopen(&f).player_key("nope").unwrap(), None);
}

#[tokio::test]
async fn a_deleted_players_username_and_phone_are_free_again() {
    let f = bootstrapped().await;
    add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let mut store = Store::open(&f.path, T0).unwrap();
    store
        .write_tx(|tx| {
            tx.execute("UPDATE players SET deleted_at = 1", [])
                .map_err(arena_store::db_error)?;
            Ok(())
        })
        .unwrap();
    drop(store);
    add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_member_without_the_permission_is_refused() {
    let f = bootstrapped().await;
    let kitchen = staff(&f, "cook", ROLE_KITCHEN).await;
    let before = counts(&f);
    let e = add(&f, kitchen, &form("asha", "+919876543210"))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(counts(&f), before);
}

#[tokio::test]
async fn a_malformed_payload_is_refused_on_its_field() {
    let f = bootstrapped().await;
    let before = counts(&f);
    type Edit = fn(&mut arena_proto::PlayerCreate);
    let edits: [(&str, Edit); 8] = [
        ("player_id", |p| p.player_id = "x".into()),
        ("username", |p| p.username = "Bad Name".into()),
        ("password_hash", |p| p.password_hash = "plain".into()),
        ("phone_lookup", |p| p.phone_lookup = "abc".into()),
        ("name_enc", |p| p.name_enc = "Asha Rao".into()),
        ("email_enc", |p| p.email_enc = Some("a@b.c".into())),
        ("tags", |p| p.tags = vec![String::new()]),
        ("policy_version", |p| p.policy_version = String::new()),
    ];
    for (field, edit) in edits {
        let (_, IntentPayload::PlayerCreate(mut p)) = create(&form("asha", "+919876543210")) else {
            unreachable!()
        };
        edit(&mut p);
        let e = f
            .writer
            .submit_intent(intent(owner(), IntentPayload::PlayerCreate(p)))
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some(field))
        );
    }
    assert_eq!(counts(&f), before);
}

fn edit_form(id: &str, rev: i64, phone: &str, consent: bool) -> arena_proto::PlayerEditForm {
    arena_proto::PlayerEditForm {
        player_id: id.into(),
        base_rev: rev,
        phone: phone.into(),
        name: "Asha Verma".into(),
        email: String::new(),
        dob: String::new(),
        notes: "moved to the front desk".into(),
        tags: vec!["regular".into()],
        consent,
        policy_version: "2".into(),
    }
}

async fn edit(
    f: &Fixture,
    actor: Actor,
    form: &arena_proto::PlayerEditForm,
) -> Result<Applied, AppError> {
    let wrapped = reopen(f).player_key(&form.player_id).unwrap().unwrap();
    let payload =
        IntentPayload::PlayerUpdate(crate::players::reseal(&KEK, form, &wrapped).unwrap());
    let mut i = intent(actor, payload);
    i.base_rev = Some(form.base_rev);
    f.writer.submit_intent(i).await
}

fn player_rev(f: &Fixture, id: &str) -> i64 {
    reopen(f).player(id).unwrap().unwrap().1
}

#[tokio::test]
async fn an_edit_reencrypts_the_fields_bumps_rev_and_keeps_the_log_clear_of_plaintext() {
    let f = bootstrapped().await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let (id, _) = add(&f, counter.clone(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let rev = player_rev(&f, &id);
    let applied = edit(&f, counter, &edit_form(&id, rev, "+91 91111-11111", true))
        .await
        .unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["players"]
    );
    let store = reopen(&f);
    let (row, new_rev, wrapped) = store.player(&id).unwrap().unwrap();
    assert_eq!(new_rev, rev + 1);
    assert_eq!(row.username, "asha");
    assert_eq!(row.tags, "[\"regular\"]");
    assert_eq!(row.email_enc, None);
    assert_eq!(
        open_field(&KEK, &id, &wrapped, "phone", &row.phone_enc).unwrap(),
        "+919111111111"
    );
    assert_eq!(
        open_field(&KEK, &id, &wrapped, "name", &row.name_enc).unwrap(),
        "Asha Verma"
    );
    let log: Vec<u8> = store
        .changes()
        .unwrap()
        .into_iter()
        .flat_map(|c| c.row)
        .collect();
    let log = String::from_utf8_lossy(&log).into_owned();
    for plain in ["Asha Verma", "9111111111", "front desk"] {
        assert!(!log.contains(plain), "{plain}");
    }
    assert_eq!(store.rows(Table::Consents).unwrap().len(), 1);
}

#[tokio::test]
async fn a_stale_edit_is_refused_and_a_missing_base_rev_is_invalid() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let rev = player_rev(&f, &id);
    edit(&f, owner(), &edit_form(&id, rev, "+919876543210", true))
        .await
        .unwrap();
    let before = counts(&f);
    let e = edit(&f, owner(), &edit_form(&id, rev, "+919876543210", true))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::Stale, Some("base_rev"))
    );
    let wrapped = reopen(&f).player_key(&id).unwrap().unwrap();
    let payload = IntentPayload::PlayerUpdate(
        crate::players::reseal(&KEK, &edit_form(&id, 0, "+919876543210", true), &wrapped).unwrap(),
    );
    let e = f
        .writer
        .submit_intent(intent(owner(), payload))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("base_rev"))
    );
    assert_eq!(counts(&f), before);
}

#[tokio::test]
async fn an_edit_keeps_uniqueness_and_may_keep_its_own_phone() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    add(&f, owner(), &form("ravi", "+919000000001"))
        .await
        .unwrap();
    let rev = player_rev(&f, &id);
    let e = edit(&f, owner(), &edit_form(&id, rev, "+91 90000-00001", true))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::PhoneTaken, Some("phone"))
    );
    edit(&f, owner(), &edit_form(&id, rev, "+91 98765 43210", true))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_changed_consent_appends_a_row_and_keeps_the_earlier_ones() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let applied = edit(
        &f,
        owner(),
        &edit_form(&id, player_rev(&f, &id), "+919876543210", false),
    )
    .await
    .unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["players", "consents"]
    );
    let store = reopen(&f);
    let (granted, version) = store.latest_consent(&id, "transactional").unwrap().unwrap();
    assert_eq!((granted, version.as_str()), (false, "2"));
    assert_eq!(store.rows(Table::Consents).unwrap().len(), 2);
    edit(
        &f,
        owner(),
        &edit_form(&id, player_rev(&f, &id), "+919876543210", true),
    )
    .await
    .unwrap();
    assert_eq!(reopen(&f).rows(Table::Consents).unwrap().len(), 3);
}

#[tokio::test]
async fn a_password_reset_stores_the_new_hash_and_needs_the_permission() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let reset = |hash: &str| {
        IntentPayload::PlayerPasswordReset(arena_proto::PlayerPasswordReset {
            player_id: id.clone(),
            password_hash: hash.into(),
        })
    };
    let kitchen = staff(&f, "cook", ROLE_KITCHEN).await;
    let e = f
        .writer
        .submit_intent(intent(kitchen, reset("$argon2id$new")))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    let e = f
        .writer
        .submit_intent(intent(owner(), reset("plain")))
        .await
        .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("password_hash"));
    f.writer
        .submit_intent(intent(owner(), reset("$argon2id$new")))
        .await
        .unwrap();
    let store = reopen(&f);
    assert_eq!(
        store.player(&id).unwrap().unwrap().0.password_hash,
        "$argon2id$new"
    );
    let missing = IntentPayload::PlayerPasswordReset(arena_proto::PlayerPasswordReset {
        player_id: "nope".into(),
        password_hash: "$argon2id$new".into(),
    });
    let e = f
        .writer
        .submit_intent(intent(owner(), missing))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
}
