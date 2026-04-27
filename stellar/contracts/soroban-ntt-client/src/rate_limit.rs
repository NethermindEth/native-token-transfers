use soroban_sdk::{contracttype, Env};

/// Token bucket rate limiter for controlling transfer throughput.
///
/// Capacity refills linearly over the configured duration. For example,
/// with `limit=1000` and `duration=86400` (24h), capacity refills at
/// ~0.0116 tokens per second.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct RateLimitParams {
    /// Maximum capacity (bucket size).
    pub limit: u64,
    /// Capacity remaining at `last_tx_timestamp`.
    pub current_capacity: u64,
    /// Timestamp of the last capacity update.
    pub last_tx_timestamp: u64,
}

impl RateLimitParams {
    /// Creates a new rate limiter with full capacity.
    pub fn new(limit: u64, env: &Env) -> Self {
        Self {
            limit,
            current_capacity: limit,
            last_tx_timestamp: env.ledger().timestamp(),
        }
    }

    /// Calculates current capacity accounting for time-based refill.
    ///
    /// Uses the configured rate limit duration for the refill period.
    pub fn capacity_at(&self, env: &Env, duration: u64) -> u64 {
        let now = env.ledger().timestamp();
        let time_passed = now.saturating_sub(self.last_tx_timestamp);
        let refill_u128 = (self.limit as u128) * (time_passed as u128) / (duration as u128);
        let refill = core::cmp::min(refill_u128, self.limit as u128) as u64;

        core::cmp::min(self.current_capacity.saturating_add(refill), self.limit)
    }

    /// Attempts to consume capacity for a transfer.
    ///
    /// Returns `None` if sufficient capacity exists, otherwise returns
    /// `Some(timestamp)` indicating when the transfer can be released.
    pub fn consume_or_delay(&mut self, amount: u64, env: &Env, duration: u64) -> Option<u64> {
        let now = env.ledger().timestamp();
        let capacity = self.capacity_at(env, duration);

        if capacity >= amount {
            self.current_capacity = capacity - amount;
            self.last_tx_timestamp = now;
            None
        } else {
            let deficit = amount - capacity;
            // Guard against division by zero: limit == 0 means "queue all transfers"
            // (valid configuration, matching EVM behavior). Fall back to the full
            // rate limit duration since no capacity will ever refill.
            let time_needed = if self.limit > 0 {
                ((deficit as u128) * (duration as u128) / (self.limit as u128))
                    .min(u64::MAX as u128) as u64
            } else {
                duration
            };
            Some(now + time_needed + 1)
        }
    }

    /// Adds capacity back to the bucket (backflow from reverse transfers).
    ///
    /// Inbound transfers refill outbound capacity and vice versa. Capacity
    /// is capped at `limit`.
    pub fn refill(&mut self, amount: u64, env: &Env, duration: u64) {
        let now = env.ledger().timestamp();
        let current = self.capacity_at(env, duration);
        self.current_capacity = core::cmp::min(current.saturating_add(amount), self.limit);
        self.last_tx_timestamp = now;
    }

    /// Updates the rate limit, adjusting capacity proportionally.
    ///
    /// Preserves the amount already consumed (`limit - capacity`) so that
    /// changing the limit doesn't instantly create or destroy usable capacity.
    /// For example, if 300 of 1000 was consumed and the limit increases to
    /// 2000, the new capacity is 1700 (not 2000).
    pub fn set_limit(&mut self, new_limit: u64, env: &Env, duration: u64) {
        let now = env.ledger().timestamp();
        let current = self.capacity_at(env, duration);
        let old_limit = self.limit;

        self.current_capacity = if new_limit < old_limit {
            current.saturating_sub(old_limit - new_limit)
        } else {
            core::cmp::min(current.saturating_add(new_limit - old_limit), new_limit)
        };

        self.limit = new_limit;
        self.last_tx_timestamp = now;
    }
}
