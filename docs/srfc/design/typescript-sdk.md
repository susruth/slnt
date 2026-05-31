# SLNT TypeScript Wallet SDK — Service Design

| | |
|---|---|
| **Component** | TypeScript wallet SDK |
| **Package** | `@slnt/sdk` |
| **Status** | Draft / experimental (v0.1.0) |
| **Spec** | sRFC-0042 §5 (normative) |
| **Location** | `clients/typescript/` |
| **Siblings** | `rust-sdk.md` (reference SDK), `cli.md` (the `slnt` CLI) |

---

## 1. Goal — byte-compatibility with the Rust SDK

`@slnt/sdk` is the browser/wallet-oriented client implementation of sRFC-0042 §5. Its single
defining requirement is **byte-compatibility** with the Rust reference SDK (`rust-sdk.md`): the
two implementations are independent ports of the same normative spec, and an artifact produced by
one **MUST** be consumed and reproduced exactly by the other.

"Byte-compatible" is concrete, not aspirational. Given the same inputs, both SDKs produce:

- the **same meta-address string** — identical `slnt1…` bech32m text, byte-for-byte;
- the **same stealth address** — identical compressed Ed25519 point, hence identical base58 Solana
  address;
- the **same view tag** — the same single byte from `SHA-256(len(tag) || tag || S)`;
- the **same ephemeral key `R`**, tweak scalar `t`, and reconstructed stealth scalar `p_stealth`.

This is what makes a payment created by a Rust sender spendable by a TypeScript recipient and vice
versa — there is one wire format, defined by sRFC-0042, and two conforming codecs for it.

### How parity is proven

Parity is not asserted; it is tested against fixed vectors that the Rust reference actually emitted.
`test/slnt.test.ts` embeds a **cross-implementation known-answer test (KAT)**: vectors produced by
the `slnt` Rust CLI (`cli.md`) are hardcoded, and the TypeScript SDK must reproduce them. Concretely
(`test/slnt.test.ts:14-22`):

- `slnt derive --signature 77…77` (64 bytes of `0x77`) → the meta-address `RUST_META`;
- `slnt pay --meta <RUST_META> --rng aa…aa` → ephemeral `R = RUST_R`, `view_tag = 0xa5`, and the
  stealth address `RUST_STEALTH`.

