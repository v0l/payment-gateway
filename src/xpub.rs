use anyhow::{Context, Result, bail};
use bitcoin::base58;
use bitcoin::{Address, Network, NetworkKind};
use miniscript::descriptor::{DescriptorPublicKey, ShInner};
use miniscript::{Descriptor, ForEachKey};
use std::str::FromStr;

pub const PREVIEW_COUNT: u32 = 3;

const MAIN_PUBLIC: [u8; 4] = [0x04, 0x88, 0xB2, 0x1E];
const TEST_PUBLIC: [u8; 4] = [0x04, 0x35, 0x87, 0xCF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    Pkh,
    ShWpkh,
    Wpkh,
    Tr,
}

impl ScriptKind {
    pub fn name(self) -> &'static str {
        match self {
            ScriptKind::Pkh => "pkh",
            ScriptKind::ShWpkh => "sh(wpkh)",
            ScriptKind::Wpkh => "wpkh",
            ScriptKind::Tr => "tr",
        }
    }
}

#[derive(Debug, Clone)]
pub struct XpubSpec {
    descriptor: Descriptor<DescriptorPublicKey>,
    pub kind: ScriptKind,
    pub network: Network,
}

pub fn network_from_name(name: &str) -> Result<Network> {
    Ok(match name {
        "mainnet" | "bitcoin" => Network::Bitcoin,
        "testnet" => Network::Testnet,
        "signet" => Network::Signet,
        "regtest" => Network::Regtest,
        other => bail!("unknown bitcoin network {other}"),
    })
}

fn kind_of(version: [u8; 4]) -> Option<(ScriptKind, bool)> {
    Some(match version {
        [0x04, 0x88, 0xB2, 0x1E] => (ScriptKind::Pkh, true),
        [0x04, 0x9D, 0x7C, 0xB2] => (ScriptKind::ShWpkh, true),
        [0x04, 0xB2, 0x47, 0x46] => (ScriptKind::Wpkh, true),
        [0x04, 0x35, 0x87, 0xCF] => (ScriptKind::Pkh, false),
        [0x04, 0x4A, 0x52, 0x62] => (ScriptKind::ShWpkh, false),
        [0x04, 0x5F, 0x1C, 0xF6] => (ScriptKind::Wpkh, false),
        _ => return None,
    })
}

fn is_slip132_prefix(input: &str) -> bool {
    ["xpub", "ypub", "zpub", "tpub", "upub", "vpub"]
        .iter()
        .any(|p| input.starts_with(p))
}

fn normalise_extended(input: &str) -> Result<(String, ScriptKind, bool)> {
    let mut data =
        base58::decode_check(input).map_err(|_| anyhow::anyhow!("invalid extended public key"))?;
    if data.len() != 78 {
        bail!("invalid extended public key");
    }
    let version: [u8; 4] = data[..4].try_into().unwrap();
    let (kind, mainnet) = kind_of(version).context("unsupported extended key version")?;
    data[..4].copy_from_slice(if mainnet { &MAIN_PUBLIC } else { &TEST_PUBLIC });
    Ok((base58::encode_check(&data), kind, mainnet))
}

