// On-chain smoke test against the live deployments.
//   npx ts-node scripts/onchain-smoke.ts <devnet|testnet>
//
// Exercises the deployed pinboard + registry programs end-to-end using the
// @slnt/sdk instruction builders, signed by the local deployer keypair.

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  buildPostInstruction,
  tryParseNoteLog,
  registryPda,
  buildRegisterInstruction,
  buildUpdateInstruction,
  tryParseMetaAddressEntry,
  deriveStealthKeysFromSignature,
  metaFromKeys,
} from "../src/index";

const PINBOARD = new PublicKey("SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2");
const REGISTRY = new PublicKey("SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2");
const SCHEME_ID = 1;

const CLUSTERS: Record<string, string> = {
  devnet: "https://api.devnet.solana.com",
  testnet: "https://api.testnet.solana.com",
};

function loadPayer(): Keypair {
  const path = `${homedir()}/.config/solana/id.json`;
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(path, "utf8"))));
}

function bytes(n: number, fill: number): Uint8Array {
  return new Uint8Array(n).fill(fill);
}

async function getLogs(conn: Connection, sig: string): Promise<string[]> {
  const tx = await conn.getTransaction(sig, {
    maxSupportedTransactionVersion: 0,
    commitment: "confirmed",
  });
  return tx?.meta?.logMessages ?? [];
}

async function main() {
  const cluster = process.argv[2] ?? "devnet";
  const url = CLUSTERS[cluster];
  if (!url) throw new Error(`unknown cluster: ${cluster}`);

  const conn = new Connection(url, "confirmed");
  const payer = loadPayer();
  console.log(`== on-chain smoke test :: ${cluster} ==`);
  console.log(`payer:    ${payer.publicKey.toBase58()}`);
  console.log(`balance:  ${(await conn.getBalance(payer.publicKey)) / 1e9} SOL`);

  // ---- 1. pinboard.post → parse the Note event back ----
  const ephemeralPub = bytes(32, 0xab);
  const viewTag = 0x42;
  const metadata = new Uint8Array([1, 2, 3, 4]);
  const postIx = buildPostInstruction(PINBOARD, payer.publicKey, SCHEME_ID, ephemeralPub, viewTag, metadata);
  const postSig = await sendAndConfirmTransaction(conn, new Transaction().add(postIx), [payer]);
  console.log(`\n[pinboard] post tx: ${postSig}`);

  const logs = await getLogs(conn, postSig);
  const note = logs.map((l) => tryParseNoteLog(l)).find((n) => n !== null);
  if (!note) throw new Error("pinboard: Note event not found in logs");
  const okPost =
    note.schemeId === SCHEME_ID &&
    note.viewTag === viewTag &&
    Buffer.from(note.ephemeralPub).equals(Buffer.from(ephemeralPub)) &&
    Buffer.from(note.metadata).equals(Buffer.from(metadata));
  console.log(`[pinboard] Note event parsed: scheme=${note.schemeId} viewTag=0x${note.viewTag.toString(16)} ${okPost ? "✓ matches" : "✗ MISMATCH"}`);
  if (!okPost) throw new Error("pinboard: Note event fields mismatch");

  // ---- 2. registry.register/update → read the PDA back ----
  const keys = deriveStealthKeysFromSignature(bytes(64, 0x77));
  const meta = metaFromKeys(keys);
  const payload = { version: meta.version, bSpend: meta.bSpend, bScan: meta.bScan, flags: meta.flags };

  const [pda] = registryPda(REGISTRY, payer.publicKey, SCHEME_ID);
  const existing = await conn.getAccountInfo(pda);
  const regIx = existing
    ? buildUpdateInstruction(REGISTRY, payer.publicKey, SCHEME_ID, payload)
    : buildRegisterInstruction(REGISTRY, payer.publicKey, SCHEME_ID, payload);
  const regSig = await sendAndConfirmTransaction(conn, new Transaction().add(regIx), [payer]);
  console.log(`\n[registry] ${existing ? "update" : "register"} tx: ${regSig}`);
  console.log(`[registry] PDA: ${pda.toBase58()}`);

  const acct = await conn.getAccountInfo(pda);
  if (!acct) throw new Error("registry: PDA account not found after write");
  const entry = tryParseMetaAddressEntry(new Uint8Array(acct.data));
  if (!entry) throw new Error("registry: account did not parse as MetaAddressEntry");
  const okReg =
    entry.registrant.equals(payer.publicKey) &&
    entry.schemeId === SCHEME_ID &&
    entry.version === 1 &&
    Buffer.from(entry.bSpend).equals(Buffer.from(meta.bSpend)) &&
    Buffer.from(entry.bScan).equals(Buffer.from(meta.bScan));
  console.log(`[registry] entry parsed: registrant=${entry.registrant.toBase58().slice(0, 8)}… scheme=${entry.schemeId} ${okReg ? "✓ matches" : "✗ MISMATCH"}`);
  if (!okReg) throw new Error("registry: entry fields mismatch");

  console.log(`\n== ${cluster}: PASS — both programs live and behaving correctly ==`);
}

main().catch((e) => {
  console.error("SMOKE TEST FAILED:", e);
  process.exit(1);
});
