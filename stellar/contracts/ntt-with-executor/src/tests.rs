use soroban_ntt_client::{NttManagerError, NttManagerPeer, RateLimitParams, TransferResult};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    token, Address, Bytes, BytesN, Env, IntoVal,
};

use crate::executor::ExecutorError;
use crate::{
    encoding, fee, Destination, ExecutorArgs, FeeArgs, NttWithExecutor, NttWithExecutorClient,
    WrapperError,
};

const SRC_CHAIN: u32 = 61;
const DEST_CHAIN: u32 = 2;
const SRC_DECIMALS: u32 = 7; // Stellar Asset Contracts default to 7.
const DST_DECIMALS: u32 = 6;
const SEQUENCE: u64 = 42;
const AMOUNT: i128 = 123_456_789;
const DBPS: u32 = 1234;

#[contracttype]
struct ManagerConfig {
    token: Address,
    token_decimals: u32,
    chain_id: u32,
    peer_chain: u32,
    peer: NttManagerPeer,
    sequence: u64,
}

/// Arguments the wrapper forwarded to the manager's `transfer`.
#[contracttype]
pub struct RecordedTransfer {
    pub sender: Address,
    pub amount: i128,
    pub recipient_chain: u32,
    pub recipient: BytesN<32>,
    pub should_queue: bool,
}

/// Records the transfer it receives and returns a canned sequence.
#[contract]
pub struct MockManager;

#[contractimpl]
impl MockManager {
    pub fn __constructor(
        env: Env,
        token: Address,
        token_decimals: u32,
        chain_id: u32,
        peer_chain: u32,
        peer: NttManagerPeer,
        sequence: u64,
    ) {
        env.storage().instance().set(
            &symbol_short!("cfg"),
            &ManagerConfig {
                token,
                token_decimals,
                chain_id,
                peer_chain,
                peer,
                sequence,
            },
        );
    }

    pub fn get_peer(env: Env, chain_id: u32) -> Option<NttManagerPeer> {
        let cfg = Self::cfg(&env);
        (chain_id == cfg.peer_chain).then_some(cfg.peer)
    }

    pub fn get_token(env: Env) -> Result<Address, NttManagerError> {
        Ok(Self::cfg(&env).token)
    }

    pub fn token_decimals(env: Env) -> Result<u32, NttManagerError> {
        Ok(Self::cfg(&env).token_decimals)
    }

    pub fn get_chain_id(env: Env) -> Result<u32, NttManagerError> {
        Ok(Self::cfg(&env).chain_id)
    }

    pub fn transfer(
        env: Env,
        sender: Address,
        amount: i128,
        recipient_chain: u32,
        recipient: BytesN<32>,
        should_queue: bool,
    ) -> Result<TransferResult, NttManagerError> {
        sender.require_auth();
        env.storage().instance().set(
            &symbol_short!("xfer"),
            &RecordedTransfer {
                sender,
                amount,
                recipient_chain,
                recipient,
                should_queue,
            },
        );
        Ok(TransferResult {
            sequence: Self::cfg(&env).sequence,
            queued: false,
            digest: BytesN::from_array(&env, &[0u8; 32]),
        })
    }

    pub fn last_transfer(env: Env) -> Option<RecordedTransfer> {
        env.storage().instance().get(&symbol_short!("xfer"))
    }

    fn cfg(env: &Env) -> ManagerConfig {
        env.storage().instance().get(&symbol_short!("cfg")).unwrap()
    }
}

/// Records every field the wrapper forwarded to `request_execution`.
#[contracttype]
pub struct RecordedRequest {
    pub dst_chain: u32,
    pub dst_addr: BytesN<32>,
    pub refund: Address,
    pub payer: Address,
    pub payee: Address,
    pub amount: i128,
    pub signed_quote: Bytes,
    pub request: Bytes,
    pub relay_instructions: Bytes,
}

/// Requires the payer's authorization, then records the request.
#[contract]
pub struct MockExecutor;

#[contractimpl]
impl MockExecutor {
    pub fn request_execution(
        env: Env,
        dst_chain: u32,
        dst_addr: BytesN<32>,
        refund: Address,
        payer: Address,
        payee: Address,
        amount: i128,
        signed_quote_bytes: Bytes,
        request: Bytes,
        relay_instructions: Bytes,
    ) -> Result<(), ExecutorError> {
        payer.require_auth();
        env.storage().instance().set(
            &symbol_short!("req"),
            &RecordedRequest {
                dst_chain,
                dst_addr,
                refund,
                payer,
                payee,
                amount,
                signed_quote: signed_quote_bytes,
                request,
                relay_instructions,
            },
        );
        Ok(())
    }

