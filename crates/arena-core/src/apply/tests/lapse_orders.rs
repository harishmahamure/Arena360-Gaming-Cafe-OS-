//! Story 10.13: a lower licence leaves open orders and tickets as they are and stops only new
//! kitchen tickets and kiosk orders; the `users` limit decides who may open a new sign-in, never
//! who stays signed in.

use super::kitchen_ticket::{advance, basic, flags, tickets};
use super::order_line::{code, place, place_of};
use super::session::{int, table, text};
use super::stock::{owner, staff};
use super::*;
use crate::rbac::ROLE_COUNTER;
use crate::staff::StaffDirectory;

#[tokio::test]
async fn an_open_ticket_still_moves_after_the_kitchen_flag_drops_and_no_new_one_opens() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let open = text(&tickets(&f)[0], "id");
    let orders_before = table(&f, Table::Orders);

    flags(&f, "").await;
    assert_eq!(table(&f, Table::Orders), orders_before);
    for to in ["preparing", "ready", "served"] {
        advance(&f, &open, to).await.unwrap();
    }
    assert_eq!(text(&tickets(&f)[0], "stage"), "served");

    // A placement after the drop opens no ticket.
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(tickets(&f).len(), 1);

    // The plan rises again: tickets open again without a restart.
    flags(&f, "kitchen").await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(tickets(&f).len(), 2);
}

#[tokio::test]
async fn a_wrong_stage_move_after_a_lapse_is_bad_stage_and_not_a_plan_refusal() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    flags(&f, "").await;
    // A move that is not the next stage is still `BAD_STAGE`, not a plan refusal.
    assert_eq!(code(advance(&f, &id, "ready").await), ErrorCode::BadStage);
    assert_eq!(int(&tickets(&f)[0], "number"), 1);
}

async fn user_cap(f: &Fixture, n: u32) {
    super::prepaid::sql(
        f,
        format!("UPDATE settings SET value = '{n}' WHERE key = 'licence.user_cap'"),
    )
    .await;
}

#[tokio::test]
async fn a_lower_user_cap_refuses_new_sign_ins_beyond_the_owner_and_the_most_recently_active() {
    let f = bootstrapped().await;
    let owner_staff = staff(&f, "boss", crate::rbac::ROLE_OWNER).await;
    let first = staff(&f, "first", ROLE_COUNTER).await;
    let second = staff(&f, "second", ROLE_COUNTER).await;
    let third = staff(&f, "third", ROLE_COUNTER).await;
    let _ = (&owner_staff, &first, &third);
    let dir = StaffDirectory::open(&f.path).unwrap();
    user_cap(&f, 10).await;
    for login in ["boss", "first", "second", "third"] {
        dir.sign_in(login, "pw").unwrap();
    }

    // `second` works the till most recently.
    let (b, session) = basic(&f).await;
    place(&f, first.clone(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    f.clock.set(f.clock.now_ms() + 60_000);
    place(&f, second.clone(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();

    // Two of the four may sign in: the Owner, then the most recently active.
    user_cap(&f, 2).await;
    dir.sign_in("boss", "pw").unwrap();
    dir.sign_in("second", "pw").unwrap();
    let e = dir.sign_in("third", "pw").unwrap_err();
    assert_eq!(e.code, ErrorCode::UserLimit);
    assert_eq!(
        e.message,
        "This branch's plan allows 2 users — ask the owner."
    );
    // A wrong password still says nothing about the cap.
    assert_eq!(
        dir.sign_in("third", "bad").unwrap_err().code,
        ErrorCode::AuthFailed
    );

    // The plan rises again: everyone may sign in.
    user_cap(&f, 10).await;
    dir.sign_in("third", "pw").unwrap();
}
