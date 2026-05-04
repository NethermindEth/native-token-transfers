use soroban_sdk::{contracttype, Env};

use crate::storage::InstanceStorage;

/// Default rate-limit refill window (24 hours).
///
/// The bucket fully refills over this period.
pub const RATE_LIMIT_DURATION: u64 = 86400;

/// Result of attempting to consume rate limit capacity.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum RateLimitResult {
    /// Transfer can proceed immediately; capacity was consumed.
    Consumed,
    /// Transfer must be queued until the specified timestamp.
    Delayed(u64),
}

/// Attempts to consume outbound rate limit capacity.
///
/// If consumed, updates storage. If delayed, returns the release timestamp
/// without modifying storage — the caller decides whether to queue.
pub fn consume_or_delay_outbound(env: &Env, amount: u64) -> RateLimitResult {
    let storage = InstanceStorage::new(env);
    let mut rate_limit = storage.outbound_rate_limit();
    let duration = storage.rate_limit_duration();
    let result = match rate_limit.consume_or_delay(amount, env, duration) {
        Some(release_timestamp) => RateLimitResult::Delayed(release_timestamp),
        None => RateLimitResult::Consumed,
    };

    if matches!(result, RateLimitResult::Consumed) {
        storage.set_outbound_rate_limit(&rate_limit);
    }

    result
}

/// Refills outbound capacity when an inbound transfer completes.
pub fn refill_outbound(env: &Env, amount: u64) {
    let storage = InstanceStorage::new(env);
    let mut rate_limit = storage.outbound_rate_limit();
    let duration = storage.rate_limit_duration();
    rate_limit.refill(amount, env, duration);
    storage.set_outbound_rate_limit(&rate_limit);
}
