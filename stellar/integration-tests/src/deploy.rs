//! Typed deployers for the four contracts the harness puts on localnet —
//! [`WormholeCore`], [`MockToken`], [`Manager`], [`Transceiver`] — and a
//! [`Stack`] that bundles them together with the orchestration helpers
//! tests need (peer + transceiver registration, threshold management,
//! pause / ownership flows, token balance reads, inbound submission).
//!
//! The Wormhole core enforces a fixed governance emitter at deploy time
//! ([`GOVERNANCE_EMITTER`]); the harness bakes it in so tests don't
//! have to know that detail.

use serde_json::Value;
use soroban_ntt_client::types::Mode;
use wormhole_soroban_client::GOVERNANCE_EMITTER;

use crate::cli;
use crate::ctx::TestContext;
use crate::events::EventQuery;
use crate::vaa;

/// Vendored NethermindEth Wormhole core.
pub struct WormholeCore;
/// Parameterised-decimals fixture token.
pub struct MockToken;
/// NTT manager.
pub struct Manager;
/// NTT Wormhole transceiver.
pub struct Transceiver;

impl WormholeCore {
    /// Deploys the vendored Wormhole core with `guardians` as the initial
    /// guardian set (index 0). Returns the new contract id. The governance
    /// emitter is taken from `wormhole_soroban_client::GOVERNANCE_EMITTER`
    /// — the core enforces it at construction.
    pub fn deploy(ctx: &TestContext, guardians: &[[u8; 20]]) -> String {
        let json = serde_json::to_string(
            &guardians.iter().map(hex::encode).collect::<Vec<_>>(),
        )
        .expect("encode guardian array");
        let governance_emitter_hex = hex::encode(GOVERNANCE_EMITTER);
        cli::deploy(
            &ctx.admin_identity,
            &ctx.wormhole_core_wasm_path,
            &[
                "--initial_guardians",
                &json,
                "--governance_emitter",
                &governance_emitter_hex,
            ],
        )
    }

    /// Deploys with the single test guardian whose secret lives in
    /// `ctx.guardian_secret`. The harness can then sign VAAs that verify
    /// against this guardian set.
    pub fn deploy_with_test_guardian(ctx: &TestContext) -> String {
        let addr = vaa::eth_address_from_privkey(&ctx.guardian_secret);
        Self::deploy(ctx, &[addr])
    }
}

impl MockToken {
    /// Deploys a fresh mock token with the given decimal precision.
    pub fn deploy(ctx: &TestContext, decimals: u32) -> String {
        let decimals_s = decimals.to_string();
        cli::deploy(
            &ctx.admin_identity,
            &ctx.mock_token_wasm_path,
            &["--decimals", &decimals_s],
        )
    }
}

impl Manager {
    /// Deploys the manager bound to `token`. `mode` is passed as the integer
    /// discriminant because the CLI's JSON deserializer doesn't yet support
    /// unit-variant enums.
    #[allow(clippy::too_many_arguments)]
    pub fn deploy(
        ctx: &TestContext,
        owner: &str,
        token: &str,
        mode: Mode,
        chain_id: u32,
        outbound_limit: u64,
        rate_limit_duration: u64,
        wormhole_core: &str,
    ) -> String {
        let mode_str = match mode {
            Mode::Locking => "0",
            Mode::Burning => "1",
        };
        let chain_id_s = chain_id.to_string();
        let outbound_s = outbound_limit.to_string();
        let rate_s = rate_limit_duration.to_string();
        cli::deploy(
            &ctx.admin_identity,
            &ctx.manager_wasm_path,
            &[
                "--owner",
                owner,
                "--token",
                token,
                "--mode",
                mode_str,
                "--chain_id",
                &chain_id_s,
                "--outbound_limit",
                &outbound_s,
                "--rate_limit_duration",
                &rate_s,
                "--wormhole_core",
                wormhole_core,
            ],
        )
    }
}

impl Transceiver {
    /// Deploys a fresh transceiver bound to `manager` + `wormhole_core`.
    /// Multiple transceivers can target the same manager — registry order
    /// determines the index used in attestation bitmaps.
    pub fn deploy(
        ctx: &TestContext,
        owner: &str,
        manager: &str,
        wormhole_core: &str,
    ) -> String {
        cli::deploy(
            &ctx.admin_identity,
            &ctx.transceiver_wasm_path,
            &[
                "--owner",
                owner,
                "--manager",
                manager,
                "--wormhole_core",
                wormhole_core,
            ],
        )
    }
}

