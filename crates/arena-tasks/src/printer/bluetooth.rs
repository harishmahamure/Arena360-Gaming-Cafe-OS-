//! Bluetooth printing on the Android Hub.
//!
//! The Android shell supplies a [`BluetoothLink`] backed by a Kotlin Bluetooth SPP bridge (JNI,
//! like the Keystore and `NsdManager` bridges). [`AndroidPrinter`] puts it behind the same
//! [`Printer`] trait as the Windows adapter, next to the LAN scanner and sender, so the rest of
//! the Hub prints without knowing which transport carries the bytes. [`FakeBluetooth`] is the
//! recorded fake the bridge's CI tests run against.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_proto::{BluetoothAccess, FoundPrinter, PrinterKind};

use super::{BoxFuture, LanScanner, Printer, PrinterError, PrinterTarget, SEND_TIMEOUT, send_lan};

/// A job to a printer that is off or out of range must fail within this long.
pub const BLUETOOTH_SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// A printer already paired in Android settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedPrinter {
    /// The device address, e.g. `00:11:22:AA:BB:CC`.
    pub address: String,
    pub name: String,
}

/// Why the bridge could not list or print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BluetoothFailure {
    /// The Bluetooth permission is off.
    Permission,
    /// Bluetooth is switched off, or the device has none.
    Unavailable,
    /// The printer is off, out of range, not paired, or dropped the connection (plain text).
    Link(String),
}

/// What the Android shell offers. The calls block, so the adapter runs them off the async runtime.
pub trait BluetoothLink: Send + Sync + 'static {
    fn access(&self) -> BluetoothAccess;

    fn paired(&self) -> Result<Vec<PairedPrinter>, BluetoothFailure>;

    /// Connects (a fresh connection every time, so the next print retries after a drop), writes
    /// all of `bytes` and closes. Gives up by `timeout`.
    fn send(&self, address: &str, bytes: &[u8], timeout: Duration) -> Result<(), BluetoothFailure>;
}

impl BluetoothFailure {
    /// The answer of the Kotlin bridge's `send`: `permission`, `unavailable` or `link:<reason>`.
    pub fn from_bridge(answer: &str) -> Self {
        match answer {
            "permission" => Self::Permission,
            "unavailable" => Self::Unavailable,
            other => Self::Link(
                other
                    .strip_prefix("link:")
                    .filter(|reason| !reason.trim().is_empty())
                    .unwrap_or("the printer did not answer")
                    .to_owned(),
            ),
        }
    }
}

/// The bridge's `access` code: 0 granted, 1 denied, 2 off, anything else unavailable.
pub fn access_from_bridge(code: i32) -> BluetoothAccess {
    match code {
        0 => BluetoothAccess::Granted,
        1 => BluetoothAccess::Denied,
        2 => BluetoothAccess::Off,
        _ => BluetoothAccess::Unavailable,
    }
}

fn printer_error(address: &str, e: BluetoothFailure) -> PrinterError {
    match e {
        BluetoothFailure::Permission => PrinterError::BluetoothPermission,
        BluetoothFailure::Unavailable => PrinterError::BluetoothUnavailable,
        BluetoothFailure::Link(detail) => PrinterError::Bluetooth(address.to_owned(), detail),
    }
}

/// Bluetooth and LAN printing; USB is not supported on Android.
pub struct AndroidPrinter {
    link: Arc<dyn BluetoothLink>,
    lan: LanScanner,
    send_timeout: Duration,
}

impl AndroidPrinter {
    pub fn new(link: Arc<dyn BluetoothLink>) -> Self {
        Self {
            link,
            lan: LanScanner::default(),
            send_timeout: BLUETOOTH_SEND_TIMEOUT,
        }
    }

    #[must_use]
    pub fn with_send_timeout(mut self, timeout: Duration) -> Self {
        self.send_timeout = timeout;
        self
    }

