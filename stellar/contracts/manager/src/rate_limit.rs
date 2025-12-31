use soroban_sdk::{contracttype, Env};

use crate::constants::DEFAULT_RATE_LIMIT_DURATION;
use crate::state::DataKey;

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

/// Result of attempting to consume rate limit capacity.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum RateLimitResult {
    /// Transfer can proceed immediately; capacity was consumed.
    Consumed,
    /// Transfer must be queued until the specified timestamp.
    Delayed(u64),
}

impl RateLimitParams {
    /// Creates a new rate limiter with full capacity.
    pub fn new(limit: u64, now: u64) -> Self {
        Self {
            limit,
            current_capacity: limit,
            last_tx_timestamp: now,
        }
    }

    /// Calculates current capacity accounting for time-based refill.
    ///
    /// Returns `limit` if `duration` is zero (unlimited mode).
    pub fn capacity_at(&self, now: u64, duration: u64) -> u64 {
        if duration == 0 {
            return self.limit;
        }

        let time_passed = now.saturating_sub(self.last_tx_timestamp);
        let refill = ((self.limit as u128) * (time_passed as u128) / (duration as u128)) as u64;

        core::cmp::min(self.current_capacity.saturating_add(refill), self.limit)
    }

    /// Attempts to consume capacity for a transfer.
    ///
    /// Returns `Consumed` if sufficient capacity exists, otherwise returns
    /// `Delayed(timestamp)` indicating when the transfer can be released.
    pub fn consume_or_delay(&mut self, amount: u64, now: u64, duration: u64) -> RateLimitResult {
        let capacity = self.capacity_at(now, duration);

        if capacity >= amount {
            self.current_capacity = capacity - amount;
            self.last_tx_timestamp = now;
            RateLimitResult::Consumed
        } else {
            let deficit = amount - capacity;
            let time_needed = if self.limit > 0 {
                ((deficit as u128) * (duration as u128) / (self.limit as u128)) as u64
            } else {
                duration
            };
            let release_timestamp = now + time_needed + 1;
            RateLimitResult::Delayed(release_timestamp)
        }
    }

    /// Adds capacity back to the bucket (backflow from reverse transfers).
    ///
    /// Inbound transfers refill outbound capacity and vice versa. Capacity
    /// is capped at `limit`.
    pub fn refill(&mut self, amount: u64, now: u64, duration: u64) {
        let current = self.capacity_at(now, duration);
        self.current_capacity = core::cmp::min(current.saturating_add(amount), self.limit);
        self.last_tx_timestamp = now;
    }

    /// Updates the rate limit, adjusting capacity proportionally.
    ///
    /// When reducing the limit, capacity is reduced by the difference.
    /// When increasing, capacity grows but stays capped at the new limit.
    pub fn set_limit(&mut self, new_limit: u64, now: u64, duration: u64) {
        let current = self.capacity_at(now, duration);
        let old_limit = self.limit;

        self.current_capacity = if new_limit < old_limit {
            current.saturating_sub(old_limit - new_limit)
        } else {
            core::cmp::min(current.saturating_add(new_limit - old_limit), new_limit)
        };

        self.limit = new_limit;
        self.last_tx_timestamp = now;
    }

    /// Returns whether the given amount can be consumed without queueing.
    pub fn can_consume(&self, amount: u64, now: u64, duration: u64) -> bool {
        self.capacity_at(now, duration) >= amount
    }

    /// Returns the current available capacity.
    pub fn available_capacity(&self, now: u64, duration: u64) -> u64 {
        self.capacity_at(now, duration)
    }
}

/// Retrieves the configured rate limit duration in seconds.
///
/// Defaults to `DEFAULT_RATE_LIMIT_DURATION` (24 hours) if not set.
pub fn get_rate_limit_duration(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::RateLimitDuration)
        .unwrap_or(DEFAULT_RATE_LIMIT_DURATION)
}

/// Retrieves the current outbound rate limit parameters.
///
/// Returns a new `RateLimitParams` with unlimited capacity if not initialized.
pub fn get_outbound_rate_limit(env: &Env) -> RateLimitParams {
    env.storage()
        .instance()
        .get(&DataKey::OutboundRateLimit)
        .unwrap_or_else(|| RateLimitParams::new(u64::MAX, env.ledger().timestamp()))
}

/// Attempts to consume outbound rate limit capacity.
///
/// If consumed, updates storage. If delayed, storage is unchanged and the
/// caller should queue the transfer for later execution.
pub fn consume_or_queue_outbound(env: &Env, amount: u64) -> RateLimitResult {
    let duration = get_rate_limit_duration(env);
    let now = env.ledger().timestamp();

    let mut rate_limit = get_outbound_rate_limit(env);
    let result = rate_limit.consume_or_delay(amount, now, duration);

    if matches!(result, RateLimitResult::Consumed) {
        env.storage()
            .instance()
            .set(&DataKey::OutboundRateLimit, &rate_limit);
    }

    result
}

/// Refills outbound capacity when an inbound transfer completes.
pub fn refill_outbound(env: &Env, amount: u64) {
    let duration = get_rate_limit_duration(env);
    let now = env.ledger().timestamp();

    let mut rate_limit = get_outbound_rate_limit(env);
    rate_limit.refill(amount, now, duration);
    env.storage()
        .instance()
        .set(&DataKey::OutboundRateLimit, &rate_limit);
}
