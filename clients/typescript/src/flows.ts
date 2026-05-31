// Sender transaction flows (sRFC-0042 §5.7).
//
// These build the decoupled-mode asset transfer: only the value movement
// and any required account creation — no SLNT instruction — so the
// transaction is indistinguishable from an ordinary transfer to a fresh
// address. The announcement is published separately (§5.8).

import { PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import {
  createAssociatedTokenAccountIdempotentInstruction,
  createTransferCheckedInstruction,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import { SlntError } from "./errors";

// Rent-exempt minimum for a bare system account (lamports). Added to a SOL
// payment so the fresh stealth account is valid (§5.7).
export const RENT_EXEMPT_MIN = 890_880n;

const U64_MAX = (1n << 64n) - 1n;

// Build the SOL transfer to a stealth address (§5.7).
//
// Transfers `amount + RENT_EXEMPT_MIN`: the extra rent buffer makes the fresh
// system account valid and is reclaimed by the recipient on sweep. Throws
// `LamportOverflow` if `amount + RENT_EXEMPT_MIN` overflows u64.
export function buildSolPayment(
  sender: PublicKey,
  stealthAddress: PublicKey,
  amount: bigint,
): TransactionInstruction {
  const lamports = amount + RENT_EXEMPT_MIN;
  if (lamports > U64_MAX) {
    throw new SlntError(
      "LamportOverflow",
      `amount ${amount} + rent buffer ${RENT_EXEMPT_MIN} overflows u64`,
    );
  }
  return SystemProgram.transfer({
    fromPubkey: sender,
    toPubkey: stealthAddress,
    lamports,
  });
}

// Build the SPL-token transfer to a stealth address (§5.7).
//
// Returns two instructions: idempotently create the stealth owner's ATA
// (sender pays ATA rent), then `transfer_checked` into it. Works for SPL
// Token and Token-2022 by passing the matching `tokenProgramId`; NFTs are the
// `amount = 1, decimals = 0` case (see `buildNftPayment`).
//
// A stealth address need not be on-curve, so the ATA is derived with
// `allowOwnerOffCurve = true`.
export function buildSplPayment(
  sender: PublicKey,
  stealthAddress: PublicKey,
  mint: PublicKey,
  senderTokenAccount: PublicKey,
  tokenProgramId: PublicKey,
  amount: bigint,
  decimals: number,
): TransactionInstruction[] {
  const stealthAta = getAssociatedTokenAddressSync(mint, stealthAddress, true, tokenProgramId);

  const createAta = createAssociatedTokenAccountIdempotentInstruction(
    sender,
    stealthAta,
    stealthAddress,
    mint,
    tokenProgramId,
  );

  const transfer = createTransferCheckedInstruction(
    senderTokenAccount,
    mint,
    stealthAta,
    sender,
    amount,
    decimals,
    [],
    tokenProgramId,
  );

  return [createAta, transfer];
}

// Build an NFT transfer to a stealth address (§5.7) — the
// `amount = 1, decimals = 0` SPL case. For standard and Token-2022 NFTs.
// (Programmable NFTs additionally require Metaplex token-record / rule-set
// accounts — construct those via the Metaplex SDK.)
export function buildNftPayment(
  sender: PublicKey,
  stealthAddress: PublicKey,
  mint: PublicKey,
  senderTokenAccount: PublicKey,
  tokenProgramId: PublicKey,
): TransactionInstruction[] {
  return buildSplPayment(
    sender,
    stealthAddress,
    mint,
    senderTokenAccount,
    tokenProgramId,
    1n,
    0,
  );
}
