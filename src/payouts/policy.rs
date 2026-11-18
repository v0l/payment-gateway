use bitcoin::Network;
use chrono::Duration;

#[derive(Clone, Debug)]
pub struct PayoutPolicy {
    pub network: Network,
    pub min_amount: u64,
    pub daily_max_trusted: u64,
    pub daily_max_untrusted: u64,
    pub auto_threshold: u64,
    pub destination_delay: Duration,
    pub verify_amount_lightning: u64,
    pub verify_amount_onchain: u64,
    pub lightning_fee_ppm: u64,
    pub lightning_fee_floor: u64,
    pub onchain_fee_reserve: u64,
    pub onchain_sat_per_vbyte: u64,
    pub allow_private_lnurl: bool,
    pub sending_lease: Duration,
    pub retry_delay: Duration,
    pub max_attempts: u32,
}

const SAT: u64 = 1000;

impl Default for PayoutPolicy {
    fn default() -> Self {
        Self {
            network: Network::Bitcoin,
            min_amount: 1_000 * SAT,
            daily_max_trusted: 10_000_000 * SAT,
            daily_max_untrusted: 1_000_000 * SAT,
            auto_threshold: 1_000_000 * SAT,
            destination_delay: Duration::hours(24),
            verify_amount_lightning: SAT,
            verify_amount_onchain: 1_000 * SAT,
            lightning_fee_ppm: 10_000,
            lightning_fee_floor: 5 * SAT,
            onchain_fee_reserve: 5_000 * SAT,
            onchain_sat_per_vbyte: 10,
            allow_private_lnurl: false,
            sending_lease: Duration::minutes(5),
            retry_delay: Duration::minutes(1),
            max_attempts: 5,
        }
    }
}

impl PayoutPolicy {
    pub fn from_env(env: &mut crate::config::Env) -> Self {
        let mut p = Self::default();
        if let Some(n) = env.optional("GATEWAY_PAYOUT_NETWORK") {
            match n.trim().parse() {
                Ok(n) => p.network = n,
                Err(_) => env.fail("GATEWAY_PAYOUT_NETWORK must be bitcoin, testnet, signet or regtest".into()),
            }
        }
        let sats = |env: &mut crate::config::Env, name: &str, default: u64| {
            env.number(name, default / SAT, 0u64, 21_000_000u64 * 100_000_000) * SAT
        };
        p.min_amount = sats(env, "GATEWAY_PAYOUT_MIN_SATS", p.min_amount);
        p.daily_max_trusted = sats(env, "GATEWAY_PAYOUT_DAILY_MAX_SATS", p.daily_max_trusted);
        p.daily_max_untrusted = sats(
            env,
            "GATEWAY_PAYOUT_DAILY_MAX_UNTRUSTED_SATS",
            p.daily_max_untrusted,
        );
        p.auto_threshold = sats(env, "GATEWAY_PAYOUT_AUTO_SATS", p.auto_threshold);
        let hours = env.number("GATEWAY_PAYOUT_DESTINATION_DELAY_HOURS", 24i64, 0, 24 * 365);
        p.destination_delay = Duration::hours(hours);
        p.onchain_sat_per_vbyte = env.number("GATEWAY_PAYOUT_SAT_PER_VBYTE", 10u64, 1, 10_000);
        p
    }

    pub fn lightning_max_fee(&self, amount_msat: u64) -> u64 {
        (amount_msat.saturating_mul(self.lightning_fee_ppm) / 1_000_000)
            .max(self.lightning_fee_floor)
    }

    pub fn daily_max(&self, trusted: bool) -> u64 {
        if trusted {
            self.daily_max_trusted
        } else {
            self.daily_max_untrusted
        }
    }
}
