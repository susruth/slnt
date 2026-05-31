// Key derivation and meta-address codec (sRFC-0042 §5.2), byte-compatible
// with the Rust `slnt-sdk`.

import { ed25519, x25519 } from "@noble/curves/ed25519";
import { sha256 } from "@noble/hashes/sha256";
import { sha512 } from "@noble/hashes/sha512";
import { hkdf } from "@noble/hashes/hkdf";
import { hmac } from "@noble/hashes/hmac";
import { bytesToNumberLE, numberToBytesLE } from "@noble/curves/abstract/utils";
import { bech32m } from "@scure/base";
import { SlntError } from "./errors";

/** SLNT HD purpose: ASCII bytes "SLNT" (sRFC-0042 §5.2.1.1). */
export const SLNT_HD_PURPOSE = 0x534c4e54;
/** Solana SLIP-0044 coin type. */
export const SOLANA_COIN_TYPE = 501;

/** Ed25519 group order ℓ. */
export const L =
  0x1000000000000000000000000000000014def9dea2f79cd65812631a5cf5d3edn;

export const META_ADDRESS_VERSION_V1 = 0x01;
export const SCHEME_ID_V1 = 0x0001;
const HRP = "slnt";

export type Network = "Mainnet" | "Devnet" | "Testnet" | "Localnet";

/** sRFC-0042 §5.2.1.2 canonical message (exact UTF-8, no trailing newline). */
export function canonicalMessage(network: Network): string {
  return (
    `Slnt Protocol: Derive Stealth Keys\n\nVersion: 1\nNetwork: ${network}\n` +
    `Warning: Only sign this message in the Slnt wallet or a trusted Slnt integration.\n` +
    `Signing this in any other context will reveal your stealth address scanning ability.`
  );
}

/** SC25519_reduce: little-endian bytes → scalar mod ℓ. */
export function scReduce(bytes: Uint8Array): bigint {
  const n = bytesToNumberLE(bytes) % L;
  return n;
}

export interface StealthKeys {
  /** Ed25519 spend scalar (mod ℓ). */
  bSpend: bigint;
  /** Compressed Ed25519 spend public key (32 bytes). */
  BSpend: Uint8Array;
  /** Raw 32-byte scan material (pre-clamp). View-only. */
  bScanRaw: Uint8Array;
  /** X25519 scan public key (32 bytes). */
  BScan: Uint8Array;
}

/** Method 2 derivation from a 64-byte signature over the canonical message. */
export function deriveStealthKeysFromSignature(signature: Uint8Array): StealthKeys {
  if (signature.length !== 64) {
    throw new SlntError("Derivation", "signature must be 64 bytes");
  }
  const k = hkdf(sha256, signature, utf8("slnt-v1-derive"), utf8("spend-and-scan"), 64);
  return keysFromSecrets(k.slice(0, 32), k.slice(32, 64));
}

/**
 * Method 2 with a determinism guard (sRFC-0042 §8.5): the caller supplies
 * two independent signatures of the same canonical message; if they
 * differ the wallet is a randomized signer and MUST NOT be used.
 */
export function deriveStealthKeysChecked(
  signature: Uint8Array,
  confirmation: Uint8Array,
): StealthKeys {
  if (!bytesEqual(signature, confirmation)) {
    throw new SlntError(
      "NonDeterministicSignature",
      "two signings of the canonical message differ",
    );
  }
  return deriveStealthKeysFromSignature(signature);
}

/**
 * Method 1 — wallet-native HD derivation (sRFC-0042 §5.2.1.1): SLIP-0010
 * over ed25519 at `m/0x534C4E54'/501'/account'/{0',1'}`. `seed` is the
 * 16–64 byte BIP-39 seed.
 */
export function deriveStealthKeysHd(seed: Uint8Array, account = 0): StealthKeys {
  if (seed.length < 16 || seed.length > 64) {
    throw new SlntError("InvalidSeedLength", `got ${seed.length}, need 16–64 bytes`);
  }
  const base = [harden(SLNT_HD_PURPOSE), harden(SOLANA_COIN_TYPE), harden(account)];
  const bSpendRaw = slip10Ed25519Node(seed, [...base, harden(0)]);
  const bScanRaw = slip10Ed25519Node(seed, [...base, harden(1)]);
  return keysFromSecrets(bSpendRaw, bScanRaw);
}

/** Map the two 32-byte secrets to keys (sRFC-0042 §5.2.1.3). */
function keysFromSecrets(bSpendRaw: Uint8Array, bScanRaw: Uint8Array): StealthKeys {
  const bSpend = scReduce(bSpendRaw);
  if (bSpend === 0n) {
    throw new SlntError("Derivation", "spend scalar reduced to zero");
  }
  const BSpend = ed25519.ExtendedPoint.BASE.multiply(bSpend).toRawBytes();
  const BScan = x25519.getPublicKey(bScanRaw);
  return { bSpend, BSpend, bScanRaw, BScan };
}

/** SLIP-0010 hardened-derivation offset. */
function harden(index: number): number {
  return (index | 0x80000000) >>> 0;
}

