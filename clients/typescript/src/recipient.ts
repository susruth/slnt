// Recipient-side scanning (sRFC-0042 §5.4).

import { ed25519, x25519 } from "@noble/curves/ed25519";
import { base58 } from "@scure/base";
import { L, StealthKeys, labelTweakScalar, sharedSecretIsZero } from "./keys";
import { tweakScalar, viewTagOf } from "./sender";

export interface NoteMatch {
  stealthAddress: string;
  /** p_stealth = (b_spend [+ m_i] + t) mod ℓ — the stealth private scalar. */
  stealthScalar: bigint;
  labelIndex: number;
}

function addressForScalar(scalar: bigint): { addr: string; bytes: Uint8Array } {
  const bytes = ed25519.ExtendedPoint.BASE.multiply(scalar).toRawBytes();
  return { addr: base58.encode(bytes), bytes };
}

/** View-only filter (sRFC-0042 §5.10): true if the note survives the view tag. */
export function viewTagMatches(
  bScanRaw: Uint8Array,
  ephemeralPub: Uint8Array,
  noteViewTag: number,
): boolean {
  const s = ecdhOrNull(bScanRaw, ephemeralPub);
  if (s === null) return false;
  return viewTagOf(s) === noteViewTag;
}

/** ECDH that returns null for malformed/low-order ephemeral keys, so a
 * hostile note is skipped rather than throwing. */
function ecdhOrNull(bScanRaw: Uint8Array, ephemeralPub: Uint8Array): Uint8Array | null {
  let s: Uint8Array;
  try {
    s = x25519.getSharedSecret(bScanRaw, ephemeralPub);
  } catch {
    return null;
  }
  return sharedSecretIsZero(s) ? null : s;
}

/**
 * Scan a note. On a view-tag match, returns the unlabeled candidate plus
 * one per entry in `knownLabels`; the caller checks which received funds.
 * Returns `[]` if the view tag does not match.
 */
export function scanNoteCandidates(
  keys: StealthKeys,
  ephemeralPub: Uint8Array,
  noteViewTag: number,
  knownLabels: number[] = [],
): NoteMatch[] {
  const s = ecdhOrNull(keys.bScanRaw, ephemeralPub);
  if (s === null) return [];
  if (viewTagOf(s) !== noteViewTag) return [];

  const t = tweakScalar(s, noteViewTag);
  const out: NoteMatch[] = [];

  const unlabeled = (keys.bSpend + t) % L;
  out.push({ ...addrMatch(unlabeled, 0) });

  for (const i of knownLabels) {
    if (i === 0) continue;
    const mi = labelTweakScalar(keys.bScanRaw, i);
    const scalar = (keys.bSpend + mi + t) % L;
    out.push({ ...addrMatch(scalar, i) });
  }
  return out;
}

function addrMatch(scalar: bigint, labelIndex: number): NoteMatch {
  const { addr } = addressForScalar(scalar);
  return { stealthAddress: addr, stealthScalar: scalar, labelIndex };
}
