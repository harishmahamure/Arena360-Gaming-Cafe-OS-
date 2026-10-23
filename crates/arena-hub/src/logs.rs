//! JSON log lines in a size- and age-capped ring under `<data>/logs/`.
//!
//! NO PII: log fields carry ids, codes and counts only — never names, phone numbers or email
//! addresses. Keep it that way in every `tracing` call in the Hub.
//!
//! Files are `hub-YYYY-MM-DD.jsonl` (UTC), then `hub-YYYY-MM-DD.N.jsonl` (N = 1, 2, ...) when
//! the ring fills up during a day. Retention runs at start, on the daily rollover and whenever
//! the total passes [`MAX_TOTAL_BYTES`] (which also rolls to a new part file): files older than
//! [`MAX_AGE_DAYS`] or dated in the future are deleted, then the oldest are deleted until the
//! total is at most [`MAX_TOTAL_BYTES`]. The active file is never deleted.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use arena_core::Clock;

pub const MAX_AGE_DAYS: i64 = 14;
pub const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024;

const MS_PER_DAY: i64 = 86_400_000;

/// Days since 1970-01-01 to a civil (year, month, day), proleptic Gregorian.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// A civil date to days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The log file name for a day and part (part 0 has no number).
pub fn file_name(day: i64, part: u32) -> String {
    let (y, m, d) = civil_from_days(day);
    if part == 0 {
        format!("hub-{y:04}-{m:02}-{d:02}.jsonl")
    } else {
        format!("hub-{y:04}-{m:02}-{d:02}.{part}.jsonl")
    }
}

/// Parses `hub-YYYY-MM-DD[.N].jsonl` back to (day, part).
fn parse_file_name(name: &str) -> Option<(i64, u32)> {
    let stem = name.strip_prefix("hub-")?.strip_suffix(".jsonl")?;
    let (date, part) = match stem.split_once('.') {
        Some((date, n)) => (date, n.parse::<u32>().ok().filter(|n| *n > 0)?),
        None => (stem, 0),
    };
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some()
        || !(0..=9999).contains(&y)
        || !(1..=12).contains(&m)
        || !(1..=31).contains(&d)
    {
        return None;
    }
    let day = days_from_civil(y, m, d);
    (file_name(day, part) == name).then_some((day, part))
}

/// Retention problems cannot go through `tracing` (this module is its writer), so they go to
/// stderr and the file is skipped.
fn report(what: &str, path: &Path, err: &io::Error) {
    eprintln!("arena-hub logs: {what} {}: {err}", path.display());
}

/// Lists ring files as (day, part, path, len), oldest first. Unreadable entries are skipped.
fn list(dir: &Path) -> io::Result<Vec<(i64, u32, PathBuf, u64)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                report("read entry in", dir, &e);
                continue;
            }
        };
        let Some((day, part)) = entry.file_name().to_str().and_then(parse_file_name) else {
            continue;
        };
        let path = entry.path();
        match entry.metadata() {
            Ok(meta) if meta.is_file() => files.push((day, part, path, meta.len())),
            Ok(_) => {}
            Err(e) => report("stat", &path, &e),
        }
    }
    files.sort_by_key(|(day, part, _, _)| (*day, *part));
    Ok(files)
}

/// Applies retention and returns the total size of the files kept. `active` is never deleted.
pub fn enforce_retention(
    dir: &Path,
    today: i64,
    active: &Path,
    max_age_days: i64,
    max_bytes: u64,
) -> io::Result<u64> {
    let mut kept = Vec::new();
    for (day, part, path, len) in list(dir)? {
        let expired = day > today || today - day > max_age_days;
        if expired && path != active {
            match fs::remove_file(&path) {
                Ok(()) => continue,
                Err(e) => report("delete", &path, &e),
            }
        }
        kept.push((day, part, path, len));
    }
    let mut total: u64 = kept.iter().map(|(_, _, _, len)| len).sum();
    for (_, _, path, len) in &kept {
        if total <= max_bytes {
            break;
        }
        if path == active {
            continue;
        }
        match fs::remove_file(path) {
            Ok(()) => total -= len,
            Err(e) => report("delete", path, &e),
        }
    }
    Ok(total)
}

