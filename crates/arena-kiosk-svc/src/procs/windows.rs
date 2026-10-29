//! The Windows process control behind the enforcer (LocalSystem service only).
//!
//! - **Creation events:** a WMI `Win32_ProcessStartTrace` subscription (COM, `ROOT\CIMV2`,
//!   semi-synchronous `ExecNotificationQuery` read on a dedicated MTA thread with a 500 ms
//!   `Next` timeout). The event carries the pid and parent pid; everything else is read from
//!   the process. Events go into the enforcer's bounded [`EventSink`].
//! - **Listing:** a Toolhelp32 process snapshot (pid and parent pid).
//! - **Player session:** the active console session, when its user token's SID is the player
//!   account's (`WTSGetActiveConsoleSessionId` + `WTSQueryUserToken`).
//! - **Facts:** `ProcessIdToSessionId`, the token user (`OpenProcessToken` + `TokenUser`) and the
//!   image path (`QueryFullProcessImageNameW`), all through a `PROCESS_QUERY_LIMITED_INFORMATION`
//!   handle. A process that refuses any of them is left alone; one that no longer exists is
//!   `Gone`.
//! - **Windows:** the service runs in session 0 and cannot enumerate the player desktop's
//!   windows, so `has_visible_window` keeps the trait's default (no); the toast falls back to
//!   the "started just now by the player's session" rule.
//! - **Signatures:** `WinVerifyTrust` (`WINTRUST_ACTION_GENERIC_VERIFY_V2`, no UI, no
//!   revocation or network retrieval) on the embedded signature; with none, the system
//!   catalogs (`CryptCATAdmin*`, SHA-256 then SHA-1) and `WinVerifyTrust` on the catalog member.
//!   The signer CN comes from the verified state (`WTHelperProvDataFromStateData` →
//!   `WTHelperGetProvSignerFromChain` → `CertGetNameStringW(CN)`).
//! - **Kill:** `TerminateProcess` on a handle opened with `PROCESS_TERMINATE |
//!   PROCESS_QUERY_LIMITED_INFORMATION`, after re-reading the session, token user and path on
//!   that same handle (the handle pins the process, so a reused pid is never hit).

use std::collections::HashMap;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::sync::Mutex;
use std::time::Instant;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, HANDLE, HWND, INVALID_HANDLE_VALUE, TRUST_E_NOSIGNATURE,
};
use windows::Win32::Security::Cryptography::Catalog::{
    CATALOG_INFO, CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2,
    CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
    CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext,
};
use windows::Win32::Security::Cryptography::{
    CERT_NAME_ATTR_TYPE, CertGetNameStringW, szOID_COMMON_NAME,
};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_CATALOG_INFO, WINTRUST_DATA, WINTRUST_DATA_0,
    WINTRUST_DATA_PROVIDER_FLAGS, WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL,
    WTD_CHOICE_CATALOG, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain,
    WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoInitializeSecurity, CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::RemoteDesktop::{
    ProcessIdToSessionId, WTSGetActiveConsoleSessionId, WTSQueryUserToken,
};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SUSPEND_RESUME, PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::System::Variant::{VARIANT, VariantClear, VariantToUInt32};
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_S_TIMEDOUT, WbemLocator,
};
use windows::core::{BSTR, PCWSTR, PWSTR, w};

use super::{
    EventSink, FileStamp, Inspect, PlayerSession, ProcEntry, ProcFacts, ProcStart, ProcessKiller,
    ProcessWatcher, Signature, SignatureVerifier,
};
use crate::winsec::{sid_string, user_sid, wide};

/// The exit code a killed process gets.
const KILLED_EXIT_CODE: u32 = 1;
/// How long one WMI `Next` waits before checking whether the watch is still wanted.
const NEXT_TIMEOUT_MS: i32 = 500;
/// `RPC_C_AUTHN_WINNT` / `RPC_C_AUTHZ_NONE` (not worth the `Win32_System_Rpc` feature).
const RPC_C_AUTHN_WINNT: u32 = 10;
const RPC_C_AUTHZ_NONE: u32 = 0;

