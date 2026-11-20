//! Read-only diagnostics for the settings "debug mode" panel, plus the small
//! building blocks used by the bounded network repair command.
//!
//! Everything here is best effort: a probe that cannot run returns `None` instead
//! of a guessed value, and nothing touches identity, contacts or message state.
use std::net::{Ipv4Addr, TcpListener, UdpSocket};
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::storage::{RecvKind, Store};

/// Upper bound for a directory walk so a huge custom receive folder cannot stall
/// the diagnostics call.
const MAX_WALK_ENTRIES: u64 = 20_000;
const MAX_WALK_DEPTH: u8 = 8;

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct DirectoryUsage {
    pub bytes: u64,
    pub files: u64,
    pub dirs: u64,
    /// True when the walk hit `MAX_WALK_ENTRIES` and the totals are a lower bound.
    pub truncated: bool,
}

/// Sums regular files below `directory`. Symlinks and junctions are skipped so the
/// number reflects local storage, not a linked volume.
pub fn directory_usage(directory: &Path) -> DirectoryUsage {
    struct Walk {
        usage: DirectoryUsage,
        seen: u64,
    }

    fn walk(path: &Path, depth: u8, state: &mut Walk) {
        if depth > MAX_WALK_DEPTH || state.usage.truncated { return; }
        let Ok(entries) = std::fs::read_dir(path) else { return; };
        for entry in entries.flatten() {
            if state.seen >= MAX_WALK_ENTRIES {
                state.usage.truncated = true;
                return;
            }
            state.seen += 1;
            let Ok(kind) = entry.file_type() else { continue; };
            if kind.is_symlink() { continue; }
            if kind.is_file() {
                state.usage.files += 1;
                state.usage.bytes = state.usage.bytes
                    .saturating_add(entry.metadata().map(|meta| meta.len()).unwrap_or(0));
            } else if kind.is_dir() {
                state.usage.dirs += 1;
                walk(&entry.path(), depth + 1, state);
            }
        }
    }

    let mut state = Walk { usage: DirectoryUsage::default(), seen: 0 };
    walk(directory, 0, &mut state);
    state.usage
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DiskSpace {
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
}

impl DiskSpace {
    fn unavailable(error: impl Into<String>) -> Self {
        Self { free_bytes: None, total_bytes: None, error: Some(error.into()) }
    }
}

/// Free and total space of the volume that holds `path`. On unsupported platforms
/// the values stay `None`; they are never estimated.
pub fn disk_space(path: &Path) -> DiskSpace {
    #[cfg(windows)]
    {
        return windows_disk_space(path);
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        DiskSpace::unavailable("当前平台暂不支持磁盘空间检测")
    }
}

#[cfg(windows)]
fn windows_disk_space(path: &Path) -> DiskSpace {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let (mut free_for_caller, mut total, mut free_total) = (0u64, 0u64, 0u64);
    // SAFETY: the path is a NUL-terminated UTF-16 buffer that outlives the call, and
    // the three out-pointers refer to live u64 values.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_for_caller,
            &mut total,
            &mut free_total,
        )
    };
    if ok == 0 {
        let code = std::io::Error::last_os_error();
        return DiskSpace::unavailable(format!("无法读取磁盘容量（{code}）"));
    }
    DiskSpace { free_bytes: Some(free_total), total_bytes: Some(total), error: None }
}

/// A bind test on the wildcard address: whether Qbird could start a listener there.
/// It does not inspect which process owns the port.
pub fn port_available(port: u16) -> bool {
    TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// Pulls IPv4 addresses out of `ipconfig` output. Only lines that carry the literal
/// `IPv4` marker (English and Chinese Windows both use it) are considered, so subnet
/// masks and default gateways are not reported as local addresses.
pub fn parse_ipv4_addresses(text: &str) -> Vec<String> {
    let mut addresses: Vec<String> = Vec::new();
    for line in text.lines() {
        if !line.to_ascii_lowercase().contains("ipv4") { continue; }
        let Some(token) = line.split_whitespace().last() else { continue; };
        let Ok(address) = token.parse::<Ipv4Addr>() else { continue; };
        if address.is_loopback() || address.is_unspecified() || address.is_link_local() { continue; }
        let text = address.to_string();
        if !addresses.contains(&text) { addresses.push(text); }
    }
    addresses
}

/// Local IPv4 addresses, plus a single default-route address as a fallback. The
/// fallback sends no packets: a connected UDP socket only resolves the route.
pub fn local_ipv4_addresses() -> Vec<String> {
    let mut addresses = system_ipv4_addresses();
    if addresses.is_empty() {
        if let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) {
            let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
            if socket.connect(("223.5.5.5", 53)).is_ok() {
                if let Ok(local) = socket.local_addr() {
                    if let std::net::IpAddr::V4(address) = local.ip() {
                        if !address.is_loopback() && !address.is_unspecified() {
                            addresses.push(address.to_string());
                        }
                    }
                }
            }
        }
    }
    addresses
}

#[cfg(windows)]
fn system_ipv4_addresses() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("ipconfig")
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            parse_ipv4_addresses(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Vec::new(),
    }
}

#[cfg(not(windows))]
fn system_ipv4_addresses() -> Vec<String> {
    Vec::new()
}

