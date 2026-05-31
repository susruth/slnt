// Sender-side stealth-address derivation (sRFC-0042 §5.3).

import { ed25519, x25519 } from "@noble/curves/ed25519";
import { sha256 } from "@noble/hashes/sha256";
import { base58 } from "@scure/base";
import {
  MetaAddress,
  META_ADDRESS_VERSION_V1,
  concat,
  scReduce,
  sharedSecretIsZero,
  utf8,
} from "./keys";
import { SlntError } from "./errors";

const TWEAK_TAG = "slnt-v1-tweak";

export interface StealthPayment {
  /** Solana stealth address (base58 of the compressed Ed25519 point). */
  stealthAddress: string;
  /** Compressed stealth point bytes (32). */
  stealthBytes: Uint8Array;
  /** Ephemeral X25519 public key R (32 bytes). */
  ephemeralPub: Uint8Array;
  /** First byte of SHA-256(tag || S). */
  viewTag: number;
}

export function viewTagOf(s: Uint8Array): number {
  const h = sha256(concat(Uint8Array.of(TWEAK_TAG.length), utf8(TWEAK_TAG), s));
  return h[0];
}

export function tweakScalar(s: Uint8Array, viewTag: number): bigint {
  const h = sha256(
    concat(Uint8Array.of(TWEAK_TAG.length), utf8(TWEAK_TAG), s, Uint8Array.of(viewTag)),
  );
  return scReduce(h);
}

/**
 * Derive a one-time stealth address for `meta`. `randomBytes32` is the
 * caller-supplied ephemeral randomness `r` (use a CSPRNG in production).
 */
export function derivePayment(meta: MetaAddress, randomBytes32: Uint8Array): StealthPayment {
  if (randomBytes32.length !== 32) {
    throw new SlntError("Derivation", "r must be 32 bytes");
  }
  if (meta.version !== META_ADDRESS_VERSION_V1) {
    throw new SlntError("UnsupportedVersion", `0x${meta.version.toString(16)}`);
  }
  if (meta.flags !== 0) {
    throw new SlntError("UnsupportedFlags", `0x${meta.flags.toString(16)}`);
  }

  // Decompress B_spend_effective and reject small-order (torsion) points.
  let bSpendPoint;
  try {
    bSpendPoint = ed25519.ExtendedPoint.fromHex(meta.bSpend);
  } catch {
    throw new SlntError("InvalidPoint", "B_spend is not a valid Ed25519 point");
  }
  if (bSpendPoint.multiplyUnsafe(8n).equals(ed25519.ExtendedPoint.ZERO)) {
    throw new SlntError("InvalidPoint", "B_spend is a small-order point");
  }

  const ephemeralPub = x25519.getPublicKey(randomBytes32);
  let s: Uint8Array;
  try {
    s = x25519.getSharedSecret(randomBytes32, meta.bScan);
  } catch {
    // @noble rejects low-order / malformed scan keys at the ECDH step.
    throw new SlntError("InvalidSharedSecret", "X25519 ECDH failed (low-order scan key)");
  }
  if (sharedSecretIsZero(s)) {
    throw new SlntError("InvalidSharedSecret", "X25519 shared secret is all-zero");
  }

  const viewTag = viewTagOf(s);
  const t = tweakScalar(s, viewTag);

  const pStealth = bSpendPoint.add(ed25519.ExtendedPoint.BASE.multiply(t));
  const stealthBytes = pStealth.toRawBytes();

  return {
    stealthAddress: base58.encode(stealthBytes),
    stealthBytes,
    ephemeralPub,
    viewTag,
  };
}