/**
 * SLIP-0010 ed25519 derivation. `path` entries are already-hardened
 * indices. Returns the 32-byte private key (`I_L`) at the node.
 */
function slip10Ed25519Node(seed: Uint8Array, path: number[]): Uint8Array {
  let i = hmac(sha512, utf8("ed25519 seed"), seed);
  let key = i.slice(0, 32);
  let chain = i.slice(32, 64);
  for (const index of path) {
    const data = new Uint8Array(1 + 32 + 4);
    data[0] = 0x00;
    data.set(key, 1);
    // ser32(index), big-endian unsigned.
    data[33] = (index >>> 24) & 0xff;
    data[34] = (index >>> 16) & 0xff;
    data[35] = (index >>> 8) & 0xff;
    data[36] = index & 0xff;
    i = hmac(sha512, chain, data);
    key = i.slice(0, 32);
    chain = i.slice(32, 64);
  }
  return key;
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

/** Label tweak m_i (sRFC-0042 §5.2.3). */
export function labelTweakScalar(bScanRaw: Uint8Array, labelIndex: number): bigint {
  const info = concat(utf8("label-"), leb128Encode(labelIndex));
  const out = hkdf(sha256, bScanRaw, utf8("slnt-v1-label"), info, 32);
  return scReduce(out);
}

export interface MetaAddress {
  version: number;
  bSpend: Uint8Array; // 32, effective (may incorporate a label tweak)
  bScan: Uint8Array; // 32
  labelIndex: number;
  flags: number;
}

export function metaFromKeys(keys: StealthKeys): MetaAddress {
  return {
    version: META_ADDRESS_VERSION_V1,
    bSpend: keys.BSpend,
    bScan: keys.BScan,
    labelIndex: 0,
    flags: 0,
  };
}

/** Labeled meta-address (sRFC-0042 §5.2.3). */
export function metaForLabel(keys: StealthKeys, labelIndex: number): MetaAddress {
  if (labelIndex === 0) return metaFromKeys(keys);
  const mi = labelTweakScalar(keys.bScanRaw, labelIndex);
  const base = ed25519.ExtendedPoint.fromHex(keys.BSpend);
  const tweaked = base.add(ed25519.ExtendedPoint.BASE.multiply(mi));
  return {
    version: META_ADDRESS_VERSION_V1,
    bSpend: tweaked.toRawBytes(),
    bScan: keys.BScan,
    labelIndex,
    flags: 0,
  };
}

export function encodeMetaAddress(m: MetaAddress): string {
  const payload = concat(
    Uint8Array.of(m.version),
    m.bSpend,
    m.bScan,
    leb128Encode(m.labelIndex),
    Uint8Array.of(m.flags),
  );
  return bech32m.encode(HRP, bech32m.toWords(payload), 1023);
}

export function decodeMetaAddress(s: string): MetaAddress {
  const { prefix, words } = bech32m.decode(s as `${string}1${string}`, 1023);
  if (prefix !== HRP) {
    throw new SlntError("MetaAddressDecode", `expected HRP slnt, got ${prefix}`);
  }
  const data = bech32m.fromWords(words);
  if (data.length < 67) {
    throw new SlntError("MetaAddressDecode", `payload too short: ${data.length} bytes`);
  }
  const version = data[0];
  if (version !== META_ADDRESS_VERSION_V1) {
    throw new SlntError("UnsupportedVersion", `0x${version.toString(16)}`);
  }
  const bSpend = data.slice(1, 33);
  const bScan = data.slice(33, 65);
  const [labelIndex, consumed] = leb128Decode(data.slice(65));
  const flagsOffset = 65 + consumed;
  if (data.length !== flagsOffset + 1) {
    throw new SlntError("MetaAddressDecode", "trailing bytes after payload");
  }
  const flags = data[flagsOffset];
  if (flags !== 0) {
    throw new SlntError("UnsupportedFlags", `0x${flags.toString(16)}`);
  }
  return { version, bSpend, bScan, labelIndex, flags };
}

// ---- helpers ----

export function utf8(s: string): Uint8Array {
  return new TextEncoder().encode(s);
}

/** True if an X25519 shared secret is all-zero (low-order point input). */
export function sharedSecretIsZero(s: Uint8Array): boolean {
  let acc = 0;
  for (const b of s) acc |= b;
  return acc === 0;
}

export function concat(...parts: Uint8Array[]): Uint8Array {
  const len = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(len);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

export function scalarToLeBytes(n: bigint): Uint8Array {
  return numberToBytesLE(n, 32);
}

export function leb128Encode(value: number): Uint8Array {
  const out: number[] = [];
  let v = value >>> 0;
  do {
    let byte = v & 0x7f;
    v >>>= 7;
    if (v !== 0) byte |= 0x80;
    out.push(byte);
  } while (v !== 0);
  return Uint8Array.from(out);
}

export function leb128Decode(data: Uint8Array): [number, number] {
  let result = 0;
  let shift = 0;
  for (let i = 0; i < Math.min(5, data.length); i++) {
    const byte = data[i];
    result |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return [result >>> 0, i + 1];
    shift += 7;
  }
  throw new Error("varint too long");
}
