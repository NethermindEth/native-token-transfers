//! Type-safe storage wrappers with automatic TTL extension.

use core::fmt::Debug;
use core::marker::PhantomData;
use soroban_ntt_client::{
    NttManagerError, RateLimitParams, RATE_LIMIT_DURATION, TTL_EXTEND, TTL_THRESHOLD,
};
use soroban_sdk::{Address, BytesN, Env, IntoVal, TryFromVal, Val};

use crate::{
    state::{
        AttestationInfo, DataKey, InboundQueuedTransfer, Mode, NttManagerPeer,
        OutboundQueuedTransfer,
    },
    transceivers::TransceiverInfo,
};

/// Typed accessor for instance storage with automatic TTL extension on construction.
pub struct InstanceStorage<'a> {
    env: &'a Env,
}

impl<'a> InstanceStorage<'a> {
    /// Creates the accessor, extending instance TTL.
    #[inline]
    pub fn new(env: &'a Env) -> Self {
        env.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
        Self { env }
    }

    // --- Private helpers ---

    #[inline]
    fn read_key<T: TryFromVal<Env, Val>>(&self, key: &DataKey) -> Result<T, NttManagerError>
    where
        T::Error: Debug,
    {
        self.env
            .storage()
            .instance()
            .get(key)
            .ok_or(NttManagerError::NotInitialized)
    }

    #[inline]
    fn read_key_or<T: TryFromVal<Env, Val>>(&self, key: &DataKey, default: T) -> T
    where
        T::Error: Debug,
    {
        self.env.storage().instance().get(key).unwrap_or(default)
    }

    #[inline]
    fn write_key<T: IntoVal<Env, Val>>(&self, key: &DataKey, value: &T) {
        self.env.storage().instance().set(key, value);
    }

    // --- Required getters (fail if uninitialized) ---

    #[inline]
    pub fn admin(&self) -> Result<Address, NttManagerError> {
        self.read_key(&DataKey::Admin)
    }

    #[inline]
    pub fn token(&self) -> Result<Address, NttManagerError> {
        self.read_key(&DataKey::Token)
    }

    #[inline]
    pub fn token_decimals(&self) -> Result<u32, NttManagerError> {
        self.read_key(&DataKey::TokenDecimals)
    }

    #[inline]
    pub fn mode(&self) -> Result<Mode, NttManagerError> {
        self.read_key(&DataKey::Mode)
    }

    #[inline]
    pub fn chain_id(&self) -> Result<u32, NttManagerError> {
        self.read_key(&DataKey::ChainId)
    }

    // --- Optional getters ---

    #[inline]
    pub fn pending_admin(&self) -> Option<Address> {
        self.env.storage().instance().get(&DataKey::PendingAdmin)
    }

    #[inline]
    pub fn pauser(&self) -> Option<Address> {
        self.env.storage().instance().get(&DataKey::Pauser)
    }

    // --- Default-value getters ---

    #[inline]
    pub fn threshold(&self) -> u32 {
        self.read_key_or(&DataKey::Threshold, 0)
    }

    #[inline]
    pub fn next_sequence(&self) -> u64 {
        self.read_key_or(&DataKey::NextSequence, 1)
    }

    #[inline]
    pub fn version(&self) -> u32 {
        self.read_key_or(&DataKey::Version, 1)
    }

    #[inline]
    pub fn transceiver_count(&self) -> u32 {
        self.read_key_or(&DataKey::TransceiverCount, 0)
    }

    #[inline]
    pub fn enabled_bitmap(&self) -> u64 {
        self.read_key_or(&DataKey::EnabledBitmap, 0)
    }

    /// Defaults to 24 hours (86400 seconds).
    #[inline]
    pub fn rate_limit_duration(&self) -> u64 {
        self.read_key_or(&DataKey::RateLimitDuration, RATE_LIMIT_DURATION)
    }

    // --- Setters ---

    #[inline]
    pub fn set_admin(&self, admin: &Address) {
        self.write_key(&DataKey::Admin, admin);
    }

