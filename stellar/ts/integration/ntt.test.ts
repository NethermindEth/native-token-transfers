/**
 * `StellarNtt` and `StellarNttWormholeTransceiver` against the deployed
 * contracts.
 *
 * The Rust suite in `stellar/integration-tests/tests/` already pins the
 * contracts' own behaviour, so what is asserted here is the *binding*: that a
 * read reaches the method it names and decodes the struct the contract returns,
 * that a write's arguments arrive in the order and shape the ABI wants, and
 * that the bytes a transfer puts on the wire are the ones the shared NTT
 * layouts read back.
 */
import { Keypair, StrKey } from "@stellar/stellar-sdk";
import {
  UniversalAddress,
  deserializePayload,
} from "@wormhole-foundation/sdk-definitions";
import type { ChainAddress } from "@wormhole-foundation/sdk-definitions";
import { Ntt } from "@wormhole-foundation/sdk-definitions-ntt";
import { StellarAddress } from "@wormhole-foundation/sdk-stellar";
import { asBytes, decodeTransferResult } from "../src/scval-types.js";
import type { Stack } from "./localnet.js";
import {
  admin,
  adminAddress,
  balanceOf,
  completeAfterWindow,
  deployStack,
  drive,
  findEvent,
  fund,
  hashAddress,
  inboundVaa,
  mint,
} from "./localnet.js";

/** The peer every scenario registers, mirroring the Rust suite's fixture. */
const PEER_CHAIN = "Ethereum";
const PEER = new Uint8Array(32).fill(0xaa);
const PEER_DECIMALS = 8;

const SUPPLY = 100_000_000n;
/** 1.0 of a 7-decimal token. */
const AMOUNT = 10_000_000n;
/** The same 1.0 as an 8-decimal peer sends it, and what it unwraps to here. */
const WIRE_AMOUNT = 100_000_000n;
const NO_LIMIT = 2n ** 63n;

/** A `NativeTokenTransfer` to `recipient`, in the peer's 8-decimal domain. */
const transferTo = (recipient: string, amount = WIRE_AMOUNT): Ntt.Message => ({
  id: new Uint8Array(32).fill(0x10),
  sender: new UniversalAddress(new Uint8Array(32).fill(0x20)),
  payload: {
    trimmedAmount: { decimals: PEER_DECIMALS, amount },
    sourceToken: new UniversalAddress(new Uint8Array(32).fill(0x30)),
    recipientAddress: new UniversalAddress(hashAddress(recipient)),
    recipientChain: "Stellar",
    additionalPayload: new Uint8Array(0),
  },
});

/** Registers the peer on the manager and on the transceiver, as a pair. */
async function registerPeer(stack: Stack, inboundLimit: bigint): Promise<void> {
  const peer: ChainAddress = {
    chain: PEER_CHAIN,
    address: new UniversalAddress(PEER),
  };
  await drive(
    stack.ntt.setPeer(peer, PEER_DECIMALS, inboundLimit, adminAddress())
  );
  await drive(stack.ntt.setTransceiverPeer(0, peer, adminAddress()));
}

