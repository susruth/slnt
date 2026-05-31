// Helpers for the Slnt registry program, byte-for-byte compatible with
// the Rust `slnt-sdk` `registry` module.
//
// Provides PDA derivation matching the on-chain seeds, instruction
// builders for `register` / `update` / `close`, and a borsh decoder for
// the `MetaAddressEntry` account.

import { PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import { anchorDiscriminator, ByteReader, ByteWriter } from "./anchor";
import { utf8 } from "./keys";

/** Seed prefix used by the registry program for meta-address PDAs. */
export const META_SEED = "meta";

/** `SHA-256("global:register")[..8]`. */
export const REGISTER_DISCRIMINATOR: Uint8Array = anchorDiscriminator("global", "register");
/** `SHA-256("global:update")[..8]`. */
export const UPDATE_DISCRIMINATOR: Uint8Array = anchorDiscriminator("global", "update");
/** `SHA-256("global:close")[..8]`. */
export const CLOSE_DISCRIMINATOR: Uint8Array = anchorDiscriminator("global", "close");

/** Anchor account discriminator: `SHA-256("account:MetaAddressEntry")[..8]`. */
export const META_ADDRESS_ENTRY_DISCRIMINATOR: Uint8Array = anchorDiscriminator(
  "account",
  "MetaAddressEntry",
);

/** Registry instruction argument for `register` / `update`. */
export interface MetaAddressPayload {
  version: number;
  bSpend: Uint8Array;
  bScan: Uint8Array;
  flags: number;
}

/** On-chain `MetaAddressEntry` layout. Mirrors the program exactly. */
export interface MetaAddressEntry {
  registrant: PublicKey;
  schemeId: number;
  bump: number;
  version: number;
  bSpend: Uint8Array;
  bScan: Uint8Array;
  flags: number;
}

/** `scheme_id` as 2-byte little-endian seed bytes. */
function schemeIdSeed(schemeId: number): Uint8Array {
  return new ByteWriter().u16LE(schemeId).toBytes();
}

/** Derive the registry PDA for a `(registrant, scheme_id)` pair. */
export function registryPda(
  programId: PublicKey,
  registrant: PublicKey,
  schemeId: number,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [utf8(META_SEED), registrant.toBytes(), schemeIdSeed(schemeId)],
    programId,
  );
}

/** Anchor instruction data: `discriminator || borsh(scheme_id) || borsh(payload)?`. */
function encodePayloadIx(
  discriminator: Uint8Array,
  schemeId: number,
  payload: MetaAddressPayload | null,
): Buffer {
  const w = new ByteWriter();
  w.bytes(discriminator);
  w.u16LE(schemeId);
  if (payload) {
    w.u8(payload.version).bytes(payload.bSpend).bytes(payload.bScan).u8(payload.flags);
  }
  return Buffer.from(w.toBytes());
}

/**
 * Build a `registry.register(scheme_id, payload)` instruction
 * (sRFC-0042 §5.6.2). Creates the PDA; the registrant pays rent and
 * signs. Fails on-chain if the `(registrant, scheme_id)` entry exists.
 */
export function buildRegisterInstruction(
  programId: PublicKey,
  registrant: PublicKey,
  schemeId: number,
  payload: MetaAddressPayload,
): TransactionInstruction {
  const [pda] = registryPda(programId, registrant, schemeId);
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: registrant, isSigner: true, isWritable: true },
      { pubkey: pda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: encodePayloadIx(REGISTER_DISCRIMINATOR, schemeId, payload),
  });
}

/**
 * Build a `registry.update(scheme_id, payload)` instruction. Overwrites
 * the existing entry in place; only the owning registrant may sign.
 */
export function buildUpdateInstruction(
  programId: PublicKey,
  registrant: PublicKey,
  schemeId: number,
  payload: MetaAddressPayload,
): TransactionInstruction {
  const [pda] = registryPda(programId, registrant, schemeId);
  return new TransactionInstruction({
    programId,
    keys: [
      // `update` does not mark the registrant `mut` on-chain.
      { pubkey: registrant, isSigner: true, isWritable: false },
      { pubkey: pda, isSigner: false, isWritable: true },
    ],
    data: encodePayloadIx(UPDATE_DISCRIMINATOR, schemeId, payload),
  });
}

/**
 * Build a `registry.close(scheme_id)` instruction. Closes the PDA,
 * returning rent to the registrant; the pair may be re-registered later.
 */
export function buildCloseInstruction(
  programId: PublicKey,
  registrant: PublicKey,
  schemeId: number,
): TransactionInstruction {
  const [pda] = registryPda(programId, registrant, schemeId);
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: registrant, isSigner: true, isWritable: true },
      { pubkey: pda, isSigner: false, isWritable: true },
    ],
    data: encodePayloadIx(CLOSE_DISCRIMINATOR, schemeId, null),
  });
}

/**
 * Parse the raw bytes of a registry account into a `MetaAddressEntry`.
 *
 * Validates the 8-byte Anchor discriminator and borsh-decodes the rest.
 * Returns `null` if the bytes do not start with the expected
 * discriminator (mirrors the Rust `Ok(None)`).
 */
export function tryParseMetaAddressEntry(data: Uint8Array): MetaAddressEntry | null {
  if (data.length < 8) {
    return null;
  }
  for (let i = 0; i < 8; i++) {
    if (data[i] !== META_ADDRESS_ENTRY_DISCRIMINATOR[i]) {
      return null;
    }
  }
  const r = new ByteReader(data.slice(8));
  const registrant = new PublicKey(r.bytes(32));
  const schemeId = r.u16LE();
  const bump = r.u8();
  const version = r.u8();
  const bSpend = r.bytes(32);
  const bScan = r.bytes(32);
  const flags = r.u8();
  return { registrant, schemeId, bump, version, bSpend, bScan, flags };
}