struct Inner {
    day: i64,
    part: u32,
    path: PathBuf,
    file: File,
    /// Bytes in all ring files, as of the last retention pass plus what was written since.
    total: u64,
}

/// A `tracing` writer: appends to the active file, rolls daily and when the ring is full.
pub struct RingLog {
    dir: PathBuf,
    clock: Arc<dyn Clock>,
    max_age_days: i64,
    max_bytes: u64,
    inner: Mutex<Inner>,
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

impl RingLog {
    /// Creates `dir`, opens today's latest file and applies retention.
    pub fn open(dir: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> io::Result<Self> {
        Self::open_with(dir, clock, MAX_AGE_DAYS, MAX_TOTAL_BYTES)
    }

    /// [`RingLog::open`] with explicit limits (tests).
    pub fn open_with(
        dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        max_age_days: i64,
        max_bytes: u64,
    ) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let day = clock.now_ms().div_euclid(MS_PER_DAY);
        let part = list(&dir)?
            .iter()
            .filter(|(d, _, _, _)| *d == day)
            .map(|(_, p, _, _)| *p)
            .max()
            .unwrap_or(0);
        let path = dir.join(file_name(day, part));
        let file = open_append(&path)?;
        let total = enforce_retention(&dir, day, &path, max_age_days, max_bytes)?;
        Ok(Self {
            dir,
            clock,
            max_age_days,
            max_bytes,
            inner: Mutex::new(Inner {
                day,
                part,
                path,
                file,
                total,
            }),
        })
    }

    fn switch(&self, inner: &mut Inner, day: i64, part: u32) -> io::Result<()> {
        inner.file.flush()?;
        let path = self.dir.join(file_name(day, part));
        inner.file = open_append(&path)?;
        inner.path = path;
        inner.day = day;
        inner.part = part;
        // Retention failures must never stop logging.
        if let Ok(total) = enforce_retention(
            &self.dir,
            day,
            &inner.path,
            self.max_age_days,
            self.max_bytes,
        ) {
            inner.total = total;
        }
        Ok(())
    }
}

impl Write for &RingLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let today = self.clock.now_ms().div_euclid(MS_PER_DAY);
        if today != inner.day {
            self.switch(&mut inner, today, 0)?;
        }
        let n = inner.file.write(buf)?;
        inner.total += n as u64;
        if inner.total > self.max_bytes {
            let (day, part) = (inner.day, inner.part + 1);
            self.switch(&mut inner, day, part)?;
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .file
            .flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::ManualClock;

    fn day_of(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d)
    }

    fn touch(dir: &Path, day: i64, part: u32, len: u64) -> PathBuf {
        let p = dir.join(file_name(day, part));
        File::create(&p).unwrap().set_len(len).unwrap();
        p
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(file_name(day_of(2026, 10, 23), 0), "hub-2026-10-23.jsonl");
        assert_eq!(file_name(day_of(2026, 10, 23), 3), "hub-2026-10-23.3.jsonl");
        assert_eq!(file_name(day_of(2024, 2, 29), 0), "hub-2024-02-29.jsonl");
        for day in [-1, 0, 19_000, 20_384, 60_000] {
            assert_eq!(parse_file_name(&file_name(day, 0)), Some((day, 0)));
            assert_eq!(parse_file_name(&file_name(day, 2)), Some((day, 2)));
        }
        assert_eq!(parse_file_name("hub-2026-02-30.jsonl"), None);
        assert_eq!(parse_file_name("hub-2026-10-23.0.jsonl"), None);
        assert_eq!(parse_file_name("hub-10000-01-01.jsonl"), None);
        assert_eq!(parse_file_name("hub--001-01-01.jsonl"), None);
        assert_eq!(parse_file_name("other.jsonl"), None);
    }

    #[test]
    fn deletes_files_older_than_max_age_and_future_files() {
        let dir = tempfile::tempdir().unwrap();
        let today = day_of(2026, 10, 23);
        let active = touch(dir.path(), today, 0, 1);
        let old = touch(dir.path(), today - 15, 0, 10);
        let edge = touch(dir.path(), today - 14, 0, 10);
        let fresh = touch(dir.path(), today - 1, 1, 10);
        let future = touch(dir.path(), today + 2, 0, 10);
        let other = dir.path().join("keep.txt");
        File::create(&other).unwrap();
        let total =
            enforce_retention(dir.path(), today, &active, MAX_AGE_DAYS, MAX_TOTAL_BYTES).unwrap();
        assert!(!old.exists() && !future.exists());
        assert!(edge.exists() && fresh.exists() && active.exists() && other.exists());
        assert_eq!(total, 21);
    }