    #[inline]
    pub fn set_token(&self, token: &Address) {
        self.write_key(&DataKey::Token, token);
    }

    #[inline]
    pub fn set_token_decimals(&self, decimals: u32) {
        self.write_key(&DataKey::TokenDecimals, &decimals);
    }

    #[inline]
    pub fn set_mode(&self, mode: &Mode) {
        self.write_key(&DataKey::Mode, mode);
    }

    #[inline]
    pub fn set_chain_id(&self, chain_id: u32) {
        self.write_key(&DataKey::ChainId, &chain_id);
    }

    #[inline]
    pub fn set_version(&self, version: u32) {
        self.write_key(&DataKey::Version, &version);
    }

    #[inline]
    pub fn set_rate_limit_duration(&self, duration: u64) {
        self.write_key(&DataKey::RateLimitDuration, &duration);
    }

    #[inline]
    pub fn set_pending_admin(&self, pending_admin: &Address) {
        self.write_key(&DataKey::PendingAdmin, pending_admin);
    }

    #[inline]
    pub fn remove_pending_admin(&self) {
        self.env.storage().instance().remove(&DataKey::PendingAdmin);
    }

    /// Pass `None` to remove the pauser.
    #[inline]
    pub fn set_pauser(&self, pauser: Option<&Address>) {
        match pauser {
            Some(p) => self.write_key(&DataKey::Pauser, p),
            None => self.env.storage().instance().remove(&DataKey::Pauser),
        }
    }

    #[inline]
    pub fn set_threshold(&self, threshold: u32) {
        self.write_key(&DataKey::Threshold, &threshold);
    }

    #[inline]
    pub fn set_next_sequence(&self, sequence: u64) {
        self.write_key(&DataKey::NextSequence, &sequence);
    }

    #[inline]
    pub fn set_transceiver_count(&self, count: u32) {
        self.write_key(&DataKey::TransceiverCount, &count);
    }

    #[inline]
    pub fn set_enabled_bitmap(&self, bitmap: u64) {
        self.write_key(&DataKey::EnabledBitmap, &bitmap);
    }
    /// Returns the outbound rate limit params, defaulting to unlimited if unset.
    #[inline]
    pub fn outbound_rate_limit(&self) -> RateLimitParams {
        self.env
            .storage()
            .instance()
            .get(&DataKey::OutboundRateLimit)
            .unwrap_or_else(|| RateLimitParams::new(u64::MAX, self.env))
    }

    #[inline]
    pub fn set_outbound_rate_limit(&self, value: &RateLimitParams) {
        self.write_key(&DataKey::OutboundRateLimit, value);
    }

    // --- Compound operations ---

    /// Atomically gets and increments the sequence counter. Starts at 1.
    #[inline]
    pub fn use_sequence(&self) -> u64 {
        let current = self.next_sequence();
        self.set_next_sequence(current + 1);
        current
    }

    /// Authenticates `caller` and verifies they are the admin.
    pub fn require_admin(&self, caller: &Address) -> Result<(), NttManagerError> {
        caller.require_auth();
        if *caller != self.admin()? {
            return Err(NttManagerError::Unauthorized);
        }
        Ok(())
    }

    /// Authenticates `caller` and verifies they are admin or pauser.
    pub fn require_admin_or_pauser(&self, caller: &Address) -> Result<(), NttManagerError> {
        caller.require_auth();
        if *caller == self.admin()? {
            return Ok(());
        }
        if let Some(pauser) = self.pauser() {
            if *caller == pauser {
                return Ok(());
            }
        }
        Err(NttManagerError::NotAdminOrPauser)
    }
}

#[inline]
fn extend_ttl(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND);
}

/// Generic accessor for persistent storage with automatic TTL extension.
pub struct PersistentEntry<'a, V> {
    env: &'a Env,
    key: DataKey,
    _marker: PhantomData<V>,
}

