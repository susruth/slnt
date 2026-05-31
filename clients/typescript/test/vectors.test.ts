import { expect } from "chai";
import { readFileSync } from "node:fs";
import path from "node:path";
import { base64 } from "@scure/base";
import { PublicKey } from "@solana/web3.js";
import {
  ByteWriter,
  buildPostInstruction,
  buildRegisterInstruction,
  canonicalMessage,
  decodeMetaAddress,
  derivePayment,
  deriveStealthKeysFromSignature,
  deriveStealthKeysHd,
  encodeMetaAddress,
  labelTweakScalar,
  metaForLabel,
  metaFromKeys,
  META_ADDRESS_ENTRY_DISCRIMINATOR,
  NOTE_EVENT_DISCRIMINATOR,
  registryPda,
  scalarToLeBytes,
  scanNoteCandidates,
  SCHEME_ID_V1,
  SlntError,
} from "../src/index";

const vectors = JSON.parse(
  readFileSync(path.resolve(process.cwd(), "../../test-vectors.json"), "utf8"),
);

function hexToBytes(hex: string): Uint8Array {
  const clean = hex.startsWith("0x") ? hex.slice(2) : hex;
  return Uint8Array.from(Buffer.from(clean, "hex"));
}

function bytesToHex(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("hex");
}

function viewTag(hex: string): number {
  return Number.parseInt(hex.startsWith("0x") ? hex.slice(2) : hex, 16);
}

