/**
 * `StellarNttWithExecutor` against the deployed `ntt-with-executor` wrapper and
 * a real Executor rail.
 *
 * This is the one path whose arguments the unit tests cannot fully pin: the
 * wrapper takes three `#[contracttype]` structs, and a struct crosses the ABI as
 * an ScMap whose keys must be **symbols** in the host's byte order —
 * `scValToNative` reads a symbol key and a string key back alike, so only the
 * host can say whether `structArg` built the right thing. The executor verifies
 * the quote's signature off-chain and parses only its 68-byte header on-chain,
 * so a header built here is enough to drive the whole call without a quoter that
 * supports chain 61.
 */
import { Keypair, StrKey } from "@stellar/stellar-sdk";
import { encoding } from "@wormhole-foundation/sdk-base";
import {
  UniversalAddress,
  deserializePayload,
} from "@wormhole-foundation/sdk-definitions";
import type { ChainAddress } from "@wormhole-foundation/sdk-definitions";
import type { NttWithExecutor } from "@wormhole-foundation/sdk-definitions-ntt";
import { StellarAddress } from "@wormhole-foundation/sdk-stellar";
import type { StellarNttWithExecutor } from "../src/nttWithExecutor.js";
import { asBigint, asBytes } from "../src/scval-types.js";
import type { Stack } from "./localnet.js";
import {
  CHAIN,
  NETWORK,
  admin,
  adminAddress,
  balanceOf,
  deployExecutor,
  deployStack,
  drive,
  findEvent,
  fund,
  hashAddress,
  ledgerTime,
  mint,
  nativeSac,
} from "./localnet.js";

const PEER_CHAIN = "Ethereum";
const PEER = new Uint8Array(32).fill(0xaa);
const PEER_DECIMALS = 8;
const RECIPIENT = new Uint8Array(32).fill(0x40);

const SUPPLY = 100_000_000n;
const AMOUNT = 10_000_000n;
/** 1000 tenths of a basis point over a 100_000 denominator — 1% of `AMOUNT`. */
const DBPS = 1000n;
const REFERRER_FEE = 100_000n;
/** Stroops of XLM the executor pulls from the sender for the relay. */
const RELAY_COST = 5_000_000n;