/// Configuration consumed by [`Stack::deploy`].
pub struct StackOptions {
    pub mode: Mode,
    /// Decimals for the mock token in Burning mode; ignored in Locking
    /// mode (which uses the native SAC, fixed at 7 decimals).
    pub token_decimals: u32,
    pub outbound_limit: u64,
    pub rate_limit_duration: u64,
}

impl Default for StackOptions {
    /// Burning, 7-decimal mock token, no outbound rate limit, 1s window.
    /// Tests with non-default needs use struct-update form:
    /// `StackOptions { mode: Mode::Locking, ..Default::default() }`.
    fn default() -> Self {
        Self {
            mode: Mode::Burning,
            token_decimals: 7,
            outbound_limit: u64::MAX,
            rate_limit_duration: 1,
        }
    }
}

/// All four deployed contract ids for one test fixture, plus the mode the
/// manager was deployed with. Fields are public so tests can also call
/// contracts directly when a [`Stack`] helper doesn't cover the case.
pub struct Stack {
    pub mode: Mode,
    pub wormhole_core: String,
    pub token: String,
    pub manager: String,
    pub transceiver: String,
}

impl Stack {
    /// Deploys a Wormhole core (with test guardian), a token (native SAC for
    /// `Locking`, fresh `MockToken` for `Burning`), the manager, and one
    /// transceiver. The transceiver is not yet registered with the manager;
    /// the test calls [`Self::register_transceiver`] when it needs that.
    pub fn deploy(ctx: &TestContext, opts: &StackOptions) -> Self {
        let wormhole_core = WormholeCore::deploy_with_test_guardian(ctx);
        let token = match opts.mode {
            Mode::Locking => ctx.native_sac(),
            Mode::Burning => MockToken::deploy(ctx, opts.token_decimals),
        };
        let manager = Manager::deploy(
            ctx,
            &ctx.admin_address,
            &token,
            opts.mode,
            ctx.stellar_chain_id,
            opts.outbound_limit,
            opts.rate_limit_duration,
            &wormhole_core,
        );
        let transceiver = Transceiver::deploy(
            ctx,
            &ctx.admin_address,
            &manager,
            &wormhole_core,
        );
        Self {
            mode: opts.mode,
            wormhole_core,
            token,
            manager,
            transceiver,
        }
    }

