use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;

#[derive(Clone, Copy)]
pub struct TrustedProxy {
    base: IpAddr,
    prefix: u8,
}

pub(crate) fn parse_trusted_proxies(raw: Option<String>) -> Vec<TrustedProxy> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    raw.split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            let (base, prefix) = match entry.split_once('/') {
                Some((base, prefix)) => (base, prefix.parse::<u8>().ok()?),
                None => (entry, if entry.contains(':') { 128 } else { 32 }),
            };
            let base: IpAddr = base.parse().ok()?;
            let maximum = if base.is_ipv4() { 32 } else { 128 };
            if prefix > maximum {
                return None;
            }
            Some(TrustedProxy { base, prefix })
        })
        .collect()
}

fn ip_bits(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(value) => u32::from(value) as u128,
        IpAddr::V6(value) => u128::from(value),
    }
}

pub(crate) fn is_trusted_proxy(ip: IpAddr, trusted: &[TrustedProxy]) -> bool {
    trusted.iter().any(|proxy| {
        if proxy.base.is_ipv4() != ip.is_ipv4() {
            return false;
        }
        let width = if ip.is_ipv4() { 32 } else { 128 };
        let shift = width - u32::from(proxy.prefix);
        (ip_bits(ip) >> shift) == (ip_bits(proxy.base) >> shift)
    })
}

pub(crate) fn client_ip(
    address: &SocketAddr,
    headers: &HeaderMap,
    trusted: &[TrustedProxy],
) -> IpAddr {
    let peer = address.ip();
    if !is_trusted_proxy(peer, trusted) {
        return peer;
    }
    if let Some(value) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        let candidates: Vec<IpAddr> = value
            .split(',')
            .filter_map(|entry| entry.trim().parse().ok())
            .collect();
        if let Some(client) = candidates
            .iter()
            .rev()
            .find(|candidate| !is_trusted_proxy(**candidate, trusted))
            .or_else(|| candidates.first())
        {
            return *client;
        }
    }
    if let Some(ip) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
    {
        return ip;
    }
    peer
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn socket(ip: &str) -> SocketAddr {
        format!("{ip}:1234").parse().unwrap()
    }

    fn forwarded_for(value: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(
            axum::http::HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_str(value).unwrap(),
        );
        map
    }

    #[test]
    fn parses_cidr_and_bare_entries() {
        let proxies = parse_trusted_proxies(Some("172.16.0.0/12, 10.0.0.5".to_owned()));
        assert_eq!(proxies.len(), 2);
        assert!(is_trusted_proxy("172.20.1.2".parse().unwrap(), &proxies));
        assert!(is_trusted_proxy("10.0.0.5".parse().unwrap(), &proxies));
        assert!(!is_trusted_proxy("172.32.0.1".parse().unwrap(), &proxies));
        assert!(!is_trusted_proxy("192.168.1.1".parse().unwrap(), &proxies));
    }

    #[test]
    fn invalid_prefix_is_dropped() {
        let proxies = parse_trusted_proxies(Some("10.0.0.0/99".to_owned()));
        assert!(proxies.is_empty());
        assert!(parse_trusted_proxies(None).is_empty());
    }

    #[test]
    fn direct_peer_is_used_when_proxy_is_not_trusted() {
        let proxies = parse_trusted_proxies(Some("172.16.0.0/12".to_owned()));
        let map = forwarded_for("8.8.8.8");
        assert_eq!(
            client_ip(&socket("203.0.113.9"), &map, &proxies),
            "203.0.113.9".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn trusted_proxy_resolves_forwarded_client() {
        let proxies = parse_trusted_proxies(Some("172.16.0.0/12".to_owned()));
        let map = forwarded_for("8.8.8.8, 172.16.0.9");
        assert_eq!(
            client_ip(&socket("172.16.0.9"), &map, &proxies),
            "8.8.8.8".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn untrusted_header_ignored_without_trusted_proxy_list() {
        let proxies = Vec::new();
        let map = forwarded_for("8.8.8.8");
        assert_eq!(
            client_ip(&socket("172.16.0.9"), &map, &proxies),
            "172.16.0.9".parse::<IpAddr>().unwrap()
        );
    }
}