describe("burning manager", () => {
  let stack: Stack;
  let recipient: Keypair;

  beforeAll(async () => {
    await fund(admin);
    stack = await deployStack({ mode: "burning", tokenDecimals: 7 });
    await registerPeer(stack, NO_LIMIT);
    await mint(stack.token, admin.publicKey(), SUPPLY);
    recipient = await fund(Keypair.random());
  });

  it("reads its deployed configuration back", async () => {
    const { ntt } = stack;
    expect(await ntt.getMode()).toBe("burning");
    expect(await ntt.getThreshold()).toBe(1);
    expect(await ntt.getTokenDecimals()).toBe(7);
    expect(await ntt.getChainId()).toBe(61);
    expect((await ntt.getOwner()).toString()).toBe(admin.publicKey());
    expect(await ntt.getPauser()).toBeNull();
    expect(await ntt.isPaused()).toBe(false);
    // Nothing to report means the config the class was built with is the
    // config on chain, which is the only way this returns null.
    expect(await ntt.verifyAddresses()).toBeNull();

    const peer = await ntt.getPeer(PEER_CHAIN);
    expect(peer?.address.address.toUint8Array()).toEqual(PEER);
    expect(peer?.tokenDecimals).toBe(PEER_DECIMALS);
    expect(peer?.inboundLimit).toBe(NO_LIMIT);
    expect(await ntt.getPeer("Solana")).toBeNull();
  });

  it("reads the transceiver through the manager", async () => {
    const transceiver = await stack.ntt.getTransceiver(0);
    expect(transceiver?.address).toBe(stack.transceiver);
    // Decoded from the `Bytes` the contract returns, not assumed.
    expect(await transceiver!.getTransceiverType()).toBe("wormhole");
    expect(await transceiver!.getPauser()).toBeNull();
    expect(await transceiver!.isPeerEnabled(PEER_CHAIN)).toBe(true);
    expect((await transceiver!.getPeerInfo(PEER_CHAIN))?.enabled).toBe(true);
    // The emitter a peer registers is the raw contract id, never hash_address.
    expect((await transceiver!.getAddress()).address.toUint8Array()).toEqual(
      new Uint8Array(StrKey.decodeContract(stack.transceiver))
    );
  });

  it("puts a transfer on the wire in the shared NTT format", async () => {
    const before = await balanceOf(stack.token, admin.publicKey());
    const to = new Uint8Array(32).fill(0x40);

    await drive(
      stack.ntt.transfer(
        adminAddress(),
        AMOUNT,
        { chain: PEER_CHAIN, address: new UniversalAddress(to) },
        { queue: false, automatic: false }
      )
    );

    expect(await balanceOf(stack.token, admin.publicKey())).toBe(
      before - AMOUNT
    );

    // What the core published is what a guardian would sign, so decoding it
    // with the layouts a peer chain uses is the round-trip that matters.
    const published = await findEvent(stack.core, "message_published");
    expect(published["emitter_address"]).toEqual(
      Buffer.from(StrKey.decodeContract(stack.transceiver))
    );
    expect(published["consistency_level"]).toBe(1);

    const message = deserializePayload(
      "Ntt:WormholeTransfer",
      asBytes(published["payload"])
    );
    expect(message.sourceNttManager.toUint8Array()).toEqual(
      hashAddress(stack.manager)
    );
    expect(message.recipientNttManager.toUint8Array()).toEqual(PEER);

    const { sender, payload } = message.nttManagerPayload;
    expect(sender.toUint8Array()).toEqual(hashAddress(admin.publicKey()));
    expect(payload.sourceToken.toUint8Array()).toEqual(
      hashAddress(stack.token)
    );
    expect(payload.recipientAddress.toUint8Array()).toEqual(to);
    expect(payload.recipientChain).toBe(PEER_CHAIN);
    // Trimmed to min(8, local 7, peer 8), so the amount is unchanged at 7.
    expect(payload.trimmedAmount).toEqual({ decimals: 7, amount: AMOUNT });
  });

  it("redeems an inbound transfer to its recipient, and only once", async () => {
    // hash_address is one-way, so the core has to hold the reverse before the
    // manager can turn the message's 32 bytes back into an address to pay.
    await drive(
      stack.ntt.recordAddress(
        adminAddress(),
        new StellarAddress(recipient.publicKey())
      )
    );

    const redeemed = inboundVaa({
      emitterChain: PEER_CHAIN,
      emitter: PEER,
      sequence: 1n,
      sourceNttManager: PEER,
      recipientNttManager: hashAddress(stack.manager),
      nttManagerPayload: transferTo(recipient.publicKey()),
    });
    await drive(stack.ntt.redeem([redeemed], adminAddress()));

    expect(await balanceOf(stack.token, recipient.publicKey())).toBe(AMOUNT);
    expect(await stack.ntt.getIsApproved(redeemed)).toBe(true);
    // Implies nothing is left queued: `getIsExecuted` is `executed && !queued`.
    expect(await stack.ntt.getIsExecuted(redeemed)).toBe(true);

    // Replay protection is the transceiver's, keyed by (chain, emitter,
    // sequence) rather than by the manager's digest.
    const transceiver = await stack.ntt.getTransceiver(0);
    expect(await transceiver!.isVaaConsumed(redeemed)).toBe(true);
    await expect(
      drive(stack.ntt.redeem([redeemed], adminAddress()))
    ).rejects.toThrow("TransceiverError::ReplayDetected (34)");
    expect(await balanceOf(stack.token, recipient.publicKey())).toBe(AMOUNT);
  });
});

/**
 * The limits are tiny and the token is 8-decimal — the peer's precision, so the
 * trimmed domain is 1:1 in both directions and the numbers below are the ones
 * the contract compares.
 *
 * A primer that consumes the whole bucket empties it, and capacity refills at
 * `limit / duration` = 2.5 a second. That sets a deadline rather than a margin:
 * the second transfer has `OVER_LIMIT / 2.5` = 30 ledger-seconds to be
 * simulated, after which capacity has caught up, it is not over the limit any
 * more and there is no queue entry to assert on. One submission is about three
 * seconds here, so the headroom is ~10x. Its window once queued —
 * `deficit × duration / limit` — is about 20 seconds.
 */