    async fn bluetooth_printers(&self) -> Vec<FoundPrinter> {
        let link = Arc::clone(&self.link);
        let listed = tokio::task::spawn_blocking(move || {
            if link.access() != BluetoothAccess::Granted {
                return Vec::new();
            }
            link.paired().unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        listed
            .into_iter()
            .map(|p| FoundPrinter {
                id: format!("bt:{}", p.address),
                kind: PrinterKind::Bluetooth,
                name: p.name,
                addr: Some(p.address),
            })
            .collect()
    }

    async fn send_bluetooth(&self, address: &str, bytes: &[u8]) -> Result<(), PrinterError> {
        let link = Arc::clone(&self.link);
        let (to, data, timeout) = (address.to_owned(), bytes.to_vec(), self.send_timeout);
        let job = tokio::task::spawn_blocking(move || link.send(&to, &data, timeout));
        // The bridge honours `timeout`; this is the backstop so a stuck call never blocks the Hub.
        match tokio::time::timeout(self.send_timeout + Duration::from_secs(1), job).await {
            Ok(Ok(result)) => result.map_err(|e| printer_error(address, e)),
            Ok(Err(e)) => Err(PrinterError::Bluetooth(address.to_owned(), e.to_string())),
            Err(_) => {
                tracing::warn!(
                    printer = address,
                    "the Bluetooth bridge did not finish in time; its thread is abandoned"
                );
                Err(PrinterError::Bluetooth(
                    address.to_owned(),
                    format!(
                        "the printer did not answer within {} s",
                        self.send_timeout.as_secs().max(1)
                    ),
                ))
            }
        }
    }
}

impl Printer for AndroidPrinter {
    fn discover(&self) -> BoxFuture<'_, Vec<FoundPrinter>> {
        Box::pin(async move {
            let (mut found, lan) = tokio::join!(self.bluetooth_printers(), self.lan.scan());
            found.extend(lan);
            found
        })
    }

    fn send<'a>(
        &'a self,
        target: &'a PrinterTarget,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<(), PrinterError>> {
        Box::pin(async move {
            match target {
                PrinterTarget::Bluetooth(address) => self.send_bluetooth(address, bytes).await,
                PrinterTarget::Lan(addr) => send_lan(*addr, bytes, SEND_TIMEOUT).await,
                PrinterTarget::Usb(_) => Err(PrinterError::UsbUnavailable),
            }
        })
    }

    fn bluetooth_access(&self) -> BoxFuture<'_, Option<BluetoothAccess>> {
        let link = Arc::clone(&self.link);
        Box::pin(async move {
            Some(
                tokio::task::spawn_blocking(move || link.access())
                    .await
                    .unwrap_or(BluetoothAccess::Unavailable),
            )
        })
    }

    fn usb_supported(&self) -> bool {
        false
    }
}

#[derive(Debug)]
struct FakeLinkState {
    access: BluetoothAccess,
    paired: Vec<PairedPrinter>,
    failure: Option<BluetoothFailure>,
    delay: Duration,
    sent: Vec<(String, Vec<u8>)>,
}

/// A recorded fake for tests and dev: scripted access, paired list and failure; it remembers what
/// was sent. A scripted delay makes a send block, to exercise the timeout.
#[derive(Debug)]
pub struct FakeBluetooth {
    state: Mutex<FakeLinkState>,
}

impl FakeBluetooth {
    pub fn new(paired: Vec<PairedPrinter>) -> Self {
        Self {
            state: Mutex::new(FakeLinkState {
                access: BluetoothAccess::Granted,
                paired,
                failure: None,
                delay: Duration::ZERO,
                sent: Vec::new(),
            }),
        }
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut FakeLinkState) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()))
    }

    pub fn set_access(&self, access: BluetoothAccess) {
        self.with_state(|s| s.access = access);
    }

    pub fn fail_with(&self, failure: Option<BluetoothFailure>) {
        self.with_state(|s| s.failure = failure);
    }

    pub fn delay_sends(&self, delay: Duration) {
        self.with_state(|s| s.delay = delay);
    }

    pub fn sent(&self) -> Vec<(String, Vec<u8>)> {
        self.with_state(|s| s.sent.clone())
    }
}