describe("test-vectors.json", () => {
  it("matches key derivation and meta-address vectors", () => {
    expect(vectors.srfc).to.equal("sRFC-0042");
    expect(vectors.version).to.equal(1);

    for (const c of vectors.vectors.method1_hd) {
      const keys = deriveStealthKeysHd(hexToBytes(c.seed_hex), c.account);
      const meta = metaFromKeys(keys);
      expect(bytesToHex(keys.BSpend)).to.equal(c.b_spend_hex);
      expect(bytesToHex(keys.BScan)).to.equal(c.b_scan_hex);
      expect(encodeMetaAddress(meta)).to.equal(c.meta_address);
    }

    for (const c of vectors.vectors.method2_signature) {
      const keys = deriveStealthKeysFromSignature(hexToBytes(c.signature_hex));
      const meta = metaFromKeys(keys);
      expect(canonicalMessage(c.network)).to.equal(c.canonical_message_utf8);
      expect(bytesToHex(keys.BSpend)).to.equal(c.b_spend_hex);
      expect(bytesToHex(keys.bScanRaw)).to.equal(c.b_scan_raw_hex);
      expect(bytesToHex(keys.BScan)).to.equal(c.b_scan_hex);
      expect(encodeMetaAddress(meta)).to.equal(c.meta_address);
    }

    for (const c of vectors.vectors.labels) {
      const keys = deriveStealthKeysFromSignature(hexToBytes(c.signature_hex));
      const meta = metaForLabel(keys, c.label_index);
      expect(bytesToHex(scalarToLeBytes(labelTweakScalar(keys.bScanRaw, c.label_index)))).to.equal(
        c.label_tweak_hex,
      );
      expect(bytesToHex(meta.bSpend)).to.equal(c.b_spend_hex);
      expect(bytesToHex(meta.bScan)).to.equal(c.b_scan_hex);
      expect(encodeMetaAddress(meta)).to.equal(c.meta_address);
    }
  });

  it("matches sender derivation and recipient scan vectors", () => {
    for (const c of vectors.vectors.sender_derivation) {
      const payment = derivePayment(
        decodeMetaAddress(c.meta_address),
        hexToBytes(c.ephemeral_secret_hex),
      );
      expect(payment.stealthAddress).to.equal(c.stealth_address);
      expect(bytesToHex(payment.stealthBytes)).to.equal(c.stealth_address_hex);
      expect(bytesToHex(payment.ephemeralPub)).to.equal(c.ephemeral_pub_hex);
      expect(`0x${payment.viewTag.toString(16).padStart(2, "0")}`).to.equal(c.view_tag_hex);
    }

    for (const c of vectors.vectors.recipient_scan) {
      const keys = deriveStealthKeysFromSignature(hexToBytes(c.signature_hex));
      const matches = scanNoteCandidates(
        keys,
        hexToBytes(c.ephemeral_pub_hex),
        viewTag(c.view_tag_hex),
        c.known_labels,
      );
      expect(matches).to.have.length(c.matches.length);
      for (const [i, actual] of matches.entries()) {
        expect(actual.labelIndex).to.equal(c.matches[i].label_index);
        expect(actual.stealthAddress).to.equal(c.matches[i].stealth_address);
        expect(bytesToHex(scalarToLeBytes(actual.stealthScalar))).to.equal(
          c.matches[i].stealth_scalar_hex,
        );
      }
    }
  });

  it("matches pinboard and registry wire vectors", () => {
    const note = vectors.vectors.pinboard.note_event;
    const eventBody = new ByteWriter()
      .u16LE(note.scheme_id)
      .bytes(hexToBytes(note.ephemeral_pub_hex))
      .u8(viewTag(note.view_tag_hex))
      .vecU8(hexToBytes(note.metadata_hex))
      .toBytes();
    const eventPayload = new ByteWriter().bytes(NOTE_EVENT_DISCRIMINATOR).bytes(eventBody).toBytes();

    expect(note.scheme_id).to.equal(SCHEME_ID_V1);
    expect(bytesToHex(NOTE_EVENT_DISCRIMINATOR)).to.equal(note.event_discriminator_hex);
    expect(bytesToHex(eventBody)).to.equal(note.borsh_body_hex);
    expect(bytesToHex(eventPayload)).to.equal(note.event_payload_hex);
    expect(base64.encode(eventPayload)).to.equal(note.program_data_base64);

    const postIx = buildPostInstruction(
      new PublicKey(note.program_id),
      new PublicKey(note.fee_payer),
      note.scheme_id,
      hexToBytes(note.ephemeral_pub_hex),
      viewTag(note.view_tag_hex),
      hexToBytes(note.metadata_hex),
    );
    expect(bytesToHex(postIx.data)).to.equal(note.post_instruction_data_hex);

    const reg = vectors.vectors.registry.register;
    const programId = new PublicKey(reg.program_id);
    const registrant = new PublicKey(reg.registrant);
    const [pda, bump] = registryPda(programId, registrant, reg.scheme_id);
    const payload = {
      version: reg.payload.version,
      bSpend: hexToBytes(reg.payload.b_spend_hex),
      bScan: hexToBytes(reg.payload.b_scan_hex),
      flags: reg.payload.flags,
    };
    expect(pda.toBase58()).to.equal(reg.pda);
    expect(bump).to.equal(reg.bump);
    expect(bytesToHex(buildRegisterInstruction(programId, registrant, reg.scheme_id, payload).data))
      .to.equal(reg.instruction_data_hex);

    const accountData = new ByteWriter()
      .bytes(META_ADDRESS_ENTRY_DISCRIMINATOR)
      .bytes(registrant.toBytes())
      .u16LE(reg.scheme_id)
      .u8(bump)
      .u8(payload.version)
      .bytes(payload.bSpend)
      .bytes(payload.bScan)
      .u8(payload.flags)
      .toBytes();
    expect(bytesToHex(META_ADDRESS_ENTRY_DISCRIMINATOR)).to.equal(reg.account_discriminator_hex);
    expect(bytesToHex(accountData)).to.equal(reg.account_data_hex);
  });

  it("rejects invalid hardening vectors", () => {
    const invalid = vectors.vectors.invalid;
    const baseMeta = decodeMetaAddress(invalid.base_meta_address);
    const rng = hexToBytes(invalid.ephemeral_secret_hex);

    expect(() =>
      derivePayment({ ...baseMeta, bSpend: hexToBytes(invalid.bad_spend_identity_hex) }, rng),
    )
      .to.throw(SlntError)
      .with.property("code", "InvalidPoint");

    expect(() =>
      derivePayment({ ...baseMeta, bSpend: hexToBytes(invalid.bad_spend_with_torsion_hex) }, rng),
    )
      .to.throw(SlntError)
      .with.property("code", "InvalidPoint");

    expect(() =>
      derivePayment({ ...baseMeta, bScan: hexToBytes(invalid.bad_scan_low_order_hex) }, rng),
    )
      .to.throw(SlntError)
      .with.property("code", "InvalidSharedSecret");
  });
});
