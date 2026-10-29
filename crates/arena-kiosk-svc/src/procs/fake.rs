//! An in-memory process table playing all three platform parts, for tests (and any host with
//! no process control: it simply holds no processes).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{
    EventSink, FileStamp, Inspect, PlayerSession, ProcEntry, ProcFacts, ProcStart, ProcessKiller,
    ProcessWatcher, Signature, SignatureVerifier,
};

/// One fake process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeProc {
    pub session_id: u32,
    pub user_sid: String,
    pub path: String,
    /// The process that started it.
    pub parent: Option<u32>,
}

#[derive(Default)]
struct State {
    procs: BTreeMap<u32, FakeProc>,
    player: Option<(String, PlayerSession)>,
    signatures: HashMap<String, Signature>,
    stamps: HashMap<String, FileStamp>,
    killed: Vec<u32>,
    verifies: usize,
    verify_delay: Option<Duration>,
    fail_subscribe: bool,
    unreadable: HashMap<u32, (Option<u32>, &'static str)>,
    fail_kill: HashSet<u32>,
    kill_attempts: usize,
    suspended: HashSet<u32>,
    fail_suspend: HashSet<u32>,
    visible: HashSet<u32>,
    sink: Option<EventSink>,
}

/// The fake system.
#[derive(Default)]
pub struct FakeSystem(Mutex<State>);

impl FakeSystem {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `account` (SID `sid`) is logged on in `session_id`.
    pub fn set_player(&self, account: &str, sid: &str, session_id: u32) {
        self.state().player = Some((
            account.to_owned(),
            PlayerSession {
                sid: sid.to_owned(),
                session_id,
            },
        ));
    }

    /// Makes the next `subscribe` fail (the watch is unavailable).
    pub fn fail_subscribe(&self) {
        self.state().fail_subscribe = true;
    }

    /// Adds a process that was already running (no creation event).
    pub fn add_running(&self, pid: u32, p: FakeProc) {
        self.state().procs.insert(pid, p);
    }

    /// Starts a process: adds it and sends its creation event.
    pub fn spawn(&self, pid: u32, p: FakeProc) {
        let mut s = self.state();
        let parent_pid = p.parent;
        s.procs.insert(pid, p);
        if let Some(sink) = &s.sink {
            sink.send(ProcStart {
                pid,
                parent_pid,
                at: Instant::now(),
            });
        }
    }

    /// Ends the creation watch (as a crashed WMI subscription would).
    pub fn end_watch(&self) {
        self.state().sink = None;
    }

    /// Whether the creation watch is running.
    pub fn subscribed(&self) -> bool {
        self.state().sink.as_ref().is_some_and(|t| !t.is_closed())
    }

    /// `path` carries a valid signature by `signer` (`valid: false` for a broken one).
    pub fn sign(&self, path: &str, signer: &str, valid: bool) {
        self.state().signatures.insert(
            path.to_lowercase(),
            Signature {
                signer: Some(signer.to_owned()),
                valid,
            },
        );
    }

    /// Sets `path`'s size and modification time (a changed stamp re-verifies).
    pub fn stamp_file(&self, path: &str, size: u64, mtime: i64) {
        self.state()
            .stamps
            .insert(path.to_lowercase(), FileStamp { size, mtime });
    }

    /// Makes every signature check take `d` (to overlap concurrent checks).
    pub fn slow_verify(&self, d: Duration) {
        self.state().verify_delay = Some(d);
    }

    /// `pid`'s details cannot be read (`reason`), with its session known or not.
    pub fn make_unreadable(&self, pid: u32, session_id: Option<u32>, reason: &'static str) {
        self.state().unreadable.insert(pid, (session_id, reason));
    }

    /// Every kill of `pid` fails with access denied.
    pub fn fail_kill(&self, pid: u32) {
        self.state().fail_kill.insert(pid);
    }

    /// `pid` owns a visible top-level window.
    pub fn show_window(&self, pid: u32) {
        self.state().visible.insert(pid);
    }

    /// The pids killed so far, in order.
    pub fn killed(&self) -> Vec<u32> {
        self.state().killed.clone()
    }