impl BluetoothLink for FakeBluetooth {
    fn access(&self) -> BluetoothAccess {
        self.with_state(|s| s.access)
    }

    fn paired(&self) -> Result<Vec<PairedPrinter>, BluetoothFailure> {
        self.with_state(|s| match s.access {
            BluetoothAccess::Granted => Ok(s.paired.clone()),
            BluetoothAccess::Denied => Err(BluetoothFailure::Permission),
            BluetoothAccess::Off | BluetoothAccess::Unavailable => {
                Err(BluetoothFailure::Unavailable)
            }
        })
    }

    fn send(&self, address: &str, bytes: &[u8], timeout: Duration) -> Result<(), BluetoothFailure> {
        let delay = self.with_state(|s| s.delay);
        if !delay.is_zero() {
            std::thread::sleep(delay);
            if delay >= timeout {
                return Err(BluetoothFailure::Link("the printer did not answer".into()));
            }
        }
        self.with_state(|s| {
            if let Some(failure) = &s.failure {
                return Err(failure.clone());
            }
            s.sent.push((address.to_owned(), bytes.to_vec()));
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDR: &str = "00:11:22:AA:BB:CC";

    fn rig() -> (Arc<FakeBluetooth>, AndroidPrinter) {
        let link = Arc::new(FakeBluetooth::new(vec![PairedPrinter {
            address: ADDR.into(),
            name: "RPP02N".into(),
        }]));
        let printer = AndroidPrinter::new(Arc::clone(&link) as Arc<dyn BluetoothLink>);
        (link, printer)
    }

    #[test]
    fn the_bridge_answers_map_to_failures_and_access() {
        assert_eq!(
            BluetoothFailure::from_bridge("permission"),
            BluetoothFailure::Permission
        );
        assert_eq!(
            BluetoothFailure::from_bridge("unavailable"),
            BluetoothFailure::Unavailable
        );
        assert_eq!(
            BluetoothFailure::from_bridge("link:the connection dropped while printing"),
            BluetoothFailure::Link("the connection dropped while printing".into())
        );
        for odd in ["link:", "link:  ", "boom"] {
            assert_eq!(
                BluetoothFailure::from_bridge(odd),
                BluetoothFailure::Link("the printer did not answer".into()),
                "{odd:?}"
            );
        }
        let codes: Vec<_> = (0..5).map(access_from_bridge).collect();
        assert_eq!(
            codes,
            [
                BluetoothAccess::Granted,
                BluetoothAccess::Denied,
                BluetoothAccess::Off,
                BluetoothAccess::Unavailable,
                BluetoothAccess::Unavailable
            ]
        );
    }

    #[tokio::test]
    async fn paired_printers_are_listed_as_bluetooth_and_usb_is_not_supported() {
        let (_, printer) = rig();
        let found: Vec<_> = printer
            .bluetooth_printers()
            .await
            .into_iter()
            .map(|p| (p.id, p.kind, p.name, p.addr))
            .collect();
        assert_eq!(
            found,
            vec![(
                format!("bt:{ADDR}"),
                PrinterKind::Bluetooth,
                "RPP02N".to_owned(),
                Some(ADDR.to_owned())
            )]
        );
        assert!(!printer.usb_supported());
        assert_eq!(
            printer.send(&PrinterTarget::Usb("X".into()), b"x").await,
            Err(PrinterError::UsbUnavailable)
        );
    }

    #[tokio::test]
    async fn without_the_permission_no_bluetooth_printer_is_listed_and_the_access_says_why() {
        let (link, printer) = rig();
        assert_eq!(
            printer.bluetooth_access().await,
            Some(BluetoothAccess::Granted)
        );
        link.set_access(BluetoothAccess::Denied);
        assert!(printer.bluetooth_printers().await.is_empty());
        assert_eq!(
            printer.bluetooth_access().await,
            Some(BluetoothAccess::Denied)
        );
        link.set_access(BluetoothAccess::Off);
        assert_eq!(printer.bluetooth_access().await, Some(BluetoothAccess::Off));
    }

    #[tokio::test]
    async fn a_page_goes_to_the_paired_printer_and_is_recorded() {
        let (link, printer) = rig();
        let target = PrinterTarget::Bluetooth(ADDR.into());
        printer.send(&target, b"page").await.unwrap();
        assert_eq!(link.sent(), vec![(ADDR.to_owned(), b"page".to_vec())]);
    }

    #[tokio::test]
    async fn a_dropped_link_is_a_bluetooth_error_and_the_next_print_connects_again() {
        let (link, printer) = rig();
        let target = PrinterTarget::Bluetooth(ADDR.into());
        link.fail_with(Some(BluetoothFailure::Link(
            "the printer dropped the connection".into(),
        )));
        assert_eq!(
            printer.send(&target, b"a").await,
            Err(PrinterError::Bluetooth(
                ADDR.into(),
                "the printer dropped the connection".into()
            ))
        );
        assert!(link.sent().is_empty());
        link.fail_with(None);
        printer.send(&target, b"b").await.unwrap();
        assert_eq!(link.sent().len(), 1);
    }

    #[tokio::test]
    async fn a_refused_permission_or_a_switched_off_radio_maps_to_its_own_error() {
        let (link, printer) = rig();
        let target = PrinterTarget::Bluetooth(ADDR.into());
        link.fail_with(Some(BluetoothFailure::Permission));
        assert_eq!(
            printer.send(&target, b"a").await,
            Err(PrinterError::BluetoothPermission)
        );
        link.fail_with(Some(BluetoothFailure::Unavailable));
        assert_eq!(
            printer.send(&target, b"a").await,
            Err(PrinterError::BluetoothUnavailable)
        );
    }

    #[tokio::test]
    async fn a_printer_that_is_off_fails_within_the_timeout_and_does_not_block_the_hub() {
        let (link, printer) = rig();
        let printer = printer.with_send_timeout(Duration::from_millis(200));
        link.delay_sends(Duration::from_millis(300));
        let target = PrinterTarget::Bluetooth(ADDR.into());
        let started = std::time::Instant::now();
        let err = printer.send(&target, b"x").await.unwrap_err();
        assert!(matches!(err, PrinterError::Bluetooth(..)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
        // Another job is not held up by the stuck one.
        link.delay_sends(Duration::ZERO);
        printer.send(&target, b"y").await.unwrap();
    }

    #[tokio::test]
    async fn the_backstop_ends_a_bridge_call_that_ignores_its_timeout() {
        struct Stuck;
        impl BluetoothLink for Stuck {
            fn access(&self) -> BluetoothAccess {
                BluetoothAccess::Granted
            }
            fn paired(&self) -> Result<Vec<PairedPrinter>, BluetoothFailure> {
                Ok(Vec::new())
            }
            fn send(&self, _: &str, _: &[u8], _: Duration) -> Result<(), BluetoothFailure> {
                std::thread::sleep(Duration::from_millis(2500));
                Ok(())
            }
        }
        let printer =
            AndroidPrinter::new(Arc::new(Stuck)).with_send_timeout(Duration::from_millis(100));
        let started = std::time::Instant::now();
        let err = printer
            .send(&PrinterTarget::Bluetooth(ADDR.into()), b"x")
            .await
            .unwrap_err();
        assert!(matches!(err, PrinterError::Bluetooth(..)), "{err:?}");
        assert!(started.elapsed() < Duration::from_millis(2000));
    }
}