    pub fn last_request(env: Env) -> Option<RecordedRequest> {
        env.storage().instance().get(&symbol_short!("req"))
    }
}

struct Fixture<'a> {
    env: Env,
    sender: Address,
    referrer: Address,
    token: Address,
    manager: Address,
    executor: Address,
    peer_addr: BytesN<32>,
    recipient: BytesN<32>,
    exec: ExecutorArgs,
    wrapper: NttWithExecutorClient<'a>,
}

fn setup(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();

    let sender = Address::generate(env);
    let referrer = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(env))
        .address();
    token::StellarAssetClient::new(env, &token).mint(&sender, &AMOUNT);

    let peer_addr = BytesN::from_array(env, &[0x11; 32]);
    let peer = NttManagerPeer {
        address: peer_addr.clone(),
        token_decimals: DST_DECIMALS,
        inbound_rate_limit: RateLimitParams {
            limit: 0,
            current_capacity: 0,
            last_tx_timestamp: 0,
        },
    };
    let manager = env.register(
        MockManager,
        (
            &token,
            &SRC_DECIMALS,
            &SRC_CHAIN,
            &DEST_CHAIN,
            &peer,
            &SEQUENCE,
        ),
    );
    let executor = env.register(MockExecutor, ());
    let wrapper = NttWithExecutorClient::new(env, &env.register(NttWithExecutor, (&executor,)));

    let exec = ExecutorArgs {
        payee: Address::generate(env),
        amount: 5_000,
        refund: Address::generate(env),
        signed_quote: Bytes::from_array(env, &[0xEE; 8]),
        relay_instructions: Bytes::from_array(env, &[0xAB; 4]),
    };

    Fixture {
        env: env.clone(),
        sender,
        referrer,
        token,
        manager,
        executor,
        peer_addr,
        recipient: BytesN::from_array(env, &[0x22; 32]),
        exec,
        wrapper,
    }
}

impl Fixture<'_> {
    fn destination(&self, chain: u32) -> Destination {
        Destination {
            chain,
            recipient: self.recipient.clone(),
        }
    }

    fn fee_args(&self, dbps: u32) -> FeeArgs {
        FeeArgs {
            referrer: self.referrer.clone(),
            dbps,
        }
    }

    fn token_balance(&self, addr: &Address) -> i128 {
        token::Client::new(&self.env, &self.token).balance(addr)
    }
}

// The wrapper must pay the referrer the trimmed fee, bridge the remainder with
// should_queue = false, and forward the executor its verbatim arguments plus the
// ERN1 request for this transfer. Guards the entire happy-path payload.
#[test]
fn pays_referrer_and_forwards_executor_request() {
    let env = Env::default();
    let f = setup(&env);
    let fee = fee::referrer_fee(AMOUNT, DBPS, SRC_DECIMALS as u8, DST_DECIMALS as u8).unwrap();
    assert!(fee > 0);

    let sequence = f.wrapper.transfer(
        &f.sender,
        &f.manager,
        &AMOUNT,
        &f.destination(DEST_CHAIN),
        &f.fee_args(DBPS),
        &f.exec,
    );
    assert_eq!(sequence, SEQUENCE);
    assert_eq!(f.token_balance(&f.referrer), fee);
    assert_eq!(f.token_balance(&f.sender), AMOUNT - fee);

    let sent = MockManagerClient::new(&env, &f.manager)
        .last_transfer()
        .unwrap();
    assert_eq!(sent.sender, f.sender);
    assert_eq!(sent.amount, AMOUNT - fee);
    assert_eq!(sent.recipient_chain, DEST_CHAIN);
    assert_eq!(sent.recipient, f.recipient);
    assert!(!sent.should_queue);

    let req = MockExecutorClient::new(&env, &f.executor)
        .last_request()
        .unwrap();
    assert_eq!(req.dst_chain, DEST_CHAIN);
    assert_eq!(req.dst_addr, f.peer_addr);
    assert_eq!(req.payer, f.sender);
    assert_eq!(req.payee, f.exec.payee);
    assert_eq!(req.amount, f.exec.amount);
    assert_eq!(req.refund, f.exec.refund);
    assert_eq!(req.signed_quote, f.exec.signed_quote);
    assert_eq!(req.relay_instructions, f.exec.relay_instructions);
    assert_eq!(
        req.request,
        encoding::ntt_request(&env, SRC_CHAIN as u16, &f.manager, SEQUENCE)
    );
}