The TS SDK re-derives keys from the same signature, re-encodes the meta-address, and re-scans the
Rust-produced `(R, view_tag)` — and must land on exactly `RUST_STEALTH`. Because the Ed25519/X25519
backends differ (`@noble/curves` in TS vs `curve25519-dalek`/`x25519-dalek` in Rust), agreement on
these bytes is strong evidence that both implementations encode the spec identically at the byte
level. See [§8](#8-the-cross-impl-known-answer-test).

---

## 2. Dependency choices

The SDK has exactly three runtime dependencies, all audited, zero-dependency, browser-safe
JavaScript crypto libraries from the `paulmillr` / `scure` ecosystem. No Node built-ins, no WASM, no
`Buffer` — everything is `Uint8Array`, so the SDK runs unchanged in a browser, a wallet extension,
or Node.

| Dependency | Version | Used for | Imported as |
|---|---|---|---|
| `@noble/curves` | `^1.6.0` | Ed25519 spend points, X25519 ECDH, scalar↔bytes helpers | `ed25519`, `x25519` from `@noble/curves/ed25519`; `bytesToNumberLE`/`numberToBytesLE` from `@noble/curves/abstract/utils` |
| `@noble/hashes` | `^1.5.0` | SHA-256 (view tag / tweak), SHA-512 + HKDF-SHA256 (key & label derivation) | `sha256`, `hkdf` (and `hexToBytes` in tests) |
| `@scure/base` | `^1.1.9` | bech32m meta-address codec, base58 stealth addresses | `bech32m`, `base58` |

**Why this set.** sRFC-0042 §5.1 mandates Ed25519, X25519, SHA-256, HKDF-SHA256, and bech32m. The
noble/scure libraries provide all five with no native bindings and are the de-facto standard in the
Solana/web wallet ecosystem (the same primitives `@solana/web3.js` and Phantom-class wallets ship).
Critically, `@noble/curves/ed25519` exposes **both** `ed25519` (Edwards, for the spendable wallet
point) **and** `x25519` (Montgomery, for clean ECDH) from one module, mirroring the spec's
two-curve split. `x25519.getPublicKey` clamps its input internally, which matches Rust's
`x25519_dalek::StaticSecret::from(...)` clamping — so neither side must clamp by hand (see
[§5](#5-method-2-key-derivation)).

### Module/resolution setup and the `.js`-extension pitfall

`tsconfig.json` deliberately targets **CommonJS** (`"module": "CommonJS"`, `"moduleResolution":
"node"`, `target: ES2020`). The test runner is `ts-mocha` driven by `.mocharc.json`
(`{ "require": "ts-mocha", "spec": "test/**/*.test.ts", "extension": ["ts"] }`).

CommonJS was chosen specifically to **avoid the ESM `.js`-extension pitfall**: under
`"module": "NodeNext"`/ESM, TypeScript requires relative imports to be written with explicit `.js`
extensions even though the source files are `.ts` (e.g. `import … from "./keys.js"`). That convention
is correct for emitted ESM but breaks `ts-mocha`'s on-the-fly transpilation, which resolves the
`.ts` source directly and does not rewrite extensions. The source therefore uses bare extensionless
relative imports (`import { MetaAddress } from "./keys"`), which CommonJS resolution accepts and
`ts-mocha` runs without a build step. `package.json` points `"main"` at `src/index.ts` for the same
reason — consumers and tests share one untranspiled entry point during development; `npm run build`
(`tsc`) emits declarations + JS into `dist/` for publishing.

---

## 3. Architecture

Three source modules, re-exported flat from `src/index.ts` (`export * from "./keys" / "./sender" /
"./recipient"`):

| Module | sRFC-0042 § | Responsibility |
|---|---|---|
| `src/keys.ts` | §5.2 | Method-2 key derivation, canonical message, meta-address bech32m codec, labels, LEB128, scalar helpers |
| `src/sender.ts` | §5.3 | One-time stealth-address derivation from a meta-address + caller-supplied randomness |
| `src/recipient.ts` | §5.4, §5.10 | View-tag scanning, candidate enumeration (labels), stealth-scalar reconstruction, view-only filter |

The module boundaries match the Rust crate (`keys.rs` / `sender.rs` / `recipient.rs`) so the parity
tables below read straight across.

---

## 4. The crypto — byte-level fidelity

Every hash input in SLNT is **length-prefixed** as `H(len(tag) || tag || …)` (§5.1) so inputs are
unambiguous. Both SDKs implement this identically. The subsections below give the byte-level
construction and the explicit Rust↔TS parity note for each step.

### 4.1 `scReduce` — little-endian reduce mod ℓ

```ts
export const L = 0x1000000000000000000000000000000014def9dea2f79cd65812631a5cf5d3edn;
export function scReduce(bytes: Uint8Array): bigint {
  return bytesToNumberLE(bytes) % L;
}
```

`ℓ = 2^252 + 27742317777372353535851937790883648493` (§5.1), expressed as a TS `bigint` literal.
`scReduce` interprets `bytes` as a little-endian integer and reduces mod ℓ — `SC25519_reduce` from
the spec.

| Rust | TS | Spec |
|---|---|---|
| `Scalar::from_bytes_mod_order(x)` | `scReduce(x)` (`bytesToNumberLE(x) % L`) | §5.1 `SC25519_reduce` |

`curve25519-dalek`'s `from_bytes_mod_order` does the same LE-interpret-then-reduce; the TS side does
it explicitly with `bigint` arithmetic. Reduction targets the same ℓ, so outputs match bit-for-bit.

### 4.2 Method-2 stealth-key derivation (§5.2.1.2)

```ts
const k = hkdf(sha256, signature, utf8("slnt-v1-derive"), utf8("spend-and-scan"), 64);
const bSpendRaw = k.slice(0, 32);
const bScanRaw  = k.slice(32, 64);
const bSpend = scReduce(bSpendRaw);               // throws if 0n
const BSpend = ed25519.ExtendedPoint.BASE.multiply(bSpend).toRawBytes();
const BScan  = x25519.getPublicKey(bScanRaw);     // clamps internally
```

- **HKDF.** `hkdf(sha256, ikm=signature, salt="slnt-v1-derive", info="spend-and-scan", 64)` produces
  64 bytes, split into the spend secret (`[0:32]`) and the raw scan material (`[32:64]`). The noble
  `hkdf(hash, ikm, salt, info, length)` argument order is honored exactly.
- **Spend point.** `b_spend = scReduce(bSpendRaw)`; the SDK throws `"derivation produced zero spend
  scalar"` if it reduces to `0n` (§5.2.1.3 — MUST abort, MUST NOT retry). `B_spend =
  ed25519.ExtendedPoint.BASE.multiply(bSpend).toRawBytes()` is the compressed Edwards point.
- **Scan key.** `BScan = x25519.getPublicKey(bScanRaw)`. `bScanRaw` is the **pre-clamp** material;
  `getPublicKey` clamps it (`b[0] &= 248; b[31] &= 127; b[31] |= 64`) internally. The raw bytes are
  retained in `StealthKeys.bScanRaw` because §5.10 view-key delegation publishes the pre-clamp
  material.

| Rust | TS | Spec |
|---|---|---|
| `derive_stealth_keys(&sig)` | `deriveStealthKeysFromSignature(sig)` | §5.2.1.2 |
| `Hkdf::<Sha256>::new(Some(b"slnt-v1-derive"), sig).expand(b"spend-and-scan", &mut [0u8;64])` | `hkdf(sha256, sig, utf8("slnt-v1-derive"), utf8("spend-and-scan"), 64)` | §5.2.1.2 |
| `b_spend * ED25519_BASEPOINT_POINT` | `ed25519.ExtendedPoint.BASE.multiply(bSpend)` | §5.2.1.3 |
| `X25519StaticSecret::from(raw)` (clamps) | `x25519.getPublicKey(bScanRaw)` (clamps) | §5.2.1.3 |

**Parity note — clamping.** The TS comment "clamps internally — note this matches Rust StaticSecret
clamping" is load-bearing: both sides feed the *raw* 32 bytes to a clamping API and never clamp
themselves, so the clamped scalar — and therefore `B_scan` and every ECDH result — is identical.

### 4.3 Canonical message (§5.2.1.2)

```ts
export function canonicalMessage(network: Network): string {
  return (
    `Slnt Protocol: Derive Stealth Keys\n\nVersion: 1\nNetwork: ${network}\n` +
    `Warning: Only sign this message in the Slnt wallet or a trusted Slnt integration.\n` +
    `Signing this in any other context will reveal your stealth address scanning ability.`
  );
}
```

Exact UTF-8, **no trailing newline** (string ends at `ability.`). `Network` is the literal type
`"Mainnet" | "Devnet" | "Testnet" | "Localnet"`; the value is substituted verbatim into the
`Network:` line so keys differ per network (§5.2.1.2). `Localnet` is a non-spec convenience for local
validators, matching the Rust `Network::Localnet`. This string is byte-identical to the Rust
`canonical_message(network)` `format!` template.

The SDK does **not** sign — the wallet signs this message and hands the 64-byte signature to
`deriveStealthKeysFromSignature`. For the §8.5 determinism guard, the wallet obtains two independent
signatures and calls `deriveStealthKeysChecked(sig, confirmation)`, which rejects a randomized signer
(parity with Rust's `derive_stealth_keys_checked`; see [§7](#7-feature-parity-with-the-rust-sdk)).

### 4.4 Meta-address bech32m codec (§5.2.2)

Payload bytes, in order: `version (1) || B_spend (32) || B_scan (32) || leb128(labelIndex) ||
flags (1)` — 67–71 bytes, encoded with **bech32m**, HRP `slnt`:

```ts
const payload = concat(
  Uint8Array.of(m.version), m.bSpend, m.bScan,
  leb128Encode(m.labelIndex), Uint8Array.of(m.flags),
);
return bech32m.encode("slnt", bech32m.toWords(payload), 1023);
```

The `1023` limit override lifts `@scure/base`'s default 90-char bech32 cap, since a meta-address is
~120–126 chars (§5.2.2). `decodeMetaAddress` reverses it: rejects HRP ≠ `slnt`, payloads < 67 bytes,
`version ≠ 0x01`, and any **trailing bytes** after `flags` (`data.length !== flagsOffset + 1`),
matching the Rust decoder's strict trailing-byte and version checks one-to-one.

| Rust | TS | Spec |
|---|---|---|
| `MetaAddress::encode_bech32m` | `encodeMetaAddress(m)` | §5.2.2 |
| `MetaAddress::decode_bech32m` | `decodeMetaAddress(s)` | §5.2.2 |
| `bech32::encode::<Bech32m>(hrp, &payload)` | `bech32m.encode("slnt", bech32m.toWords(payload), 1023)` | §5.2.2 |
| `write_leb128_u32` / `read_leb128_u32` | `leb128Encode` / `leb128Decode` | §5.2.2 |

**Hand-rolled LEB128.** Neither `bech32m` nor noble provides unsigned-LEB128, so `keys.ts` ships a
small encoder/decoder (`leb128Encode`/`leb128Decode`) identical in behavior to Rust's
`write_leb128_u32`/`read_leb128_u32` (7-bit groups, high bit = continuation, max 5 bytes for a u32,
throws `"varint too long"` past that). `labelIndex` is a JS `number`; the encoder masks with `>>> 0`
to stay in unsigned-32-bit range.

### 4.5 Labels (§5.2.3)

```ts
export function labelTweakScalar(bScanRaw: Uint8Array, labelIndex: number): bigint {
  const info = concat(utf8("label-"), leb128Encode(labelIndex));
  const out = hkdf(sha256, bScanRaw, utf8("slnt-v1-label"), info, 32);
  return scReduce(out);
}
```

`m_i = SC25519_reduce(HKDF-SHA256(salt="slnt-v1-label", ikm=b_scan_raw, info="label-"||leb128(i),
32))`. `metaForLabel(keys, i)` then sets `B_spend_i = B_spend + m_i·G_ed` by **point addition**:

```ts
const base = ed25519.ExtendedPoint.fromHex(keys.BSpend);
const tweaked = base.add(ed25519.ExtendedPoint.BASE.multiply(mi));
```

`labelIndex === 0` short-circuits to the unlabeled meta-address (no tweak), matching Rust's
`for_label` guard.

| Rust | TS | Spec |
|---|---|---|
| `label_tweak_scalar(scan, i)` | `labelTweakScalar(bScanRaw, i)` | §5.2.3 |
| `MetaAddress::for_label(spend, scan, i)` | `metaForLabel(keys, i)` | §5.2.3 |
| `spend.point + m_i * G_ed` | `base.add(BASE.multiply(mi))` | §5.2.3 |

### 4.6 Sender — `derivePayment` (§5.3)

```ts
export function derivePayment(meta: MetaAddress, randomBytes32: Uint8Array): StealthPayment {
  const ephemeralPub = x25519.getPublicKey(randomBytes32);
  const s = x25519.getSharedSecret(randomBytes32, meta.bScan);
  const viewTag = viewTagOf(s);
  const t = tweakScalar(s, viewTag);
  const pStealth = ed25519.ExtendedPoint.fromHex(meta.bSpend)
    .add(ed25519.ExtendedPoint.BASE.multiply(t));
  const stealthBytes = pStealth.toRawBytes();
  return { stealthAddress: base58.encode(stealthBytes), stealthBytes, ephemeralPub, viewTag };
}
```

with the two tag hashes (note `slnt-v1-tweak` is 13 bytes, prefixed as `Uint8Array.of(13)`):

```ts
const TWEAK_TAG = "slnt-v1-tweak";
export function viewTagOf(s: Uint8Array): number {
  return sha256(concat(Uint8Array.of(TWEAK_TAG.length), utf8(TWEAK_TAG), s))[0];
}
export function tweakScalar(s: Uint8Array, viewTag: number): bigint {
  return scReduce(sha256(
    concat(Uint8Array.of(TWEAK_TAG.length), utf8(TWEAK_TAG), s, Uint8Array.of(viewTag))));
}
```

- `R = x25519.getPublicKey(r)`, `S = x25519.getSharedSecret(r, B_scan)` — the ephemeral pubkey and
  ECDH shared secret.
- `viewTagOf(S)` = first byte of `SHA-256(len("slnt-v1-tweak") || "slnt-v1-tweak" || S)`.
- `tweakScalar(S, view_tag)` = `scReduce(SHA-256(len || tag || S || [view_tag]))` — the **view tag
  is appended** to the tweak input, length-prefixed exactly as in §5.3.
- `P_stealth = B_spend_effective + t·G_ed`, compressed, base58-encoded as the Solana address.

| Rust | TS | Spec |
|---|---|---|
| `derive_payment(meta, rng)` | `derivePayment(meta, randomBytes32)` | §5.3 |
| `compute_view_tag(s)` | `viewTagOf(s)` | §5.3 |
| `compute_tweak(s, view_tag)` | `tweakScalar(s, viewTag)` | §5.3 |
| `b_spend + t * G_ed`, `Pubkey::new_from_array(compress)` | `BSpend.add(BASE.multiply(t))`, `base58.encode` | §5.3 |

**Parity note — `r` is randomness, not a CSPRNG.** The Rust `derive_payment` takes a
`&mut impl CryptoRngCore` and draws `r` internally; the TS `derivePayment` instead takes a
**caller-supplied 32-byte `r`** (`randomBytes32`, validated `length === 32`). This is deliberate: it
makes the function pure and deterministic, which is exactly what the cross-impl KAT needs (feed the
Rust CLI's `--rng aa…aa` bytes and reproduce its output). The docstring tells callers to source `r`
from a CSPRNG in production. The clamp on `r` happens inside `x25519.getPublicKey`/`getSharedSecret`,
matching Rust's `StaticSecret::from`.

> **Validation (parity with Rust).** `derivePayment` validates `version == 0x01` and `flags == 0x00`,
> decompresses `B_spend` and rejects small-order (torsion) points, and aborts on an all-zero /
> non-contributory shared secret — throwing `SlntError` with the matching `code`. `@noble` rejects
> low-order scan keys at the ECDH step, which is mapped to `InvalidSharedSecret` (§5.3).

### 4.7 Recipient — scanning (§5.4, §5.10)

```ts
export function viewTagMatches(bScanRaw, ephemeralPub, noteViewTag): boolean {
  const s = x25519.getSharedSecret(bScanRaw, ephemeralPub);
  return viewTagOf(s) === noteViewTag;
}

export function scanNoteCandidates(keys, ephemeralPub, noteViewTag, knownLabels = []): NoteMatch[] {
  const s = x25519.getSharedSecret(keys.bScanRaw, ephemeralPub);
  if (viewTagOf(s) !== noteViewTag) return [];
  const t = tweakScalar(s, noteViewTag);
  const out = [ addrMatch((keys.bSpend + t) % L, 0) ];
  for (const i of knownLabels) {
    if (i === 0) continue;
    out.push(addrMatch((keys.bSpend + labelTweakScalar(keys.bScanRaw, i) + t) % L, i));
  }
  return out;
}
```

`scanNoteCandidates` recomputes the ECDH from the recipient's scan material, applies the **view-tag
fast filter** (returns `[]` on miss — ~255/256 of garbage rejected here, §5.10), then enumerates the
unlabeled candidate plus one per `knownLabels` entry. Each `NoteMatch` carries the **stealth scalar**
as a `bigint` reduced mod ℓ — `p_stealth = (b_spend [+ m_i] + t) mod ℓ` — which is the Ed25519
private scalar of the stealth address. The address is `(scalar·G_ed)` compressed and base58-encoded;
the caller checks on-chain which candidate actually received funds.

| Rust | TS | Spec |
|---|---|---|
| `scan_note_candidates(spend, scan, R, vt, labels)` | `scanNoteCandidates(keys, R, vt, knownLabels)` | §5.4 |
| `view_tag_matches(scan, R, vt)` | `viewTagMatches(bScanRaw, R, vt)` | §5.10 |
| `stealth_scalar = spend.scalar + t` (Scalar mod ℓ) | `(keys.bSpend + t) % L` (bigint) | §5.4 |
| `spend.scalar + m_i + t` | `(keys.bSpend + mi + t) % L` | §5.4 |

**Parity note — `% L` vs `Scalar` arithmetic.** Rust adds in `curve25519_dalek::Scalar` (already mod
ℓ); the TS side adds `bigint`s and reduces with `% L`. Same residue class, same scalar, so
`scalar·G_ed` yields the identical compressed point and the identical base58 address. The Rust
`scan_note` (single-result) convenience has no separate TS twin — `scanNoteCandidates(...)[0]` is the
unlabeled result.

> The TS `viewTagMatches`/`scanNoteCandidates` route ECDH through `ecdhOrNull`, which returns `null`
> for a low-order / malformed `R` or an all-zero shared secret — so a hostile note is **skipped**
> (returns `false` / `[]`) rather than throwing, matching the intent of Rust's `shared_secret_is_zero`
> guard (§5.4, §8.4 low-order-`R` DoS).

---

## 5. API surface

All exports are flat from `@slnt/sdk` (`src/index.ts`).

### Types

| Export | Module | Shape |
|---|---|---|
| `Network` | keys | `"Mainnet" \| "Devnet" \| "Testnet" \| "Localnet"` |
| `StealthKeys` | keys | `{ bSpend: bigint; BSpend: Uint8Array; bScanRaw: Uint8Array; BScan: Uint8Array }` |
| `MetaAddress` | keys | `{ version, bSpend, bScan, labelIndex, flags }` |
| `StealthPayment` | sender | `{ stealthAddress: string; stealthBytes; ephemeralPub; viewTag: number }` |
| `NoteMatch` | recipient | `{ stealthAddress: string; stealthScalar: bigint; labelIndex: number }` |

### Constants

`L` (ℓ as bigint) · `META_ADDRESS_VERSION_V1` (`0x01`) · `SCHEME_ID_V1` (`0x0001`).

### Functions

| Export | Module | Purpose |
|---|---|---|
| `canonicalMessage(network)` | keys | §5.2.1.2 message to sign |
| `scReduce(bytes)` | keys | `SC25519_reduce` |
| `deriveStealthKeysFromSignature(sig)` | keys | Method-2 derivation → `StealthKeys` |
| `labelTweakScalar(bScanRaw, i)` | keys | label tweak `m_i` |
| `metaFromKeys(keys)` | keys | unlabeled `MetaAddress` |
| `metaForLabel(keys, i)` | keys | labeled `MetaAddress` |
| `encodeMetaAddress(m)` / `decodeMetaAddress(s)` | keys | bech32m codec |
| `utf8` / `concat` / `scalarToLeBytes` / `leb128Encode` / `leb128Decode` | keys | byte helpers (exported for parity tooling) |
| `viewTagOf(s)` | sender | view-tag byte |
| `tweakScalar(s, viewTag)` | sender | tweak scalar `t` |
| `derivePayment(meta, r)` | sender | one-time stealth address + `(R, view_tag)` |
| `viewTagMatches(bScanRaw, R, vt)` | recipient | §5.10 view-only filter |
| `scanNoteCandidates(keys, R, vt, knownLabels?)` | recipient | scan → `NoteMatch[]` |

---

## 6. Usage sketch

```ts
import {
  canonicalMessage, deriveStealthKeysFromSignature,
  metaFromKeys, encodeMetaAddress,
  derivePayment, scanNoteCandidates,
} from "@slnt/sdk";

// Recipient: wallet signs the canonical message, SDK derives keys + meta-address.
const sig = await wallet.signMessage(utf8(canonicalMessage("Mainnet"))); // 64 bytes
const keys = deriveStealthKeysFromSignature(sig);
const metaStr = encodeMetaAddress(metaFromKeys(keys));   // share `slnt1…`

// Sender: derive a one-time stealth address (r from a CSPRNG).
const r = crypto.getRandomValues(new Uint8Array(32));
const { stealthAddress, ephemeralPub, viewTag } = derivePayment(decodeMetaAddress(metaStr), r);
// → transfer to `stealthAddress`; publish (ephemeralPub, viewTag) per §5.5 (out of SDK scope).

// Recipient scan: per observed pinboard note (R, viewTag).
const [match] = scanNoteCandidates(keys, ephemeralPub, viewTag);
// match.stealthAddress === stealthAddress; match.stealthScalar is the spend key.
```

---

## 7. Feature parity with the Rust SDK

`@slnt/sdk` is now **feature-equivalent** with `slnt-sdk`. Every module has a TS counterpart, and the
parts that can be checked byte-for-byte are pinned by cross-impl known-answer tests against the Rust
reference. Module map:

| Rust (`crates/slnt-sdk/src`) | TypeScript (`clients/typescript/src`) | Notes |
|---|---|---|
| `keys.rs` — Method 1 HD (`derive_stealth_keys_hd`), Method 2, guard, codec, labels | `keys.ts` — `deriveStealthKeysHd`, `deriveStealthKeysFromSignature`, `deriveStealthKeysChecked`, codec, labels | HD derivation pinned to Rust by a cross-impl KAT (`derive-hd` vectors) |
| `error.rs` — `SlntError` | `errors.ts` — `SlntError` class + `code` | code names mirror the Rust variants |
| `sender.rs` (incl. version/flags/small-order/zero-secret hardening) | `sender.ts` | same rejections; `@noble` rejects low-order scan keys at ECDH, mapped to `InvalidSharedSecret` |
| `recipient.rs` (scan, view-only, hardening) | `recipient.ts` | hostile notes skipped, not thrown |
| `stealth_signing.rs` — scalar-mode RFC 8032 | `signing.ts` — `StealthSigningKey` | signatures verify against a standard Ed25519 verifier |
| `pinboard.rs` | `pinboard.ts` | discriminators computed via SHA-256, asserted equal to Rust |
| `registry.rs` | `registry.ts` | PDA, parse, register/update/close builders |
| `flows.rs` | `flows.ts` | SOL/SPL/NFT builders (`@solana/web3.js` + `@solana/spl-token`) |
| `sweep.rs` | `sweep.ts` | SOL/SPL sweep + `ensureNotMainWallet` close-to-relayer rule |
| `announce.rs` + `announce_client.rs` | `announce.ts` | wire types, self-announce decision, dedup, `AnnounceClient` (fetch) |
| `scan_stream.rs` | `scan.ts` | `subscribePinboardNotes(_WithSlot)` over `connection.onLogs`, `notesFromLogLines` |
| `anchor` discriminator/borsh (hand-rolled in Rust) | `anchor.ts` — `anchorDiscriminator`, `ByteWriter`/`ByteReader` | shared wire helper |

Remaining intentional differences (API ergonomics, not capability):

- **`derivePayment(meta, r)`** takes the caller-supplied 32-byte ephemeral randomness `r` directly,
  where Rust takes a `CryptoRngCore`. Browser callers pass `crypto.getRandomValues(new Uint8Array(32))`.
- **pNFT** transfers use the standard-NFT (`amount=1, decimals=0`) path in both SDKs; Metaplex
  token-record/rule-set accounts remain a follow-up keyed on `mpl-token-metadata` (mirrors the Rust note).
- Lamport/token amounts are `bigint` in TS (u64-safe) vs `u64` in Rust.

---

## 8. The cross-impl known-answer test

`test/slnt.test.ts` is the parity oracle. The hardcoded vectors (`test/slnt.test.ts:17-22`) come from
the `slnt` Rust CLI:

| Vector | Value | Origin |
|---|---|---|
| `SIG_77` | `0x77` × 64 | derivation IKM (`slnt derive --signature 77…77`) |
| `RUST_META` | `slnt1qytx5j6qsy4pr4un72tf6rr0f0vpzf7my2swgx3sdy0ug5l057h7uwfysf0e2gavthnjy4r553rucxv09hr8texhdwhycnsmz7msshzhqqqqwug2z6` | Rust meta-address for `SIG_77` |
| `RUST_R` | `0e4af3530b966e62131cf24d898fb8a7b24ef15580c46fd57c3a5115f8e19c6e` | ephemeral `R` from `slnt pay … --rng aa…aa` |
| `RUST_VIEW_TAG` | `0xa5` | view tag from the same `pay` |
| `RUST_STEALTH` | `Ac8MM66HM2tVVPkLZVag7h2XzCeHRCm912yeMmSy5RqV` | stealth address (base58) |

The six `mocha` tests and what each proves:

1. **"derives the same meta-address as the Rust reference"** — `encodeMetaAddress(metaFromKeys(...))`
   over `SIG_77` equals `RUST_META`. Proves HKDF derivation + Edwards point + X25519 clamp + bech32m
   encoding are all byte-identical across implementations.
2. **"recovers the Rust stealth address from its R + view tag (cross-impl)"** — `scanNoteCandidates`
   on the Rust-produced `(RUST_R, 0xa5)` returns `RUST_STEALTH` at `labelIndex 0`. This is the
   headline test: a TS recipient reconstructs the exact address a Rust sender computed — proving
   ECDH, the length-prefixed view-tag/tweak hashing, `% L` scalar arithmetic, and base58 all agree.
3. **"canonical message embeds the network verbatim"** — `canonicalMessage("Devnet")` contains
   `Network: Devnet`; the Mainnet message ends with `ability.` (no trailing newline). Proves the
   exact message bytes.
4. **"meta-address bech32m round-trips"** — `decodeMetaAddress(encodeMetaAddress(meta))` recovers
   version, `B_spend`, `B_scan`, `labelIndex`. Proves the codec is self-inverse incl. LEB128.
5. **"sender↔recipient round-trips locally"** — `derivePayment` then `scanNoteCandidates` agree on
   the address, and `viewTagMatches` returns `true`. Proves the local send/scan loop is consistent
   end-to-end (independent of Rust).
6. **"labeled payment is recovered only with the known label index"** — a payment to
   `metaForLabel(keys, 7)` is recovered by `scanNoteCandidates(..., [7])` and tagged `labelIndex 7`.
   Proves the label tweak (`m_i` HKDF + point/scalar add) is consistent between the labeled
   meta-address and the labeled scan candidate.

Tests 1, 2, and 3 are the byte-compatibility proof against Rust; 4–6 are internal-consistency
guards. If `@noble/curves` or `@scure/base` ever changed an encoding detail, tests 1–2 would fail
immediately.

---

## 9. Build & test

```bash
cd clients/typescript
npm install
npm test          # ts-mocha via .mocharc.json — runs test/slnt.test.ts
npm run build     # tsc → dist/ (declarations + CommonJS) for publishing
```

`npm test` runs `mocha`, which `.mocharc.json` wires to `ts-mocha` (no separate compile step;
20 000 ms timeout). All six tests should pass.

**Cosmetic warning.** Running the tests emits a Node
`[MODULE_TYPELESS_PACKAGE_JSON]` warning because `package.json` has no `"type"` field and Node has to
guess module kind for the on-the-fly `ts-mocha` transpile. It is harmless and expected — adding
`"type": "commonjs"` would silence it but is left off to keep the dual untranspiled-source /
`dist` build ergonomic. The tests pass regardless.

---

## 10. References

- **sRFC-0042 §5** — `docs/srfc/0001-slnt-silent-payments.md` (normative spec).
- **`rust-sdk.md`** — the Rust reference SDK this client mirrors byte-for-byte.
- **`cli.md`** — the `slnt` CLI that produced the cross-impl KAT vectors.
- Source: `clients/typescript/src/{keys,sender,recipient,index}.ts`; tests:
  `clients/typescript/test/slnt.test.ts`.
