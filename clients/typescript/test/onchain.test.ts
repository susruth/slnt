import { expect } from "chai";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import { base64 } from "@scure/base";
import { ByteReader, ByteWriter } from "../src/anchor";
import {
  NOTE_EVENT_DISCRIMINATOR,
  POST_BATCH_DISCRIMINATOR,
  POST_DISCRIMINATOR,
  buildPostBatchInstruction,
  buildPostInstruction,
  tryParseNoteLog,
  type NoteEntry,
} from "../src/pinboard";
import {
  CLOSE_DISCRIMINATOR,
  META_ADDRESS_ENTRY_DISCRIMINATOR,
  REGISTER_DISCRIMINATOR,
  UPDATE_DISCRIMINATOR,
  buildCloseInstruction,
  buildRegisterInstruction,
  buildUpdateInstruction,
  registryPda,
  tryParseMetaAddressEntry,
  type MetaAddressEntry,
  type MetaAddressPayload,
} from "../src/registry";

// Discriminator byte arrays produced by the Rust reference (`slnt-sdk`).
const RUST_POST = [223, 96, 234, 236, 158, 106, 145, 94];
const RUST_POST_BATCH = [172, 123, 234, 102, 14, 213, 76, 36];
const RUST_NOTE_EVENT = [40, 182, 5, 151, 115, 43, 27, 97];
const RUST_REGISTER = [211, 124, 67, 15, 211, 194, 178, 240];
const RUST_UPDATE = [219, 200, 88, 176, 158, 63, 253, 127];
const RUST_CLOSE = [98, 165, 201, 177, 108, 65, 206, 96];
const RUST_META_ENTRY = [165, 7, 241, 154, 7, 172, 74, 178];

const PROGRAM = PublicKey.unique();
const FEE_PAYER = PublicKey.unique();
const REGISTRANT = PublicKey.unique();

describe("pinboard", () => {
  it("computes the Rust discriminators", () => {
    expect([...POST_DISCRIMINATOR]).to.deep.equal(RUST_POST);
    expect([...POST_BATCH_DISCRIMINATOR]).to.deep.equal(RUST_POST_BATCH);
    expect([...NOTE_EVENT_DISCRIMINATOR]).to.deep.equal(RUST_NOTE_EVENT);
  });

  it("buildPostInstruction has one signer+writable account and correct data", () => {
    const ephemeralPub = new Uint8Array(32).fill(7);
    const metadata = new Uint8Array([0xab, 0xcd]);
    const ix = buildPostInstruction(PROGRAM, FEE_PAYER, 1, ephemeralPub, 0x42, metadata);

    expect(ix.programId.equals(PROGRAM)).to.equal(true);
    expect(ix.keys.length).to.equal(1);
    expect(ix.keys[0].pubkey.equals(FEE_PAYER)).to.equal(true);
    expect(ix.keys[0].isSigner).to.equal(true);
    expect(ix.keys[0].isWritable).to.equal(true);
    expect([...ix.data.slice(0, 8)]).to.deep.equal(RUST_POST);

    // Round-trip via a synthetic Program data line built from the same body.
    const payload = new ByteWriter()
      .bytes(NOTE_EVENT_DISCRIMINATOR)
      .bytes(ix.data.slice(8))
      .toBytes();
    const line = `Program data: ${base64.encode(payload)}`;
    const parsed = tryParseNoteLog(line);
    expect(parsed).to.not.equal(null);
    expect(parsed!.schemeId).to.equal(1);
    expect([...parsed!.ephemeralPub]).to.deep.equal([...ephemeralPub]);
    expect(parsed!.viewTag).to.equal(0x42);
    expect([...parsed!.metadata]).to.deep.equal([...metadata]);
  });

  it("buildPostBatchInstruction encodes discriminator and entry count", () => {
    const entries: NoteEntry[] = [
      { schemeId: 1, ephemeralPub: new Uint8Array(32).fill(1), viewTag: 0x11, metadata: new Uint8Array() },
      { schemeId: 1, ephemeralPub: new Uint8Array(32).fill(2), viewTag: 0x22, metadata: new Uint8Array([9, 9]) },
    ];
    const ix = buildPostBatchInstruction(PROGRAM, FEE_PAYER, entries);

    expect(ix.keys.length).to.equal(1);
    expect(ix.keys[0].isSigner).to.equal(true);
    expect(ix.keys[0].isWritable).to.equal(true);
    expect([...ix.data.slice(0, 8)]).to.deep.equal(RUST_POST_BATCH);

    const r = new ByteReader(new Uint8Array(ix.data.slice(8)));
    expect(r.u32LE()).to.equal(entries.length);
    // First entry decodes to the same fields.
    expect(r.u16LE()).to.equal(1);
    expect([...r.bytes(32)]).to.deep.equal([...entries[0].ephemeralPub]);
    expect(r.u8()).to.equal(0x11);
    expect([...r.vecU8()]).to.deep.equal([]);
  });

  it("tryParseNoteLog returns null for non-matching lines", () => {
    expect(tryParseNoteLog("Program log: hello")).to.equal(null);
    const wrong = new ByteWriter().bytes(new Uint8Array(8)).bytes(new Uint8Array([1, 2, 3, 4])).toBytes();
    expect(tryParseNoteLog(`Program data: ${base64.encode(wrong)}`)).to.equal(null);
  });
});

