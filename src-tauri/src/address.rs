use std::{net::{IpAddr, SocketAddr}, str::FromStr};

/// Parse a literal IP address and port; discovery names are not identity evidence.
pub fn parse_manual_address(input: &str) -> Result<SocketAddr, &'static str> {
    if input.len() > 128 || input.trim() != input {
        return Err("请输入 IP:端口；IPv6 地址需使用方括号");
    }
    let address = SocketAddr::from_str(input).map_err(|_| "请输入有效的 IP:端口")?;
    if address.port() == 0 || !is_unicast(address.ip()) {
        return Err("地址必须是单播地址，端口须为 1–65535");
    }
    Ok(address)
}

pub fn is_unicast(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => !value.is_unspecified() && !value.is_multicast() && !value.is_broadcast(),
        IpAddr::V6(value) => !value.is_unspecified() && !value.is_multicast(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ipv4_and_bracketed_ipv6() {
        assert_eq!(parse_manual_address("192.168.1.8:42853").unwrap().port(), 42853);
        assert_eq!(parse_manual_address("[::1]:443").unwrap().port(), 443);
        assert!(parse_manual_address("[fe80::1%3]:42853").is_ok());
    }

    #[test]
    fn rejects_ambiguous_or_non_unicast_addresses() {
        for value in ["", "host.local:42", " 127.0.0.1:1", "127.0.0.1:0", "0.0.0.0:1", "255.255.255.255:2", "224.0.0.1:2", "[ff02::1]:4", "[::]:3", "::1:44", "127.0.0.1:65536"] {
            assert!(parse_manual_address(value).is_err(), "accepted {value}");
        }
    }
}
