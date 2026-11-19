//! Bounded IPv4 range scanning for other subnets.
//!
//! mDNS only covers the local link, so users can probe a single range on demand.
//! The scope is deliberately narrow: /24 to /32 only, at most 254 targets, four
//! workers with a short connect timeout and a small per-target pause. The scanner
//! only proves key possession; it never grants trust, and blocked entries are
//! skipped before any connection is attempted.
use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::Duration;

use crate::identity::Identity;
use crate::probe;

pub const MIN_PREFIX: u8 = 24;
pub const MAX_PREFIX: u8 = 32;
pub const MAX_TARGETS: usize = 254;
pub const WORKERS: usize = 4;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(400);
const THROTTLE: Duration = Duration::from_millis(40);

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub struct ScanHit {
    pub address: String,
    pub fingerprint: String,
}

pub fn parse_cidr(input: &str) -> Result<(Ipv4Addr, u8), String> {
    let text = input.trim();
    let (ip_text, prefix_text) = text.split_once('/').ok_or_else(|| "请输入 CIDR，例如 192.168.1.0/24".to_owned())?;
    let ip: Ipv4Addr = ip_text.trim().parse().map_err(|_| "网段地址无效".to_owned())?;
    let prefix: u8 = prefix_text.trim().parse().map_err(|_| "前缀长度无效".to_owned())?;
    if !(MIN_PREFIX..=MAX_PREFIX).contains(&prefix) {
        return Err(format!("仅支持 /{MIN_PREFIX} 到 /{MAX_PREFIX} 的网段"));
    }
    let mask: u32 = if prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(prefix)) };
    if u32::from(ip) & !mask != 0 {
        return Err("网段地址需与前缀对齐，例如 192.168.1.0/24".into());
    }
    Ok((ip, prefix))
}

/// Hosts in the range, excluding network and broadcast addresses when they exist.
pub fn targets(ip: Ipv4Addr, prefix: u8) -> Vec<Ipv4Addr> {
    let base = u32::from(ip);
    let count: u32 = 1u32 << (32 - u32::from(prefix));
    let (start, end) = if count >= 4 { (1, count - 1) } else { (0, count) };
    let mut list = Vec::with_capacity((end - start) as usize);
    for offset in start..end {
        list.push(Ipv4Addr::from(base + offset));
    }
    list.truncate(MAX_TARGETS);
    list
}

fn blocked(ip: Ipv4Addr, entries: &[String]) -> bool {
    entries.iter().any(|entry| {
        if let Ok((base, prefix)) = parse_cidr(entry) {
            let mask: u32 = if prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(prefix)) };
            return u32::from(ip) & mask == u32::from(base);
        }
        entry.trim().parse::<Ipv4Addr>().map(|value| value == ip).unwrap_or(false)
    })
}

pub fn validate_blacklist(entries: &[String]) -> Result<(), String> {
    if entries.len() > 256 { return Err("黑名单最多 256 项".into()); }
    for entry in entries {
        let text = entry.trim();
        if text.is_empty() || text.len() > 64 { return Err("黑名单条目无效".into()); }
        if parse_cidr(text).is_err() && text.parse::<Ipv4Addr>().is_err() {
            return Err(format!("无法识别黑名单条目：{text}"));
        }
    }
    Ok(())
}

/// Probe every unblocked host in `cidr` with a bounded worker pool.
pub fn scan(identity: &Identity, cidr: &str, port: u16, blacklist: &[String]) -> Result<Vec<ScanHit>, String> {
    if port == 0 { return Err("端口无效".into()); }
    validate_blacklist(blacklist)?;
    let (ip, prefix) = parse_cidr(cidr)?;
    let queue: Mutex<VecDeque<SocketAddr>> = Mutex::new(
        targets(ip, prefix).into_iter()
            .filter(|host| !blocked(*host, blacklist))
            .map(|host| SocketAddr::from((host, port)))
            .collect(),
    );
    if queue.lock().map_err(|_| "扫描状态不可用".to_owned())?.is_empty() {
        return Err("没有需要扫描的地址（可能全部在黑名单中）".into());
    }
    let hits: Mutex<Vec<ScanHit>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            let queue = &queue;
            let hits = &hits;
            scope.spawn(move || loop {
                let next = queue.lock().ok().and_then(|mut list| list.pop_front());
                let Some(address) = next else { break; };
                if let Ok(fingerprint) = probe::connect_with_timeout(identity, address, CONNECT_TIMEOUT) {
                    if let Ok(mut list) = hits.lock() {
                        list.push(ScanHit { address: address.to_string(), fingerprint });
                    }
                }
                // Small pause per target keeps the scan from looking like a flood.
                std::thread::sleep(THROTTLE);
            });
        }
    });
    let mut list = hits.into_inner().map_err(|_| "扫描结果不可用".to_owned())?;
    list.sort_by(|a, b| a.address.cmp(&b.address));
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_is_bounded_and_aligned() {
        assert!(parse_cidr("192.168.1.0/24").is_ok());
        assert!(parse_cidr("192.168.1.0/23").is_err());
        assert!(parse_cidr("192.168.1.7/24").is_err());
        assert!(parse_cidr("192.168.1.0").is_err());
        assert!(parse_cidr("10.0.0.1/32").is_ok());
        let hosts = targets(Ipv4Addr::new(192, 168, 1, 0), 24);
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0], Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(hosts[253], Ipv4Addr::new(192, 168, 1, 254));
    }

    #[test]
    fn blacklist_matches_single_addresses_and_ranges() {
        let entries = vec!["192.168.1.5".to_owned(), "192.168.2.0/24".to_owned()];
        assert!(blocked(Ipv4Addr::new(192, 168, 1, 5), &entries));
        assert!(blocked(Ipv4Addr::new(192, 168, 2, 77), &entries));
        assert!(!blocked(Ipv4Addr::new(192, 168, 1, 6), &entries));
        assert!(validate_blacklist(&entries).is_ok());
        assert!(validate_blacklist(&["not-an-ip".to_owned()]).is_err());
        assert!(validate_blacklist(&vec!["192.168.1.1".to_owned(); 257]).is_err());
    }

    #[test]
    fn full_blacklist_leaves_nothing_to_scan() {
        let entries = vec!["192.168.9.0/24".to_owned()];
        let (ip, prefix) = parse_cidr("192.168.9.0/30").unwrap();
        assert!(targets(ip, prefix).iter().all(|host| blocked(*host, &entries)));
    }
}
