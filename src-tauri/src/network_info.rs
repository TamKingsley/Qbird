//! Best-effort local network hints for the contact hover card.
//!
//! The MAC address is read from the Windows ARP table, which only knows peers the
//! machine has recently talked to on the same link. Anything unavailable stays
//! `None`; the UI shows "未获取" instead of inventing a value. Neither IP nor MAC
//! is ever used as an identity.
use std::collections::BTreeMap;

/// Splits `192.168.1.8:42853` into its IP part when the format is plausible.
pub fn ip_of(address: &str) -> Option<String> {
    let (host, port) = address.rsplit_once(':')?;
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) { return None; }
    if host.parse::<std::net::IpAddr>().is_err() { return None; }
    Some(host.to_owned())
}

fn looks_like_mac(token: &str) -> bool {
    let separator = if token.contains('-') { '-' } else if token.contains(':') { ':' } else { return false; };
    let parts: Vec<&str> = token.split(separator).collect();
    parts.len() == 6 && parts.iter().all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Parses `arp -a` output into IP → MAC. Tested separately from the process call.
pub fn parse_arp(text: &str) -> BTreeMap<String, String> {
    let mut table = BTreeMap::new();
    for line in text.lines() {
        let mut tokens = line.split_whitespace();
        let Some(first) = tokens.next() else { continue };
        if first.parse::<std::net::Ipv4Addr>().is_err() { continue; }
        for token in tokens {
            if looks_like_mac(token) {
                table.insert(first.to_owned(), token.replace('-', ":").to_ascii_lowercase());
                break;
            }
        }
    }
    table
}

/// Reads the system ARP table. Missing tool or unsupported platform yields `None`.
pub fn mac_for(ip: &str) -> Option<String> {
    if ip.parse::<std::net::Ipv4Addr>().is_err() { return None; }
    let mut command = std::process::Command::new("arp");
    command.arg("-a");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().ok()?;
    if !output.status.success() { return None; }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_arp(&text).remove(ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_split_requires_a_plausible_socket() {
        assert_eq!(ip_of("192.168.1.8:42853").as_deref(), Some("192.168.1.8"));
        assert_eq!(ip_of("2001:db8::1"), None);
        assert_eq!(ip_of("host:123"), None);
        assert_eq!(ip_of("192.168.1.8:"), None);
        assert_eq!(ip_of("192.168.1.8:abc"), None);
    }

    #[test]
    fn arp_output_is_parsed_without_guessing() {
        let text = "\n接口: 192.168.1.20 --- 0x8\n  Internet 地址         物理地址              类型\n  192.168.1.1           b4-2e-99-11-22-33     动态\n  192.168.1.8           a4:5e:60:aa:bb:cc     动态\n  192.168.1.9           不完整\n  224.0.0.22            01-00-5e-00-00-16     静态\n";
        let table = parse_arp(text);
        assert_eq!(table.get("192.168.1.1").map(String::as_str), Some("b4:2e:99:11:22:33"));
        assert_eq!(table.get("192.168.1.8").map(String::as_str), Some("a4:5e:60:aa:bb:cc"));
        assert_eq!(table.get("192.168.1.9"), None);
        assert_eq!(table.len(), 3);
    }

    #[test]
    fn mac_lookup_rejects_non_ipv4_input() {
        assert_eq!(mac_for("not-an-ip"), None);
        assert_eq!(mac_for("::1"), None);
    }
}
