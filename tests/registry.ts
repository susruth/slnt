import * as anchor from "@coral-xyz/anchor";
import { Program, EventParser, BorshCoder } from "@coral-xyz/anchor";
import { UmbraRegistry } from "../target/types/umbra_registry";
import { PublicKey, Keypair, SystemProgram } from "@solana/web3.js";
import { expect } from "chai";

describe("registry", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.UmbraRegistry as Program<UmbraRegistry>;

  async function eventsFromTx(txSig: string) {
    await provider.connection.confirmTransaction(txSig, "confirmed");
    const tx = await provider.connection.getTransaction(txSig, {
      commitment: "confirmed",
      maxSupportedTransactionVersion: 0,
    });
    if (!tx?.meta?.logMessages) throw new Error("no logs in tx");
    const parser = new EventParser(
      program.programId,
      new BorshCoder(program.idl),
    );
    return [...parser.parseLogs(tx.meta.logMessages)];
  }

  /** Derive the registry PDA for a (registrant, schemeId) pair. */
  function pda(registrant: PublicKey, schemeId: number): [PublicKey, number] {
    const schemeIdBuf = Buffer.alloc(2);
    schemeIdBuf.writeUInt16LE(schemeId);
    return PublicKey.findProgramAddressSync(
      [Buffer.from("meta"), registrant.toBuffer(), schemeIdBuf],
      program.programId,
    );
  }

  /** Fund a fresh keypair with enough SOL to pay rent + fees. */
  async function freshFunded(): Promise<Keypair> {
    const kp = Keypair.generate();
    const sig = await provider.connection.requestAirdrop(
      kp.publicKey,
      2_000_000_000, // 2 SOL
    );
    await provider.connection.confirmTransaction(sig, "confirmed");
    return kp;
  }

  const validPayload = () => ({
    version: 1,
    bSpend: new Array(32).fill(0xaa),
    bScan: new Array(32).fill(0xbb),
    flags: 0,
  });

  it("register: initializes PDA and emits event", async () => {
    const registrant = await freshFunded();
    const schemeId = 1;
    const [entry] = pda(registrant.publicKey, schemeId);

    const txSig = await program.methods
      .register(schemeId, validPayload())
      .accounts({
        registrant: registrant.publicKey,
        entry,
        systemProgram: SystemProgram.programId,
      })
      .signers([registrant])
      .rpc();

    const account = await program.account.metaAddressEntry.fetch(entry);
    expect(account.registrant.toBase58()).to.equal(
      registrant.publicKey.toBase58(),
    );
    expect(account.schemeId).to.equal(schemeId);
    expect(account.version).to.equal(1);
    expect(Buffer.from(account.bSpend)).to.deep.equal(
      Buffer.from(new Array(32).fill(0xaa)),
    );
    expect(Buffer.from(account.bScan)).to.deep.equal(
      Buffer.from(new Array(32).fill(0xbb)),
    );
    expect(account.flags).to.equal(0);

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    expect(events[0].name).to.equal("metaAddressRegistered");
    const data = events[0].data as { schemeId: number; version: number };
    expect(data.schemeId).to.equal(schemeId);
    expect(data.version).to.equal(1);
  });

  it("register: same (registrant, scheme_id) twice fails", async () => {
    const registrant = await freshFunded();
    const schemeId = 1;
    const [entry] = pda(registrant.publicKey, schemeId);

    await program.methods
      .register(schemeId, validPayload())
      .accounts({
        registrant: registrant.publicKey,
        entry,
        systemProgram: SystemProgram.programId,
      })
      .signers([registrant])
      .rpc();

    let threw = false;
    try {
      await program.methods
        .register(schemeId, validPayload())
        .accounts({
          registrant: registrant.publicKey,
          entry,
          systemProgram: SystemProgram.programId,
        })
        .signers([registrant])
        .rpc();
    } catch (_err) {
      threw = true;
    }
    expect(threw, "expected second register to throw").to.equal(true);
  });

  it("register: different scheme_ids for same registrant coexist", async () => {
    const registrant = await freshFunded();
    const [entry1] = pda(registrant.publicKey, 1);
    const [entry2] = pda(registrant.publicKey, 2);

    // Note: we use scheme_id=2 here purely to test PDA independence.
    // The version-check tests in Task 4 confirm we don't accept arbitrary
    // versions. The current `validPayload` has version=1, which the
    // v1 program accepts.
    await program.methods
      .register(1, validPayload())
      .accounts({
        registrant: registrant.publicKey,
        entry: entry1,
        systemProgram: SystemProgram.programId,
      })
      .signers([registrant])
      .rpc();

    await program.methods
      .register(2, validPayload())
      .accounts({
        registrant: registrant.publicKey,
        entry: entry2,
        systemProgram: SystemProgram.programId,
      })
      .signers([registrant])
      .rpc();

    const account1 = await program.account.metaAddressEntry.fetch(entry1);
    const account2 = await program.account.metaAddressEntry.fetch(entry2);
    expect(account1.schemeId).to.equal(1);
    expect(account2.schemeId).to.equal(2);
  });
});
