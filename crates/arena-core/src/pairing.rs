use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arena_proto::UiError;
use tokio::sync::watch;

pub type PairFuture<'a> = Pin<Box<dyn Future<Output = Result<(), UiError>> + Send + 'a>>;

pub trait HubPairer: Send + Sync {
    fn pair<'a>(&'a self, code: &'a str) -> PairFuture<'a>;
}

#[derive(Clone)]
pub struct PairGate {
    paired: Arc<watch::Sender<bool>>,
    /// The Hub has a branch but lost the key that wrapped its secrets (Android Keystore reset):
    /// it is unpaired until a new code replaces the token, and it must never mint a new branch
    /// key meanwhile.
    needs_repair: Arc<AtomicBool>,
}

impl PairGate {
    pub fn new(paired: bool) -> Self {
        Self {
            paired: Arc::new(watch::Sender::new(paired)),
            needs_repair: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Unpaired because the secrets' key is lost, not because the Hub is new.
    pub fn needing_repair() -> Self {
        let gate = Self::new(false);
        gate.needs_repair.store(true, Ordering::SeqCst);
        gate
    }

    pub fn needs_repair(&self) -> bool {
        self.needs_repair.load(Ordering::SeqCst)
    }

    pub fn is_paired(&self) -> bool {
        *self.paired.borrow()
    }

    pub fn mark_paired(&self) {
        self.needs_repair.store(false, Ordering::SeqCst);
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
            .field("needs_repair", &self.needs_repair())
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

    #[test]
    fn a_gate_that_lost_its_key_is_unpaired_until_a_new_pairing() {
        let gate = PairGate::needing_repair();
        assert!(!gate.is_paired() && gate.needs_repair());
        assert!(!PairGate::new(false).needs_repair());
        let other = gate.clone();
        gate.mark_paired();
        assert!(other.is_paired() && !other.needs_repair());
    }
}
