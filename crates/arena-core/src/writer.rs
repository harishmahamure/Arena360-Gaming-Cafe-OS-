//! The Writer actor: the only owner of the branch's write connection.
//!
//! Each submitted job runs in one `BEGIN IMMEDIATE` transaction on a dedicated thread. Once the
//! writer is closed (demotion or shutdown), every `submit` fails `NOT_PRIMARY`. A job that
//! panics is rolled back and fails `INTERNAL`; the thread keeps running. If the thread exits
//! for any other reason, the writer counts as closed and `submit` fails `INTERNAL`. Story 1.7
//! replaces the closure job API with `apply()`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arena_proto::{AppError, ErrorCode};
use arena_store::{Store, Transaction};
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce(&mut Store) + Send>;

enum Msg {
    Job(Job),
    Close(oneshot::Sender<()>),
}

const QUEUE_DEPTH: usize = 256;

fn not_primary() -> AppError {
    AppError::new(ErrorCode::NotPrimary, "this hub is not the primary writer")
}

fn writer_gone() -> AppError {
    AppError::new(ErrorCode::Internal, "the writer thread stopped")
}

#[derive(Default)]
struct Flags {
    /// Set by `close()`.
    closed: AtomicBool,
    /// Set when the writer thread exits, however it exits.
    dead: AtomicBool,
}

/// Marks the writer dead when the thread ends (normal exit or unwinding).
struct DeadOnDrop(Arc<Flags>);

impl Drop for DeadOnDrop {
    fn drop(&mut self) {
        self.0.dead.store(true, Ordering::SeqCst);
    }
}

/// Spawns the writer actor.
pub struct Writer;

impl Writer {
    /// Moves `store` (the write connection) onto the writer thread and returns its handle.
    pub fn spawn(store: Store) -> Result<WriterHandle, AppError> {
        let (tx, mut rx) = mpsc::channel::<Msg>(QUEUE_DEPTH);
        let flags = Arc::new(Flags::default());
        let actor_flags = Arc::clone(&flags);
        std::thread::Builder::new()
            .name("arena-writer".into())
            .spawn(move || {
                let _dead = DeadOnDrop(Arc::clone(&actor_flags));
                let mut store = Some(store);
                while let Some(msg) = rx.blocking_recv() {
                    match msg {
                        Msg::Job(job) => {
                            // A job queued before close() is dropped unrun: its caller gets
                            // NOT_PRIMARY.
                            if actor_flags.closed.load(Ordering::SeqCst) {
                                continue;
                            }
                            if let Some(s) = store.as_mut() {
                                job(s);
                            }
                        }
                        Msg::Close(ack) => {
                            actor_flags.closed.store(true, Ordering::SeqCst);
                            drop(store.take());
                            rx.close();
                            let _ = ack.send(());
                        }
                    }
                }
            })
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("spawn writer thread: {e}")))?;
        Ok(WriterHandle { tx, flags })
    }
}

/// A cloneable handle for submitting write jobs.
#[derive(Clone)]
pub struct WriterHandle {
    tx: mpsc::Sender<Msg>,
    flags: Arc<Flags>,
}

impl std::fmt::Debug for WriterHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterHandle")
            .field("closed", &self.is_closed())
            .field("alive", &self.is_alive())
            .finish()
    }
}