    /// Registers `self.transceiver` on the manager under admin auth.
    /// Auto-bumps the manager's threshold from 0 to 1 on first registration.
    pub fn register_transceiver(&self, ctx: &TestContext) {
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "set_transceiver",
            &["--transceiver", &self.transceiver],
        );
    }

    /// Registers `peer_addr` for `chain_id` on both the manager (as the peer
    /// NTT manager address) and `self.transceiver` (as the peer emitter).
    /// This is the common case — outbound flows route via the transceiver,
    /// inbound flows verify the source against both sides.
    pub fn register_peer(
        &self,
        ctx: &TestContext,
        chain_id: u32,
        peer_addr: &[u8; 32],
        peer_decimals: u32,
        inbound_limit: u64,
    ) {
        let peer_hex = hex::encode(peer_addr);
        let chain_id_s = chain_id.to_string();
        let decimals_s = peer_decimals.to_string();
        let limit_s = inbound_limit.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "set_peer",
            &[
                "--chain_id",
                &chain_id_s,
                "--peer_address",
                &peer_hex,
                "--token_decimals",
                &decimals_s,
                "--inbound_limit",
                &limit_s,
            ],
        );
        cli::invoke(
            &ctx.admin_identity,
            &self.transceiver,
            "set_peer",
            &["--chain_id", &chain_id_s, "--emitter", &peer_hex],
        );
    }

    /// Registers `addr` on the Wormhole core address registry so an inbound
    /// transfer can resolve its hashed `to`. Permissionless; submitted under
    /// admin auth. Recipients must be registered before they can receive.
    pub fn record_address(&self, ctx: &TestContext, addr: &str) {
        cli::invoke(
            &ctx.admin_identity,
            &self.wormhole_core,
            "record_address",
            &["--address", addr],
        );
    }

    /// Returns `account`'s balance on `self.token`.
    pub fn token_balance(&self, ctx: &TestContext, account: &str) -> i128 {
        let v = cli::invoke(
            &ctx.admin_identity,
            &self.token,
            "balance",
            &["--id", account],
        );
        parse_i128(&v)
    }

    /// Credits `amount` to `recipient` on the mock token. Panics in Locking
    /// mode (the native SAC has no test-friendly mint — fund via friendbot).
    pub fn mint_to(&self, ctx: &TestContext, recipient: &str, amount: i128) {
        if matches!(self.mode, Mode::Locking) {
            panic!("mint_to only works in Burning mode; Locking uses friendbot-funded XLM");
        }
        let amount_s = amount.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.token,
            "mint",
            &["--to", recipient, "--amount", &amount_s],
        );
    }

    /// Registers `transceiver` (any address) on the manager. Used to wire a
    /// second or third transceiver returned by [`Self::deploy_extra_transceiver`].
    pub fn register_transceiver_addr(&self, ctx: &TestContext, transceiver: &str) {
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "set_transceiver",
            &["--transceiver", transceiver],
        );
    }

    /// Registers `peer_addr` only on the transceiver's peer table, leaving
    /// the manager's peer table empty. Lets tests isolate the manager-side
    /// peer-not-found error path that the transceiver-side check would
    /// otherwise short-circuit.
    pub fn register_transceiver_peer_only(
        &self,
        ctx: &TestContext,
        chain_id: u32,
        peer_addr: &[u8; 32],
    ) {
        self.set_transceiver_peer(ctx, &self.transceiver, chain_id, peer_addr);
    }

    /// Sets `peer_addr` as the peer emitter for `chain_id` on the given
    /// transceiver contract id. Used to wire additional transceivers
    /// returned by [`Self::deploy_extra_transceiver`].
    pub fn set_transceiver_peer(
        &self,
        ctx: &TestContext,
        transceiver: &str,
        chain_id: u32,
        peer_addr: &[u8; 32],
    ) {
        let peer_hex = hex::encode(peer_addr);
        let chain_id_s = chain_id.to_string();
        cli::invoke(
            &ctx.admin_identity,
            transceiver,
            "set_peer",
            &["--chain_id", &chain_id_s, "--emitter", &peer_hex],
        );
    }

    /// Sets the manager's attestation threshold under admin auth.
    pub fn set_threshold(&self, ctx: &TestContext, threshold: u32) {
        let t = threshold.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "set_threshold",
            &["--threshold", &t],
        );
    }

    /// Deploys an additional transceiver bound to the same manager and
    /// Wormhole core. Returns its contract id; the caller registers it via
    /// [`Self::register_transceiver_addr`].
    pub fn deploy_extra_transceiver(&self, ctx: &TestContext) -> String {
        Transceiver::deploy(
            ctx,
            &ctx.admin_address,
            &self.manager,
            &self.wormhole_core,
        )
    }

    /// Reads the manager's `paused` getter.
    pub fn paused(&self, ctx: &TestContext) -> bool {
        cli::invoke(&ctx.admin_identity, &self.manager, "paused", &[])
            .as_bool()
            .expect("paused must return bool")
    }

    /// Reads the manager's current owner (G-address).
    pub fn owner(&self, ctx: &TestContext) -> String {
        cli::invoke(&ctx.admin_identity, &self.manager, "get_owner", &[])
            .as_str()
            .expect("get_owner must return string")
            .to_string()
    }

    /// Builds an [`EventQuery`] scoped to this stack's manager — the common
    /// case for tests asserting on manager-emitted events.
    pub fn manager_events<'a>(&'a self, ctx: &'a TestContext) -> EventQuery<'a> {
        EventQuery::new(ctx, &self.manager)
    }

    /// Initiates the OZ two-step ownership transfer. `source` is the
    /// current owner; `new_owner` has until `live_until_ledger` to call
    /// `accept_ownership`.
    pub fn transfer_ownership(
        &self,
        source: &str,
        new_owner: &str,
        live_until_ledger: u32,
    ) {
        let live_until_s = live_until_ledger.to_string();
        cli::invoke(
            source,
            &self.manager,
            "transfer_ownership",
            &[
                "--new_owner",
                new_owner,
                "--live_until_ledger",
                &live_until_s,
            ],
        );
    }

    /// Completes the OZ two-step ownership transfer; `source` is the pending
    /// new owner.
    pub fn accept_ownership(&self, source: &str) {
        cli::invoke(source, &self.manager, "accept_ownership", &[]);
    }

    /// Sets the manager's pauser identity to `new_pauser` under admin auth.
    /// The CLI's `Option<Address>` arg requires JSON-string quoting.
    pub fn set_pauser_to(&self, ctx: &TestContext, new_pauser: &str) {
        let json = format!("\"{new_pauser}\"");
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "transfer_pauser",
            &[
                "--caller",
                &ctx.admin_address,
                "--new_pauser",
                &json,
            ],
        );
    }

    /// Pauses the manager. `source` is the signing identity; `caller_addr`
    /// must be owner or pauser.
    pub fn pause(&self, source: &str, caller_addr: &str) {
        cli::invoke(
            source,
            &self.manager,
            "pause",
            &["--caller", caller_addr],
        );
    }

    /// `pause` returning a `Result` for negative tests (e.g. asserting a
    /// non-pauser is rejected with `NotAdminOrPauser`).
    pub fn try_pause(
        &self,
        source: &str,
        caller_addr: &str,
    ) -> Result<Value, cli::CliError> {
        cli::try_invoke(
            source,
            &self.manager,
            "pause",
            &["--caller", caller_addr],
        )
    }

    /// Unpauses the manager. `source` must be the owner — the contract's
    /// `unpause` calls `enforce_owner_auth` and ignores `caller_addr`,
    /// but the arg is still required by the CLI signature.
    pub fn unpause(&self, source: &str, caller_addr: &str) {
        cli::invoke(
            source,
            &self.manager,
            "unpause",
            &["--caller", caller_addr],
        );
    }

    /// `unpause` returning a `Result` for negative tests (e.g. asserting a
    /// pauser-only identity cannot unpause).
    pub fn try_unpause(
        &self,
        source: &str,
        caller_addr: &str,
    ) -> Result<Value, cli::CliError> {
        cli::try_invoke(
            source,
            &self.manager,
            "unpause",
            &["--caller", caller_addr],
        )
    }

    /// Sets the manager's outbound rate-limit capacity. Owner-only.
    pub fn set_outbound_limit(&self, source: &str, limit: u64) {
        let limit_s = limit.to_string();
        cli::invoke(
            source,
            &self.manager,
            "set_outbound_limit",
            &["--limit", &limit_s],
        );
    }

    /// `set_outbound_limit` returning a `Result` for tests that need to
    /// assert auth failure from a non-owner caller.
    pub fn try_set_outbound_limit(
        &self,
        source: &str,
        limit: u64,
    ) -> Result<Value, cli::CliError> {
        let limit_s = limit.to_string();
        cli::try_invoke(
            source,
            &self.manager,
            "set_outbound_limit",
            &["--limit", &limit_s],
        )
    }

    /// `remove_transceiver` returning a `Result`. Used by tests that expect
    /// `CannotDisableLastTransceiver`.
    pub fn try_remove_transceiver(
        &self,
        ctx: &TestContext,
        transceiver: &str,
    ) -> Result<Value, cli::CliError> {
        cli::try_invoke(
            &ctx.admin_identity,
            &self.manager,
            "remove_transceiver",
            &["--transceiver", transceiver],
        )
    }

    /// Initiates an outbound transfer from `ctx.admin_address`, signed by
    /// `ctx.admin_identity`. Returns the raw JSON response so callers can
    /// read `queued` and `sequence`.
    pub fn transfer(
        &self,
        ctx: &TestContext,
        amount: i128,
        recipient_chain: u32,
        recipient: &[u8; 32],
        should_queue: bool,
    ) -> Value {
        let amount_s = amount.to_string();
        let chain_s = recipient_chain.to_string();
        let recipient_hex = hex::encode(recipient);
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "transfer",
            &[
                "--sender", &ctx.admin_address,
                "--amount", &amount_s,
                "--recipient_chain", &chain_s,
                "--recipient", &recipient_hex,
                "--should_queue", if should_queue { "true" } else { "false" },
            ],
        )
    }

    /// `transfer` returning a `Result` for negative tests (rate-limit
    /// rejection, pause enforcement, peer-not-found).
    pub fn try_transfer(
        &self,
        ctx: &TestContext,
        amount: i128,
        recipient_chain: u32,
        recipient: &[u8; 32],
        should_queue: bool,
    ) -> Result<Value, cli::CliError> {
        let amount_s = amount.to_string();
        let chain_s = recipient_chain.to_string();
        let recipient_hex = hex::encode(recipient);
        cli::try_invoke(
            &ctx.admin_identity,
            &self.manager,
            "transfer",
            &[
                "--sender", &ctx.admin_address,
                "--amount", &amount_s,
                "--recipient_chain", &chain_s,
                "--recipient", &recipient_hex,
                "--should_queue", if should_queue { "true" } else { "false" },
            ],
        )
    }

    /// Reads the outbound queue entry for `sequence`. Returns
    /// `Value::Null` if the slot is empty (released, cancelled, or never
    /// queued).
    pub fn outbound_queue_item(&self, ctx: &TestContext, sequence: u64) -> Value {
        let s = sequence.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "get_outbound_queue_item",
            &["--sequence", &s],
        )
    }

    /// Cancels a queued outbound transfer for `ctx.admin_address`, refunding
    /// the queued amount.
    pub fn cancel_queued_transfer(&self, ctx: &TestContext, sequence: u64) {
        let s = sequence.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "cancel_queued_transfer",
            &["--sender", &ctx.admin_address, "--sequence", &s],
        );
    }

    /// Releases a queued outbound transfer once its rate-limit window has
    /// elapsed.
    pub fn complete_queued_transfer(&self, ctx: &TestContext, sequence: u64) {
        let s = sequence.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "complete_queued_transfer",
            &["--sequence", &s],
        );
    }

    /// `complete_queued_transfer` returning a `Result` so callers can poll the
    /// rate-limit release gate (which rejects with `TransferNotReleasable`
    /// until ledger time reaches the queued `release_timestamp`).
    pub fn try_complete_queued_transfer(
        &self,
        ctx: &TestContext,
        sequence: u64,
    ) -> Result<Value, cli::CliError> {
        let s = sequence.to_string();
        cli::try_invoke(
            &ctx.admin_identity,
            &self.manager,
            "complete_queued_transfer",
            &["--sequence", &s],
        )
    }

    /// Submits a signed VAA to `transceiver_addr.receive_message`. Used for
    /// inbound flows after constructing the VAA via
    /// `messages::build_inbound_vaa_hex`.
    pub fn receive_message(
        &self,
        ctx: &TestContext,
        transceiver_addr: &str,
        vaa_hex: &str,
    ) {
        cli::invoke(
            &ctx.admin_identity,
            transceiver_addr,
            "receive_message",
            &["--vaa_bytes", vaa_hex],
        );
    }

    /// `receive_message` returning a `Result` for negative inbound tests
    /// (peer-not-found, replay rejection, etc.).
    pub fn try_receive_message(
        &self,
        ctx: &TestContext,
        transceiver_addr: &str,
        vaa_hex: &str,
    ) -> Result<Value, cli::CliError> {
        cli::try_invoke(
            &ctx.admin_identity,
            transceiver_addr,
            "receive_message",
            &["--vaa_bytes", vaa_hex],
        )
    }

    /// Reads the inbound queue entry for `digest_hex`. Returns `Value::Null`
    /// if the entry has been released or never queued.
    pub fn inbound_queue_item(&self, ctx: &TestContext, digest_hex: &str) -> Value {
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "get_inbound_queue_item",
            &["--digest", digest_hex],
        )
    }

    /// Releases a queued inbound transfer once its rate-limit window has
    /// elapsed.
    pub fn complete_inbound_transfer(&self, ctx: &TestContext, digest_hex: &str) {
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "complete_inbound_transfer",
            &["--digest", digest_hex],
        );
    }

    /// `complete_inbound_transfer` returning a `Result` so callers can poll the
    /// rate-limit release gate (which rejects with `TransferNotReleasable`
    /// until ledger time reaches the queued `release_timestamp`).
    pub fn try_complete_inbound_transfer(
        &self,
        ctx: &TestContext,
        digest_hex: &str,
    ) -> Result<Value, cli::CliError> {
        cli::try_invoke(
            &ctx.admin_identity,
            &self.manager,
            "complete_inbound_transfer",
            &["--digest", digest_hex],
        )
    }

    /// Disables `transceiver` on the manager under admin auth. When this
    /// drops the enabled count below the threshold, the manager auto-reduces
    /// the threshold to the new enabled count.
    pub fn remove_transceiver(&self, ctx: &TestContext, transceiver: &str) {
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "remove_transceiver",
            &["--transceiver", transceiver],
        );
    }

    /// Enables or disables the peer for `chain_id` on `transceiver`. The
    /// kill-switch: a disabled peer's inbound VAAs are rejected with
    /// `PeerDisabled` even when verification passes and the emitter matches.
    pub fn set_transceiver_peer_enabled(
        &self,
        ctx: &TestContext,
        transceiver: &str,
        chain_id: u32,
        enabled: bool,
    ) {
        let chain_id_s = chain_id.to_string();
        cli::invoke(
            &ctx.admin_identity,
            transceiver,
            "set_peer_enabled",
            &[
                "--chain_id",
                &chain_id_s,
                "--enabled",
                if enabled { "true" } else { "false" },
            ],
        );
    }

    /// `set_peer` on `transceiver`, signed by `source`, returning a
    /// `Result`. Serves both the owner-gate paths (new vs old owner) and the
    /// invalid-argument CLI paths (chain 0, zero emitter, chain > u16::MAX,
    /// re-registration of an existing chain).
    pub fn try_set_transceiver_peer(
        &self,
        source: &str,
        transceiver: &str,
        chain_id: u32,
        emitter: &[u8; 32],
    ) -> Result<Value, cli::CliError> {
        let emitter_hex = hex::encode(emitter);
        let chain_id_s = chain_id.to_string();
        cli::try_invoke(
            source,
            transceiver,
            "set_peer",
            &["--chain_id", &chain_id_s, "--emitter", &emitter_hex],
        )
    }

    /// Reads the transceiver's own `paused` getter — independent of the
    /// manager's pause state.
    pub fn transceiver_paused(&self, ctx: &TestContext) -> bool {
        cli::invoke(&ctx.admin_identity, &self.transceiver, "paused", &[])
            .as_bool()
            .expect("paused must return bool")
    }

    /// Pauses the transceiver (owner-only). `source` signs; `caller_addr` is
    /// required by the CLI signature but ignored by the `#[only_owner]` gate.
    pub fn transceiver_pause(&self, source: &str, caller_addr: &str) {
        cli::invoke(
            source,
            &self.transceiver,
            "pause",
            &["--caller", caller_addr],
        );
    }

    /// Unpauses the transceiver (owner-only).
    pub fn transceiver_unpause(&self, source: &str, caller_addr: &str) {
        cli::invoke(
            source,
            &self.transceiver,
            "unpause",
            &["--caller", caller_addr],
        );
    }

    /// Initiates the transceiver's own OZ two-step ownership transfer —
    /// separate from the manager's owner.
    pub fn transceiver_transfer_ownership(
        &self,
        source: &str,
        new_owner: &str,
        live_until_ledger: u32,
    ) {
        let live_until_s = live_until_ledger.to_string();
        cli::invoke(
            source,
            &self.transceiver,
            "transfer_ownership",
            &[
                "--new_owner",
                new_owner,
                "--live_until_ledger",
                &live_until_s,
            ],
        );
    }

    /// Completes the transceiver's two-step ownership transfer; `source` is
    /// the pending new owner.
    pub fn transceiver_accept_ownership(&self, source: &str) {
        cli::invoke(source, &self.transceiver, "accept_ownership", &[]);
    }

    /// Reads the transceiver's current owner (G-address).
    pub fn transceiver_owner(&self, ctx: &TestContext) -> String {
        cli::invoke(&ctx.admin_identity, &self.transceiver, "get_owner", &[])
            .as_str()
            .expect("get_owner must return string")
            .to_string()
    }
}

/// Parses a Soroban `i128` return value out of the CLI's JSON output, which
/// may arrive as a string, a positive integer, or a negative integer
/// depending on magnitude.
pub fn parse_i128(v: &Value) -> i128 {
    if let Some(s) = v.as_str() {
        return s.parse().expect("i128 not parseable");
    }
    if let Some(n) = v.as_i64() {
        return i128::from(n);
    }
    if let Some(n) = v.as_u64() {
        return i128::from(n);
    }
    panic!("cannot parse i128 from JSON value: {v}");
}