// With dbps = 0 no referrer is paid and the full amount is bridged, while the
// executor is still engaged. Catches a fee charged on a disabled referrer.
#[test]
fn zero_dbps_bridges_full_amount() {
    let env = Env::default();
    let f = setup(&env);

    f.wrapper.transfer(
        &f.sender,
        &f.manager,
        &AMOUNT,
        &f.destination(DEST_CHAIN),
        &f.fee_args(0),
        &f.exec,
    );

    assert_eq!(f.token_balance(&f.referrer), 0);
    assert_eq!(f.token_balance(&f.sender), AMOUNT);
    assert_eq!(
        MockManagerClient::new(&env, &f.manager)
            .last_transfer()
            .unwrap()
            .amount,
        AMOUNT
    );
    assert!(MockExecutorClient::new(&env, &f.executor)
        .last_request()
        .is_some());
}

// A recipient chain with no registered peer must fail before any token moves.
#[test]
fn missing_peer_returns_peer_not_found() {
    let env = Env::default();
    let f = setup(&env);

    assert_eq!(
        f.wrapper.try_transfer(
            &f.sender,
            &f.manager,
            &AMOUNT,
            &f.destination(DEST_CHAIN + 1),
            &f.fee_args(DBPS),
            &f.exec,
        ),
        Err(Ok(WrapperError::PeerNotFound))
    );
    // Fails before any side effect: no fee paid, no manager or executor call.
    assert_eq!(f.token_balance(&f.referrer), 0);
    assert!(MockManagerClient::new(&env, &f.manager)
        .last_transfer()
        .is_none());
    assert!(MockExecutorClient::new(&env, &f.executor)
        .last_request()
        .is_none());
}

// A single sender-rooted authorization must cover all three sub-invocations —
// the referrer fee, the manager transfer, and the executor request — proving the
// wrapper propagates auth rather than requiring independent signatures.
#[test]
fn authorized_under_single_sender_auth() {
    let env = Env::default();
    let f = setup(&env);
    let fee = fee::referrer_fee(AMOUNT, DBPS, SRC_DECIMALS as u8, DST_DECIMALS as u8).unwrap();
    let destination = f.destination(DEST_CHAIN);
    let request = encoding::ntt_request(&env, SRC_CHAIN as u16, &f.manager, SEQUENCE);

    env.mock_auths(&[MockAuth {
        address: &f.sender,
        invoke: &MockAuthInvoke {
            contract: &f.wrapper.address,
            fn_name: "transfer",
            args: (
                f.sender.clone(),
                f.manager.clone(),
                AMOUNT,
                destination.clone(),
                f.fee_args(DBPS),
                f.exec.clone(),
            )
                .into_val(&env),
            sub_invokes: &[
                MockAuthInvoke {
                    contract: &f.token,
                    fn_name: "transfer",
                    args: (f.sender.clone(), f.referrer.clone(), fee).into_val(&env),
                    sub_invokes: &[],
                },
                MockAuthInvoke {
                    contract: &f.manager,
                    fn_name: "transfer",
                    args: (
                        f.sender.clone(),
                        AMOUNT - fee,
                        DEST_CHAIN,
                        f.recipient.clone(),
                        false,
                    )
                        .into_val(&env),
                    sub_invokes: &[],
                },
                MockAuthInvoke {
                    contract: &f.executor,
                    fn_name: "request_execution",
                    args: (
                        DEST_CHAIN,
                        f.peer_addr.clone(),
                        f.exec.refund.clone(),
                        f.sender.clone(),
                        f.exec.payee.clone(),
                        f.exec.amount,
                        f.exec.signed_quote.clone(),
                        request,
                        f.exec.relay_instructions.clone(),
                    )
                        .into_val(&env),
                    sub_invokes: &[],
                },
            ],
        },
    }]);

    let sequence = f.wrapper.transfer(
        &f.sender,
        &f.manager,
        &AMOUNT,
        &destination,
        &f.fee_args(DBPS),
        &f.exec,
    );
    assert_eq!(sequence, SEQUENCE);
}

// The largest fee rate the wire accepts must still leave a bridgeable
// remainder. dbps is capped at u16::MAX, below the 100_000 denominator, which
// is what keeps the fee under the amount and the subtraction in range.
#[test]
fn max_dbps_bridges_the_remainder() {
    let env = Env::default();
    let f = setup(&env);
    let fee = fee::referrer_fee(
        AMOUNT,
        u16::MAX as u32,
        SRC_DECIMALS as u8,
        DST_DECIMALS as u8,
    )
    .unwrap();
    assert!(fee > 0 && fee < AMOUNT);

    f.wrapper.transfer(
        &f.sender,
        &f.manager,
        &AMOUNT,
        &f.destination(DEST_CHAIN),
        &f.fee_args(u16::MAX as u32),
        &f.exec,
    );

    assert_eq!(f.token_balance(&f.referrer), fee);
    assert_eq!(
        MockManagerClient::new(&env, &f.manager)
            .last_transfer()
            .unwrap()
            .amount,
        AMOUNT - fee
    );
}
