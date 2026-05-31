# @slnt/sdk

TypeScript wallet SDK for **SLNT**, the silent-payment protocol for Solana
([sRFC-0042](https://github.com/susruth/slnt/blob/main/docs/srfc/0001-slnt-silent-payments.md)).

SLNT lets a recipient publish **one reusable meta-address** while every payment
lands at a **fresh, unlinkable Solana wallet** that only the recipient can
recognize and spend. On-chain, each payment looks like an ordinary transfer to a
brand-new address — there is no shared public identifier tying payments
together.

This package is byte-for-byte compatible with the Rust `slnt-sdk` reference
implementation: meta-addresses, stealth addresses, and signatures produced by
one verify against the other (see [Cross-implementation compatibility](#cross-implementation-compatibility)).

> ⚠️ **Experimental and unaudited.** Do not use with funds you cannot afford to
> lose.

---

## Install

```bash
npm install @slnt/sdk
```

Peer ecosystem: the SDK builds `@solana/web3.js` `TransactionInstruction`s and
relies on `@solana/spl-token`; cryptography is provided by
[`@noble/curves`](https://github.com/paulmillr/noble-curves) and
[`@noble/hashes`](https://github.com/paulmillr/noble-hashes). These ship as
dependencies — you don't need to install them yourself.

---

## Mental model

There are two roles. Read them in order; the rest of this README follows the
same flow.

| Role | Holds | Does |
|------|-------|------|
| **Recipient** | A seed (or signature) → `StealthKeys`. Publishes a `MetaAddress`. | Scans announcements, recognizes payments, signs & sweeps the funds out. |
| **Sender** | The recipient's published meta-address. | Derives a one-time stealth address, sends a normal transfer to it, publishes a small announcement. |

The cryptographic objects:

- **Meta-address** (`slnt1…`, bech32m) — the recipient's reusable public handle.
  Encodes a spend public key `B_spend` and a scan public key `B_scan`.
- **Stealth address** — a one-time Solana address derived by the sender from the
  meta-address + fresh randomness. Each payment gets a new one.
- **Announcement** — the tuple `(schemeId, R, viewTag, metadata)` the sender
  publishes (`R` is the sender's ephemeral key). It lets the recipient — and
  *only* the recipient — recompute which stealth address received funds.
- **Pinboard** — an on-chain program that records announcements as `Note`
  events. The recipient scans these.
- **Registry** — an optional on-chain program mapping a wallet → its
  meta-address, so senders can look one up.

By default SLNT uses **decoupled mode**: the asset transfer carries no SLNT
instruction (so it's indistinguishable from any other transfer), and the
announcement is published separately — ideally by an announcement service.

---

## 1. Recipient: derive keys

Your scan/spend keys come from a single secret. Pick the method that matches
your wallet.

### Method 2 — from a signature (most wallets)

The wallet signs a fixed canonical message; the 64-byte signature is the only
input. **The signing wallet must be deterministic** — if it produces randomized
signatures, scanning ability breaks.

```ts
import {
  canonicalMessage,
  deriveStealthKeysFromSignature,
  deriveStealthKeysChecked,
} from "@slnt/sdk";

const message = canonicalMessage("Mainnet"); // exact UTF-8, no trailing newline
const signature = await wallet.signMessage(new TextEncoder().encode(message)); // 64 bytes

const keys = deriveStealthKeysFromSignature(signature);

// Safer: sign twice and require the signatures match (rejects randomized signers).
const keysChecked = deriveStealthKeysChecked(signature, signatureAgain);
```

> 🔒 The canonical message carries a warning telling users to sign it **only**
> inside a trusted SLNT integration. Signing it anywhere else leaks the ability
> to scan for your payments. Show that warning to the user.

### Method 1 — wallet-native HD derivation

If you control the BIP-39 seed, derive deterministically via SLIP-0010 over
ed25519 at `m/0x534C4E54'/501'/account'/{0',1'}`:

```ts
import { deriveStealthKeysHd } from "@slnt/sdk";

const keys = deriveStealthKeysHd(seed /* 16–64 bytes */, /* account */ 0);
```

Either way you get a `StealthKeys`:

```ts
interface StealthKeys {
  bSpend: bigint;       // Ed25519 spend scalar
  BSpend: Uint8Array;   // 32-byte compressed spend public key
  bScanRaw: Uint8Array; // 32-byte scan material — VIEW-ONLY, give to your indexer
  BScan: Uint8Array;    // 32-byte X25519 scan public key
}
```

`bScanRaw` is the **view key**: it can detect incoming payments but cannot spend
them. You can safely hand it to a scanning service while keeping `bSpend`
offline.

---

## 2. Recipient: publish a meta-address

Encode your keys into the shareable `slnt1…` string:

```ts
import { metaFromKeys, encodeMetaAddress } from "@slnt/sdk";

const metaAddress = encodeMetaAddress(metaFromKeys(keys));
// → "slnt1q…"  share this anywhere: bio, ENS-style record, QR code
```

### Optional: on-chain registry

Map your wallet → meta-address so senders can resolve it on-chain:

```ts
import { PublicKey, Transaction } from "@solana/web3.js";
import {
  buildRegisterInstruction,
  registryPda,
  tryParseMetaAddressEntry,
} from "@slnt/sdk";

const REGISTRY = new PublicKey("SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2");
const SCHEME_ID = 1;

const meta = metaFromKeys(keys);
const ix = buildRegisterInstruction(REGISTRY, myWallet.publicKey, SCHEME_ID, {
  version: meta.version,
  bSpend: meta.bSpend,
  bScan: meta.bScan,
  flags: meta.flags,
});
await sendAndConfirmTransaction(connection, new Transaction().add(ix), [myWallet]);

// Read it back:
const [pda] = registryPda(REGISTRY, myWallet.publicKey, SCHEME_ID);
const acct = await connection.getAccountInfo(pda);
const entry = acct && tryParseMetaAddressEntry(new Uint8Array(acct.data));
```

`buildUpdateInstruction` overwrites the entry; `buildCloseInstruction` reclaims
the PDA rent.

### Optional: labels

A labeled meta-address lets one recipient distinguish payment sources (e.g. per
customer) while sharing one scan key. Use a distinct `metaForLabel(keys, i)` per
label and remember which indices you handed out — you must pass them to the
scanner later.

```ts
import { metaForLabel } from "@slnt/sdk";
const customerMeta = encodeMetaAddress(metaForLabel(keys, 7));
```

---

## 3. Sender: pay a meta-address

```ts
import { randomBytes } from "node:crypto";
import { PublicKey, Transaction } from "@solana/web3.js";
import {
  decodeMetaAddress,
  derivePayment,
  buildSolPayment,
} from "@slnt/sdk";

const meta = decodeMetaAddress("slnt1q…");

// 1. Derive a one-time stealth address. `r` MUST be fresh CSPRNG randomness.
const r = randomBytes(32);
const payment = derivePayment(meta, r);
// payment = { stealthAddress, stealthBytes, ephemeralPub (R), viewTag }

// 2. Build a NORMAL transfer to it. No SLNT instruction → silent on-chain.
const stealth = new PublicKey(payment.stealthBytes);
const transferIx = buildSolPayment(sender.publicKey, stealth, 1_000_000_000n /* lamports */);

await sendAndConfirmTransaction(connection, new Transaction().add(transferIx), [sender]);
```

`buildSolPayment` adds the rent-exempt minimum on top of the amount so the fresh
account is valid (the recipient reclaims it on sweep). For tokens use
`buildSplPayment` (works for SPL Token *and* Token-2022 via `tokenProgramId`);
for NFTs use `buildNftPayment`.

### Announce the payment

Publish the announcement so the recipient can find the funds. Decoupled mode
sends it through an announcement service:

```ts
import {
  announcementFromPayment,
  announceRequestFromAnnouncement,
  AnnounceClient,
} from "@slnt/sdk";

const announcement = announcementFromPayment(payment, /* optional metadata ≤64B */);
const client = new AnnounceClient("https://announce.example.com");
const { batch_id } = await client.submit(announceRequestFromAnnouncement(announcement));
const status = await client.status(batch_id); // poll until "confirmed"
```

If no service is available you can publish it yourself with
`buildPostInstruction` against the pinboard program (this links the announcement
tx to you — the coupled escape hatch). The protocol also defines a
self-announce fallback (`shouldSelfAnnounce`) for when a service accepts but
never publishes within a timeout.

---

## 4. Recipient: scan for payments

Subscribe to pinboard `Note` events and run the local scan for each. The
subscription itself learns nothing — all key work happens in `onNote`.

```ts
import { Connection, PublicKey } from "@solana/web3.js";
import { subscribePinboardNotes, scanNoteCandidates } from "@slnt/sdk";

const PINBOARD = new PublicKey("SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2");
const connection = new Connection("https://api.mainnet-beta.solana.com", "confirmed");

const knownLabels = [7]; // label indices you handed out (omit if none)

const subId = await subscribePinboardNotes(connection, PINBOARD, (note) => {
  // Cheap view-tag filter happens inside scanNoteCandidates.
  const candidates = scanNoteCandidates(keys, note.ephemeralPub, note.viewTag, knownLabels);
  for (const c of candidates) {
    // c.stealthAddress is a candidate you control IF it actually holds funds.
    // Check the balance, then keep c.stealthScalar to spend it.
  }
});

// later: connection.removeOnLogsListener(subId);
```

`scanNoteCandidates` returns `[]` when the view tag doesn't match (the common
case — most notes aren't yours). On a match it returns one `NoteMatch` per
candidate; you confirm by checking which addresses actually received value. Each
match carries the **`stealthScalar`** — the private key for that address.

> **Backfill:** the live subscription covers notes posted while you're online.
> For gaps, replay historical transactions with
> `getSignaturesForAddress` + `getTransaction`, pass each tx's log lines to
> `notesFromLogLines`, and scan the results the same way.
>
> **View-only scanning:** to run scanning on an untrusted server, share only
> `bScanRaw` and `BScan` and use `viewTagMatches(bScanRaw, R, viewTag)` to filter
> — it never touches the spend key.

---

## 5. Recipient: sweep the funds out

A stealth account holds value but only enough SOL to be rent-exempt, so it can't
pay its own fees. A **relayer** signs as fee payer and takes a cut of the swept
value. The transaction is signed by *both* the relayer and the stealth key.

```ts
import { Transaction } from "@solana/web3.js";
import { StealthSigningKey, buildSolSweep } from "@slnt/sdk";

// `match.stealthScalar` came from scanNoteCandidates.
const signer = new StealthSigningKey(match.stealthScalar);
// signer.publicKey === the stealth address bytes; signer.sign(msg) → 64-byte sig.

const ixs = buildSolSweep(
  stealthPubkey,
  destination,       // ⚠️ MUST NOT be your main wallet
  relayer.publicKey,
  balance,           // current lamports in the stealth account
  relayerTake,       // fee paid to the relayer, < balance
  mainWallet,        // pass your main wallet here; the builder REJECTS it as destination
);
```

> 🔒 **Never sweep to your main wallet.** A `stealth → main` transfer publicly
> links the payment to your identity and defeats the whole protocol. The sweep
> builders enforce this: pass your main wallet as the last argument and they
> throw `CloseToMainWallet` if it's used as a destination. Sweep to another
> stealth address, a fresh wallet, or an exchange deposit address instead.

For tokens, `buildSplSweep` produces transfer-to-destination, pay-relayer
(in-kind), and close-ATA instructions; reclaimed rent goes to a non-main
destination.

`StealthSigningKey` implements RFC 8032 Ed25519 over a raw scalar, so its
signatures verify against any standard Ed25519 verifier — you can use it to sign
arbitrary transactions from the stealth account, not just sweeps.

---

## API reference

### Keys & meta-address (`keys`)
- `canonicalMessage(network)` — the exact message to sign for Method 2.
- `deriveStealthKeysFromSignature(sig)` / `deriveStealthKeysChecked(sig, sig2)` — Method 2.
- `deriveStealthKeysHd(seed, account?)` — Method 1 (SLIP-0010).
- `metaFromKeys(keys)` / `metaForLabel(keys, i)` — build a `MetaAddress`.
- `encodeMetaAddress(meta)` / `decodeMetaAddress(str)` — bech32m `slnt1…` codec.

### Sender (`sender`, `flows`)
- `derivePayment(meta, r)` → `StealthPayment`.
- `buildSolPayment` / `buildSplPayment` / `buildNftPayment` — transfer instructions.

### Announcements (`announce`)
- `announcementFromPayment`, `announceRequestFromAnnouncement`.
- `AnnounceClient` — `submit` / `status` against an announcement service.
- `shouldSelfAnnounce`, `dedupByEphemeralPub` — decoupled-mode helpers.

### Recipient scanning (`recipient`, `scan`, `pinboard`)
- `scanNoteCandidates(keys, R, viewTag, labels?)` → `NoteMatch[]`.
- `viewTagMatches(bScanRaw, R, viewTag)` — view-only filter.
- `subscribePinboardNotes(...)` / `subscribePinboardNotesWithSlot(...)` — live stream.
- `notesFromLogLines(lines)`, `tryParseNoteLog(line)` — parse `Note` events.
- `buildPostInstruction` / `buildPostBatchInstruction` — publish to pinboard.

### Sweeping & signing (`sweep`, `signing`)
- `StealthSigningKey` — scalar-mode Ed25519 signer for a stealth address.
- `buildSolSweep` / `buildSplSweep` — relayer-paid sweep instructions.
- `ensureNotMainWallet` — the unlinkability guard.

### Registry (`registry`)
- `registryPda`, `buildRegisterInstruction` / `buildUpdateInstruction` / `buildCloseInstruction`.
- `tryParseMetaAddressEntry` — decode a registry account.

### Errors (`errors`)
- `SlntError` with a typed `.code` (`"InvalidPoint"`, `"CloseToMainWallet"`,
  `"NonDeterministicSignature"`, …) — branch on `err.code`:

```ts
import { SlntError } from "@slnt/sdk";
try {
  derivePayment(meta, r);
} catch (e) {
  if (e instanceof SlntError && e.code === "InvalidPoint") { /* malformed meta-address */ }
}
```

---

## Security notes

- **Sign the canonical message only in trusted contexts.** It is the seed of
  your scan ability; signing it elsewhere leaks the ability to track your
  payments. Prefer `deriveStealthKeysChecked` to reject randomized signers.
- **Use a CSPRNG for the sender's `r`.** Reusing or biasing `r` can deanonymize
  or burn a payment.
- **Never link stealth → main.** Sweep to fresh/stealth/exchange addresses; let
  the builders' `mainWallet` guard catch mistakes.
- **The view key (`bScanRaw`) detects but cannot spend** — safe to delegate to a
  scanner; keep `bSpend` offline.
- Inputs from the network (meta-addresses, ephemeral keys) are validated:
  off-curve / small-order / non-torsion-free points and zero shared secrets are
  rejected rather than silently mishandled.

---

## Cross-implementation compatibility

The SDK is verified against golden vectors from the Rust reference (`slnt` CLI).
A meta-address, stealth address, and view tag derived in Rust are recovered
identically here — see `test/slnt.test.ts` and `test/vectors.test.ts`. Run them
with:

```bash
npm test
```

An on-chain smoke test against the live devnet/testnet deployments lives at
`scripts/onchain-smoke.ts`.

---

## Links

- **sRFC-0042 spec:** <https://github.com/susruth/slnt/blob/main/docs/srfc/0001-slnt-silent-payments.md>
- **Test vectors:** <https://github.com/susruth/slnt/blob/main/test-vectors.json>
- **Security policy:** <https://github.com/susruth/slnt/blob/main/SECURITY.md>

## License

MIT
