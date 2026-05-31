// Anchor wire helpers shared by the on-chain instruction builders and
// event parsers, mirroring how the Rust SDK hand-builds Anchor calls:
// an 8-byte discriminator followed by borsh-serialized fields.

import { sha256 } from "@noble/hashes/sha256";
import { utf8 } from "./keys";

/** Anchor 8-byte discriminator: SHA-256("<namespace>:<name>")[..8]. */
export function anchorDiscriminator(
  namespace: "global" | "event" | "account",
  name: string,
): Uint8Array {
  return sha256(utf8(`${namespace}:${name}`)).slice(0, 8);
}

/** Minimal little-endian borsh writer for the fields SLNT uses. */
export class ByteWriter {
  private chunks: number[] = [];

  u8(v: number): this {
    this.chunks.push(v & 0xff);
    return this;
  }
  u16LE(v: number): this {
    this.chunks.push(v & 0xff, (v >>> 8) & 0xff);
    return this;
  }
  u32LE(v: number): this {
    this.chunks.push(v & 0xff, (v >>> 8) & 0xff, (v >>> 16) & 0xff, (v >>> 24) & 0xff);
    return this;
  }
  bytes(b: Uint8Array): this {
    for (const x of b) this.chunks.push(x);
    return this;
  }
  /** borsh `Vec<u8>`: u32 little-endian length prefix then the bytes. */
  vecU8(b: Uint8Array): this {
    this.u32LE(b.length);
    return this.bytes(b);
  }
  toBytes(): Uint8Array {
    return Uint8Array.from(this.chunks);
  }
}

/** Minimal little-endian borsh reader. */
export class ByteReader {
  offset = 0;
  constructor(private readonly data: Uint8Array) {}

  u8(): number {
    return this.data[this.offset++];
  }
  u16LE(): number {
    const v = this.data[this.offset] | (this.data[this.offset + 1] << 8);
    this.offset += 2;
    return v;
  }
  u32LE(): number {
    const d = this.data;
    const v =
      (d[this.offset] |
        (d[this.offset + 1] << 8) |
        (d[this.offset + 2] << 16) |
        (d[this.offset + 3] << 24)) >>>
      0;
    this.offset += 4;
    return v;
  }
  bytes(n: number): Uint8Array {
    const b = this.data.slice(this.offset, this.offset + n);
    this.offset += n;
    return b;
  }
  vecU8(): Uint8Array {
    return this.bytes(this.u32LE());
  }
  remaining(): number {
    return this.data.length - this.offset;
  }
}