    /// Every suspend of `pid` fails with access denied.
    pub fn fail_suspend(&self, pid: u32) {
        self.state().fail_suspend.insert(pid);
    }

    /// The pids suspended now, ascending.
    pub fn suspended(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.state().suspended.iter().copied().collect();
        v.sort_unstable();
        v
    }

    /// How many kills were attempted (including failed ones).
    pub fn kill_attempts(&self) -> usize {
        self.state().kill_attempts
    }

    /// Whether `pid` still runs.
    pub fn alive(&self, pid: u32) -> bool {
        self.state().procs.contains_key(&pid)
    }

    /// How many signature checks ran (cache misses).
    pub fn verifies(&self) -> usize {
        self.state().verifies
    }
}

impl ProcessWatcher for FakeSystem {
    fn subscribe(&self, sink: EventSink) -> io::Result<()> {
        let mut s = self.state();
        if std::mem::take(&mut s.fail_subscribe) {
            return Err(io::Error::other("fake: the watch is unavailable"));
        }
        s.sink = Some(sink);
        Ok(())
    }

    fn running(&self) -> io::Result<Vec<ProcEntry>> {
        Ok(self
            .state()
            .procs
            .iter()
            .map(|(pid, p)| ProcEntry {
                pid: *pid,
                parent_pid: p.parent,
            })
            .collect())
    }

    fn player_session(&self, account: &str) -> Option<PlayerSession> {
        self.state()
            .player
            .as_ref()
            .filter(|(a, _)| a.eq_ignore_ascii_case(account))
            .map(|(_, p)| p.clone())
    }

    fn inspect(&self, pid: u32) -> Inspect {
        let s = self.state();
        if let Some((session_id, reason)) = s.unreadable.get(&pid) {
            return Inspect::Unreadable {
                session_id: *session_id,
                reason,
            };
        }
        match s.procs.get(&pid) {
            Some(p) => Inspect::Facts(ProcFacts {
                pid,
                session_id: p.session_id,
                user_sid: p.user_sid.clone(),
                path: p.path.clone(),
            }),
            None => Inspect::Gone,
        }
    }

    fn has_visible_window(&self, pid: u32) -> bool {
        self.state().visible.contains(&pid)
    }
}

impl SignatureVerifier for FakeSystem {
    fn stamp(&self, path: &str) -> Option<FileStamp> {
        let s = self.state();
        Some(
            s.stamps
                .get(&path.to_lowercase())
                .copied()
                .unwrap_or(FileStamp { size: 1, mtime: 1 }),
        )
    }

    fn verify(&self, path: &str) -> Signature {
        let delay = {
            let mut s = self.state();
            s.verifies += 1;
            s.verify_delay
        };
        if let Some(d) = delay {
            std::thread::sleep(d);
        }
        self.state()
            .signatures
            .get(&path.to_lowercase())
            .cloned()
            .unwrap_or_default()
    }
}

impl ProcessKiller for FakeSystem {
    fn kill(&self, facts: &ProcFacts) -> io::Result<()> {
        let mut s = self.state();
        s.kill_attempts += 1;
        if s.fail_kill.contains(&facts.pid) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fake: access denied",
            ));
        }
        let same = s.procs.get(&facts.pid).is_some_and(|p| {
            p.path == facts.path && p.user_sid == facts.user_sid && p.session_id == facts.session_id
        });
        if !same {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "fake: not the process that was inspected",
            ));
        }
        s.procs.remove(&facts.pid);
        s.suspended.remove(&facts.pid);
        s.killed.push(facts.pid);
        Ok(())
    }

    fn set_suspended(&self, facts: &ProcFacts, suspended: bool) -> io::Result<()> {
        let mut s = self.state();
        if s.fail_suspend.contains(&facts.pid) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fake: access denied",
            ));
        }
        let same = s.procs.get(&facts.pid).is_some_and(|p| {
            p.path == facts.path && p.user_sid == facts.user_sid && p.session_id == facts.session_id
        });
        if !same {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "fake: not the process that was inspected",
            ));
        }
        if suspended {
            s.suspended.insert(facts.pid);
        } else {
            s.suspended.remove(&facts.pid);
        }
        Ok(())
    }
}
