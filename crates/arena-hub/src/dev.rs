//! Dev-only fixtures (feature `dev-fixtures`, absent from default and release builds). Epic 2
//! replaces them with real onboarding and pairing.
//!
//! - `dev seed` opens the store, takes the lease once as the Hub does at start (no renewal
//!   loop), submits `branch.bootstrap` as `actor{system, "dev-seed"}`, then `device.register`
//!   (a `paired` kiosk labelled "Dev Kiosk") and one `staff.upsert` per [`DEV_STAFF`] member as
//!   `actor{owner, "dev-seed"}` (the owner holds every permission) through the Writer, then
//!   releases the lease (it expires now) so a real Hub can start on the same data dir at once.
//!   The device token and the staff credentials are returned once to be printed; only argon2id
//!   hashes are stored.
//! - `dev revoke-device <id>` asks the running Hub to revoke a device through the loopback
//!   `POST /dev/revoke-device` on its health port.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
use arena_core::rbac::{BUILTIN_ROLES, ROLE_COUNTER, ROLE_KITCHEN, ROLE_MANAGER, ROLE_OWNER};
use arena_core::staff::hash_secret;
use arena_core::{Clock, HubStatus, load_or_mint_instance_id, status_channel};
use arena_proto::{
    Actor, ActorKind, AppError, BranchBootstrap, ClientClass, DeviceRegister, ErrorCode, Intent,
    IntentPayload, StaffUpsert,
};
use arena_store::Store;
use arena_tasks::LeaseTask;

use crate::config::Config;

/// The fixture branch.
pub fn seed_payload() -> BranchBootstrap {
    BranchBootstrap {
        name: "Dev Branch".into(),
        branch_code: "DEV01".into(),
        currency: "INR".into(),
        tz: "Asia/Kolkata".into(),
        day_start_min: 360,
        fy_start_month: 4,
    }
}

/// The label of the seeded kiosk.
pub const DEV_KIOSK_LABEL: &str = "Dev Kiosk";

/// The seeded staff: `(login, display name, built-in role, PIN)`. Each password is
/// `dev-<login>-pass`.
pub const DEV_STAFF: [(&str, &str, &str, &str); 4] = [
    ("owner", "Owner", ROLE_OWNER, "1111"),
    ("manager", "Manager", ROLE_MANAGER, "2222"),
    ("counter", "Counter", ROLE_COUNTER, "3333"),
    ("kitchen", "Kitchen", ROLE_KITCHEN, "4444"),
];

/// The dev password of `login`.
pub fn dev_password(login: &str) -> String {
    format!("dev-{login}-pass")
}

/// One seeded staff member and their credentials (shown once).
#[derive(Debug)]
pub struct SeededStaff {
    pub staff_id: String,
    pub login: &'static str,
    pub role: &'static str,
    pub password: String,
    pub pin: &'static str,
}

/// What `dev seed` created.
#[derive(Debug)]
pub struct Seeded {
    /// Change rows written by the bootstrap, the registration and the staff.
    pub changes: usize,
    pub device_id: String,
    /// The device token, shown once.
    pub token: String,
    pub staff: Vec<SeededStaff>,
}