// Both are exported by ntdll and take a process handle opened with `PROCESS_SUSPEND_RESUME`;
// each returns an `NTSTATUS` (negative: failed).
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSuspendProcess(process: HANDLE) -> i32;
    fn NtResumeProcess(process: HANDLE) -> i32;
}

/// A kernel handle closed on drop.
struct Owned(HANDLE);
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: owned handle.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn win(e: windows::core::Error) -> io::Error {
    io::Error::other(e.to_string())
}

/// The real thing.
#[derive(Default)]
pub struct WinProcesses {
    /// Account name (lowercase) → SID.
    sids: Mutex<HashMap<String, String>>,
}

impl WinProcesses {
    pub fn new() -> Self {
        Self::default()
    }

    fn account_sid(&self, account: &str) -> Option<String> {
        let key = account.to_lowercase();
        if let Some(s) = self.sids.lock().ok()?.get(&key) {
            return Some(s.clone());
        }
        let sid = user_sid(account).ok()?;
        if let Ok(mut m) = self.sids.lock() {
            m.insert(key, sid.clone());
        }
        Some(sid)
    }
}

/// The token user SID of `token`.
fn token_user_sid(token: HANDLE) -> Option<String> {
    // u64 storage keeps TOKEN_USER aligned.
    let mut buf = vec![0u64; 64];
    let mut len = 0u32;
    // SAFETY: `buf` is writable for its byte length; the SID points into it.
    unsafe {
        if GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            (buf.len() * 8) as u32,
            &mut len,
        )
        .is_err()
        {
            if len as usize <= buf.len() * 8 {
                return None;
            }
            buf = vec![0u64; (len as usize).div_ceil(8)];
            GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast()),
                (buf.len() * 8) as u32,
                &mut len,
            )
            .ok()?;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        sid_string(user.User.Sid).ok()
    }
}

