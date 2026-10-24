mod fake;
mod lan;
mod spooler;

pub use arena_core::printer::{
    BoxFuture, PaperWidth, Printer, PrinterError, PrinterTarget, test_page, test_page_date,
};
use arena_proto::FoundPrinter;
pub use fake::{FakePrinter, SentJob};
pub use lan::{
    LanScanner, SCAN_CONNECT_TIMEOUT, SCAN_DEADLINE, SCAN_MAX_CONCURRENT, SEND_TIMEOUT, Subnet,
    send_lan,
};
pub use spooler::{RAW_DATATYPE, RawDocInfo, TEST_DOC_NAME};

pub const USB_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Default)]
pub struct SystemPrinter {
    lan: LanScanner,
}

impl SystemPrinter {
    pub fn new() -> Self {
        Self::default()
    }
}

async fn usb_printers() -> Vec<FoundPrinter> {
    match tokio::task::spawn_blocking(spooler::local_printers).await {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!(detail = %e, "printer enumeration task failed");
            Vec::new()
        }
    }
}

impl Printer for SystemPrinter {
    fn discover(&self) -> BoxFuture<'_, Vec<FoundPrinter>> {
        Box::pin(async move {
            let (mut usb, lan) = tokio::join!(usb_printers(), self.lan.scan());
            usb.extend(lan);
            usb
        })
    }

    fn send<'a>(
        &'a self,
        target: &'a PrinterTarget,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<(), PrinterError>> {
        Box::pin(async move {
            match target {
                PrinterTarget::Lan(addr) => send_lan(*addr, bytes, SEND_TIMEOUT).await,
                PrinterTarget::Usb(name) => {
                    let task_name = name.clone();
                    let bytes = bytes.to_vec();
                    let job =
                        tokio::task::spawn_blocking(move || spooler::print_raw(&task_name, &bytes));
                    match tokio::time::timeout(USB_SEND_TIMEOUT, job).await {
                        Ok(joined) => joined
                            .map_err(|e| PrinterError::Spooler(name.clone(), e.to_string()))?,
                        Err(_) => {
                            tracing::warn!(
                                printer = %name,
                                "the spooler did not finish within 10 s; its thread is abandoned"
                            );
                            Err(PrinterError::Spooler(
                                name.clone(),
                                "the print queue did not answer within 10 s".to_owned(),
                            ))
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[tokio::test]
    async fn other_systems_find_no_usb_printer_and_cannot_print_to_one() {
        assert!(usb_printers().await.is_empty());
        let p = SystemPrinter::new();
        assert_eq!(
            p.send(&PrinterTarget::Usb("X".into()), b"x").await,
            Err(PrinterError::UsbUnavailable)
        );
    }

    #[tokio::test]
    async fn the_system_printer_sends_a_test_page_to_a_lan_target() {
        use tokio::io::AsyncReadExt;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let got = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).await.unwrap();
            buf
        });
        let page = test_page("Dev Branch", "5 Oct 2026", PaperWidth::Mm58);
        let target = PrinterTarget::Lan(format!("127.0.0.1:{port}").parse().unwrap());
        SystemPrinter::new().send(&target, &page).await.unwrap();
        assert_eq!(got.await.unwrap(), page);
    }
}
