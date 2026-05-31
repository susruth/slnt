// Scalar-mode Ed25519 signing (sRFC-0042 §5.9), byte-compatible with the
// Rust `StealthSigningKey`.
//
// The recipient holds the stealth private key as a scalar `p_stealth`
// (not an RFC 8032 seed), so we implement RFC 8032 signing directly with
// a deterministic nonce derived from the scalar. Signatures verify
// cleanly against any standard Ed25519 verifier.

import { ed25519 } from "@noble/curves/ed25519";
import { sha512 } from "@noble/hashes/sha512";
import { bytesToNumberLE, numberToBytesLE } from "@noble/curves/abstract/utils";
import { L, concat, utf8 } from "./keys";

const NONCE_TAG = "slnt-v1-nonce";

export class StealthSigningKey {
  /** The stealth private scalar. */
  readonly scalar: bigint;
  /** Compressed Ed25519 public key (= the stealth address bytes). */
  readonly publicKey: Uint8Array;
  /** RFC 8032 nonce hash-prefix, derived from the scalar. */
  private readonly hashPrefix: Uint8Array;

  constructor(scalar: bigint) {
    this.scalar = scalar;
    const scalarBytes = numberToBytesLE(scalar, 32);
    // hash_prefix = SHA-512("slnt-v1-nonce" || scalar)[32..64]
    this.hashPrefix = sha512(concat(utf8(NONCE_TAG), scalarBytes)).slice(32, 64);
    this.publicKey = ed25519.ExtendedPoint.BASE.multiply(scalar).toRawBytes();
  }

  /** RFC 8032 Ed25519 signature (64 bytes) over `message`. */
  sign(message: Uint8Array): Uint8Array {
    // r = SHA-512(hash_prefix || message)  (reduce wide mod ℓ)
    const r = bytesToNumberLE(sha512(concat(this.hashPrefix, message))) % L;
    const R = ed25519.ExtendedPoint.BASE.multiply(r).toRawBytes();
    // k = SHA-512(R || A || message)  (reduce wide mod ℓ)
    const k = bytesToNumberLE(sha512(concat(R, this.publicKey, message))) % L;
    // s = r + k·scalar (mod ℓ)
    const s = (r + ((k * this.scalar) % L)) % L;
    return concat(R, numberToBytesLE(s, 32));
  }
}