impl<'a, V> PersistentEntry<'a, V> {
    #[inline]
    fn from_key(env: &'a Env, key: DataKey) -> Self {
        Self {
            env,
            key,
            _marker: PhantomData,
        }
    }
}

impl<'a, V> PersistentEntry<'a, V>
where
    V: TryFromVal<Env, Val> + IntoVal<Env, Val>,
    V::Error: Debug,
{
    #[inline]
    pub fn get(&self) -> Option<V> {
        let value = self.env.storage().persistent().get(&self.key);
        if value.is_some() {
            extend_ttl(self.env, &self.key);
        }
        value
    }

    #[inline]
    pub fn set(&self, value: &V) {
        self.env.storage().persistent().set(&self.key, value);
        extend_ttl(self.env, &self.key);
    }

    #[inline]
    pub fn remove(&self) {
        self.env.storage().persistent().remove(&self.key);
    }
}

// --- Concrete entry types ---

pub type PeerEntry<'a> = PersistentEntry<'a, NttManagerPeer>;
pub type AttestationEntry<'a> = PersistentEntry<'a, AttestationInfo>;
pub type OutboundQueueEntry<'a> = PersistentEntry<'a, OutboundQueuedTransfer>;
pub type InboundQueueEntry<'a> = PersistentEntry<'a, InboundQueuedTransfer>;
pub type TransceiverEntry<'a> = PersistentEntry<'a, TransceiverInfo>;
pub type TransceiverIndexEntry<'a> = PersistentEntry<'a, u32>;

// --- Typed constructors and specialized methods ---

impl<'a> PersistentEntry<'a, NttManagerPeer> {
    #[inline]
    pub fn new(env: &'a Env, chain_id: u32) -> Self {
        Self::from_key(env, DataKey::Peer(chain_id))
    }

    /// Returns `PeerNotFound` if the peer doesn't exist.
    #[inline]
    pub fn get_or_err(&self) -> Result<NttManagerPeer, NttManagerError> {
        self.get().ok_or(NttManagerError::PeerNotFound)
    }
}

impl<'a> PersistentEntry<'a, AttestationInfo> {
    #[inline]
    pub fn new(env: &'a Env, digest: BytesN<32>) -> Self {
        Self::from_key(env, DataKey::Attestation(digest))
    }

    /// Returns a default `AttestationInfo` (not executed, no attesters) if absent.
    #[inline]
    pub fn get_or_default(&self) -> AttestationInfo {
        self.get().unwrap_or(AttestationInfo {
            executed: false,
            attested_transceivers: 0,
        })
    }
}

impl<'a> PersistentEntry<'a, OutboundQueuedTransfer> {
    #[inline]
    pub fn new(env: &'a Env, sequence: u64) -> Self {
        Self::from_key(env, DataKey::OutboundQueue(sequence))
    }

    /// Returns `TransferNotQueued` if no transfer exists at this sequence.
    #[inline]
    pub fn get_or_err(&self) -> Result<OutboundQueuedTransfer, NttManagerError> {
        self.get().ok_or(NttManagerError::TransferNotQueued)
    }
}

impl<'a> PersistentEntry<'a, InboundQueuedTransfer> {
    #[inline]
    pub fn new(env: &'a Env, digest: BytesN<32>) -> Self {
        Self::from_key(env, DataKey::InboundQueue(digest))
    }

    /// Returns `TransferNotQueued` if no transfer exists for this digest.
    #[inline]
    pub fn get_or_err(&self) -> Result<InboundQueuedTransfer, NttManagerError> {
        self.get().ok_or(NttManagerError::TransferNotQueued)
    }
}

impl<'a> PersistentEntry<'a, TransceiverInfo> {
    #[inline]
    pub fn new(env: &'a Env, index: u32) -> Self {
        Self::from_key(env, DataKey::Transceiver(index))
    }
}

impl<'a> PersistentEntry<'a, u32> {
    #[inline]
    pub fn new(env: &'a Env, address: Address) -> Self {
        Self::from_key(env, DataKey::TransceiverIndex(address))
    }
}