/// Host name for display only; an unset variable is reported as unknown instead of
/// being filled with a placeholder that looks like a real machine name.
pub fn hostname() -> String {
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() { return trimmed.to_owned(); }
        }
    }
    "未知".to_owned()
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NetworkReport {
    pub platform: String,
    pub hostname: String,
    pub local_ipv4: Vec<String>,
    pub service_running: bool,
    /// True when the running listener also accepts chat traffic, not probes only.
    pub chat_enabled: bool,
    pub service_port: Option<u16>,
    pub default_port: u16,
    /// A bind test of the saved default port; the local Qbird listener counts as busy.
    pub default_port_available: bool,
    pub discovery_started: bool,
    pub lan_candidates: usize,
    pub contacts: usize,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RepairStep {
    pub step: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RepairReport {
    pub steps: Vec<RepairStep>,
}

impl RepairReport {
    pub fn new() -> Self { Self { steps: Vec::new() } }

    pub fn push(&mut self, step: impl Into<String>, ok: bool, detail: impl Into<String>) {
        self.steps.push(RepairStep { step: step.into(), ok, detail: detail.into() });
    }
}

impl Default for RepairReport {
    fn default() -> Self { Self::new() }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RecvDirReport {
    /// `direct` or `group`; the UI maps it to a label.
    pub kind: String,
    pub path: String,
    pub custom: bool,
    pub exists: bool,
    pub usage: DirectoryUsage,
    pub disk: DiskSpace,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SpaceReport {
    pub data_dir: String,
    pub data_dir_usage: DirectoryUsage,
    pub data_dir_disk: DiskSpace,
    pub database_bytes: u64,
    pub attachments_bytes: u64,
    pub attachments_quota: u64,
    pub direct: RecvDirReport,
    pub group: RecvDirReport,
    pub notes: Vec<String>,
}

fn recv_report(kind: &str, dir: std::path::PathBuf, custom: bool) -> RecvDirReport {
    let exists = dir.is_dir();
    RecvDirReport {
        kind: kind.to_owned(),
        path: dir.to_string_lossy().to_string(),
        custom,
        exists,
        usage: if exists { directory_usage(&dir) } else { DirectoryUsage::default() },
        disk: if exists { disk_space(&dir) } else { DiskSpace::unavailable("目录不存在") },
    }
}

/// Snapshot of where local data lives and how much room is left. All values are
/// measurements; nothing is cleaned or moved.
pub fn space_report(data_dir: &Path, store: &Store) -> SpaceReport {
    let database_bytes = std::fs::metadata(data_dir.join("qbird.sqlite3"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let attachments_bytes = store.storage_used();
    let attachments_quota = store.storage_quota().unwrap_or(crate::storage::DEFAULT_STORAGE_QUOTA);
    let direct = recv_report("direct", store.recv_dir(RecvKind::Direct), store.recv_dir_custom(RecvKind::Direct));
    let group = recv_report("group", store.recv_dir(RecvKind::Group), store.recv_dir_custom(RecvKind::Group));
    let mut notes = Vec::new();
    if attachments_quota > 0 && attachments_bytes >= attachments_quota {
        notes.push("已接收附件占用达到配额上限，新文件会被拒绝".to_owned());
    }
    if let Some(free) = direct.disk.free_bytes {
        if free < 512 * 1024 * 1024 {
            notes.push(format!("单聊接收目录所在磁盘剩余不足 512 MB（{}）", direct.path));
        }
    }
    if group.path != direct.path {
        if let Some(free) = group.disk.free_bytes {
            if free < 512 * 1024 * 1024 {
                notes.push(format!("群聊接收目录所在磁盘剩余不足 512 MB（{}）", group.path));
            }
        }
    }
    if !direct.exists { notes.push("单聊接收目录不存在，接收文件前请先创建或重新选择".to_owned()); }
    if !group.exists { notes.push("群聊接收目录不存在，接收文件前请先创建或重新选择".to_owned()); }
    SpaceReport {
        data_dir: data_dir.to_string_lossy().to_string(),
        data_dir_usage: directory_usage(data_dir),
        data_dir_disk: disk_space(data_dir),
        database_bytes,
        attachments_bytes,
        attachments_quota,
        direct,
        group,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_lines_ignore_masks_and_unusable_addresses() {
        let text = "\n以太网适配器 以太网:\n\
            IPv4 地址 . . . . . . . . . . . . : 192.168.1.8\n\
            子网掩码  . . . . . . . . . . . . : 255.255.255.0\n\
            默认网关. . . . . . . . . . . . . : 192.168.1.1\n\
            IPv4 Address. . . . . . . . . . . : 10.0.0.5\n\
            IPv4 地址 . . . . . . . . . . . . : 127.0.0.1\n\
            IPv4 地址 . . . . . . . . . . . . : 169.254.10.20\n";
        assert_eq!(parse_ipv4_addresses(text), vec!["192.168.1.8".to_owned(), "10.0.0.5".to_owned()]);
    }

    #[test]
    fn directory_usage_counts_files_and_reports_missing_dirs_as_zero() {
        let base = std::env::temp_dir().join(format!(
            "qbird-diag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let nested = base.join("a");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(base.join("one.txt"), b"12345").unwrap();
        std::fs::write(nested.join("two.txt"), b"123").unwrap();
        let usage = directory_usage(&base);
        assert_eq!(usage.bytes, 8);
        assert_eq!(usage.files, 2);
        assert!(!usage.truncated);
        assert_eq!(directory_usage(&base.join("missing")), DirectoryUsage::default());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn port_probe_reports_a_held_listener() {
        let listener = TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!port_available(port));
        drop(listener);
        // The port may be immediately reusable; either result is a valid observation.
        let _ = port_available(port);
    }
}
