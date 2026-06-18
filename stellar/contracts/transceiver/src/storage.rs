use core::fmt::Debug;
use core::marker::PhantomData;
use soroban_ntt_client::{PeerInfo, TransceiverError, TTL_EXTEND, TTL_THRESHOLD};
use soroban_sdk::{Address, BytesN, Env, IntoVal, TryFromVal, Val};
use wormhole_soroban_client::hash_address;

use crate::state::{ConsumedKey, DataKey};

pub struct InstanceStorage<'a> {
    env: &'a Env,
}

impl<'a> InstanceStorage<'a> {
    pub fn new(env: &'a Env) -> Self {
        env.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
        Self { env }
    }

    fn read<T>(&self, key: &DataKey) -> Option<T>
    where
        T: TryFromVal<Env, Val>,
        T::Error: Debug,
    {
        self.env.storage().instance().get(key)
    }

    fn write<T: IntoVal<Env, Val>>(&self, key: &DataKey, value: &T) {
        self.env.storage().instance().set(key, value);
    }

    fn require<T>(&self, key: &DataKey) -> Result<T, TransceiverError>
    where
        T: TryFromVal<Env, Val>,
        T::Error: Debug,
    {
        self.read(key).ok_or(TransceiverError::NotInitialized)
    }

    pub fn manager(&self) -> Result<Address, TransceiverError> {
        self.require(&DataKey::Manager)
    }

    pub fn manager_id(&self) -> Result<BytesN<32>, TransceiverError> {
        Ok(hash_address(self.env, &self.manager()?))
    }

    pub fn wormhole_core(&self) -> Result<Address, TransceiverError> {
        self.require(&DataKey::WormholeCore)
    }

    pub fn version(&self) -> u32 {
        self.read(&DataKey::Version).unwrap_or(1)
    }

    pub fn set_version(&self, version: u32) {
        self.write(&DataKey::Version, &version);
    }

    pub fn require_manager_auth(&self) -> Result<(), TransceiverError> {
        self.manager()?.require_auth();
        Ok(())
    }

    pub fn initialize(&self, manager: &Address, wormhole_core: &Address) {
        self.write(&DataKey::Manager, manager);
        self.write(&DataKey::WormholeCore, wormhole_core);
    }
}

pub(crate) struct PersistentEntry<'a, V> {
    env: &'a Env,
    key: DataKey,
    _marker: PhantomData<V>,
}

impl<'a, V> PersistentEntry<'a, V>
where
    V: TryFromVal<Env, Val> + IntoVal<Env, Val>,
    V::Error: Debug,
{
    fn from_key(env: &'a Env, key: DataKey) -> Self {
        Self {
            env,
            key,
            _marker: PhantomData,
        }
    }

    pub fn get(&self) -> Option<V> {
        self.env
            .storage()
            .persistent()
            .get(&self.key)
            .inspect(|_| self.extend_ttl())
    }

    pub fn set(&self, value: &V) {
        self.env.storage().persistent().set(&self.key, value);
        self.extend_ttl();
    }

    fn extend_ttl(&self) {
        self.env
            .storage()
            .persistent()
            .extend_ttl(&self.key, TTL_THRESHOLD, TTL_EXTEND);
    }
}

pub type PeerEntry<'a> = PersistentEntry<'a, PeerInfo>;
pub type ConsumedEntry<'a> = PersistentEntry<'a, bool>;

impl<'a> PeerEntry<'a> {
    pub fn new(env: &'a Env, chain_id: u32) -> Self {
        Self::from_key(env, DataKey::Peer(chain_id))
    }
}

impl<'a> ConsumedEntry<'a> {
    pub fn new(
        env: &'a Env,
        emitter_chain: u32,
        emitter_address: &BytesN<32>,
        sequence: u64,
    ) -> Self {
        Self::from_key(
            env,
            DataKey::Consumed(ConsumedKey {
                emitter_chain,
                emitter_address: emitter_address.clone(),
                sequence,
            }),
        )
    }

    pub fn is_consumed(&self) -> bool {
        self.get().unwrap_or(false)
    }

    pub fn mark_consumed(&self) {
        self.set(&true);
    }
}
