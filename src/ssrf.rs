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
        || (o[0] == 192 && o[1] == 88 && o[2] == 99)
        || o[0] >= 224)
}

fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = ip.segments();
    if s[..6].iter().all(|&x| x == 0) {
        return public_v4(Ipv4Addr::new(
            (s[6] >> 8) as u8,
            s[6] as u8,
            (s[7] >> 8) as u8,
            s[7] as u8,
        ));
    }
    if s[0] == 0x2002 {
        return public_v4(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            s[1] as u8,
            (s[2] >> 8) as u8,
            s[2] as u8,
        ));
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        || (s[0] == 0x2001 && s[1] == 0)
        || (s[0] == 0x64 && s[1] == 0xff9b)
        || (s[0] == 0x100 && s[1] == 0 && s[2] == 0 && s[3] == 0))
}

pub struct Target {
    pub host: String,
    pub addr: SocketAddr,
}

pub fn pick_public(host: &str, addrs: &[SocketAddr]) -> Result<SocketAddr> {
    if addrs.is_empty() {
        bail!("{host} did not resolve");
    }
    if let Some(bad) = addrs.iter().find(|a| !is_public_ip(a.ip())) {
        bail!("{host} resolves to a non-public address ({})", bad.ip());
    }
    Ok(addrs[0])
}

#[async_trait::async_trait]
pub trait Resolver: Send + Sync {
    async fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>>;
}

pub struct SystemResolver;

#[async_trait::async_trait]
impl Resolver for SystemResolver {
    async fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        Ok(tokio::net::lookup_host((host, port)).await?.collect())
    }
}

pub async fn resolve_public(url: &str) -> Result<Target> {
    resolve_public_with(&SystemResolver, url).await
}

pub async fn resolve_public_with(resolver: &dyn Resolver, url: &str) -> Result<Target> {
    let parsed = url::Url::parse(url)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!("only http and https urls are allowed");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("url must not contain credentials");
    }
    let port = parsed.port_or_known_default().unwrap_or(443);
    let (host, addrs): (String, Vec<SocketAddr>) = match parsed.host() {
        None => bail!("url has no host"),
        Some(url::Host::Ipv4(ip)) => (ip.to_string(), vec![SocketAddr::new(ip.into(), port)]),
        Some(url::Host::Ipv6(ip)) => (ip.to_string(), vec![SocketAddr::new(ip.into(), port)]),
        Some(url::Host::Domain(d)) => (d.to_string(), resolver.lookup(d, port).await?),
    };
    let addr = pick_public(&host, &addrs)?;
    Ok(Target { host, addr })
}

pub async fn pin_url(resolver: &dyn Resolver, url: &str) -> Result<String> {
    let target = resolve_public_with(resolver, url).await?;
    let mut parsed = url::Url::parse(url)?;
    let set = match target.addr.ip() {
        IpAddr::V4(ip) => parsed.set_ip_host(IpAddr::V4(ip)),
        IpAddr::V6(ip) => parsed.set_ip_host(IpAddr::V6(ip)),
    };
    set.map_err(|_| anyhow::anyhow!("cannot pin {url}"))?;
    Ok(parsed.to_string())
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

    struct Flipping(std::sync::Mutex<Vec<Vec<SocketAddr>>>);

    #[async_trait::async_trait]
    impl Resolver for Flipping {
        async fn lookup(&self, _host: &str, _port: u16) -> Result<Vec<SocketAddr>> {
            Ok(self.0.lock().unwrap().remove(0))
        }
    }

    #[tokio::test]
    async fn pinned_url_uses_the_checked_address_when_dns_flips() {
        let r = Flipping(std::sync::Mutex::new(vec![
            vec!["8.8.8.8:10009".parse().unwrap()],
            vec!["10.0.0.5:10009".parse().unwrap()],
        ]));
        let pinned = pin_url(&r, "https://lnd.example.com:10009").await.unwrap();
        assert_eq!(pinned, "https://8.8.8.8:10009/");
        assert_eq!(r.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn pinning_refuses_an_internal_first_answer() {
        let r = Flipping(std::sync::Mutex::new(vec![vec![
            "10.0.0.5:10009".parse().unwrap(),
        ]]));
        assert!(pin_url(&r, "https://lnd.example.com:10009").await.is_err());
    }

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
            "::127.0.0.1",
            "::10.0.0.1",
            "2002:7f00:1::1",
            "2002:a9fe:a9fe::1",
            "fec0::1",
            "64:ff9b::1",
            "192.88.99.1",
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
            "http://2130706433/",
            "http://0x7f000001/",
            "http://0177.0.0.1/",
            "http://127.1/",
            "http://0/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:7f00:1]/",
            "http://[::127.0.0.1]/",
            "http://user@127.0.0.1/",
            "http://example.com@127.0.0.1/",
            "http://8.8.8.8@127.0.0.1/",
            "http://8.8.8.8:80@10.0.0.1/",
            "http://127.0.0.1%2f@8.8.8.8/",
            "not a url",
        ] {
            assert!(resolve_public(u).await.is_err(), "{u} should be refused");
        }
    }

    #[tokio::test]
    async fn public_ip_literals_resolve_without_dns() {
        let t = resolve_public("http://8.8.8.8:8080/x").await.unwrap();
        assert_eq!(t.addr, "8.8.8.8:8080".parse().unwrap());
    }

    #[test]
    fn any_private_answer_among_many_refuses_the_host() {
        let ok: SocketAddr = "8.8.8.8:80".parse().unwrap();
        let bad: SocketAddr = "10.0.0.1:80".parse().unwrap();
        assert!(pick_public("h", &[ok, bad]).is_err());
        assert!(pick_public("h", &[]).is_err());
        assert_eq!(pick_public("h", &[ok]).unwrap(), ok);
    }

    #[test]
    fn https_only_and_no_embedded_credentials() {
        assert!(require_https("https://lnd.example.com:10009").is_ok());
        assert!(require_https("http://lnd.example.com").is_err());
        assert!(require_https("https://user:pw@lnd.example.com").is_err());
    }
}