    #[test]
    fn deletes_oldest_until_within_total_size() {
        let dir = tempfile::tempdir().unwrap();
        let today = day_of(2026, 10, 23);
        let mb = 1024 * 1024;
        let a = touch(dir.path(), today - 3, 0, 20 * mb);
        let b = touch(dir.path(), today - 2, 0, 20 * mb);
        let c = touch(dir.path(), today - 1, 0, 20 * mb);
        let t = touch(dir.path(), today, 0, 5 * mb);
        enforce_retention(dir.path(), today, &t, MAX_AGE_DAYS, MAX_TOTAL_BYTES).unwrap();
        assert!(!a.exists());
        assert!(b.exists() && c.exists() && t.exists());
    }

    #[test]
    fn never_deletes_the_active_file() {
        let dir = tempfile::tempdir().unwrap();
        let today = day_of(2026, 10, 23);
        let older = touch(dir.path(), today, 0, 10);
        let active = touch(dir.path(), today, 1, 100);
        enforce_retention(dir.path(), today, &active, MAX_AGE_DAYS, 50).unwrap();
        assert!(!older.exists());
        assert!(active.exists());
    }

    #[test]
    fn writes_roll_over_daily_and_apply_retention() {
        let dir = tempfile::tempdir().unwrap();
        let day = day_of(2026, 10, 23);
        let ancient = touch(dir.path(), day - 30, 0, 10);
        let clock = Arc::new(ManualClock::new(day * MS_PER_DAY + 1_000));
        let log = RingLog::open(dir.path(), clock.clone()).unwrap();
        assert!(!ancient.exists(), "retention runs at start");
        (&log).write_all(b"{\"a\":1}\n").unwrap();
        let stale = touch(dir.path(), day + 1 - 20, 0, 10);
        clock.advance(MS_PER_DAY);
        (&log).write_all(b"{\"b\":2}\n").unwrap();
        (&log).flush().unwrap();
        assert!(!stale.exists(), "retention runs on rollover");
        let first = fs::read_to_string(dir.path().join(file_name(day, 0))).unwrap();
        let second = fs::read_to_string(dir.path().join(file_name(day + 1, 0))).unwrap();
        assert_eq!(first, "{\"a\":1}\n");
        assert_eq!(second, "{\"b\":2}\n");
    }

    #[test]
    fn a_full_ring_rolls_to_part_files_within_the_day() {
        let dir = tempfile::tempdir().unwrap();
        let day = day_of(2026, 10, 23);
        let clock = Arc::new(ManualClock::new(day * MS_PER_DAY));
        let log = RingLog::open_with(dir.path(), clock, MAX_AGE_DAYS, 25).unwrap();
        let line = b"0123456789\n"; // 11 bytes
        for _ in 0..7 {
            (&log).write_all(line).unwrap();
        }
        (&log).flush().unwrap();
        // Writes 1-3 (33 B) pass the cap: roll to part 1, and retention drops part 0. Writes
        // 4-6 do the same for part 1. Write 7 lands in part 2.
        assert_eq!(names(dir.path()), vec!["hub-2026-10-23.2.jsonl"]);
        let active = fs::read(dir.path().join(file_name(day, 2))).unwrap();
        assert_eq!(active, line);
    }

    #[test]
    fn reopening_continues_the_latest_part() {
        let dir = tempfile::tempdir().unwrap();
        let day = day_of(2026, 10, 23);
        touch(dir.path(), day, 0, 1);
        touch(dir.path(), day, 2, 1);
        let clock = Arc::new(ManualClock::new(day * MS_PER_DAY));
        let log = RingLog::open(dir.path(), clock).unwrap();
        (&log).write_all(b"x\n").unwrap();
        (&log).flush().unwrap();
        assert_eq!(
            fs::read(dir.path().join(file_name(day, 2))).unwrap().len(),
            3
        );
    }
}
