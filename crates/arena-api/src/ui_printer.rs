use std::sync::Arc;

use arena_core::printer::{
    PaperWidth, Printer, PrinterError, PrinterTarget, test_page, test_page_date,
};
use arena_proto::{
    ActorKind, AlertKind, AlertRaise, AppError, ErrorCode, FLAG_KITCHEN, HubSnapshot, Intent,
    IntentPayload, KEY_KITCHEN_KOT_PAPER_MM, KEY_KITCHEN_KOT_PRINTER, KEY_LICENCE_FLAGS,
    KEY_PRINTER_PAPER_MM, KEY_PRINTER_SELECTED, Permission, PrinterErrorKind, PrinterFindResult,
    PrinterHealth, PrinterJobFailure, PrinterOutcome, PrinterSelected, PrinterState, PrinterStatus,
    PrinterStatusReply, PrinterStatusSet, SessionStaff, SettingEntry, UiError, has_flag,
    printer_error_text,
};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bearer, fail, internal, ui_error};

fn not_allowed() -> UiError {
    ui_error(AppError::new(
        ErrorCode::PermissionDenied,
        "only a manager or owner can set up the printer",
    ))
}

fn not_primary() -> UiError {
    ui_error(AppError::new(
        ErrorCode::NotPrimary,
        "this hub is not the primary writer",
    ))
}

fn no_adapter() -> UiError {
    ui_error(AppError::new(
        ErrorCode::Unsupported,
        "printing is not available on this hub",
    ))
}

async fn editor(auth: &UiAuth, headers: &HeaderMap) -> Result<SessionStaff, UiError> {
    let token = bearer(headers).ok_or_else(auth_failed)?.to_owned();
    let me = auth.acting_session(&token).await?;
    if !me.permissions.contains(&Permission::SettingsEdit) {
        return Err(not_allowed());
    }
    Ok(me)
}

fn printer_of(auth: &UiAuth) -> Result<Arc<dyn Printer>, UiError> {
    auth.printer().cloned().ok_or_else(no_adapter)
}

async fn read_state(auth: &UiAuth) -> Result<(Vec<SettingEntry>, HubSnapshot), UiError> {
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    tokio::task::spawn_blocking(move || {
        Ok::<_, AppError>((reader.settings()?, reader.snapshot(now)?))
    })
    .await
    .map_err(|_| internal())?
    .map_err(ui_error)
}

fn value<'a>(settings: &'a [SettingEntry], key: &str) -> Option<&'a str> {
    settings
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.value.as_str())
}

fn reply(result: Result<PrinterStatusReply, UiError>) -> Response {
    match result {
        Ok(body) => Json(body).into_response(),
        Err(e) => fail(e),
    }
}

