use std::sync::Mutex;

use arena_proto::{FoundPrinter, PrinterKind};

use super::{BoxFuture, Printer, PrinterError, PrinterTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentJob {
    pub target: PrinterTarget,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Default)]
struct FakeState {
    found: Vec<FoundPrinter>,
    failure: Option<PrinterError>,
    sent: Vec<SentJob>,
}

#[derive(Debug, Default)]
pub struct FakePrinter {
    state: Mutex<FakeState>,
}

impl FakePrinter {
    pub fn new(found: Vec<FoundPrinter>) -> Self {
        Self {
            state: Mutex::new(FakeState {
                found,
                ..FakeState::default()
            }),
        }
    }

    pub fn dev() -> Self {
        Self::new(vec![
            FoundPrinter {
                id: "fake-usb".into(),
                kind: PrinterKind::Usb,
                name: "Fake USB printer".into(),
                addr: None,
            },
            FoundPrinter {
                id: "lan:192.168.1.250:9100".into(),
                kind: PrinterKind::Lan,
                name: "Fake network printer".into(),
                addr: Some("192.168.1.250:9100".into()),
            },
        ])
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut FakeState) -> T) -> T {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut state)
    }

    pub fn set_found(&self, found: Vec<FoundPrinter>) {
        self.with_state(|s| s.found = found);
    }

    pub fn fail_with(&self, failure: Option<PrinterError>) {
        self.with_state(|s| s.failure = failure);
    }

    pub fn sent(&self) -> Vec<SentJob> {
        self.with_state(|s| s.sent.clone())
    }
}

impl Printer for FakePrinter {
    fn discover(&self) -> BoxFuture<'_, Vec<FoundPrinter>> {
        let found = self.with_state(|s| s.found.clone());
        Box::pin(async move { found })
    }

    fn send<'a>(
        &'a self,
        target: &'a PrinterTarget,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<(), PrinterError>> {
        let outcome = self.with_state(|s| match &s.failure {
            Some(e) => Err(e.clone()),
            None => {
                s.sent.push(SentJob {
                    target: target.clone(),
                    bytes: bytes.to_vec(),
                });
                Ok(())
            }
        });
        Box::pin(async move { outcome })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_fake_lists_its_script_and_records_what_it_prints() {
        let fake = FakePrinter::dev();
        let found = fake.discover().await;
        assert_eq!(
            found.iter().map(|p| p.kind).collect::<Vec<_>>(),
            [PrinterKind::Usb, PrinterKind::Lan]
        );
        let target = PrinterTarget::Usb("fake-usb".into());
        fake.send(&target, b"abc").await.unwrap();
        assert_eq!(
            fake.sent(),
            vec![SentJob {
                target: target.clone(),
                bytes: b"abc".to_vec()
            }]
        );
        fake.set_found(Vec::new());
        assert!(fake.discover().await.is_empty());
    }

    #[tokio::test]
    async fn a_scripted_failure_is_returned_and_nothing_is_recorded() {
        let fake = FakePrinter::new(Vec::new());
        let addr = "10.0.0.9:9100".parse().unwrap();
        fake.fail_with(Some(PrinterError::Refused(addr)));
        let target = PrinterTarget::Lan(addr);
        assert_eq!(
            fake.send(&target, b"x").await,
            Err(PrinterError::Refused(addr))
        );
        assert!(fake.sent().is_empty());
        fake.fail_with(None);
        fake.send(&target, b"x").await.unwrap();
        assert_eq!(fake.sent().len(), 1);
    }
}
