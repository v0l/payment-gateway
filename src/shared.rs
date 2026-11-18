pub const SHARED_KEY_BASE: u64 = 1 << 62;
pub const BPS_DENOMINATOR: u128 = 10_000;
pub const MAX_FEE_BPS: u32 = 10_000;

pub const NETWORKS: &[&str] = &["mainnet", "testnet", "signet", "regtest"];
pub const TEST_NETWORKS: &[&str] = &["testnet", "signet", "regtest"];

pub fn shared_key(shared_id: u64) -> u64 {
    SHARED_KEY_BASE + shared_id
}

pub fn shared_id_of_key(key: u64) -> Option<u64> {
    key.checked_sub(SHARED_KEY_BASE)
        .filter(|_| key >= SHARED_KEY_BASE)
}

pub fn is_test_network(network: &str) -> bool {
    TEST_NETWORKS.contains(&network)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeTerms {
    pub bps: u32,
    pub base_msat: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    pub fee: u64,
    pub net: u64,
}

pub fn split(received_msat: u64, terms: FeeTerms) -> Split {
    let proportional = (received_msat as u128 * terms.bps as u128).div_ceil(BPS_DENOMINATOR);
    let fee = proportional
        .saturating_add(terms.base_msat as u128)
        .min(received_msat as u128) as u64;
    Split {
        fee,
        net: received_msat - fee,
    }
}

pub fn may_use(trusted: bool, network: &str, allow_trusted: bool, allow_untrusted: bool) -> bool {
    if trusted {
        allow_trusted
    } else {
        allow_untrusted && is_test_network(network)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(bps: u32, base: u64) -> FeeTerms {
        FeeTerms {
            bps,
            base_msat: base,
        }
    }

    #[test]
    fn fee_rounds_up_and_net_is_the_remainder() {
        let s = split(1_000_001, t(100, 0));
        assert_eq!(s.fee, 10_001);
        assert_eq!(s.net, 990_000);
        assert_eq!(s.fee + s.net, 1_000_001);
    }

    #[test]
    fn exact_percentages_do_not_round_up() {
        assert_eq!(split(1_000_000, t(100, 0)).fee, 10_000);
    }

    #[test]
    fn tiny_amounts_still_pay_a_fee() {
        assert_eq!(split(1, t(1, 0)).fee, 1);
    }

    #[test]
    fn base_fee_adds_and_is_capped_at_the_amount() {
        assert_eq!(split(100_000, t(0, 1_000)).fee, 1_000);
        let s = split(500, t(0, 1_000));
        assert_eq!(s.fee, 500);
        assert_eq!(s.net, 0);
    }

    #[test]
    fn zero_terms_take_nothing() {
        assert_eq!(split(5_000, t(0, 0)), Split { fee: 0, net: 5_000 });
    }

    #[test]
    fn tenant_is_never_credited_more_than_received() {
        for received in [0u64, 1, 999, 1_000, 123_457, u32::MAX as u64, u64::MAX / 2] {
            for bps in [0u32, 1, 33, 250, 9_999, 10_000] {
                for base in [0u64, 1, 1_000, u32::MAX as u64] {
                    let s = split(received, t(bps, base));
                    assert!(s.net <= received);
                    assert_eq!(s.fee + s.net, received);
                }
            }
        }
    }

    #[test]
    fn key_space_round_trips_and_stays_apart_from_config_ids() {
        assert_eq!(shared_id_of_key(shared_key(7)), Some(7));
        assert_eq!(shared_id_of_key(7), None);
    }

    #[test]
    fn untrusted_projects_only_get_test_networks() {
        assert!(!may_use(false, "mainnet", true, true));
        assert!(may_use(false, "regtest", true, true));
        assert!(may_use(false, "signet", true, true));
        assert!(!may_use(false, "regtest", true, false));
        assert!(may_use(true, "mainnet", true, false));
        assert!(!may_use(true, "mainnet", false, true));
    }
}
