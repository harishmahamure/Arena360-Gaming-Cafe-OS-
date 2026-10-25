use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use arena_proto::UiError;
use tokio::sync::watch;

pub type PairFuture<'a> = Pin<Box<dyn Future<Output = Result<(), UiError>> + Send + 'a>>;

pub trait HubPairer: Send + Sync {
    fn pair<'a>(&'a self, code: &'a str) -> PairFuture<'a>;
}

#[derive(Clone)]
pub struct PairGate {
    paired: Arc<watch::Sender<bool>>,
}

impl PairGate {
    pub fn new(paired: bool) -> Self {
        Self {
            paired: Arc::new(watch::Sender::new(paired)),
        }
    }

    pub fn is_paired(&self) -> bool {
        *self.paired.borrow()
    }

    pub fn mark_paired(&self) {
        self.paired.send_replace(true);
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.paired.subscribe()
    }
}

impl std::fmt::Debug for PairGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairGate")
            .field("paired", &self.is_paired())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_opens_once_and_every_clone_sees_it() {
        let gate = PairGate::new(false);
        let other = gate.clone();
        let mut rx = gate.subscribe();
        assert!(!other.is_paired());
        gate.mark_paired();
        assert!(other.is_paired());
        assert!(rx.has_changed().unwrap());
        assert!(*rx.borrow_and_update());
        assert!(PairGate::new(true).is_paired());
    }
}
