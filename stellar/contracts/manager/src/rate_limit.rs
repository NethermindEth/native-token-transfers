use soroban_sdk::contracttype;

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct RateLimitParams {
    pub limit: u64,
    pub current_capacity: u64,
    pub last_tx_timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum RateLimitResult {
    Consumed,
    Delayed(u64),
}

impl RateLimitParams {
    pub fn new(limit: u64, now: u64) -> Self {
        Self {
            limit,
            current_capacity: limit,
            last_tx_timestamp: now,
        }
    }

    pub fn capacity_at(&self, now: u64, duration: u64) -> u64 {
        if duration == 0 {
            return self.limit;
        }

        let time_passed = now.saturating_sub(self.last_tx_timestamp);
        let refill = ((self.limit as u128) * (time_passed as u128) / (duration as u128)) as u64;

        core::cmp::min(self.current_capacity.saturating_add(refill), self.limit)
    }

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

    pub fn refill(&mut self, amount: u64, now: u64, duration: u64) {
        let current = self.capacity_at(now, duration);
        self.current_capacity = core::cmp::min(current.saturating_add(amount), self.limit);
        self.last_tx_timestamp = now;
    }

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

    pub fn can_consume(&self, amount: u64, now: u64, duration: u64) -> bool {
        self.capacity_at(now, duration) >= amount
    }

    pub fn available_capacity(&self, now: u64, duration: u64) -> u64 {
        self.capacity_at(now, duration)
    }
}