/// The token user and image path of the process behind `process`, or which one could not be
/// read (`token`, `path`).
fn user_and_path(process: HANDLE) -> Result<(String, String), &'static str> {
    // SAFETY: plain queries on a live handle; the token is closed by its guard.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(|_| "token")?;
        let token = Owned(token);
        let sid = token_user_sid(token.0).ok_or("token")?;
        let mut buf = vec![0u16; 32_768];
        let mut len = buf.len() as u32;
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .map_err(|_| "path")?;
        Ok((sid, String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// Whether `e` says the pid names no process (any more).
fn is_gone(e: &windows::core::Error) -> bool {
    e.code() == ERROR_INVALID_PARAMETER.to_hresult()
}

fn session_of(pid: u32) -> Option<u32> {
    let mut session = 0u32;
    // SAFETY: a plain query.
    unsafe { ProcessIdToSessionId(pid, &mut session) }.ok()?;
    Some(session)
}

/// The WMI watch: runs on its own thread until `tx` closes or WMI fails. `ready` gets the
/// setup result.
fn wmi_watch(tx: EventSink, ready: std::sync::mpsc::Sender<io::Result<()>>) {
    // SAFETY: COM is initialised for this thread and uninitialised at the end; every COM
    // object is dropped before that.
    unsafe {
        if let Err(e) = CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            let _ = ready.send(Err(win(e)));
            return;
        }
        // Process-wide; fails harmlessly (RPC_E_TOO_LATE) when already set.
        let _ = CoInitializeSecurity(
            None,
            -1,
            None,
            None,
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
            None,
        );
        let setup = || -> windows::core::Result<IEnumWbemClassObject> {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let services = locator.ConnectServer(
                &BSTR::from(r"ROOT\CIMV2"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )?;
            CoSetProxyBlanket(
                &services,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )?;
            services.ExecNotificationQuery(
                &BSTR::from("WQL"),
                &BSTR::from("SELECT ProcessID, ParentProcessID FROM Win32_ProcessStartTrace"),
                WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_RETURN_IMMEDIATELY.0 | WBEM_FLAG_FORWARD_ONLY.0),
                None,
            )
        };
        match setup() {
            Ok(events) => {
                let _ = ready.send(Ok(()));
                while !tx.is_closed() {
                    let mut objs: [Option<IWbemClassObject>; 1] = [None];
                    let mut n = 0u32;
                    let hr = events.Next(NEXT_TIMEOUT_MS, &mut objs, &mut n);
                    let at = Instant::now();
                    if hr.0 == WBEM_S_TIMEDOUT.0 {
                        continue;
                    }
                    if hr.is_err() {
                        tracing::error!(hresult = hr.0, "the WMI process watch failed");
                        break;
                    }
                    let Some(obj) = objs[0].take().filter(|_| n == 1) else {
                        continue;
                    };
                    let read = |name: PCWSTR| {
                        let mut v = VARIANT::default();
                        let got = obj
                            .Get(name, 0, &mut v, None, None)
                            .ok()
                            .and_then(|()| VariantToUInt32(&v).ok());
                        let _ = VariantClear(&mut v);
                        got
                    };
                    if let Some(pid) = read(w!("ProcessID")) {
                        let parent_pid = read(w!("ParentProcessID"));
                        if !tx.send(ProcStart {
                            pid,
                            parent_pid,
                            at,
                        }) {
                            break;
                        }
                    }
                }
                drop(events);
            }
            Err(e) => {
                let _ = ready.send(Err(win(e)));
            }
        }
        CoUninitialize();
    }
}

impl ProcessWatcher for WinProcesses {
    fn subscribe(&self, tx: EventSink) -> io::Result<()> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("process-watch".into())
            .spawn(move || wmi_watch(tx, ready_tx))?;
        ready_rx
            .recv()
            .unwrap_or_else(|_| Err(io::Error::other("the WMI watch thread ended")))
    }

    fn running(&self) -> io::Result<Vec<ProcEntry>> {
        // SAFETY: a snapshot walked with a correctly sized entry, then closed.
        unsafe {
            let snap = Owned(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(win)?);
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut pids = Vec::new();
            if Process32FirstW(snap.0, &mut entry).is_ok() {
                loop {
                    pids.push(ProcEntry {
                        pid: entry.th32ProcessID,
                        parent_pid: Some(entry.th32ParentProcessID),
                    });
                    if Process32NextW(snap.0, &mut entry).is_err() {
                        break;
                    }
                }
            }
            Ok(pids)
        }
    }

    fn player_session(&self, account: &str) -> Option<PlayerSession> {
        let sid = self.account_sid(account)?;
        // SAFETY: plain Win32 calls; the token is closed by its guard.
        unsafe {
            let session = WTSGetActiveConsoleSessionId();
            if session == u32::MAX {
                return None;
            }
            let mut token = HANDLE::default();
            WTSQueryUserToken(session, &mut token).ok()?;
            let token = Owned(token);
            let user = token_user_sid(token.0)?;
            user.eq_ignore_ascii_case(&sid).then_some(PlayerSession {
                sid,
                session_id: session,
            })
        }
    }

    fn inspect(&self, pid: u32) -> Inspect {
        // SAFETY: the handle is closed by its guard.
        let process = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            Ok(h) => Owned(h),
            Err(e) if is_gone(&e) => return Inspect::Gone,
            Err(_) => {
                return Inspect::Unreadable {
                    session_id: session_of(pid),
                    reason: "open",
                };
            }
        };
        let Some(session_id) = session_of(pid) else {
            return Inspect::Unreadable {
                session_id: None,
                reason: "session",
            };
        };
        match user_and_path(process.0) {
            Ok((user_sid, path)) => Inspect::Facts(ProcFacts {
                pid,
                session_id,
                user_sid,
                path,
            }),
            Err(reason) => Inspect::Unreadable {
                session_id: Some(session_id),
                reason,
            },
        }
    }
}