fn intent(clock: &dyn Clock, kind: ActorKind, payload: IntentPayload) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: clock.now_ms(),
        base_rev: None,
        actor: Actor {
            kind,
            id: "dev-seed".into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

/// Seeds the branch and the dev kiosk in `cfg.data_dir`.
pub async fn seed(cfg: &Config, clock: Arc<dyn Clock>) -> Result<Seeded, AppError> {
    std::fs::create_dir_all(&cfg.data_dir)
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("create data dir: {e}")))?;
    let mut store = Store::open(cfg.db_path(), clock.now_ms())?;
    let schema_version = store.schema_version()?;
    let instance_id = load_or_mint_instance_id(&mut store)?;
    let (status_tx, _status_rx) =
        status_channel(HubStatus::opened(schema_version, instance_id.clone()));
    let lease = LeaseTask::start(store, instance_id, Arc::clone(&clock), status_tx).await?;
    let Some(writer) = lease.writer() else {
        lease.release().await?;
        return Err(AppError::new(
            ErrorCode::NotPrimary,
            "another hub holds this branch database",
        ));
    };
    // The token's and the staff hashes are computed here, outside any transaction.
    let secret = new_token_secret()?;
    let token_hash = hash_token_secret(&secret)?;
    let mut hashes = Vec::with_capacity(DEV_STAFF.len());
    for (login, _, _, pin) in DEV_STAFF {
        hashes.push((hash_secret(&dev_password(login))?, hash_secret(pin)?));
    }
    let result = async {
        let bootstrap = writer
            .submit_intent(intent(
                clock.as_ref(),
                ActorKind::System,
                IntentPayload::BranchBootstrap(seed_payload()),
            ))
            .await?;
        // Bootstrap writes the built-in roles in BUILTIN_ROLES order.
        let role_ids: Vec<&str> = bootstrap
            .changes
            .iter()
            .filter(|c| c.tbl == "roles")
            .map(|c| c.row_id.as_str())
            .collect();
        let role_id = |name: &str| {
            BUILTIN_ROLES
                .iter()
                .position(|r| *r == name)
                .and_then(|i| role_ids.get(i))
                .map(|id| (*id).to_owned())
                .ok_or_else(|| AppError::new(ErrorCode::Internal, "bootstrap wrote no roles"))
        };
        let device = writer
            .submit_intent(intent(
                clock.as_ref(),
                ActorKind::Owner,
                IntentPayload::DeviceRegister(DeviceRegister {
                    class: ClientClass::Paired,
                    label: DEV_KIOSK_LABEL.into(),
                    token_hash,
                }),
            ))
            .await?;
        let device_id = device
            .changes
            .first()
            .map(|c| c.row_id.clone())
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "device.register wrote no row"))?;
        let mut changes = bootstrap.changes.len() + device.changes.len();
        let mut staff = Vec::with_capacity(DEV_STAFF.len());
        for ((login, display_name, role, pin), (password_hash, pin_hash)) in
            DEV_STAFF.into_iter().zip(hashes)
        {
            let applied = writer
                .submit_intent(intent(
                    clock.as_ref(),
                    ActorKind::Owner,
                    IntentPayload::StaffUpsert(StaffUpsert {
                        staff_id: None,
                        login: login.into(),
                        display_name: display_name.into(),
                        role_id: role_id(role)?,
                        password_hash: Some(password_hash),
                        pin_hash: Some(pin_hash),
                        active: true,
                    }),
                ))
                .await?;
            changes += applied.changes.len();
            staff.push(SeededStaff {
                staff_id: applied
                    .changes
                    .first()
                    .map(|c| c.row_id.clone())
                    .unwrap_or_default(),
                login,
                role,
                password: dev_password(login),
                pin,
            });
        }
        Ok::<_, AppError>(Seeded {
            changes,
            token: format_token(&device_id, &secret),
            device_id,
            staff,
        })
    }
    .await;
    let released = lease.release().await;
    let seeded = result?;
    released?;
    Ok(seeded)
}

/// `POST /dev/revoke-device` to the Hub on `127.0.0.1:health_port`. Returns the response body
/// on 200, else an error carrying the Hub's answer.
pub fn revoke_device(device_id: &str, health_port: u16) -> Result<String, AppError> {
    let io = |e: std::io::Error| {
        AppError::new(
            ErrorCode::Internal,
            format!("talk to the hub on 127.0.0.1:{health_port}: {e}"),
        )
    };
    let body = serde_json::json!({ "device_id": device_id }).to_string();
    let mut tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, health_port)).map_err(io)?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(io)?;
    write!(
        tcp,
        "POST /dev/revoke-device HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(io)?;
    let mut response = String::new();
    tcp.read_to_string(&mut response).map_err(io)?;
    let (head, payload) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    if head.starts_with("HTTP/1.1 200") {
        Ok(payload.to_owned())
    } else {
        let error = serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| serde_json::from_value::<AppError>(v.get("error")?.clone()).ok())
            .unwrap_or_else(|| {
                AppError::new(
                    ErrorCode::Internal,
                    format!("unexpected answer: {}", head.lines().next().unwrap_or("")),
                )
            });
        Err(error)
    }
}
