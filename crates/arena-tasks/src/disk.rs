//! How much room is left on the disk a folder is on (Story 8.8: the daily snapshot is skipped
//! when less than twice the database is free).

use std::path::Path;

/// The bytes a normal process can still write on the disk holding `dir`, or `None` when the
/// platform cannot say (the caller then goes ahead rather than blocking on a question it cannot
/// answer).
#[cfg(windows)]
pub fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;

    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; `available` is a valid out slot.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }
        .ok()
        .map(|()| available)
}

#[cfg(unix)]
pub fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;

    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `path` is NUL-terminated and `stat` is a valid out slot for the call.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: `statvfs` returned 0, so it filled `stat`.
    let stat = unsafe { stat.assume_init() };
    #[allow(clippy::unnecessary_cast)]
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(not(any(windows, unix)))]
pub fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_folder_reports_some_room() {
        let dir = tempfile::tempdir().unwrap();
        let free = free_bytes(dir.path()).expect("this platform can say");
        assert!(free > 0);
    }

    #[test]
    fn a_missing_folder_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_bytes(&dir.path().join("no/such/folder")), None);
    }
}
