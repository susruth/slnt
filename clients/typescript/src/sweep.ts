// Recipient sweep flows (sRFC-0042 §5.9).
//
// A stealth address holds value but only the rent-exempt minimum in SOL, so
// it cannot pay its own fees. A relayer signs as fee payer and is compensated
// from the swept value. The transactions these builders produce are signed by
// both the relayer (fee payer) and the stealth key (authority over the swept
// funds).
//
// Close-to-relayer rule (§5.9, §8.3): rent reclaimed by closing the stealth
// account/ATA MUST go to the relayer or another stealth address the recipient
// controls — never the recipient's main wallet, which would create a direct
// `stealth → main` link. These builders enforce that by rejecting a
// destination equal to `mainWallet`.

import { PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import { createCloseAccountInstruction, createTransferCheckedInstruction } from "@solana/spl-token";
import { SlntError } from "./errors";

// Reject `candidate` if it equals the recipient's `mainWallet` (§8.3).
//
// Pass `null` for `mainWallet` only when the caller has independently
// guaranteed unlinkability (e.g. a known-stealth destination).
export function ensureNotMainWallet(candidate: PublicKey, mainWallet: PublicKey | null): void {
  if (mainWallet !== null && candidate.equals(mainWallet)) {
    throw new SlntError("CloseToMainWallet", "destination equals the recipient's main wallet");
  }
}

// Build a SOL sweep from a stealth account (§5.9).
//
// Two `SystemProgram.transfer`s from the stealth account: one paying the
// relayer `relayerTake` lamports, one paying `destination` the remainder
// (`balance - relayerTake`). The account reaches zero and is reclaimed by the
// runtime. `destination` MUST NOT be `mainWallet`.
//
// The returned instructions must be assembled into a transaction whose fee
// payer is the relayer and which is signed by the stealth key.
export function buildSolSweep(
  stealthAddress: PublicKey,
  destination: PublicKey,
  relayer: PublicKey,
  balance: bigint,
  relayerTake: bigint,
  mainWallet: PublicKey | null,
): TransactionInstruction[] {
  ensureNotMainWallet(destination, mainWallet);
  if (relayerTake >= balance) {
    throw new SlntError(
      "RelayerTakeTooLarge",
      `relayer take ${relayerTake} >= balance ${balance}`,
    );
  }
  const toRecipient = balance - relayerTake;
  return [
    SystemProgram.transfer({
      fromPubkey: stealthAddress,
      toPubkey: relayer,
      lamports: relayerTake,
    }),
    SystemProgram.transfer({
      fromPubkey: stealthAddress,
      toPubkey: destination,
      lamports: toRecipient,
    }),
  ];
}

// Build an SPL-token sweep from a stealth ATA (§5.9).
//
// Three instructions, all with the stealth key as authority:
//   1. `transfer_checked` the token to `destinationAta` (`amount - relayerTake`);
//   2. `transfer_checked` `relayerTake` to `relayerTokenAccount` (in-kind fee);
//   3. `CloseAccount` the stealth ATA, sending reclaimed rent to `closeDestination`.
//
// `closeDestination` MUST NOT be `mainWallet` (§8.3); the relayer fronts the
// SOL fee. Works for SPL Token and Token-2022 via `tokenProgramId`.
export function buildSplSweep(
  stealthAuthority: PublicKey,
  stealthAta: PublicKey,
  destinationAta: PublicKey,
  relayerTokenAccount: PublicKey,
  mint: PublicKey,
  tokenProgramId: PublicKey,
  amount: bigint,
  relayerTake: bigint,
  decimals: number,
  closeDestination: PublicKey,
  mainWallet: PublicKey | null,
): TransactionInstruction[] {
  ensureNotMainWallet(closeDestination, mainWallet);
  if (relayerTake > amount) {
    throw new SlntError(
      "RelayerTakeTooLarge",
      `relayer take ${relayerTake} > amount ${amount}`,
    );
  }
  const toRecipient = amount - relayerTake;

  const transferToDest = createTransferCheckedInstruction(
    stealthAta,
    mint,
    destinationAta,
    stealthAuthority,
    toRecipient,
    decimals,
    [],
    tokenProgramId,
  );

  const payRelayer = createTransferCheckedInstruction(
    stealthAta,
    mint,
    relayerTokenAccount,
    stealthAuthority,
    relayerTake,
    decimals,
    [],
    tokenProgramId,
  );

  const close = createCloseAccountInstruction(
    stealthAta,
    closeDestination,
    stealthAuthority,
    [],
    tokenProgramId,
  );

  return [transferToDest, payRelayer, close];
}
