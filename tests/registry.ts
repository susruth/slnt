import * as anchor from "@coral-xyz/anchor";
import { Program, EventParser, BorshCoder } from "@coral-xyz/anchor";
import { UmbraRegistry } from "../target/types/umbra_registry";
import { PublicKey, Keypair } from "@solana/web3.js";
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

    await program.methods
      .register(schemeId, validPayload())
      .accounts({
        registrant: registrant.publicKey,
      })
      .signers([registrant])
      .rpc();

    let threw = false;
    try {
      await program.methods
        .register(schemeId, validPayload())
        .accounts({
          registrant: registrant.publicKey,
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

    // We use scheme_id=2 here purely to test PDA independence; the
    // payload itself is the v1-valid one.
    await program.methods
      .register(1, validPayload())
      .accounts({
        registrant: registrant.publicKey,
      })
      .signers([registrant])
      .rpc();

    await program.methods
      .register(2, validPayload())
      .accounts({
        registrant: registrant.publicKey,
      })
      .signers([registrant])
      .rpc();

    const account1 = await program.account.metaAddressEntry.fetch(entry1);
    const account2 = await program.account.metaAddressEntry.fetch(entry2);
    expect(account1.schemeId).to.equal(1);
    expect(account2.schemeId).to.equal(2);
  });

  it("register: scheme_id = 0 fails with InvalidSchemeId", async () => {
    const registrant = await freshFunded();
    let errMessage = "";
    try {
      await program.methods
        .register(0, validPayload())
        .accounts({
          registrant: registrant.publicKey,
        })
        .signers([registrant])
        .rpc();
    } catch (err: any) {
      errMessage = err?.error?.errorMessage ?? err?.message ?? "";
    }
    expect(errMessage).to.match(/scheme_id must be non-zero/i);
  });

  it("register: version != 0x01 fails with InvalidVersion", async () => {
    const registrant = await freshFunded();
    const badPayload = { ...validPayload(), version: 2 };
    let errMessage = "";
    try {
      await program.methods
        .register(1, badPayload)
        .accounts({
          registrant: registrant.publicKey,
        })
        .signers([registrant])
        .rpc();
    } catch (err: any) {
      errMessage = err?.error?.errorMessage ?? err?.message ?? "";
    }
    expect(errMessage).to.match(/only meta-address version 0x01/i);
  });

  it("register: version = 0 fails with InvalidVersion", async () => {
    const registrant = await freshFunded();
    const badPayload = { ...validPayload(), version: 0 };
    let errMessage = "";
    try {
      await program.methods
        .register(1, badPayload)
        .accounts({
          registrant: registrant.publicKey,
        })
        .signers([registrant])
        .rpc();
    } catch (err: any) {
      errMessage = err?.error?.errorMessage ?? err?.message ?? "";
    }
    expect(errMessage).to.match(/only meta-address version 0x01/i);
  });

  it("register: flags != 0 fails with InvalidFlags", async () => {
    const registrant = await freshFunded();
    const badPayload = { ...validPayload(), flags: 1 };
    let errMessage = "";
    try {
      await program.methods
        .register(1, badPayload)
        .accounts({
          registrant: registrant.publicKey,
        })
        .signers([registrant])
        .rpc();
    } catch (err: any) {
      errMessage = err?.error?.errorMessage ?? err?.message ?? "";
    }
    expect(errMessage).to.match(/flags must be 0x00/i);
  });

  it("update: changes fields in-place and emits event", async () => {
    const registrant = await freshFunded();
    const schemeId = 1;
    const [entry] = pda(registrant.publicKey, schemeId);

    await program.methods
      .register(schemeId, validPayload())
      .accounts({
        registrant: registrant.publicKey,
      })
      .signers([registrant])
      .rpc();

    const newPayload = {
      version: 1,
      bSpend: new Array(32).fill(0xcc),
      bScan: new Array(32).fill(0xdd),
      flags: 0,
    };

    const accountBefore = await provider.connection.getAccountInfo(entry);
    const sizeBefore = accountBefore!.data.length;

    const txSig = await program.methods
      .update(schemeId, newPayload)
      .accounts({
        registrant: registrant.publicKey,
      })
      .signers([registrant])
      .rpc();

    const account = await program.account.metaAddressEntry.fetch(entry);
    expect(Buffer.from(account.bSpend)).to.deep.equal(
      Buffer.from(new Array(32).fill(0xcc)),
    );
    expect(Buffer.from(account.bScan)).to.deep.equal(
      Buffer.from(new Array(32).fill(0xdd)),
    );

    const accountAfter = await provider.connection.getAccountInfo(entry);
    expect(accountAfter!.data.length).to.equal(sizeBefore);

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    expect(events[0].name).to.equal("metaAddressUpdated");
  });

  it("update: non-existent PDA fails", async () => {
    const registrant = await freshFunded();
    let threw = false;
    try {
      await program.methods
        .update(1, validPayload())
        .accounts({
          registrant: registrant.publicKey,
        })
        .signers([registrant])
        .rpc();
    } catch (_err) {
      threw = true;
    }
    expect(threw, "expected update on non-existent PDA to fail").to.equal(true);
  });

  it("update: non-registrant signer fails", async () => {
    const registrant = await freshFunded();
    const attacker = await freshFunded();
    const schemeId = 1;
    const [entry] = pda(registrant.publicKey, schemeId);

    await program.methods
      .register(schemeId, validPayload())
      .accounts({
        registrant: registrant.publicKey,
      })
      .signers([registrant])
      .rpc();

    let threw = false;
    try {
      // Attacker tries to call update with their own pubkey as `registrant`
      // while explicitly passing the original registrant's `entry`. We must
      // use `.accountsPartial()` here so we can override Anchor's
      // auto-derivation of `entry` and force it to point at the wrong PDA.
      // Without this override the test would only verify "can't update a
      // non-existent PDA" — which the previous test already covers.
      await program.methods
        .update(schemeId, validPayload())
        .accountsPartial({
          registrant: attacker.publicKey,
          entry, // the original entry, owned by `registrant`
        })
        .signers([attacker])
        .rpc();
    } catch (_err) {
      threw = true;
    }
    expect(threw, "expected non-registrant update to fail").to.equal(true);
  });

  it("update: validations fire (version = 2)", async () => {
    // We can't actually call update with scheme_id = 0 against an existing
    // entry because scheme_id is in the seeds — the PDA wouldn't match.
    // But the require!() check still fires before account validation when
    // we test an invalid version on a valid scheme_id.
    const registrant = await freshFunded();
    const schemeId = 1;

    await program.methods
      .register(schemeId, validPayload())
      .accounts({
        registrant: registrant.publicKey,
      })
      .signers([registrant])
      .rpc();

    let errMessage = "";
    try {
      await program.methods
        .update(schemeId, { ...validPayload(), version: 2 })
        .accounts({
          registrant: registrant.publicKey,
        })
        .signers([registrant])
        .rpc();
    } catch (err: any) {
      errMessage = err?.error?.errorMessage ?? err?.message ?? "";
    }
    expect(errMessage).to.match(/only meta-address version 0x01/i);
  });
});
