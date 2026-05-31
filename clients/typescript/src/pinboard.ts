// Build instructions and parse events for the pinboard program,
// byte-for-byte compatible with the Rust `slnt-sdk` `pinboard` module.
//
// We don't depend on a heavy Anchor client; instead we hand-build the
// instruction using Anchor's 8-byte discriminator
// (`SHA-256("global:<instruction_snake>")[..8]`) followed by borsh-
// serialized args. For events, the on-chain logs contain
// `Program data: <base64>` where the payload is
// `SHA-256("event:<EventName>")[..8] || borsh(event)`.

import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { base64 } from "@scure/base";
import { anchorDiscriminator, ByteReader, ByteWriter } from "./anchor";

/** `SHA-256("global:post")[..8]`. Equals the Rust hardcoded constant. */
export const POST_DISCRIMINATOR: Uint8Array = anchorDiscriminator("global", "post");

/** `SHA-256("global:post_batch")[..8]`. */
export const POST_BATCH_DISCRIMINATOR: Uint8Array = anchorDiscriminator(
  "global",
  "post_batch",
);

/** `SHA-256("event:Note")[..8]`. */
export const NOTE_EVENT_DISCRIMINATOR: Uint8Array = anchorDiscriminator("event", "Note");

/** One entry of a `post_batch` call. Mirrors `NoteEntry` in the program. */
export interface NoteEntry {
  schemeId: number;
  ephemeralPub: Uint8Array;
  viewTag: number;
  metadata: Uint8Array;
}

/** On-chain `Note` event payload. Same borsh layout as `NoteEntry`/`PostArgs`. */
export interface NoteEvent {
  schemeId: number;
  ephemeralPub: Uint8Array;
  viewTag: number;
  metadata: Uint8Array;
}

/** Serialize the `post` args layout: u16 || [u8;32] || u8 || Vec<u8>. */
function writePostArgs(w: ByteWriter, schemeId: number, ephemeralPub: Uint8Array, viewTag: number, metadata: Uint8Array): void {
  w.u16LE(schemeId).bytes(ephemeralPub).u8(viewTag).vecU8(metadata);
}

/** Build a `pinboard.post(...)` instruction. */
export function buildPostInstruction(
  programId: PublicKey,
  feePayer: PublicKey,
  schemeId: number,
  ephemeralPub: Uint8Array,
  viewTag: number,
  metadata: Uint8Array,
): TransactionInstruction {
  const w = new ByteWriter();
  w.bytes(POST_DISCRIMINATOR);
  writePostArgs(w, schemeId, ephemeralPub, viewTag, metadata);
  return new TransactionInstruction({
    programId,
    keys: [{ pubkey: feePayer, isSigner: true, isWritable: true }],
    data: Buffer.from(w.toBytes()),
  });
}

/**
 * Build a `pinboard.post_batch(...)` instruction (sRFC-0042 §5.5.1).
 *
 * `entries` must be non-empty (the program rejects an empty batch);
 * practical size is bounded by the transaction compute budget.
 */
export function buildPostBatchInstruction(
  programId: PublicKey,
  feePayer: PublicKey,
  entries: NoteEntry[],
): TransactionInstruction {
  const w = new ByteWriter();
  w.bytes(POST_BATCH_DISCRIMINATOR);
  w.u32LE(entries.length);
  for (const e of entries) {
    writePostArgs(w, e.schemeId, e.ephemeralPub, e.viewTag, e.metadata);
  }
  return new TransactionInstruction({
    programId,
    keys: [{ pubkey: feePayer, isSigner: true, isWritable: true }],
    data: Buffer.from(w.toBytes()),
  });
}

/**
 * Parse a `Program data: <base64>` log line into a `NoteEvent`.
 *
 * Returns `null` if the line is not a `Program data:` line or if the
 * discriminator doesn't match `Note` (mirrors the Rust `Ok(None)`).
 */
export function tryParseNoteLog(line: string): NoteEvent | null {
  const PREFIX = "Program data: ";
  if (!line.startsWith(PREFIX)) {
    return null;
  }
  const raw = base64.decode(line.slice(PREFIX.length).trim());
  if (raw.length < 8) {
    return null;
  }
  for (let i = 0; i < 8; i++) {
    if (raw[i] !== NOTE_EVENT_DISCRIMINATOR[i]) {
      return null;
    }
  }
  const r = new ByteReader(raw.slice(8));
  const schemeId = r.u16LE();
  const ephemeralPub = r.bytes(32);
  const viewTag = r.u8();
  const metadata = r.vecU8();
  return { schemeId, ephemeralPub, viewTag, metadata };
}
