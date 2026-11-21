//! Minimal self-contained file logging.
//!
//! Appends timestamped lines to `<data dir>/logs/qbird.log`, rotating the file
//! when it exceeds [`MAX_FILE_BYTES`] and keeping [`KEEP_ROTATIONS`] older
//! copies. The logger is intentionally dependency-free: startup, panics and
//! listener failures must be recorded even when nothing else works.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const KEEP_ROTATIONS: usize = 5;
const FILE_NAME: &str = "qbird.log";

static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Initializes logging under `dir` (normally `<data dir>/logs`) and installs a
/// panic hook that records panics before the default hook runs.
pub fn init(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let file = dir.join(FILE_NAME);
    let _ = LOG_FILE.set(file);
    install_panic_hook();
    info("---- Qbird 启动 ----");
}

pub fn log_path() -> Option<&'static Path> {
    LOG_FILE.get().map(|p| p.as_path())
}

pub fn info(message: &str) { append("INFO", message); }
pub fn warn(message: &str) { append("WARN", message); }
pub fn error(message: &str) { append("ERROR", message); }

/// Convenience wrapper: `log_info!("port {} ready", port);`
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::logging::info(&format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::logging::warn(&format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::logging::error(&format!($($arg)*)) };
}

pub fn timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    // Civil-time conversion without chrono: days since 1970-01-01 to y-m-d.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}")
}

/// Howard Hinnant's days-from-civil inverse, so no extra dependency is needed.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn append(level: &str, message: &str) {
    let Some(path) = LOG_FILE.get() else { return };
    rotate_if_needed(path);
    let line = format!("{} [{level}] {}\n", timestamp(), message.replace('\n', " | "));
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn rotate_if_needed(path: &Path) {
    let Ok(meta) = fs::metadata(path) else { return };
    if meta.len() < MAX_FILE_BYTES { return; }
    // Shift qbird.log.4 -> .5 (dropped) ... qbird.log -> .1
    for i in (1..KEEP_ROTATIONS).rev() {
        let from = path.with_extension(format!("{FILE_NAME}.{i}"));
        let to = path.with_extension(format!("{FILE_NAME}.{}", i + 1));
        if i + 1 > KEEP_ROTATIONS { let _ = fs::remove_file(&to); continue; }
        let _ = fs::rename(&from, &to);
    }
    // The base file has no extension change; build sibling names manually.
    let _ = fs::rename(path, sibling(path, 1));
}

fn sibling(path: &Path, index: usize) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{index}"));
    path.with_file_name(name)
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| l.to_string()).unwrap_or_default();
        let payload = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "未知 panic".to_owned());
        append("PANIC", &format!("{payload} @ {location}"));
        default_hook(info);
    }));
}

/// Last `max_bytes` of the current log file, for the debug panel.
pub fn read_recent(max_bytes: u64) -> std::io::Result<String> {
    let Some(path) = LOG_FILE.get() else { return Ok(String::new()) };
    let data = fs::read(path)?;
    let start = data.len().saturating_sub(max_bytes as usize);
    // Start at a line boundary when possible.
    let start = if start == 0 || data[start] == b'\n' { start } else {
        data[start..].iter().position(|b| *b == b'\n').map(|p| start + p + 1).unwrap_or(start)
    };
    Ok(String::from_utf8_lossy(&data[start..]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_conversion_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // 2024-01-01
    }

    #[test]
    fn read_recent_handles_missing_file() {
        // init never called: empty output, no error.
        assert_eq!(read_recent(1024).unwrap(), "");
    }
}
