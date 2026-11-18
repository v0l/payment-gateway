use anyhow::{Result, bail};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 198 && (18..=19).contains(&o[1]))
        || o[0] >= 224)
}

fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        || (s[0] == 0x64 && s[1] == 0xff9b))
}

pub struct Target {
    pub host: String,
    pub addr: SocketAddr,
}

pub async fn resolve_public(url: &str) -> Result<Target> {
    let parsed = url::Url::parse(url)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!("only http and https urls are allowed");
    }
    let Some(host) = parsed.host_str() else {
        bail!("url has no host");
    };
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await?.collect();
    if addrs.is_empty() {
        bail!("{host} did not resolve");
    }
    if let Some(bad) = addrs.iter().find(|a| !is_public_ip(a.ip())) {
        bail!("{host} resolves to a non-public address ({})", bad.ip());
    }
    Ok(Target {
        host: host.to_string(),
        addr: addrs[0],
    })
}

pub fn require_https(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url)?;
    if parsed.scheme() != "https" {
        bail!("url must be https");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("url must not contain credentials");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_special_ranges_are_blocked() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "10.245.31.103",
            "172.16.5.4",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fc00::1",
            "fd00:10:96::1",
            "fe80::1",
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip} should be blocked");
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        for ip in [
            "1.1.1.1",
            "8.8.8.8",
            "185.18.221.135",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip} should be allowed");
        }
    }

    #[tokio::test]
    async fn urls_pointing_inside_are_refused() {
        for u in [
            "http://127.0.0.1:8080/x",
            "http://localhost/x",
            "http://10.245.31.103:3306/",
            "http://[::1]/",
            "http://169.254.169.254/latest/meta-data",
            "ftp://example.com/",
            "not a url",
        ] {
            assert!(resolve_public(u).await.is_err(), "{u} should be refused");
        }
    }

    #[test]
    fn https_only_and_no_embedded_credentials() {
        assert!(require_https("https://lnd.example.com:10009").is_ok());
        assert!(require_https("http://lnd.example.com").is_err());
        assert!(require_https("https://user:pw@lnd.example.com").is_err());
    }
}