describe("ntt-with-executor", () => {
  let stack: Stack;
  let sac: string;
  let executor: string;
  let withExecutor: StellarNttWithExecutor<typeof NETWORK, typeof CHAIN>;
  let referrer: Keypair;
  let payee: Keypair;

  beforeAll(async () => {
    await fund(admin);
    stack = await deployStack({ mode: "burning", tokenDecimals: 7 });
    const peer: ChainAddress = {
      chain: PEER_CHAIN,
      address: new UniversalAddress(PEER),
    };
    await drive(
      stack.ntt.setPeer(peer, PEER_DECIMALS, 2n ** 63n, adminAddress())
    );
    await drive(stack.ntt.setTransceiverPeer(0, peer, adminAddress()));
    await mint(stack.token, admin.publicKey(), SUPPLY);
    ({ executor, withExecutor } = await deployExecutor(stack.ntt));
    sac = await nativeSac();
    referrer = await fund(Keypair.random());
    payee = await fund(Keypair.random());
  });

  it("bridges the amount less the referrer fee and pays the executor", async () => {
    const quote = await signedQuote(payee.rawPublicKey(), DBPS);
    const paidBefore = await balanceOf(sac, payee.publicKey());

    const [sequence] = await drive(
      withExecutor.transfer(
        adminAddress(),
        { chain: PEER_CHAIN, address: new UniversalAddress(RECIPIENT) },
        AMOUNT,
        quote,
        stack.ntt
      )
    );

    // The whole amount crosses the ABI and the wrapper splits it on chain, so
    // the fee comes out of it rather than being added to it.
    expect(await balanceOf(stack.token, referrer.publicKey())).toBe(
      REFERRER_FEE
    );
    expect(await balanceOf(stack.token, admin.publicKey())).toBe(
      SUPPLY - AMOUNT
    );
    expect(await balanceOf(sac, payee.publicKey())).toBe(
      paidBefore + RELAY_COST
    );

    const published = await findEvent(stack.core, "message_published");
    const { nttManagerPayload } = deserializePayload(
      "Ntt:WormholeTransfer",
      asBytes(published["payload"])
    );
    expect(nttManagerPayload.payload.trimmedAmount).toEqual({
      decimals: 7,
      amount: AMOUNT - REFERRER_FEE,
    });

    const request = await findEvent(executor, "RequestForExecution");
    expect(request["amt_paid"]).toBe(RELAY_COST);
    expect(request["dst_chain"]).toBe(2);
    // The wrapper addresses the delivery at the peer manager it read off the
    // manager's own peer table, not at anything the caller passed.
    expect(request["dst_addr"]).toEqual(Buffer.from(PEER));
    expect(request["refund_addr"]).toBe(admin.publicKey());
    expect(request["signed_quote"]).toEqual(Buffer.from(quote.signedQuote));
    expect(request["relay_instructions"]).toEqual(
      Buffer.from(quote.relayInstructions)
    );
    // ERN1: "ERN1" ‖ srcChain u16 BE ‖ hash_address(manager) ‖ 32-byte message id.
    expect(request["request"]).toEqual(
      Buffer.from(
        encoding.bytes.concat(
          encoding.bytes.encode("ERN1"),
          encoding.bignum.toBytes(61, 2),
          hashAddress(stack.manager),
          encoding.bignum.toBytes(asBigint(sequence), 32)
        )
      )
    );
  });

  it("pays a payee the quote names as a contract", async () => {
    // The quote carries 32 raw bytes and the Executor accepts them as either
    // address form, so the binding asks the RPC which one is deployed. Here the
    // answer is the Executor's own id, and the fee lands in its balance.
    const quote = await signedQuote(
      new Uint8Array(StrKey.decodeContract(executor)),
      0n
    );
    const paidBefore = await balanceOf(sac, executor);

    await drive(
      withExecutor.transfer(
        adminAddress(),
        { chain: PEER_CHAIN, address: new UniversalAddress(RECIPIENT) },
        AMOUNT,
        quote,
        stack.ntt
      )
    );

    expect(await balanceOf(sac, executor)).toBe(paidBefore + RELAY_COST);
    // No fee at zero dbps, so the referrer keeps only what the first test paid.
    expect(await balanceOf(stack.token, referrer.publicKey())).toBe(
      REFERRER_FEE
    );
  });

  /**
   * A quote whose 68-byte header says what the Executor checks: the quoter at
   * `[4..24)`, the payee at `[24..56)`, the source and destination chains, and
   * an expiry an hour out on the ledger clock. Everything past the header is
   * opaque to the contract and is emitted verbatim.
   */
  async function signedQuote(
    payeeAddress: Uint8Array,
    referrerFeeDbps: bigint
  ): Promise<NttWithExecutor.Quote> {
    const expiry = (await ledgerTime()) + 3600n;
    const header = new Uint8Array(68);
    header.set(encoding.bytes.encode("EQ01"), 0);
    header.set(new Uint8Array(20).fill(0x11), 4);
    header.set(payeeAddress, 24);
    header.set(encoding.bignum.toBytes(61, 2), 56);
    header.set(encoding.bignum.toBytes(2, 2), 58);
    header.set(encoding.bignum.toBytes(expiry, 8), 60);

    return {
      signedQuote: header,
      relayInstructions: Uint8Array.from([0x01, 0x02, 0x03]),
      estimatedCost: RELAY_COST,
      payeeAddress,
      referrer: {
        chain: "Stellar",
        address: new StellarAddress(referrer.publicKey()),
      },
      referrerFee: (AMOUNT * referrerFeeDbps) / 100_000n,
      remainingAmount: AMOUNT - (AMOUNT * referrerFeeDbps) / 100_000n,
      referrerFeeDbps,
      expires: new Date(Number(expiry) * 1000),
      gasDropOff: 0n,
    };
  }
});
