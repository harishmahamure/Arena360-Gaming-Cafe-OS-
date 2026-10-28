mod escpos;
pub mod job;
pub mod receipt;

use std::future::Future;
use std::net::SocketAddrV4;
use std::pin::Pin;
use std::time::Duration;

use arena_proto::{
    BluetoothAccess, FoundPrinter, PrinterErrorKind, PrinterKind, PrinterSelected,
    parse_bluetooth_address, parse_printer_addr,
};

pub use escpos::{pc437_text, ruler, test_page, test_page_date};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What the Hub's printer jobs share: one job at a time to the printer, and the last job that
/// failed (cleared by the next job that works). The print task and the Settings pages use the same
/// value, so a receipt that failed shows as "printer offline" on the settle bar.
#[derive(Default)]
pub struct PrinterJobs {
    lock: tokio::sync::Mutex<()>,
    failure: std::sync::Mutex<Option<arena_proto::PrinterJobFailure>>,
}

impl PrinterJobs {
    /// Held for the length of a job.
    pub fn lock(&self) -> &tokio::sync::Mutex<()> {
        &self.lock
    }

    pub fn last_failure(&self) -> Option<arena_proto::PrinterJobFailure> {
        self.failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Records the result of a job: a failure is kept, a success clears it.
    pub fn record(&self, failure: Option<arena_proto::PrinterJobFailure>) {
        *self.failure.lock().unwrap_or_else(|e| e.into_inner()) = failure;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperWidth {
    Mm58,
    Mm80,
}

impl PaperWidth {
    pub fn from_setting(value: &str) -> Self {
        if value == "58" {
            Self::Mm58
        } else {
            Self::Mm80
        }
    }

    pub fn columns(self) -> usize {
        match self {
            Self::Mm58 => 32,
            Self::Mm80 => 48,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrinterTarget {
    Usb(String),
    Lan(SocketAddrV4),
    /// A paired Bluetooth printer by device address (Android Hub).
    Bluetooth(String),
}

impl PrinterTarget {
    pub fn from_selected(selected: &PrinterSelected) -> Result<Self, PrinterError> {
        match selected.kind {
            PrinterKind::Usb => Ok(Self::Usb(selected.id_or_addr.clone())),
            PrinterKind::Bluetooth => parse_bluetooth_address(&selected.id_or_addr)
                .map(Self::Bluetooth)
                .ok_or_else(|| PrinterError::BadAddress(selected.id_or_addr.clone())),
            PrinterKind::Lan => parse_printer_addr(&selected.id_or_addr)
                .map(Self::Lan)
                .ok_or_else(|| PrinterError::BadAddress(selected.id_or_addr.clone())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrinterError {
    Refused(SocketAddrV4),
    Timeout(SocketAddrV4, Duration),
    Network(SocketAddrV4, String),
    BadAddress(String),
    Spooler(String, String),
    UsbUnavailable,
    /// The Bluetooth printer (by address) could not be reached or dropped the job.
    Bluetooth(String, String),
    /// The Bluetooth permission is off.
    BluetoothPermission,
    /// This Hub has no Bluetooth printing (a Windows Hub) or the device has no Bluetooth.
    BluetoothUnavailable,
}

impl std::fmt::Display for PrinterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(addr) => write!(f, "The printer at {addr} refused the connection"),
            Self::Timeout(addr, after) => write!(
                f,
                "The printer at {addr} did not answer within {} s",
                after.as_secs().max(1)
            ),
            Self::Network(addr, detail) => {
                write!(f, "Could not reach the printer at {addr}: {detail}")
            }
            Self::BadAddress(addr) => write!(f, "{addr:?} is not a printer address"),
            Self::Spooler(name, detail) => {
                write!(f, "Windows could not print to {name}: {detail}")
            }
            Self::UsbUnavailable => write!(f, "USB printing works only on a Windows Hub"),
            Self::Bluetooth(addr, detail) => {
                write!(
                    f,
                    "Could not print to the Bluetooth printer {addr}: {detail}"
                )
            }
            Self::BluetoothPermission => write!(f, "The Bluetooth permission is off"),
            Self::BluetoothUnavailable => {
                write!(f, "Bluetooth printing is not available on this Hub")
            }
        }
    }
}

impl std::error::Error for PrinterError {}

impl PrinterError {
    pub fn kind(&self) -> PrinterErrorKind {
        match self {
            Self::Refused(_) => PrinterErrorKind::Refused,
            Self::Timeout(..) => PrinterErrorKind::Timeout,
            Self::Network(..) => PrinterErrorKind::Unreachable,
            Self::Spooler(..) | Self::UsbUnavailable => PrinterErrorKind::Spooler,
            Self::Bluetooth(..) | Self::BluetoothUnavailable => PrinterErrorKind::Bluetooth,
            Self::BluetoothPermission => PrinterErrorKind::BluetoothPermission,
            Self::BadAddress(_) => PrinterErrorKind::Other,
        }
    }
}

pub trait Printer: Send + Sync + 'static {
    fn discover(&self) -> BoxFuture<'_, Vec<FoundPrinter>>;

    fn send<'a>(
        &'a self,
        target: &'a PrinterTarget,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<(), PrinterError>>;

    /// What this Hub may do with Bluetooth; `None` when it has no Bluetooth printing.
    fn bluetooth_access(&self) -> BoxFuture<'_, Option<BluetoothAccess>> {
        Box::pin(async { None })
    }

    /// False on a Hub that cannot print to a USB printer (Android).
    fn usb_supported(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_and_targets() {
        assert_eq!(PaperWidth::from_setting("58").columns(), 32);
        assert_eq!(PaperWidth::from_setting("80").columns(), 48);
        assert_eq!(PaperWidth::from_setting("x"), PaperWidth::Mm80);
        let lan = PrinterSelected {
            kind: PrinterKind::Lan,
            id_or_addr: "192.168.1.50".into(),
            name: "P".into(),
        };
        assert_eq!(
            PrinterTarget::from_selected(&lan),
            Ok(PrinterTarget::Lan("192.168.1.50:9100".parse().unwrap()))
        );
        let usb = PrinterSelected {
            kind: PrinterKind::Usb,
            id_or_addr: "EPSON TM-T82".into(),
            name: "EPSON TM-T82".into(),
        };
        assert_eq!(
            PrinterTarget::from_selected(&usb),
            Ok(PrinterTarget::Usb("EPSON TM-T82".into()))
        );
        let bt = PrinterSelected {
            kind: PrinterKind::Bluetooth,
            id_or_addr: "00:11:22:aa:bb:cc".into(),
            name: "RPP02N".into(),
        };
        assert_eq!(
            PrinterTarget::from_selected(&bt),
            Ok(PrinterTarget::Bluetooth("00:11:22:AA:BB:CC".into()))
        );
        let bad_bt = PrinterSelected {
            id_or_addr: "printer".into(),
            ..bt
        };
        assert!(matches!(
            PrinterTarget::from_selected(&bad_bt),
            Err(PrinterError::BadAddress(_))
        ));
        let bad = PrinterSelected {
            id_or_addr: "abc".into(),
            ..lan
        };
        assert!(matches!(
            PrinterTarget::from_selected(&bad),
            Err(PrinterError::BadAddress(_))
        ));
    }

    #[test]
    fn errors_read_as_one_plain_line() {
        let addr: SocketAddrV4 = "10.0.0.9:9100".parse().unwrap();
        assert_eq!(
            PrinterError::Refused(addr).to_string(),
            "The printer at 10.0.0.9:9100 refused the connection"
        );
        assert_eq!(
            PrinterError::Timeout(addr, Duration::from_secs(5)).to_string(),
            "The printer at 10.0.0.9:9100 did not answer within 5 s"
        );
        assert_eq!(
            PrinterError::UsbUnavailable.to_string(),
            "USB printing works only on a Windows Hub"
        );
        assert_eq!(
            PrinterError::Refused(addr).kind(),
            PrinterErrorKind::Refused
        );
        assert_eq!(
            PrinterError::Timeout(addr, Duration::from_secs(5)).kind(),
            PrinterErrorKind::Timeout
        );
        assert_eq!(
            PrinterError::Network(addr, "x".into()).kind(),
            PrinterErrorKind::Unreachable
        );
        assert_eq!(
            PrinterError::Spooler("P".into(), "x".into()).kind(),
            PrinterErrorKind::Spooler
        );
        assert_eq!(
            PrinterError::UsbUnavailable.kind(),
            PrinterErrorKind::Spooler
        );
        assert_eq!(
            PrinterError::BadAddress("x".into()).kind(),
            PrinterErrorKind::Other
        );
        let dropped =
            PrinterError::Bluetooth("00:11:22:AA:BB:CC".into(), "the printer dropped".into());
        assert_eq!(
            dropped.to_string(),
            "Could not print to the Bluetooth printer 00:11:22:AA:BB:CC: the printer dropped"
        );
        assert_eq!(dropped.kind(), PrinterErrorKind::Bluetooth);
        assert_eq!(
            PrinterError::BluetoothUnavailable.kind(),
            PrinterErrorKind::Bluetooth
        );
        assert_eq!(
            PrinterError::BluetoothPermission.kind(),
            PrinterErrorKind::BluetoothPermission
        );
    }
}