impl ProcessKiller for WinProcesses {
    fn kill(&self, facts: &ProcFacts) -> io::Result<()> {
        // SAFETY: the handle is closed by its guard; it pins the process while it is open.
        unsafe {
            let process = Owned(
                OpenProcess(
                    PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    facts.pid,
                )
                .map_err(|e| {
                    if is_gone(&e) {
                        io::Error::new(io::ErrorKind::NotFound, e.to_string())
                    } else {
                        win(e)
                    }
                })?,
            );
            let same = session_of(facts.pid) == Some(facts.session_id)
                && user_and_path(process.0).is_ok_and(|(sid, path)| {
                    sid.eq_ignore_ascii_case(&facts.user_sid) && path == facts.path
                });
            if !same {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "the process changed since it was inspected; not killed",
                ));
            }
            TerminateProcess(process.0, KILLED_EXIT_CODE).map_err(win)
        }
    }

    fn set_suspended(&self, facts: &ProcFacts, suspended: bool) -> io::Result<()> {
        // SAFETY: the handle is closed by its guard; it pins the process while it is open.
        unsafe {
            let process = Owned(
                OpenProcess(
                    PROCESS_SUSPEND_RESUME | PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    facts.pid,
                )
                .map_err(|e| {
                    if is_gone(&e) {
                        io::Error::new(io::ErrorKind::NotFound, e.to_string())
                    } else {
                        win(e)
                    }
                })?,
            );
            let same = session_of(facts.pid) == Some(facts.session_id)
                && user_and_path(process.0).is_ok_and(|(sid, path)| {
                    sid.eq_ignore_ascii_case(&facts.user_sid) && path == facts.path
                });
            if !same {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "the process changed since it was inspected; not changed",
                ));
            }
            let status = if suspended {
                NtSuspendProcess(process.0)
            } else {
                NtResumeProcess(process.0)
            };
            if status < 0 {
                return Err(io::Error::other(format!("NTSTATUS {status:#010x}")));
            }
            Ok(())
        }
    }
}

/// The signer CN held by a verified WinVerifyTrust state.
unsafe fn signer_cn(state: HANDLE) -> Option<String> {
    // SAFETY: the caller passes the state of a WinVerifyTrust call not yet closed; every
    // pointer is checked before use.
    unsafe {
        let prov = WTHelperProvDataFromStateData(state);
        if prov.is_null() {
            return None;
        }
        let sgnr = WTHelperGetProvSignerFromChain(prov, 0, false, 0);
        if sgnr.is_null() || (*sgnr).csCertChain == 0 || (*sgnr).pasCertChain.is_null() {
            return None;
        }
        let cert = (*(*sgnr).pasCertChain).pCert;
        if cert.is_null() {
            return None;
        }
        let oid = Some(szOID_COMMON_NAME.0 as *const core::ffi::c_void);
        let n = CertGetNameStringW(cert, CERT_NAME_ATTR_TYPE, 0, oid, None) as usize;
        if n <= 1 {
            return None;
        }
        let mut buf = vec![0u16; n];
        let got = CertGetNameStringW(cert, CERT_NAME_ATTR_TYPE, 0, oid, Some(&mut buf)) as usize;
        let text = String::from_utf16_lossy(&buf[..got.saturating_sub(1)]);
        (!text.is_empty()).then_some(text)
    }
}

/// Runs WinVerifyTrust on `data` (already pointing at its subject), reads the signer, and
/// closes the state. Returns the HRESULT and the signer.
unsafe fn verify_trust(mut data: WINTRUST_DATA) -> (i32, Option<String>) {
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let no_ui = HWND(INVALID_HANDLE_VALUE.0);
    // SAFETY: `data` and its subject outlive both calls; the state is closed.
    unsafe {
        let rc = WinVerifyTrust(no_ui, &mut action, (&mut data as *mut WINTRUST_DATA).cast());
        let signer = signer_cn(data.hWVTStateData);
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(no_ui, &mut action, (&mut data as *mut WINTRUST_DATA).cast());
        (rc, signer)
    }
}

fn trust_data() -> WINTRUST_DATA {
    WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        // Offline-safe and fast: no revocation checks, no network retrieval.
        dwProvFlags: WINTRUST_DATA_PROVIDER_FLAGS(
            WTD_REVOCATION_CHECK_NONE.0 | WTD_CACHE_ONLY_URL_RETRIEVAL.0,
        ),
        ..Default::default()
    }
}