async fn record(
    auth: &UiAuth,
    me: &SessionStaff,
    state: PrinterState,
    failure: Option<(PrinterErrorKind, String)>,
) -> Result<PrinterStatus, UiError> {
    let writer = auth.writer().ok_or_else(not_primary)?;
    let issued_at = auth.clock().now_ms();
    let (error_kind, error) = match failure {
        Some((kind, detail)) => (Some(kind), Some(detail)),
        None => (None, None),
    };
    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: arena_proto::IntentKind::PrinterStatus,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at,
        base_rev: None,
        actor: arena_proto::Actor {
            kind: ActorKind::Staff,
            id: me.staff_id.clone(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::PrinterStatus(PrinterStatusSet {
            state,
            error_kind,
            error: error.clone(),
        }),
    };
    writer.submit_intent(intent).await.map_err(ui_error)?;
    Ok(PrinterStatus {
        state,
        error_kind,
        error,
        at: Some(issued_at),
    })
}

fn recorded(status: PrinterStatus) -> PrinterStatusReply {
    PrinterStatusReply {
        outcome: PrinterOutcome::Recorded,
        status: Some(status),
    }
}

pub(crate) async fn find(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let printer = match editor(&auth, &headers)
        .await
        .and_then(|_| printer_of(&auth))
    {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    let _single = auth.printer_find_lock().lock().await;
    let (printers, bluetooth) = tokio::join!(printer.discover(), printer.bluetooth_access());
    tracing::info!(found = printers.len(), bluetooth = ?bluetooth, "printer search finished");
    Json(PrinterFindResult {
        printers,
        bluetooth,
        usb_supported: printer.usb_supported(),
    })
    .into_response()
}

pub(crate) async fn health(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = editor(&auth, &headers).await {
        return fail(e);
    }
    let last_job_failure = auth
        .printer_failure()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Json(PrinterHealth { last_job_failure }).into_response()
}

/// Raises the "Printer offline" alert (system actor) for the header alert list. An alert that is
/// already open is left as it is.
async fn raise_offline(auth: &UiAuth) {
    let Some(writer) = auth.writer() else { return };
    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: arena_proto::IntentKind::AlertRaise,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: auth.clock().now_ms(),
        base_rev: None,
        actor: arena_proto::Actor {
            kind: ActorKind::System,
            id: "printer".to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::AlertRaise(AlertRaise::new(AlertKind::PrinterOffline, None)),
    };
    if let Err(e) = writer.submit_intent(intent).await
        && e.code != ErrorCode::Stale
    {
        tracing::warn!(
            code = e.code.as_str(),
            "the printer offline alert was not raised"
        );
    }
}

/// The one way a page reaches a printer. A failed job is kept as the printer's last job failure
/// (cleared by the next job that works), and a Bluetooth printer that drops raises a "Printer
/// offline" alert. The next job opens a new connection, so a printer that is back works at once.
async fn dispatch(
    auth: &UiAuth,
    printer: &dyn Printer,
    target: &PrinterTarget,
    page: &[u8],
) -> Result<(), PrinterError> {
    let sent = printer.send(target, page).await;
    let failure = sent.as_ref().err().map(|e| PrinterJobFailure {
        error_kind: e.kind(),
        error: printer_error_text(&e.to_string()),
        at: auth.clock().now_ms(),
    });
    let failed = failure.is_some();
    *auth
        .printer_failure()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = failure;
    // Only a link failure means the printer is offline; a refused permission or a switched-off
    // radio is the phone's state, which the settings page explains.
    if failed
        && matches!(target, PrinterTarget::Bluetooth(_))
        && matches!(sent, Err(PrinterError::Bluetooth(..)))
    {
        raise_offline(auth).await;
    }
    sent
}

async fn run_test(auth: &UiAuth, headers: &HeaderMap) -> Result<PrinterStatusReply, UiError> {
    let me = editor(auth, headers).await?;
    if auth.writer().is_none() {
        return Err(not_primary());
    }
    let printer = printer_of(auth)?;
    let _single = auth.printer_job_lock().lock().await;
    let (settings, snapshot) = read_state(auth).await?;
    let selected = value(&settings, KEY_PRINTER_SELECTED)
        .and_then(|v| PrinterSelected::parse(v).ok().flatten())
        .ok_or_else(|| {
            ui_error(AppError::with_field(
                ErrorCode::ValidationFailed,
                KEY_PRINTER_SELECTED,
                "choose a printer before a test print",
            ))
        })?;
    let paper = PaperWidth::from_setting(value(&settings, KEY_PRINTER_PAPER_MM).unwrap_or("80"));
    let name = snapshot.branch.map(|b| b.name).unwrap_or_default();
    let tz = snapshot.locale.map(|l| l.tz).unwrap_or_default();
    let page = test_page(&name, &test_page_date(auth.clock().now_ms(), &tz), paper);
    let sent = match PrinterTarget::from_selected(&selected) {
        Ok(target) => dispatch(auth, printer.as_ref(), &target, &page).await,
        Err(e) => Err(e),
    };
    match sent {
        Ok(()) => {
            tracing::info!(kind = ?selected.kind, columns = paper.columns(), "test print sent");
            match record(auth, &me, PrinterState::Ok, None).await {
                Ok(status) => Ok(recorded(status)),
                Err(e) => {
                    tracing::warn!(
                        code = e.code.as_str(),
                        "the test page printed but its result was not recorded"
                    );
                    Ok(PrinterStatusReply {
                        outcome: PrinterOutcome::PrintedNotRecorded,
                        status: None,
                    })
                }
            }
        }
        Err(e) => {
            tracing::warn!(kind = ?selected.kind, error = %e, "test print failed");
            let failure = (e.kind(), printer_error_text(&e.to_string()));
            record(auth, &me, PrinterState::Failed, Some(failure))
                .await
                .map(recorded)
        }
    }
}

pub(crate) async fn test(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    reply(run_test(&auth, &headers).await)
}

async fn run_kot_test(auth: &UiAuth, headers: &HeaderMap) -> Result<PrinterStatusReply, UiError> {
    editor(auth, headers).await?;
    let printer = printer_of(auth)?;
    let _single = auth.printer_job_lock().lock().await;
    let (settings, snapshot) = read_state(auth).await?;
    if !has_flag(
        value(&settings, KEY_LICENCE_FLAGS).unwrap_or(""),
        FLAG_KITCHEN,
    ) {
        return Err(ui_error(AppError::new(
            ErrorCode::TierLocked,
            "available on Basic and above",
        )));
    }
    let selected = value(&settings, KEY_KITCHEN_KOT_PRINTER)
        .and_then(|v| PrinterSelected::parse(v).ok().flatten())
        .ok_or_else(|| {
            ui_error(AppError::with_field(
                ErrorCode::ValidationFailed,
                KEY_KITCHEN_KOT_PRINTER,
                "choose a printer before a test print",
            ))
        })?;
    let paper =
        PaperWidth::from_setting(value(&settings, KEY_KITCHEN_KOT_PAPER_MM).unwrap_or("80"));
    let name = snapshot.branch.map(|b| b.name).unwrap_or_default();
    let tz = snapshot.locale.map(|l| l.tz).unwrap_or_default();
    let page = test_page(&name, &test_page_date(auth.clock().now_ms(), &tz), paper);
    let sent = match PrinterTarget::from_selected(&selected) {
        Ok(target) => dispatch(auth, printer.as_ref(), &target, &page).await,
        Err(e) => Err(e),
    };
    let (state, error_kind, error) = match sent {
        Ok(()) => (PrinterState::Ok, None, None),
        Err(e) => (
            PrinterState::Failed,
            Some(e.kind()),
            Some(printer_error_text(&e.to_string())),
        ),
    };
    Ok(PrinterStatusReply {
        outcome: PrinterOutcome::Recorded,
        status: Some(PrinterStatus {
            state,
            error_kind,
            error,
            at: Some(auth.clock().now_ms()),
        }),
    })
}

pub(crate) async fn test_kot(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    reply(run_kot_test(&auth, &headers).await)
}

pub(crate) async fn skip(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let result = match editor(&auth, &headers).await {
        Ok(me) => {
            let _single = auth.printer_job_lock().lock().await;
            record(&auth, &me, PrinterState::Skipped, None)
                .await
                .map(recorded)
        }
        Err(e) => Err(e),
    };
    reply(result)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use arena_core::printer::{
        BoxFuture, PaperWidth, PrinterError, PrinterTarget, test_page, test_page_date,
    };
    use arena_core::{SnapshotReader, StaffDirectory};
    use arena_proto::{
        Actor, ActorKind, IntentPayload, PrinterState, PrinterStatusSet, SettingsSet,
    };
    use arena_store::Store;
    use arena_tasks::printer::FakePrinter;
    use axum::Router;
    use axum::http::StatusCode;
    use serde_json::{Value, json};

    use crate::ui_auth::tests::{Hub, call_app, hub, intent_as};
    use crate::ui_auth::{UiAuth, ui_auth_router};

    const T0: i64 = 1_800_000_000_000;

    struct Rig {
        hub: Hub,
        app: Router,
    }

    fn rig_with(hub: Hub, printer: Arc<dyn arena_core::printer::Printer>) -> Rig {
        let auth = UiAuth::new(
            Arc::new(StaffDirectory::open(&hub.path).unwrap()),
            Arc::new(SnapshotReader::open(&hub.path).unwrap()),
            Some(hub.writer.clone()),
            hub.clock.clone(),
        )
        .with_printer(printer);
        Rig {
            app: ui_auth_router(Arc::new(auth)),
            hub,
        }
    }

    async fn rig(fake: &Arc<FakePrinter>) -> Rig {
        rig_with(
            hub().await,
            Arc::clone(fake) as Arc<dyn arena_core::printer::Printer>,
        )
    }

    async fn sign_in(r: &Rig, login: &str, password: &str) -> String {
        let (status, body) = call_app(
            &r.app,
            "POST",
            "/ui/auth/sign-in",
            None,
            Some(json!({"login": login, "password": password})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["token"].as_str().unwrap().to_owned()
    }

    async fn post(r: &Rig, path: &str, token: &str, body: Option<Value>) -> (StatusCode, Value) {
        call_app(&r.app, "POST", path, Some(token), body).await
    }

    fn setting(r: &Rig, key: &str) -> (String, i64) {
        Store::open_reader(&r.hub.path)
            .unwrap()
            .settings()
            .unwrap()
            .into_iter()
            .find(|s| s.key == key)
            .map(|s| (s.value, s.rev))
            .unwrap()
    }

    fn change_count(r: &Rig) -> usize {
        Store::open_reader(&r.hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .len()
    }

    async fn set(r: &Rig, token: &str, key: &str, value: &str) -> (StatusCode, Value) {
        let body = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "whoever".into() },
            IntentPayload::SettingsSet(SettingsSet { key: key.into(), value: value.into() }),
            Some(setting(r, key).1),
        ) });
        post(r, "/ui/intent", token, Some(body)).await
    }

    async fn select_lan(r: &Rig, token: &str, addr: &str) {
        let value = json!({"kind": "lan", "id_or_addr": addr, "name": addr}).to_string();
        let (status, body) = set(r, token, "printer.selected", &value).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    fn setup_printer(r: &Rig) -> Value {
        let s = SnapshotReader::open(&r.hub.path).unwrap().setup().unwrap();
        serde_json::to_value(s).unwrap()["printer"].clone()
    }

    #[tokio::test]
    async fn find_lists_the_fakes_usb_and_lan_printers() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        let (status, body) = post(&r, "/ui/printer/find", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            json!({"printers": [
                {"id": "fake-usb", "kind": "usb", "name": "Fake USB printer", "addr": null},
                {"id": "lan:192.168.1.250:9100", "kind": "lan", "name": "Fake network printer", "addr": "192.168.1.250:9100"}
            ], "bluetooth": null, "usb_supported": true})
        );
    }

    #[tokio::test]
    async fn a_manual_ip_is_stored_with_port_9100_and_a_bad_one_names_the_ip_field() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        select_lan(&r, &token, "192.168.1.50").await;
        assert_eq!(
            setting(&r, "printer.selected").0,
            r#"{"kind":"lan","id_or_addr":"192.168.1.50:9100","name":"192.168.1.50"}"#
        );
        let before = change_count(&r);
        let bad = json!({"kind": "lan", "id_or_addr": "abc", "name": "abc"}).to_string();
        let (status, body) = set(&r, &token, "printer.selected", &bad).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["code"], "VALIDATION_FAILED");
        assert_eq!(body["field"], "ip");
        assert_eq!(change_count(&r), before);
    }

    #[tokio::test]
    async fn a_test_print_at_80_mm_sends_the_page_and_records_ok() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        select_lan(&r, &token, "192.168.1.250:9100").await;
        let (status, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            json!({"outcome": "recorded", "status": {"state": "ok", "error_kind": null, "error": null, "at": T0}})
        );
        let sent = fake.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].target,
            PrinterTarget::Lan("192.168.1.250:9100".parse().unwrap())
        );
        let date = test_page_date(T0, "Asia/Kolkata");
        assert_eq!(
            sent[0].bytes,
            test_page("Dev Branch", &date, PaperWidth::Mm80)
        );
        assert!(
            sent[0]
                .bytes
                .windows(49)
                .any(|w| w == b"123456789012345678901234567890123456789012345678\n")
        );
        assert_eq!(setup_printer(&r), json!("ok"));
        let c = Store::open_reader(&r.hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(c.intent_kind, "printer.status");
        assert_eq!(c.actor_id, r.hub.manager_id);
    }

    #[tokio::test]
    async fn a_test_print_at_58_mm_uses_the_32_column_ruler() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        let usb = json!({"kind": "usb", "id_or_addr": "fake-usb", "name": "Fake USB printer"});
        let (status, body) = set(&r, &token, "printer.selected", &usb.to_string()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, body) = set(&r, &token, "printer.paper_mm", "58").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, _) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        let sent = fake.sent();
        assert_eq!(sent[0].target, PrinterTarget::Usb("fake-usb".into()));
        let date = test_page_date(T0, "Asia/Kolkata");
        assert_eq!(
            sent[0].bytes,
            test_page("Dev Branch", &date, PaperWidth::Mm58)
        );
        assert!(
            sent[0]
                .bytes
                .windows(37)
                .any(|w| w == b"\x1ba\x0012345678901234567890123456789012\n\x1b")
        );
    }

    #[tokio::test]
    async fn a_failed_print_records_the_adapter_error() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        select_lan(&r, &token, "192.168.1.250").await;
        let addr = "192.168.1.250:9100".parse().unwrap();
        fake.fail_with(Some(PrinterError::Timeout(
            addr,
            std::time::Duration::from_secs(5),
        )));
        let (status, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["outcome"], "recorded");
        assert_eq!(body["status"]["state"], "failed");
        assert_eq!(body["status"]["error_kind"], "timeout");
        assert_eq!(
            body["status"]["error"],
            "The printer at 192.168.1.250:9100 did not answer within 5 s"
        );
        assert_eq!(setup_printer(&r), json!("failed"));
        assert!(fake.sent().is_empty());
    }

    fn open_alert_kinds(r: &Rig) -> Vec<String> {
        Store::open_reader(&r.hub.path)
            .unwrap()
            .alerts()
            .unwrap()
            .into_iter()
            .filter(|a| a.acked_at.is_none())
            .map(|a| a.kind)
            .collect()
    }

    async fn select_bluetooth(r: &Rig, token: &str) {
        let value =
            json!({"kind": "bluetooth", "id_or_addr": "00:11:22:aa:bb:cc", "name": "RPP02N"})
                .to_string();
        let (status, body) = set(r, token, "printer.selected", &value).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            setting(r, "printer.selected").0,
            r#"{"kind":"bluetooth","id_or_addr":"00:11:22:AA:BB:CC","name":"RPP02N"}"#
        );
    }

    #[tokio::test]
    async fn find_reports_the_bluetooth_access_and_that_usb_is_not_supported_on_android() {
        let fake = Arc::new(FakePrinter::new(Vec::new()));
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        let (_, body) = post(&r, "/ui/printer/find", &token, None).await;
        assert_eq!(body["usb_supported"], true);
        assert_eq!(body["bluetooth"], Value::Null);
        fake.set_bluetooth(Some(arena_proto::BluetoothAccess::Denied));
        fake.set_usb_supported(false);
        let (status, body) = post(&r, "/ui/printer/find", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["bluetooth"], "denied");
        assert_eq!(body["usb_supported"], false);
        assert_eq!(body["printers"], json!([]));
    }

    #[tokio::test]
    async fn a_dropped_bluetooth_job_is_kept_as_a_failure_raises_one_alert_and_the_next_job_clears_it()
     {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        select_bluetooth(&r, &token).await;
        let (_, health) = call_app(&r.app, "GET", "/ui/printer/health", Some(&token), None).await;
        assert_eq!(health, json!({"last_job_failure": null}));
        fake.fail_with(Some(PrinterError::Bluetooth(
            "00:11:22:AA:BB:CC".into(),
            "the printer dropped the connection".into(),
        )));
        let (status, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"]["state"], "failed");
        assert_eq!(body["status"]["error_kind"], "bluetooth");
        assert_eq!(setup_printer(&r), json!("failed"));
        assert_eq!(open_alert_kinds(&r), ["printer_offline"]);
        let (_, health) = call_app(&r.app, "GET", "/ui/printer/health", Some(&token), None).await;
        assert_eq!(health["last_job_failure"]["error_kind"], "bluetooth");
        assert_eq!(
            health["last_job_failure"]["error"],
            "Could not print to the Bluetooth printer 00:11:22:AA:BB:CC: the printer dropped the connection"
        );
        // A second failure leaves the one open alert alone.
        let (status, _) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(open_alert_kinds(&r), ["printer_offline"]);
        // The next print opens a new connection and works.
        fake.fail_with(None);
        let (_, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(body["status"]["state"], "ok");
        assert_eq!(fake.sent().len(), 1);
        assert_eq!(
            fake.sent()[0].target,
            PrinterTarget::Bluetooth("00:11:22:AA:BB:CC".into())
        );
        let (_, health) = call_app(&r.app, "GET", "/ui/printer/health", Some(&token), None).await;
        assert_eq!(health, json!({"last_job_failure": null}));
    }

    #[tokio::test]
    async fn a_lan_failure_is_kept_but_raises_no_alert_and_a_counter_cannot_read_the_health() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        select_lan(&r, &token, "192.168.1.250").await;
        fake.fail_with(Some(PrinterError::Refused(
            "192.168.1.250:9100".parse().unwrap(),
        )));
        post(&r, "/ui/printer/test", &token, None).await;
        assert!(open_alert_kinds(&r).is_empty());
        let (_, health) = call_app(&r.app, "GET", "/ui/printer/health", Some(&token), None).await;
        assert_eq!(health["last_job_failure"]["error_kind"], "refused");
        let counter = sign_in(&r, "counter", "c-pass").await;
        let (status, _) = call_app(&r.app, "GET", "/ui/printer/health", Some(&counter), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn the_android_adapter_over_the_recorded_bluetooth_fake_prints_the_test_page_and_flags_a_drop()
     {
        use arena_tasks::printer::{
            AndroidPrinter, BluetoothFailure, BluetoothLink, FakeBluetooth, PairedPrinter,
        };
        let link = Arc::new(FakeBluetooth::new(vec![PairedPrinter {
            address: "00:11:22:AA:BB:CC".into(),
            name: "RPP02N".into(),
        }]));
        let r = rig_with(
            hub().await,
            Arc::new(AndroidPrinter::new(
                Arc::clone(&link) as Arc<dyn BluetoothLink>
            )),
        );
        let token = sign_in(&r, "manager", "m-pass").await;
        select_bluetooth(&r, &token).await;
        let (_, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(body["status"]["state"], "ok", "{body}");
        let page = test_page(
            "Dev Branch",
            &test_page_date(T0, "Asia/Kolkata"),
            PaperWidth::Mm80,
        );
        assert_eq!(link.sent(), vec![("00:11:22:AA:BB:CC".to_owned(), page)]);
        link.fail_with(Some(BluetoothFailure::Link(
            "the connection dropped while printing".into(),
        )));
        let (_, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(body["status"]["state"], "failed", "{body}");
        assert_eq!(body["status"]["error_kind"], "bluetooth");
        assert_eq!(open_alert_kinds(&r), ["printer_offline"]);
        link.fail_with(Some(BluetoothFailure::Permission));
        let (_, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(body["status"]["error_kind"], "bluetooth_permission");
    }

    #[tokio::test]
    async fn a_refused_bluetooth_permission_is_kept_as_a_failure_but_raises_no_offline_alert() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        select_bluetooth(&r, &token).await;
        fake.fail_with(Some(PrinterError::BluetoothPermission));
        let (_, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(body["status"]["error_kind"], "bluetooth_permission");
        assert!(open_alert_kinds(&r).is_empty());
    }

    struct Probe {
        delay: std::time::Duration,
        running: AtomicUsize,
        most: AtomicUsize,
        sent: AtomicUsize,
        before_ok: tokio::sync::Mutex<Option<BoxFuture<'static, ()>>>,
    }

    impl Probe {
        fn new(delay: std::time::Duration) -> Self {
            Self {
                delay,
                running: AtomicUsize::new(0),
                most: AtomicUsize::new(0),
                sent: AtomicUsize::new(0),
                before_ok: tokio::sync::Mutex::new(None),
            }
        }

        async fn busy(&self) {
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.running.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl arena_core::printer::Printer for Probe {
        fn discover(&self) -> BoxFuture<'_, Vec<arena_proto::FoundPrinter>> {
            Box::pin(async move {
                self.busy().await;
                Vec::new()
            })
        }

        fn send<'a>(
            &'a self,
            _target: &'a PrinterTarget,
            _bytes: &'a [u8],
        ) -> BoxFuture<'a, Result<(), PrinterError>> {
            Box::pin(async move {
                self.busy().await;
                if let Some(hook) = self.before_ok.lock().await.take() {
                    hook.await;
                }
                self.sent.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn parallel_finds_and_prints_run_one_at_a_time_and_each_gets_its_own_status() {
        let probe = Arc::new(Probe::new(std::time::Duration::from_millis(150)));
        let r = rig_with(
            hub().await,
            Arc::clone(&probe) as Arc<dyn arena_core::printer::Printer>,
        );
        let token = sign_in(&r, "manager", "m-pass").await;
        let (a, b) = tokio::join!(
            post(&r, "/ui/printer/find", &token, None),
            post(&r, "/ui/printer/find", &token, None)
        );
        assert_eq!((a.0, b.0), (StatusCode::OK, StatusCode::OK));
        assert_eq!(
            probe.most.load(Ordering::SeqCst),
            1,
            "finds waited for each other"
        );
        select_lan(&r, &token, "192.168.1.250").await;
        probe.most.store(0, Ordering::SeqCst);
        let (t, k) = tokio::join!(
            post(&r, "/ui/printer/test", &token, None),
            post(&r, "/ui/printer/skip", &token, None)
        );
        assert_eq!(
            (t.0, k.0),
            (StatusCode::OK, StatusCode::OK),
            "{} {}",
            t.1,
            k.1
        );
        assert_eq!(t.1["status"]["state"], "ok");
        assert_eq!(k.1["status"]["state"], "skipped");
        let (x, y) = tokio::join!(
            post(&r, "/ui/printer/test", &token, None),
            post(&r, "/ui/printer/test", &token, None)
        );
        assert_eq!(
            (
                x.1["status"]["state"].clone(),
                y.1["status"]["state"].clone()
            ),
            (json!("ok"), json!("ok"))
        );
        assert_eq!(
            probe.most.load(Ordering::SeqCst),
            1,
            "test prints waited for each other"
        );
        assert_eq!(probe.sent.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_page_that_printed_but_was_not_recorded_says_so() {
        let probe = Arc::new(Probe::new(std::time::Duration::ZERO));
        let r = rig_with(
            hub().await,
            Arc::clone(&probe) as Arc<dyn arena_core::printer::Printer>,
        );
        let token = sign_in(&r, "manager", "m-pass").await;
        select_lan(&r, &token, "192.168.1.250").await;
        let writer = r.hub.writer.clone();
        let staff_rev = Store::open_reader(&r.hub.path)
            .unwrap()
            .rows(arena_store::Table::Staff)
            .unwrap()
            .into_iter()
            .find(|row| row["staff_id"] == arena_store::RowValue::Text(r.hub.manager_id.clone()))
            .map(|row| match row["rev"] {
                arena_store::RowValue::Integer(n) => n,
                _ => unreachable!(),
            })
            .unwrap();
        let deactivate = intent_as(
            Actor {
                kind: ActorKind::Owner,
                id: "owner".into(),
            },
            IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                staff_id: Some(r.hub.manager_id.clone()),
                login: "manager".into(),
                display_name: "Manager".into(),
                role_id: r.hub.manager_role.clone(),
                password_hash: None,
                pin_hash: None,
                active: false,
                setup_code_hash: None,
                setup_expires_at: None,
            }),
            Some(staff_rev),
        );
        *probe.before_ok.lock().await = Some(Box::pin(async move {
            writer.submit_intent(deactivate).await.unwrap();
        }));
        let before = setting(&r, "printer.status").0;
        let (status, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            json!({"outcome": "printed_not_recorded", "status": null})
        );
        assert_eq!(probe.sent.load(Ordering::SeqCst), 1);
        assert_eq!(setting(&r, "printer.status").0, before);
    }

    #[tokio::test]
    async fn a_follower_refuses_a_test_print_and_prints_nothing() {
        let fake = Arc::new(FakePrinter::dev());
        let hub = hub().await;
        let manager = crate::ui_auth::tests::token_for(&hub, "manager", "m-pass").await;
        let (status, body) = call_app(
            &hub.app,
            "POST",
            "/ui/intent",
            Some(&manager),
            Some(json!({ "intent": intent_as(
                Actor { kind: ActorKind::Staff, id: "whoever".into() },
                IntentPayload::SettingsSet(SettingsSet {
                    key: "printer.selected".into(),
                    value: r#"{"kind":"usb","id_or_addr":"fake-usb","name":"Fake USB printer"}"#.into(),
                }),
                Some(1),
            ) })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let auth = UiAuth::new(
            Arc::new(StaffDirectory::open(&hub.path).unwrap()),
            Arc::new(SnapshotReader::open(&hub.path).unwrap()),
            None,
            hub.clock.clone(),
        )
        .with_printer(Arc::clone(&fake) as Arc<dyn arena_core::printer::Printer>);
        let r = Rig {
            app: ui_auth_router(Arc::new(auth)),
            hub,
        };
        let token = sign_in(&r, "manager", "m-pass").await;
        let before = setting(&r, "printer.status");
        let changes = change_count(&r);
        for path in ["/ui/printer/test", "/ui/printer/skip"] {
            let (status, body) = post(&r, path, &token, None).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}: {body}");
            assert_eq!(body["code"], "NOT_PRIMARY", "{path}");
        }
        assert!(fake.sent().is_empty());
        assert_eq!(setting(&r, "printer.status"), before);
        assert_eq!(change_count(&r), changes);
    }

    #[tokio::test]
    async fn skip_records_skipped_and_the_setup_status_shows_it() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        assert_eq!(setup_printer(&r), json!("unset"));
        let (status, body) = post(&r, "/ui/printer/skip", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"]["state"], "skipped");
        assert_eq!(setup_printer(&r), json!("skipped"));
        let snapshot = SnapshotReader::open(&r.hub.path)
            .unwrap()
            .snapshot(T0)
            .unwrap();
        assert_eq!(snapshot.setup.unwrap().printer, PrinterState::Skipped);
    }

    #[tokio::test]
    async fn a_test_print_without_a_printer_is_refused_and_records_nothing() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        let before = change_count(&r);
        let (status, body) = post(&r, "/ui/printer/test", &token, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["field"], "printer.selected");
        assert_eq!(change_count(&r), before);
        assert!(fake.sent().is_empty());
    }

    #[tokio::test]
    async fn a_counter_is_denied_every_printer_action() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let manager = sign_in(&r, "manager", "m-pass").await;
        select_lan(&r, &manager, "192.168.1.250").await;
        let token = sign_in(&r, "counter", "c-pass").await;
        let before = change_count(&r);
        for path in ["/ui/printer/find", "/ui/printer/test", "/ui/printer/skip"] {
            let (status, body) = post(&r, path, &token, None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
            assert_eq!(body["code"], "PERMISSION_DENIED", "{path}");
        }
        for (key, value) in [("printer.paper_mm", "58"), ("printer.selected", "")] {
            let (status, body) = set(&r, &token, key, value).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{key}: {body}");
        }
        assert_eq!(change_count(&r), before);
        assert!(fake.sent().is_empty());
        let (status, _) = call_app(&r.app, "POST", "/ui/printer/find", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_kot_printer_is_locked_on_free_and_prints_a_test_page_with_the_flag() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        let before = change_count(&r);
        let (status, body) = set(&r, &token, "kitchen.kot_mode", "none").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["code"], "TIER_LOCKED");
        let (status, body) = post(&r, "/ui/printer/test-kot", &token, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["code"], "TIER_LOCKED");
        assert_eq!(change_count(&r), before);
        assert!(fake.sent().is_empty());

        Store::open(&r.hub.path, T0)
            .unwrap()
            .write_tx(|tx| {
                tx.execute_batch(
                    "UPDATE settings SET value = 'kitchen' WHERE key = 'licence.flags'",
                )
                .map_err(arena_store::db_error)
            })
            .unwrap();
        let (status, body) = post(&r, "/ui/printer/test-kot", &token, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["field"], "kitchen.kot_printer");
        let value =
            json!({"kind": "lan", "id_or_addr": "192.168.1.251", "name": "Pass"}).to_string();
        let (status, body) = set(&r, &token, "kitchen.kot_printer", &value).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, body) = set(&r, &token, "kitchen.kot_paper_mm", "58").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let after_sets = change_count(&r);
        let (status, body) = post(&r, "/ui/printer/test-kot", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"]["state"], "ok");
        assert_eq!(change_count(&r), after_sets);
        let sent = fake.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].target,
            PrinterTarget::Lan("192.168.1.251:9100".parse().unwrap())
        );
        let date = test_page_date(T0, "Asia/Kolkata");
        assert_eq!(
            sent[0].bytes,
            test_page("Dev Branch", &date, PaperWidth::Mm58)
        );
        let counter = sign_in(&r, "counter", "c-pass").await;
        let (status, _) = post(&r, "/ui/printer/test-kot", &counter, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_dropped_bluetooth_kot_test_goes_through_the_same_dispatch() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        Store::open(&r.hub.path, T0)
            .unwrap()
            .write_tx(|tx| {
                tx.execute_batch(
                    "UPDATE settings SET value = 'kitchen' WHERE key = 'licence.flags'",
                )
                .map_err(arena_store::db_error)
            })
            .unwrap();
        let value = json!({"kind": "bluetooth", "id_or_addr": "00:11:22:AA:BB:CC", "name": "KOT"})
            .to_string();
        let (status, body) = set(&r, &token, "kitchen.kot_printer", &value).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        fake.fail_with(Some(PrinterError::Bluetooth(
            "00:11:22:AA:BB:CC".into(),
            "the printer dropped the connection".into(),
        )));
        let (status, body) = post(&r, "/ui/printer/test-kot", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"]["state"], "failed");
        assert_eq!(open_alert_kinds(&r), ["printer_offline"]);
        let (_, health) = call_app(&r.app, "GET", "/ui/printer/health", Some(&token), None).await;
        assert_eq!(health["last_job_failure"]["error_kind"], "bluetooth");
        fake.fail_with(None);
        let (_, body) = post(&r, "/ui/printer/test-kot", &token, None).await;
        assert_eq!(body["status"]["state"], "ok");
        let (_, health) = call_app(&r.app, "GET", "/ui/printer/health", Some(&token), None).await;
        assert_eq!(health, json!({"last_job_failure": null}));
    }

    #[tokio::test]
    async fn the_status_is_never_written_raw() {
        let fake = Arc::new(FakePrinter::dev());
        let r = rig(&fake).await;
        let token = sign_in(&r, "manager", "m-pass").await;
        let before = change_count(&r);
        let raw = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "whoever".into() },
            IntentPayload::PrinterStatus(PrinterStatusSet { state: PrinterState::Ok, error_kind: None, error: None }),
            None,
        ) });
        let (status, body) = post(&r, "/ui/intent", &token, Some(raw)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "UNSUPPORTED");
        let (status, body) = set(
            &r,
            &token,
            "printer.status",
            r#"{"state":"ok","error":null,"at":1}"#,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["field"], "key");
        assert_eq!(change_count(&r), before);
    }

    #[tokio::test]
    async fn a_hub_without_a_printer_adapter_says_so() {
        let hub = hub().await;
        let token = crate::ui_auth::tests::token_for(&hub, "manager", "m-pass").await;
        let (status, body) =
            call_app(&hub.app, "POST", "/ui/printer/find", Some(&token), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "UNSUPPORTED");
    }
}