describe("registry", () => {
  it("computes the Rust discriminators", () => {
    expect([...REGISTER_DISCRIMINATOR]).to.deep.equal(RUST_REGISTER);
    expect([...UPDATE_DISCRIMINATOR]).to.deep.equal(RUST_UPDATE);
    expect([...CLOSE_DISCRIMINATOR]).to.deep.equal(RUST_CLOSE);
    expect([...META_ADDRESS_ENTRY_DISCRIMINATOR]).to.deep.equal(RUST_META_ENTRY);
  });

  it("registryPda is deterministic and varies by inputs", () => {
    const [a] = registryPda(PROGRAM, REGISTRANT, 1);
    const [b] = registryPda(PROGRAM, REGISTRANT, 1);
    expect(a.equals(b)).to.equal(true);

    const [c] = registryPda(PROGRAM, REGISTRANT, 2);
    expect(a.equals(c)).to.equal(false);

    const [d] = registryPda(PROGRAM, PublicKey.unique(), 1);
    expect(a.equals(d)).to.equal(false);
  });

  function samplePayload(): MetaAddressPayload {
    return {
      version: 1,
      bSpend: new Uint8Array(32).fill(0xaa),
      bScan: new Uint8Array(32).fill(0xbb),
      flags: 0,
    };
  }

  function assertPayloadBody(body: Uint8Array, payload: MetaAddressPayload, schemeId: number): void {
    const r = new ByteReader(body);
    expect(r.u16LE()).to.equal(schemeId);
    expect(r.u8()).to.equal(payload.version);
    expect([...r.bytes(32)]).to.deep.equal([...payload.bSpend]);
    expect([...r.bytes(32)]).to.deep.equal([...payload.bScan]);
    expect(r.u8()).to.equal(payload.flags);
    expect(r.remaining()).to.equal(0);
  }

  it("buildRegisterInstruction has 3 accounts and round-trips the payload", () => {
    const payload = samplePayload();
    const [pda] = registryPda(PROGRAM, REGISTRANT, 1);
    const ix = buildRegisterInstruction(PROGRAM, REGISTRANT, 1, payload);

    expect(ix.programId.equals(PROGRAM)).to.equal(true);
    expect(ix.keys.length).to.equal(3);
    expect(ix.keys[0].pubkey.equals(REGISTRANT)).to.equal(true);
    expect(ix.keys[0].isSigner && ix.keys[0].isWritable).to.equal(true);
    expect(ix.keys[1].pubkey.equals(pda)).to.equal(true);
    expect(ix.keys[1].isWritable && !ix.keys[1].isSigner).to.equal(true);
    expect(ix.keys[2].pubkey.equals(SystemProgram.programId)).to.equal(true);
    expect(ix.keys[2].isSigner || ix.keys[2].isWritable).to.equal(false);

    expect([...ix.data.slice(0, 8)]).to.deep.equal(RUST_REGISTER);
    assertPayloadBody(new Uint8Array(ix.data.slice(8)), payload, 1);
  });

  it("buildUpdateInstruction has 2 accounts, readonly registrant, round-trips payload", () => {
    const payload = samplePayload();
    const [pda] = registryPda(PROGRAM, REGISTRANT, 1);
    const ix = buildUpdateInstruction(PROGRAM, REGISTRANT, 1, payload);

    expect(ix.keys.length).to.equal(2);
    expect(ix.keys[0].isSigner && !ix.keys[0].isWritable).to.equal(true);
    expect(ix.keys[1].pubkey.equals(pda)).to.equal(true);
    expect(ix.keys[1].isWritable).to.equal(true);
    expect([...ix.data.slice(0, 8)]).to.deep.equal(RUST_UPDATE);
    assertPayloadBody(new Uint8Array(ix.data.slice(8)), payload, 1);
  });

  it("buildCloseInstruction has writable registrant and scheme_id arg", () => {
    const ix = buildCloseInstruction(PROGRAM, REGISTRANT, 7);
    expect(ix.keys.length).to.equal(2);
    expect(ix.keys[0].isSigner && ix.keys[0].isWritable).to.equal(true);
    expect([...ix.data.slice(0, 8)]).to.deep.equal(RUST_CLOSE);
    const r = new ByteReader(new Uint8Array(ix.data.slice(8)));
    expect(r.u16LE()).to.equal(7);
    expect(r.remaining()).to.equal(0);
  });

  it("tryParseMetaAddressEntry round-trips", () => {
    const entry: MetaAddressEntry = {
      registrant: PublicKey.unique(),
      schemeId: 1,
      bump: 254,
      version: 1,
      bSpend: new Uint8Array(32).fill(0xaa),
      bScan: new Uint8Array(32).fill(0xbb),
      flags: 0,
    };
    const data = new ByteWriter()
      .bytes(META_ADDRESS_ENTRY_DISCRIMINATOR)
      .bytes(entry.registrant.toBytes())
      .u16LE(entry.schemeId)
      .u8(entry.bump)
      .u8(entry.version)
      .bytes(entry.bSpend)
      .bytes(entry.bScan)
      .u8(entry.flags)
      .toBytes();

    const parsed = tryParseMetaAddressEntry(data);
    expect(parsed).to.not.equal(null);
    expect(parsed!.registrant.equals(entry.registrant)).to.equal(true);
    expect(parsed!.schemeId).to.equal(entry.schemeId);
    expect(parsed!.bump).to.equal(entry.bump);
    expect(parsed!.version).to.equal(entry.version);
    expect([...parsed!.bSpend]).to.deep.equal([...entry.bSpend]);
    expect([...parsed!.bScan]).to.deep.equal([...entry.bScan]);
    expect(parsed!.flags).to.equal(entry.flags);

    expect(tryParseMetaAddressEntry(new Uint8Array([1, 2, 3]))).to.equal(null);
    expect(tryParseMetaAddressEntry(new Uint8Array(8 + 101))).to.equal(null);
  });
});