fn embedded(path: &[u16]) -> (i32, Option<String>) {
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(path.as_ptr()),
        ..Default::default()
    };
    let mut data = trust_data();
    data.dwUnionChoice = WTD_CHOICE_FILE;
    data.Anonymous = WINTRUST_DATA_0 { pFile: &mut file };
    // SAFETY: `file` and `path` outlive the call.
    unsafe { verify_trust(data) }
}

/// The catalog signature of `path` with the hash algorithm `alg` (`None`: SHA-1).
fn catalog(path: &str, wpath: &[u16], alg: Option<PCWSTR>) -> Option<Signature> {
    let file = std::fs::File::open(path).ok()?;
    let hfile = HANDLE(file.as_raw_handle());
    // SAFETY: the admin and catalog contexts are released below; the hash buffer and strings
    // outlive the WinVerifyTrust call.
    unsafe {
        let mut admin = 0isize;
        CryptCATAdminAcquireContext2(&mut admin, None, alg.unwrap_or(PCWSTR::null()), None, None)
            .ok()?;
        let result = (|| {
            let mut len = 0u32;
            let _ = CryptCATAdminCalcHashFromFileHandle2(admin, hfile, &mut len, None, None);
            if len == 0 {
                return None;
            }
            let mut hash = vec![0u8; len as usize];
            CryptCATAdminCalcHashFromFileHandle2(
                admin,
                hfile,
                &mut len,
                Some(hash.as_mut_ptr()),
                None,
            )
            .ok()?;
            hash.truncate(len as usize);
            let cat = CryptCATAdminEnumCatalogFromHash(admin, &hash, None, None);
            if cat == 0 {
                return None;
            }
            let mut info = CATALOG_INFO {
                cbStruct: std::mem::size_of::<CATALOG_INFO>() as u32,
                ..Default::default()
            };
            let got = CryptCATCatalogInfoFromContext(cat, &mut info, 0);
            let sig = got.ok().map(|()| {
                let tag: String = hash.iter().map(|b| format!("{b:02X}")).collect();
                let tag = wide(&tag);
                let mut member = WINTRUST_CATALOG_INFO {
                    cbStruct: std::mem::size_of::<WINTRUST_CATALOG_INFO>() as u32,
                    pcwszCatalogFilePath: PCWSTR(info.wszCatalogFile.as_ptr()),
                    pcwszMemberTag: PCWSTR(tag.as_ptr()),
                    pcwszMemberFilePath: PCWSTR(wpath.as_ptr()),
                    hMemberFile: hfile,
                    pbCalculatedFileHash: hash.as_mut_ptr(),
                    cbCalculatedFileHash: hash.len() as u32,
                    hCatAdmin: admin,
                    ..Default::default()
                };
                let mut data = trust_data();
                data.dwUnionChoice = WTD_CHOICE_CATALOG;
                data.Anonymous = WINTRUST_DATA_0 {
                    pCatalog: &mut member,
                };
                let (rc, signer) = verify_trust(data);
                Signature {
                    signer,
                    valid: rc == 0,
                }
            });
            let _ = CryptCATAdminReleaseCatalogContext(admin, cat, 0);
            sig
        })();
        let _ = CryptCATAdminReleaseContext(admin, 0);
        drop(file);
        result
    }
}

impl SignatureVerifier for WinProcesses {
    fn stamp(&self, path: &str) -> Option<FileStamp> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos() as i64;
        Some(FileStamp {
            size: meta.len(),
            mtime,
        })
    }

    fn verify(&self, path: &str) -> Signature {
        let wpath = wide(path);
        let (rc, signer) = embedded(&wpath);
        if rc != TRUST_E_NOSIGNATURE.0 {
            return Signature {
                signer,
                valid: rc == 0,
            };
        }
        // No embedded signature: most Windows images are signed through a catalog.
        catalog(path, &wpath, Some(w!("SHA256")))
            .or_else(|| catalog(path, &wpath, None))
            .unwrap_or_default()
    }
}
