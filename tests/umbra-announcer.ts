import * as anchor from "@coral-xyz/anchor";
import { Program, EventParser, BorshCoder } from "@coral-xyz/anchor";
import { UmbraAnnouncer } from "../target/types/umbra_announcer";
import { expect } from "chai";

describe("umbra-announcer", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.UmbraAnnouncer as Program<UmbraAnnouncer>;

  /**
   * Submit a tx, fetch its logs, and return all parsed Umbra events.
   */
  async function eventsFromTx(txSig: string) {
    // Wait for confirmation, then re-fetch with the logs.
    await provider.connection.confirmTransaction(txSig, "confirmed");
    const tx = await provider.connection.getTransaction(txSig, {
      commitment: "confirmed",
      maxSupportedTransactionVersion: 0,
    });
    if (!tx?.meta?.logMessages) {
      throw new Error("no logs in tx");
    }
    const parser = new EventParser(
      program.programId,
      new BorshCoder(program.idl)
    );
    return [...parser.parseLogs(tx.meta.logMessages)];
  }

  it("emits exactly one UmbraAnnouncement for a single announce", async () => {
    const ephemeralPub = Buffer.from(
      "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
      "hex"
    );
    const metadata = Buffer.from([0xab, 0xcd]);

    const txSig = await program.methods
      .announce(1, [...ephemeralPub], 0x42, metadata)
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    expect(events[0].name).to.equal("umbraAnnouncement");

    const data = events[0].data as {
      schemeId: number;
      ephemeralPub: number[];
      viewTag: number;
      metadata: Buffer;
    };
    expect(data.schemeId).to.equal(1);
    expect(Buffer.from(data.ephemeralPub)).to.deep.equal(ephemeralPub);
    expect(data.viewTag).to.equal(0x42);
    expect(Buffer.from(data.metadata)).to.deep.equal(metadata);
  });

  it("rejects metadata longer than 64 bytes", async () => {
    const ephemeralPub = new Array(32).fill(0);
    const metadata = Buffer.alloc(65, 0xaa); // 65 bytes

    let threw = false;
    try {
      await program.methods
        .announce(1, ephemeralPub, 0x00, metadata)
        .rpc();
    } catch (err: any) {
      threw = true;
      const errMessage = err?.error?.errorMessage ?? err?.message ?? "";
      expect(errMessage).to.match(/metadata exceeds 64 bytes/i);
    }
    expect(threw, "expected announce(metadata.len=65) to throw").to.equal(true);
  });

  it("accepts metadata of exactly 64 bytes", async () => {
    const ephemeralPub = new Array(32).fill(0);
    const metadata = Buffer.alloc(64, 0xbb); // 64 bytes — boundary

    const txSig = await program.methods
      .announce(1, ephemeralPub, 0x00, metadata)
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    const data = events[0].data as { metadata: Buffer };
    expect(Buffer.from(data.metadata)).to.deep.equal(metadata);
  });

  it("accepts empty metadata", async () => {
    const ephemeralPub = new Array(32).fill(0);
    const metadata = Buffer.alloc(0);

    const txSig = await program.methods
      .announce(1, ephemeralPub, 0x00, metadata)
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    const data = events[0].data as { metadata: Buffer };
    expect(Buffer.from(data.metadata)).to.deep.equal(metadata);
  });

  it("emits one event per entry for announce_batch", async () => {
    const entries = [
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x01),
        viewTag: 0x10,
        metadata: Buffer.from([0xa1]),
      },
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x02),
        viewTag: 0x20,
        metadata: Buffer.from([0xa2, 0xa2]),
      },
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x03),
        viewTag: 0x30,
        metadata: Buffer.alloc(0),
      },
    ];

    const txSig = await program.methods
      .announceBatch(entries)
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(3);

    for (let i = 0; i < entries.length; i++) {
      const data = events[i].data as {
        schemeId: number;
        ephemeralPub: number[];
        viewTag: number;
        metadata: Buffer;
      };
      expect(events[i].name).to.equal("umbraAnnouncement");
      expect(data.schemeId).to.equal(entries[i].schemeId);
      expect(Buffer.from(data.ephemeralPub)).to.deep.equal(
        Buffer.from(entries[i].ephemeralPub)
      );
      expect(data.viewTag).to.equal(entries[i].viewTag);
      expect(Buffer.from(data.metadata)).to.deep.equal(entries[i].metadata);
    }
  });
});
