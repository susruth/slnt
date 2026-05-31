import { expect } from "chai";
import { Keypair, PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import { ASSOCIATED_TOKEN_PROGRAM_ID, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import {
  RENT_EXEMPT_MIN,
  buildNftPayment,
  buildSolPayment,
  buildSplPayment,
} from "../src/flows";
import { buildSolSweep, buildSplSweep, ensureNotMainWallet } from "../src/sweep";
import { SlntError } from "../src/errors";

const key = () => Keypair.generate().publicKey;

// System Transfer: tag 2 (u32 LE) then lamports (u64 LE).
function lamportsOf(ix: TransactionInstruction): bigint {
  expect([...ix.data.subarray(0, 4)]).to.deep.equal([2, 0, 0, 0]);
  return ix.data.readBigUInt64LE(4);
}

describe("slnt TS SDK transactions", () => {
  describe("buildSolPayment", () => {
    it("is a SystemProgram transfer that adds the rent buffer", () => {
      const sender = key();
      const stealth = key();
      const ix = buildSolPayment(sender, stealth, 1_000_000n);

      expect(ix.programId.equals(SystemProgram.programId)).to.equal(true);
      expect(ix.keys[0].pubkey.equals(sender)).to.equal(true);
      expect(ix.keys[0].isSigner && ix.keys[0].isWritable).to.equal(true);
      expect(ix.keys[1].pubkey.equals(stealth)).to.equal(true);
      expect(lamportsOf(ix)).to.equal(1_000_000n + RENT_EXEMPT_MIN);
    });

    it("rejects lamport overflow near 2^64", () => {
      const sender = key();
      const stealth = key();
      const amount = (1n << 64n) - 1n;
      expect(() => buildSolPayment(sender, stealth, amount)).to.throw(SlntError);
      try {
        buildSolPayment(sender, stealth, amount);
      } catch (e) {
        expect((e as SlntError).code).to.equal("LamportOverflow");
      }
    });
  });

  describe("buildSplPayment", () => {
    it("creates the ATA then transfers (transfer-checked)", () => {
      const sender = key();
      const stealth = key();
      const mint = key();
      const senderAta = key();

      const ixs = buildSplPayment(sender, stealth, mint, senderAta, TOKEN_PROGRAM_ID, 42n, 6);
      expect(ixs.length).to.equal(2);
      // 1st: create ATA (owned by the ATA program).
      expect(ixs[0].programId.equals(ASSOCIATED_TOKEN_PROGRAM_ID)).to.equal(true);
      // 2nd: transfer_checked on the token program.
      expect(ixs[1].programId.equals(TOKEN_PROGRAM_ID)).to.equal(true);
      expect(ixs[1].data[0]).to.equal(12);
    });
  });

  describe("buildNftPayment", () => {
    it("equals buildSplPayment with amount=1, decimals=0", () => {
      const sender = key();
      const stealth = key();
      const mint = key();
      const senderAta = key();

      const nft = buildNftPayment(sender, stealth, mint, senderAta, TOKEN_PROGRAM_ID);
      const spl = buildSplPayment(sender, stealth, mint, senderAta, TOKEN_PROGRAM_ID, 1n, 0);
      expect(nft.length).to.equal(spl.length);
      expect([...nft[1].data]).to.deep.equal([...spl[1].data]);
    });
  });

  describe("buildSolSweep", () => {
    it("splits the balance between relayer and destination", () => {
      const stealth = key();
      const dest = key();
      const relayer = key();
      const ixs = buildSolSweep(stealth, dest, relayer, 1_000_000n, 5_000n, null);
      expect(ixs.length).to.equal(2);
      expect(ixs[0].keys[1].pubkey.equals(relayer)).to.equal(true);
      expect(lamportsOf(ixs[0])).to.equal(5_000n);
      expect(ixs[1].keys[1].pubkey.equals(dest)).to.equal(true);
      expect(lamportsOf(ixs[1])).to.equal(1_000_000n - 5_000n);
    });

    it("rejects destination equal to the main wallet", () => {
      const stealth = key();
      const main = key();
      const relayer = key();
      try {
        buildSolSweep(stealth, main, relayer, 1_000_000n, 5_000n, main);
        expect.fail("expected throw");
      } catch (e) {
        expect((e as SlntError).code).to.equal("CloseToMainWallet");
      }
    });

    it("rejects an oversized relayer take", () => {
      const stealth = key();
      const dest = key();
      const relayer = key();
      try {
        buildSolSweep(stealth, dest, relayer, 1_000n, 1_000n, null);
        expect.fail("expected throw");
      } catch (e) {
        expect((e as SlntError).code).to.equal("RelayerTakeTooLarge");
      }
    });
  });

  describe("buildSplSweep", () => {
    it("transfers, pays the relayer, and closes the ATA", () => {
      const auth = key();
      const stealthAta = key();
      const destAta = key();
      const relayerAta = key();
      const mint = key();
      const closeDest = key();

      const ixs = buildSplSweep(
        auth,
        stealthAta,
        destAta,
        relayerAta,
        mint,
        TOKEN_PROGRAM_ID,
        100n,
        3n,
        0,
        closeDest,
        null,
      );
      expect(ixs.length).to.equal(3);
      expect(ixs[0].data[0]).to.equal(12); // transfer_checked
      expect(ixs[1].data[0]).to.equal(12); // transfer_checked
      expect(ixs[2].data[0]).to.equal(9); // close_account
      expect(ixs[2].keys.some((k) => k.pubkey.equals(closeDest))).to.equal(true);
    });

    it("rejects closing to the main wallet", () => {
      const auth = key();
      const stealthAta = key();
      const destAta = key();
      const relayerAta = key();
      const mint = key();
      const main = key();
      try {
        buildSplSweep(auth, stealthAta, destAta, relayerAta, mint, TOKEN_PROGRAM_ID, 100n, 3n, 0, main, main);
        expect.fail("expected throw");
      } catch (e) {
        expect((e as SlntError).code).to.equal("CloseToMainWallet");
      }
    });
  });

  describe("ensureNotMainWallet", () => {
    it("throws when candidate equals the main wallet", () => {
      const main = key();
      expect(() => ensureNotMainWallet(main, main)).to.throw(SlntError);
    });

    it("passes for a null main wallet", () => {
      expect(() => ensureNotMainWallet(key(), null)).to.not.throw();
    });

    it("allows a stealth→stealth destination (different pubkey, non-null main wallet)", () => {
      const nextStealth = key();
      const main = key();
      expect(() => ensureNotMainWallet(nextStealth, main)).to.not.throw();
    });
  });
});