impl WriterHandle {
    /// Runs `job` in one `BEGIN IMMEDIATE` transaction (committed on `Ok`, rolled back on
    /// `Err` or panic). Fails `NOT_PRIMARY` once the writer is closed, `INTERNAL` if the job
    /// panicked or the writer thread is gone.
    pub async fn submit<R, F>(&self, job: F) -> Result<R, AppError>
    where
        F: FnOnce(&Transaction<'_>) -> Result<R, AppError> + Send + 'static,
        R: Send + 'static,
    {
        if self.closed_by_close() {
            return Err(not_primary());
        }
        if !self.is_alive() {
            return Err(writer_gone());
        }
        let (reply, result) = oneshot::channel();
        let job: Job = Box::new(move |store: &mut Store| {
            // Unwinding drops the transaction, which rolls it back.
            let out = catch_unwind(AssertUnwindSafe(|| store.write_tx(job)))
                .unwrap_or_else(|_| Err(AppError::new(ErrorCode::Internal, "write job panicked")));
            let _ = reply.send(out);
        });
        if self.tx.send(Msg::Job(job)).await.is_err() {
            return Err(self.channel_error());
        }
        match result.await {
            Ok(out) => out,
            Err(_) => Err(self.channel_error()),
        }
    }

    fn channel_error(&self) -> AppError {
        if self.closed_by_close() {
            not_primary()
        } else {
            writer_gone()
        }
    }

    fn closed_by_close(&self) -> bool {
        self.flags.closed.load(Ordering::SeqCst)
    }

    /// Whether the writer thread is still running.
    pub fn is_alive(&self) -> bool {
        !self.flags.dead.load(Ordering::SeqCst)
    }

    /// Whether the writer has been closed (by `close()` or because its thread exited).
    pub fn is_closed(&self) -> bool {
        self.closed_by_close() || !self.is_alive()
    }

    /// Closes the writer: no further job runs, and the write connection (and the process
    /// writer lock) are released before this returns.
    pub async fn close(&self) {
        self.flags.closed.store(true, Ordering::SeqCst);
        let (ack, done) = oneshot::channel();
        if self.tx.send(Msg::Close(ack)).await.is_ok() {
            let _ = done.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_store::{get_meta, set_meta};
    use std::path::PathBuf;
    use std::time::Duration;

    fn open() -> (tempfile::TempDir, PathBuf, Store) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, 0).unwrap();
        (dir, path, store)
    }

    #[tokio::test]
    async fn runs_jobs_and_commits() {
        let (_dir, _path, store) = open();
        let w = Writer::spawn(store).unwrap();
        w.submit(|tx| set_meta(tx, "k", "v")).await.unwrap();
        let v = w.submit(|tx| get_meta(tx, "k")).await.unwrap();
        assert_eq!(v.as_deref(), Some("v"));
    }

    #[tokio::test]
    async fn failed_job_rolls_back() {
        let (_dir, _path, store) = open();
        let w = Writer::spawn(store).unwrap();
        let err = w
            .submit(|tx| {
                set_meta(tx, "k", "v")?;
                Err::<(), _>(AppError::new(ErrorCode::Stale, "nope"))
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Stale);
        assert_eq!(w.submit(|tx| get_meta(tx, "k")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn panicking_job_rolls_back_and_writer_survives() {
        let (_dir, _path, store) = open();
        let w = Writer::spawn(store).unwrap();
        let err = w
            .submit(|tx| -> Result<(), AppError> {
                set_meta(tx, "k", "v")?;
                panic!("boom");
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(w.is_alive() && !w.is_closed());
        assert_eq!(w.submit(|tx| get_meta(tx, "k")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn submit_after_close_is_not_primary() {
        let (_dir, _path, store) = open();
        let w = Writer::spawn(store).unwrap();
        let other = w.clone();
        w.close().await;
        assert!(other.is_closed());
        let err = other.submit(|tx| set_meta(tx, "k", "v")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotPrimary);
        // Closing twice is harmless.
        w.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn job_queued_before_close_is_not_run() {
        let (_dir, path, store) = open();
        let w = Writer::spawn(store).unwrap();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (started_tx, started_rx) = oneshot::channel::<()>();

        let blocker = tokio::spawn({
            let w = w.clone();
            async move {
                w.submit(move |_| {
                    let _ = started_tx.send(());
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .await
            }
        });
        started_rx.await.unwrap();

        let queued = tokio::spawn({
            let w = w.clone();
            async move { w.submit(|tx| set_meta(tx, "queued", "yes")).await }
        });
        // Let the second job reach the queue behind the blocked one.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let closer = tokio::spawn({
            let w = w.clone();
            async move { w.close().await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        release_tx.send(()).unwrap();

        blocker.await.unwrap().unwrap();
        let err = queued.await.unwrap().unwrap_err();
        assert_eq!(err.code, ErrorCode::NotPrimary);
        closer.await.unwrap();
        let reread = Store::open(&path, 0).unwrap();
        assert_eq!(reread.meta("queued").unwrap(), None);
    }
}