describe("rate limiting", () => {
  const LIMIT = 150n;
  const DURATION = 60n;
  const OVER_LIMIT = 75n;

  let stack: Stack;
  let recipient: Keypair;

  beforeAll(async () => {
    await fund(admin);
    stack = await deployStack({
      tokenDecimals: PEER_DECIMALS,
      outboundLimit: LIMIT,
      rateLimitDuration: DURATION,
    });
    await registerPeer(stack, LIMIT);
    await mint(stack.token, admin.publicKey(), SUPPLY);
    recipient = await fund(Keypair.random());
    await drive(
      stack.ntt.recordAddress(
        adminAddress(),
        new StellarAddress(recipient.publicKey())
      )
    );
  });

  const transferOut = (amount: bigint, queue: boolean) =>
    stack.ntt.transfer(
      adminAddress(),
      amount,
      {
        chain: PEER_CHAIN,
        address: new UniversalAddress(new Uint8Array(32).fill(0x40)),
      },
      { queue, automatic: false }
    );

  const redeemInbound = (message: Ntt.Message, sequence: bigint) =>
    drive(
      stack.ntt.redeem(
        [
          inboundVaa({
            emitterChain: PEER_CHAIN,
            emitter: PEER,
            sequence,
            sourceNttManager: PEER,
            recipientNttManager: hashAddress(stack.manager),
            nttManagerPayload: message,
          }),
        ],
        adminAddress()
      )
    );

  it("queues an over-limit outbound transfer, then releases it", async () => {
    expect(await stack.ntt.getOutboundLimit()).toBe(LIMIT);
    expect(await stack.ntt.getRateLimitDuration()).toBe(DURATION);
    await drive(transferOut(LIMIT, false));

    const [result] = await drive(transferOut(OVER_LIMIT, true));
    const { sequence } = decodeTransferResult(result);

    // The queue entry, not the `queued` flag the call returned, is what says it
    // was held: the flag is read a ledger early (see `submit`), and this is
    // keyed by sequence rather than by digest, since a queued outbound transfer
    // has not been turned into a message yet.
    const item = await stack.ntt.getOutboundQueuedTransfer(sequence);
    expect(item).not.toBeNull();
    expect(item?.sender).toBe(admin.publicKey());
    expect(item?.amount).toEqual({
      decimals: PEER_DECIMALS,
      amount: OVER_LIMIT,
    });

    await completeAfterWindow(
      () =>
        drive(
          stack.ntt.completeOutboundQueuedTransfer(sequence, adminAddress())
        ),
      item!.releaseTimestamp
    );
    expect(await stack.ntt.getOutboundQueuedTransfer(sequence)).toBeNull();
  });

  it("queues an over-limit inbound transfer, then releases it", async () => {
    expect(await stack.ntt.getInboundLimit(PEER_CHAIN)).toBe(LIMIT);
    await redeemInbound(transferTo(recipient.publicKey(), LIMIT), 1n);

    const message = transferTo(recipient.publicKey(), OVER_LIMIT);
    await redeemInbound(message, 2n);

    const queued = await stack.ntt.getInboundQueuedTransfer(
      PEER_CHAIN,
      message
    );
    expect(queued?.recipient.toString()).toBe(recipient.publicKey());
    expect(queued?.amount).toBe(OVER_LIMIT);
    expect(await balanceOf(stack.token, recipient.publicKey())).toBe(LIMIT);

    await completeAfterWindow(
      () =>
        drive(
          stack.ntt.completeInboundQueuedTransfer(
            PEER_CHAIN,
            message,
            adminAddress()
          )
        ),
      BigInt(queued!.rateLimitExpiryTimestamp)
    );
    expect(await balanceOf(stack.token, recipient.publicKey())).toBe(
      LIMIT + OVER_LIMIT
    );
  });
});

describe("locking manager", () => {
  it("takes custody of the amount it bridges", async () => {
    await fund(admin);
    // Locking mode uses the native SAC, whose 7 decimals the manager reads off
    // the token itself — the `tokenDecimals` option does not apply.
    const stack = await deployStack({ mode: "locking" });
    await registerPeer(stack, NO_LIMIT);
    expect(await stack.ntt.getMode()).toBe("locking");
    expect(await stack.ntt.getTokenDecimals()).toBe(7);

    expect(await balanceOf(stack.token, stack.manager)).toBe(0n);

    await drive(
      stack.ntt.transfer(
        adminAddress(),
        AMOUNT,
        {
          chain: PEER_CHAIN,
          address: new UniversalAddress(new Uint8Array(32).fill(0x40)),
        },
        { queue: false, automatic: false }
      )
    );

    // Locked rather than burned: the manager holds exactly what left the sender.
    expect(await balanceOf(stack.token, stack.manager)).toBe(AMOUNT);
  });
});