impl XpubSpec {
    pub fn parse(input: &str, network: Network) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            bail!("an extended public key or descriptor is needed");
        }
        let text = if is_slip132_prefix(input) && !input.contains('(') {
            let (xpub, kind, mainnet) = normalise_extended(input)?;
            if mainnet != (network == Network::Bitcoin) {
                bail!(
                    "this key is for {} but the gateway runs on {}",
                    if mainnet { "mainnet" } else { "a test network" },
                    network
                );
            }
            match kind {
                ScriptKind::Pkh => format!("pkh({xpub}/0/*)"),
                ScriptKind::ShWpkh => format!("sh(wpkh({xpub}/0/*))"),
                _ => format!("wpkh({xpub}/0/*)"),
            }
        } else {
            input.to_string()
        };
        let descriptor = Descriptor::<DescriptorPublicKey>::from_str(&text)
            .map_err(|e| anyhow::anyhow!("invalid descriptor: {e}"))?;
        Self::from_descriptor(descriptor, network)
    }

    fn from_descriptor(
        descriptor: Descriptor<DescriptorPublicKey>,
        network: Network,
    ) -> Result<Self> {
        let descriptor = if descriptor.is_multipath() {
            descriptor
                .into_single_descriptors()
                .map_err(|e| anyhow::anyhow!("invalid descriptor: {e}"))?
                .into_iter()
                .next()
                .context("empty multipath descriptor")?
        } else {
            descriptor
        };
        let kind = match &descriptor {
            Descriptor::Pkh(_) => ScriptKind::Pkh,
            Descriptor::Wpkh(_) => ScriptKind::Wpkh,
            Descriptor::Sh(sh) if matches!(sh.as_inner(), ShInner::Wpkh(_)) => ScriptKind::ShWpkh,
            Descriptor::Tr(tr) if tr.tap_tree().is_none() => ScriptKind::Tr,
            _ => bail!("descriptor must be pkh, sh(wpkh), wpkh or a key only tr"),
        };
        if !descriptor.has_wildcard() {
            bail!("descriptor must end in a wildcard such as /0/*");
        }
        let mut keys = 0;
        let mut wrong_network = false;
        let mut hardened = false;
        let mut bare = false;
        let want_main = network == Network::Bitcoin;
        descriptor.for_each_key(|k| {
            keys += 1;
            match k {
                DescriptorPublicKey::XPub(x) => {
                    if (x.xkey.network == NetworkKind::Main) != want_main {
                        wrong_network = true;
                    }
                    if matches!(x.wildcard, miniscript::descriptor::Wildcard::Hardened) {
                        hardened = true;
                    }
                }
                DescriptorPublicKey::MultiXPub(_) => bare = true,
                _ => bare = true,
            }
            true
        });
        if keys != 1 || bare {
            bail!("descriptor must use exactly one extended public key");
        }
        if wrong_network {
            bail!(
                "this key is for {} but the gateway runs on {}",
                if want_main {
                    "a test network"
                } else {
                    "mainnet"
                },
                network
            );
        }
        if hardened {
            bail!("a hardened wildcard cannot be derived from a public key");
        }
        let spec = Self {
            descriptor,
            kind,
            network,
        };
        spec.address(0)?;
        Ok(spec)
    }

    pub fn descriptor(&self) -> String {
        self.descriptor.to_string()
    }

    pub fn address(&self, index: u32) -> Result<Address> {
        let definite = self
            .descriptor
            .at_derivation_index(index)
            .map_err(|e| anyhow::anyhow!("cannot derive index {index}: {e}"))?;
        definite
            .address(self.network)
            .map_err(|e| anyhow::anyhow!("cannot build an address: {e}"))
    }

    pub fn addresses(&self, from: u32, count: u32) -> Result<Vec<String>> {
        (from..from + count)
            .map(|i| self.address(i).map(|a| a.to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
    use bitcoin::secp256k1::Secp256k1;

    const SEED_HEX: &str = "5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4";

    fn account_xpub(path: &str) -> Xpub {
        let secp = Secp256k1::new();
        let seed = hex::decode(SEED_HEX).unwrap();
        let master = Xpriv::new_master(Network::Bitcoin, &seed).unwrap();
        let path = DerivationPath::from_str(path).unwrap();
        Xpub::from_priv(&secp, &master.derive_priv(&secp, &path).unwrap())
    }

    fn slip132(xpub: &Xpub, version: [u8; 4]) -> String {
        let mut data = xpub.encode().to_vec();
        data[..4].copy_from_slice(&version);
        base58::encode_check(&data)
    }

    #[test]
    fn bip84_vectors_derive_exact_addresses() {
        let zpub = slip132(&account_xpub("m/84'/0'/0'"), [0x04, 0xB2, 0x47, 0x46]);
        assert_eq!(
            zpub,
            "zpub6rFR7y4Q2AijBEqTUquhVz398htDFrtymD9xYYfG1m4wAcvPhXNfE3EfH1r1ADqtfSdVCToUG868RvUUkgDKf31mGDtKsAYz2oz2AGutZYs"
        );
        let spec = XpubSpec::parse(&zpub, Network::Bitcoin).unwrap();
        assert_eq!(spec.kind, ScriptKind::Wpkh);
        assert_eq!(
            spec.addresses(0, 2).unwrap(),
            [
                "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu",
                "bc1qnjg0jd8228aq7egyzacy8cys3knf9xvrerkf9g"
            ]
        );
    }

    #[test]
    fn bip49_vectors_derive_exact_addresses() {
        let ypub = slip132(&account_xpub("m/49'/0'/0'"), [0x04, 0x9D, 0x7C, 0xB2]);
        assert_eq!(
            ypub,
            "ypub6Ww3ibxVfGzLrAH1PNcjyAWenMTbbAosGNB6VvmSEgytSER9azLDWCxoJwW7Ke7icmizBMXrzBx9979FfaHxHcrArf3zbeJJJUZPf663zsP"
        );
        let spec = XpubSpec::parse(&ypub, Network::Bitcoin).unwrap();
        assert_eq!(spec.kind, ScriptKind::ShWpkh);
        assert_eq!(
            spec.addresses(0, 1).unwrap(),
            ["37VucYSaXLCAsxYyAPfbSi9eh4iEcbShgf"]
        );
    }

    #[test]
    fn bip44_vectors_derive_exact_addresses() {
        let xpub = account_xpub("m/44'/0'/0'").to_string();
        let spec = XpubSpec::parse(&xpub, Network::Bitcoin).unwrap();
        assert_eq!(spec.kind, ScriptKind::Pkh);
        assert_eq!(
            spec.addresses(0, 2).unwrap(),
            [
                "1LqBGSKuX5yYUonjxT5qGfpUsXKYYWeabA",
                "1Ak8PffB2meyfYnbXZR9EGfLfFZVpzJvQP"
            ]
        );
    }

    #[test]
    fn bip86_vectors_derive_exact_addresses() {
        let xpub = account_xpub("m/86'/0'/0'");
        let descriptor = format!("tr({xpub}/0/*)");
        let spec = XpubSpec::parse(&descriptor, Network::Bitcoin).unwrap();
        assert_eq!(spec.kind, ScriptKind::Tr);
        assert_eq!(
            spec.addresses(0, 2).unwrap(),
            [
                "bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr",
                "bc1p4qhjn9zdvkux4e44uhx8tc55attvtyu358kutcqkudyccelu0was9fqzwh"
            ]
        );
    }

    #[test]
    fn descriptor_with_origin_and_valid_checksum_is_accepted() {
        let xpub = account_xpub("m/84'/0'/0'");
        let fp = account_xpub("m/84'/0'").fingerprint();
        let bare = format!("wpkh([{fp}/84'/0'/0']{xpub}/0/*)");
        let parsed = XpubSpec::parse(&bare, Network::Bitcoin).unwrap();
        let with_checksum = parsed.descriptor();
        assert!(with_checksum.contains('#'));
        let again = XpubSpec::parse(&with_checksum, Network::Bitcoin).unwrap();
        assert_eq!(again.address(0).unwrap(), parsed.address(0).unwrap());
        assert_eq!(
            parsed.address(0).unwrap().to_string(),
            "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"
        );
    }

    #[test]
    fn descriptor_with_corrupted_checksum_is_refused() {
        let xpub = account_xpub("m/84'/0'/0'");
        let good = XpubSpec::parse(&format!("wpkh({xpub}/0/*)"), Network::Bitcoin)
            .unwrap()
            .descriptor();
        let (body, sum) = good.split_once('#').unwrap();
        let flipped = if sum.starts_with('q') { 'p' } else { 'q' };
        let bad = format!("{body}#{flipped}{}", &sum[1..]);
        assert!(XpubSpec::parse(&bad, Network::Bitcoin).is_err());
    }

    #[test]
    fn mainnet_key_is_refused_on_regtest_and_back() {
        let zpub = slip132(&account_xpub("m/84'/0'/0'"), [0x04, 0xB2, 0x47, 0x46]);
        assert!(
            XpubSpec::parse(&zpub, Network::Regtest)
                .unwrap_err()
                .to_string()
                .contains("mainnet")
        );
        let xpub = account_xpub("m/84'/0'/0'").to_string();
        assert!(XpubSpec::parse(&format!("wpkh({xpub}/0/*)"), Network::Regtest).is_err());
        let vpub = slip132(&account_xpub("m/84'/1'/0'"), [0x04, 0x5F, 0x1C, 0xF6]);
        assert!(XpubSpec::parse(&vpub, Network::Bitcoin).is_err());
        let spec = XpubSpec::parse(&vpub, Network::Regtest).unwrap();
        assert!(spec.address(0).unwrap().to_string().starts_with("bcrt1q"));
    }

    #[test]
    fn private_keys_and_unsupported_shapes_are_refused() {
        let master = Xpriv::new_master(Network::Bitcoin, &[7u8; 32]).unwrap();
        assert!(XpubSpec::parse(&format!("wpkh({master}/0/*)"), Network::Bitcoin).is_err());
        let xpub = account_xpub("m/84'/0'/0'");
        assert!(XpubSpec::parse(&format!("wpkh({xpub}/0)"), Network::Bitcoin).is_err());
        assert!(XpubSpec::parse(&format!("wpkh({xpub}/0/*')"), Network::Bitcoin).is_err());
        assert!(XpubSpec::parse("", Network::Bitcoin).is_err());
        assert!(XpubSpec::parse("xpub-nonsense", Network::Bitcoin).is_err());
        assert!(
            XpubSpec::parse(
                &format!("wsh(multi(1,{xpub}/0/*,{xpub}/1/*))"),
                Network::Bitcoin
            )
            .is_err()
        );
    }

    #[test]
    fn multipath_descriptor_uses_the_receive_chain() {
        let xpub = account_xpub("m/84'/0'/0'");
        let spec = XpubSpec::parse(&format!("wpkh({xpub}/<0;1>/*)"), Network::Bitcoin).unwrap();
        assert_eq!(
            spec.address(0).unwrap().to_string(),
            "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"
        );
    }
}
