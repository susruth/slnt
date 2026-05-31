# SLNT Rust SDK (`slnt-sdk`) — Design & Reference Implementation

| | |
|---|---|
| **Component** | `slnt-sdk` (Rust crate, `crates/slnt-sdk`) |
| **Status** | Reference implementation |
| **Spec** | sRFC-0042 §5 (normative) |
| **Crate version** | `0.1.0`, edition 2021, license Apache-2.0 |
| **Library name** | `slnt_sdk` (`src/lib.rs`) |

This is the canonical, byte-level reference for all SLNT off-chain cryptography:
key derivation, the meta-address codec, sender stealth-address derivation, recipient
scanning, scalar-mode Ed25519 signing, transaction/sweep instruction builders, the
announcement layer, and the networked scan stream. Every other language SDK
(`typescript-sdk.md`) mirrors the math, constants, and byte layouts defined here.
On-chain byte detail lives in the sibling program docs (`pinboard-program.md`,
`registry-program.md`); operator-facing services in `announcer.md` and
`indexer-service.md`; the end-user surface in `cli.md`.

Where this document and a sibling disagree, **this document and sRFC-0042 §5 win** for
cryptography and codecs. Where it disagrees with an on-chain program, the program's
serialized account/instruction bytes win and this SDK is the thing that must be fixed.

---

## 1. Crate layout & feature flags

### 1.1 Module map (`src/lib.rs`)

```text
slnt_sdk
├── error            SlntError — the single crate-wide error enum
├── keys             Network, SpendKey, ScanKey, MetaAddress, derivation, LEB128, labels
├── sender           StealthPayment, derive_payment, view-tag/tweak hashes
├── recipient        NoteMatch, scan_note, scan_note_candidates, view_tag_matches
├── stealth_signing  StealthSigningKey (scalar-mode RFC 8032 Ed25519)
├── flows            build_sol_payment / build_spl_payment / build_nft_payment, RENT_EXEMPT_MIN
├── sweep            build_sol_sweep / build_spl_sweep, ensure_not_main_wallet
├── announce         Announcement, AnnounceMode, self-announce logic, HTTP wire types
├── pinboard         post / post_batch instruction builders, NoteEvent log parser, discriminators
├── registry         register / update / close builders, PDA derivation, account decoder
├── announce_client  [net]  AnnounceClient — async HTTP client over §5.8.4
└── scan_stream      [net]  logsSubscribe websocket scan loop (§5.10)
```

`lib.rs` re-exports `error::SlntError` at the crate root. `announce_client` and
`scan_stream` are gated behind `#[cfg(feature = "net")]`.

### 1.2 Feature flags

| Feature | Enables | Pulls in | Gated code |
|---|---|---|---|
| `default` | nothing (empty) | pure crypto + builders only | — |
| `rpc` | `dep:solana-client` | `solana-client` 2.3 | `registry::fetch_meta_address` (async `RpcClient` account fetch) |
| `net` | `rpc` + `dep:reqwest` + `dep:futures-util` | `reqwest` 0.12 (rustls-tls, json), `futures-util` 0.3 | `announce_client` (HTTP), `scan_stream` (`PubsubClient` websocket) |

`net` implies `rpc` (`net = ["rpc", ...]`). The default build is **dependency-light and
fully offline**: it derives keys, builds and signs transactions, and parses logs without
any networking crate. A consumer that only constructs and signs transactions (e.g. a
wallet that submits through its own RPC stack) never needs `rpc`/`net`.

### 1.3 Dependency inventory

| Crate | Version | Role |
|---|---|---|
| `curve25519-dalek` | 4.1 (`digest`) | `Scalar`, `EdwardsPoint`, `ED25519_BASEPOINT_POINT`, mod-ℓ reduction |
| `ed25519-dalek` | 2.1 (`rand_core`) | `Signature`, `VerifyingKey` (verification + interop only) |
| `x25519-dalek` | 2 (`static_secrets`) | `StaticSecret` (clamped scan key + ephemeral `r`), `PublicKey`, ECDH |
| `sha2` | 0.10 | SHA-256 (view tag, tweak, discriminators), SHA-512 (SLIP-0010, RFC 8032 nonce) |
| `hkdf` | 0.12 | HKDF-SHA256 (Method 2 derive, label tweak) |
| `hmac` | 0.12 | HMAC-SHA512 (SLIP-0010 master + child) |
| `bech32` | 0.11 | `Bech32m` meta-address codec |
| `borsh` | 1 (`derive`) | instruction/event/account (de)serialization |
| `solana-sdk` | 2.3 | `Pubkey`, `Instruction`, `AccountMeta` |
| `solana-system-interface` | 1 (`bincode`) | system `transfer` instruction |
| `spl-token` | 8 (`no-entrypoint`) | `transfer_checked`, `close_account` |
| `spl-associated-token-account` | 7 (`no-entrypoint`) | idempotent ATA create, ATA address |
| `serde` / `serde_json` | 1 | HTTP wire types |
| `base64` | 0.22 | `Program data:` log decode |
| `bs58` | 0.5 | binary fields in the announce HTTP request |
| `thiserror` | 1 | `SlntError` |
| `solana-client` | 2.3 (`optional`) | `[rpc]`/`[net]` RPC + pubsub |
| `reqwest` | 0.12 (`optional`) | `[net]` HTTP |
| `futures-util` | 0.3 (`optional`) | `[net]` stream `.next()` |

Dev-deps: `rand_chacha` (deterministic test RNG), `hex`, `solana-transaction-status`,
`tokio` (rt + macros). There is one example, `examples/lifecycle.rs`.

---

## 2. Cryptographic primitives & constants

### 2.1 Curves, hashes, KDF (sRFC-0042 §5.1)

| Primitive | Choice | SDK realization |
|---|---|---|
| Spend key | Ed25519 | `curve25519_dalek::Scalar` × `ED25519_BASEPOINT_POINT` |
| Scan key | X25519 | `x25519_dalek::StaticSecret` / `PublicKey` |
| Hash | SHA-256 | `sha2::Sha256` |
| KDF | HKDF-SHA256 | `hkdf::Hkdf::<Sha256>` |
| Nonce / SLIP-0010 hash | SHA-512 | `sha2::Sha512`, `hmac::Hmac<Sha512>` |
| Meta-address encoding | bech32m, HRP `slnt` | `bech32::Bech32m` |
| On-chain stealth address | raw 32-byte Ed25519 pubkey | `solana_sdk::Pubkey::new_from_array` |

**Group constants.** `G_ed` = Ed25519 base point (`ED25519_BASEPOINT_POINT`); `G_x` =
Curve25519 Montgomery base point (implicit in `x25519_dalek`); the group order

```text
ℓ = 2^252 + 27742317777372353535851937790883648493
```

### 2.2 Domain-separation tags

All tags are ASCII byte strings. Hash inputs that incorporate a tag use the
**length-prefixed convention** `H(len(tag) || tag || …)`, where `len(tag)` is a single
byte holding the tag's length, so that no two distinct (tag, payload) pairs can collide
on the concatenated input.

| Tag (ASCII) | Length | Used by | Where |
|---|---|---|---|
| `slnt-v1-derive` | 14 | HKDF **salt** for Method 2 key derivation | `keys::HKDF_SALT_DERIVE` |
| `slnt-v1-tweak` | 13 | view-tag and stealth tweak SHA-256 inputs | `sender::TWEAK_TAG` |
| `slnt-v1-label` | 13 | HKDF **salt** for label tweak `m_i` | `keys::HKDF_SALT_LABEL` |
| `slnt-v1-nonce` | 13 | SHA-512 prefix for the RFC 8032 signing nonce | `stealth_signing::NONCE_TAG` |

(`slnt-v1-memo` is reserved in the spec for metadata encryption; not implemented in v1.)

The length-prefix convention is applied explicitly for the **tweak** hashes (where the tag
and the variable-length secret `S` share one hash input):

```rust
hasher.update([TWEAK_TAG.len() as u8]);   // 0x0D
hasher.update(TWEAK_TAG);                  // "slnt-v1-tweak"
hasher.update(S);                          // 32-byte shared secret
// view-tag: stop here, take [0]
hasher.update([view_tag]);                 // tweak: append the view-tag byte
```

For HKDF the tag is the **salt** argument (`Hkdf::new(Some(salt), ikm)`), which HKDF-Extract
already binds unambiguously, so no manual length prefix is added there.

### 2.3 `SC25519_reduce` and `X25519_clamp` realizations

**`SC25519_reduce(x)`** — interpret 32 bytes as a little-endian integer and reduce mod ℓ:

```rust
let scalar = Scalar::from_bytes_mod_order(bytes32);          // 32-byte input
let scalar = Scalar::from_bytes_mod_order_wide(&bytes64);    // 64-byte wide input (RFC 8032)
```

`from_bytes_mod_order` is used for `b_spend`, the label tweak `m_i`, and the 256-bit tweak
`t`. `from_bytes_mod_order_wide` is used inside `StealthSigningKey::sign` where SHA-512
produces a 64-byte value to be reduced (RFC 8032 `r` and `k`).

**`X25519_clamp(b)`** — `b[0] &= 248; b[31] &= 127; b[31] |= 64`. The SDK never clamps by
hand; it constructs `x25519_dalek::StaticSecret::from(raw32)`, whose Diffie–Hellman path
applies the standard clamp internally. The **raw, unclamped** 32 bytes are retained
alongside the `StaticSecret` (see `ScanKey.raw`) because the label tweak and view-key
delegation are defined over `b_scan_raw`, not the clamped scalar.

### 2.4 Versioning constants

| Constant | Value | Meaning |
|---|---|---|
| `META_ADDRESS_VERSION_V1` | `0x01` | meta-address byte-layout version |
| `SCHEME_ID_V1` | `0x0001` (`u16`) | announcement cryptographic suite id |
| `SLNT_HD_PURPOSE` | `0x534C_4E54` | ASCII `"SLNT"` = decimal 1397967188, HD purpose |
| `SOLANA_COIN_TYPE` | `501` | SLIP-0044 coin type |

`version` (the meta-address encoding) is **independent** of `scheme_id` (the crypto suite).

---

## 3. Key derivation

A recipient's stealth identity is a pair of 32-byte secrets `(b_spend_raw, b_scan_raw)`
produced by **exactly one** of two methods, then mapped to `(SpendKey, ScanKey)` by the
common §5.2.1.3 step. The two methods produce *different* keys from the same wallet; a
recipient MUST pick one method per identity and stick to it.

### 3.1 Key types

```rust
pub struct SpendKey { pub scalar: Scalar, pub point: EdwardsPoint }   // point = scalar · G_ed
pub struct ScanKey  {
    pub raw: [u8; 32],                     // b_scan_raw (UNCLAMPED) — used by labels & §5.10
    pub static_secret: X25519StaticSecret, // clamped form used for ECDH
    pub public: X25519PublicKey,           // B_scan = clamp(raw) · G_x
}
```

`SpendKey::public_bytes()` = `point.compress().to_bytes()` (32-byte compressed Ed25519).
`ScanKey::public_bytes()` = `public.to_bytes()` (32-byte X25519).

### 3.2 Method 1 — HD derivation (`derive_stealth_keys_hd`, §5.2.1.1)

Wallet-native, **no signing step**. Derives the two raw secrets directly from the 64-byte
BIP-39 seed at a dedicated SLNT branch, using SLIP-0010 for ed25519 (every level
hardened):

```text
spend:  m / 0x534C4E54' / 501' / account' / 0'
scan:   m / 0x534C4E54' / 501' / account' / 1'
```

`0x534C4E54'` is the hardened purpose = ASCII `"SLNT"`. Because every BIP-44 Solana wallet
key lives under purpose `44'`, the whole SLNT subtree is segregated from spendable wallet
keys and can never collide. `account` (default `0`) is the stealth-identity index; `0'`
selects spend, `1'` selects scan (BIP-352 sibling convention). The two 32-byte node
values are the secrets **directly — no HKDF step**.

```rust
fn harden(index: u32) -> u32 { index | 0x8000_0000 }

pub fn derive_stealth_keys_hd(seed: &[u8], account: u32) -> Result<(SpendKey, ScanKey)> {
    let base = [harden(SLNT_HD_PURPOSE), harden(SOLANA_COIN_TYPE), harden(account)];
    let spend_path = base ++ [harden(0)];   // 0' = spend
    let scan_path  = base ++ [harden(1)];   // 1' = scan
    let b_spend_raw = slip10_ed25519_node(seed, &spend_path);
    let b_scan_raw  = slip10_ed25519_node(seed, &scan_path);
    keys_from_secrets(b_spend_raw, b_scan_raw)
}
```

**SLIP-0010 ed25519 node derivation** (`slip10_ed25519_node`):

```text
# Master:
I       = HMAC-SHA512(key = "ed25519 seed", data = seed)   // 64 bytes
key     = I[0..32]      // I_L, the node private key
chain   = I[32..64]     // I_R, the chain code

# Each hardened child index i (always hardened for ed25519):
I       = HMAC-SHA512(key = chain, data = 0x00 || key || ser32_BE(i))
key     = I[0..32]
chain   = I[32..64]

return key   // I_L at the requested node
```

`ser32_BE(i)` is `i.to_be_bytes()` (4-byte big-endian). The implementation is verified
against the **official SLIP-0010 ed25519 test vector 1** (`seed =
000102030405060708090a0b0c0d0e0f`):

| Path | Expected `I_L` (hex) |
|---|---|
| `m` | `2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7` |
| `m/0'` | `68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3` |

(Test: `slip10_matches_official_ed25519_vector`.) Method 1 is deterministic from the seed
and has no randomized-signature failure mode.

### 3.3 Method 2 — Signed canonical message (`derive_stealth_keys`, §5.2.1.2)

For dapps reaching the user only through a generic signing wallet (no seed access). Keys
derive from a 64-byte Ed25519 signature over the canonical message.

**Canonical message** (`canonical_message(network)`), exact UTF-8, **no trailing newline**:

```text
Slnt Protocol: Derive Stealth Keys

Version: 1
Network: <label>
Warning: Only sign this message in the Slnt wallet or a trusted Slnt integration.
Signing this in any other context will reveal your stealth address scanning ability.
```

`<label>` is substituted verbatim from the `Network` enum so keys differ per network and a
devnet experiment can never leak a mainnet stealth identity:

```rust
pub enum Network { Mainnet, Devnet, Testnet, Localnet }
// label(): "Mainnet" | "Devnet" | "Testnet" | "Localnet"
```

`Mainnet`/`Devnet`/`Testnet` are the spec-enumerated values. `Localnet` is a
**non-spec convenience** for local validators and demos; identities under it are not
portable to a conforming wallet. `CANONICAL_MESSAGE_LOCALNET` is a precomputed `&str` for
that case. (Tests assert the Mainnet message is byte-exact and that all four networks
produce distinct messages.)

**Derivation:**

```text
ikm  = signature                                   // 64 bytes
seed = HKDF-SHA256(salt = "slnt-v1-derive", ikm, info = "spend-and-scan", L = 64)
b_spend_raw = seed[0..32]
b_scan_raw  = seed[32..64]
→ keys_from_secrets(b_spend_raw, b_scan_raw)
```

```rust
pub fn derive_stealth_keys(signature_64: &[u8; 64]) -> Result<(SpendKey, ScanKey)> {
    let hk = Hkdf::<Sha256>::new(Some(b"slnt-v1-derive"), signature_64);
    let mut seed = [0u8; 64];
    hk.expand(b"spend-and-scan", &mut seed).map_err(|_| SlntError::Derivation)?;
    keys_from_secrets(seed[0..32], seed[32..64])
}
```

HKDF treats the IKM as opaque bytes; the test suite even uses a fixed non-signature
64-byte array as input, since the math does not require it to be a real signature.

**Determinism guard (`derive_stealth_keys_checked`, §8.5).** Method 2 recoverability
depends on *deterministic* RFC 8032 signing. The checked entry point takes two independent
signatures of the **same** canonical message and rejects the wallet if they differ:

```rust
pub fn derive_stealth_keys_checked(sig: &[u8;64], confirmation: &[u8;64])
    -> Result<(SpendKey, ScanKey)> {
    if sig != confirmation { return Err(SlntError::NonDeterministicSignature); }
    derive_stealth_keys(sig)
}
```

A randomized signer (different bytes for the same message) is unusable with Method 2 and
is surfaced as `NonDeterministicSignature` rather than silently producing an unrecoverable
identity.

### 3.4 Common mapping (`keys_from_secrets`, §5.2.1.3)

Both methods funnel through one function:

```rust
fn keys_from_secrets(b_spend_raw: [u8;32], b_scan_raw: [u8;32]) -> Result<(SpendKey, ScanKey)> {
    // SC25519_reduce
    let b_spend = Scalar::from_bytes_mod_order(b_spend_raw);
    if b_spend == Scalar::ZERO { return Err(SlntError::Derivation); }   // MUST abort, no retry
    let b_spend_point = b_spend * ED25519_BASEPOINT_POINT;              // B_spend = b_spend · G_ed

    // X25519_clamp happens inside StaticSecret::from
    let scan_static = X25519StaticSecret::from(b_scan_raw);             // b_scan = clamp(raw)
    let scan_public = X25519PublicKey::from(&scan_static);             // B_scan = b_scan · G_x

    Ok((SpendKey { scalar: b_spend, point: b_spend_point },
        ScanKey  { raw: b_scan_raw, static_secret: scan_static, public: scan_public }))
}
```

The zero-`b_spend` abort is a hard error (`SlntError::Derivation`); the implementation
**MUST NOT** silently retry. The raw (unclamped) scan bytes are kept for labels and for
view-key delegation (`ScanKey::from_raw`).

---

## 4. Meta-address codec (§5.2.2)

bech32m, HRP `slnt`. Payload byte layout:

| Offset | Field | Size | Notes |
|---|---|---|---|
| 0 | `version` | 1 byte | `0x01` in v1 |
| 1..33 | `B_spend` | 32 bytes | compressed Ed25519 (already includes label tweak if labeled) |
| 33..65 | `B_scan` | 32 bytes | X25519 scan pubkey |
| 65..(65+n) | `label_index` | LEB128, 1–5 bytes | `0` = unlabeled |
| 65+n | `flags` | 1 byte | reserved, `0x00` in v1 |

Total payload **67–71 bytes** (`label_index` is 1 byte for `0..=127`, up to 5 bytes for
large values). Encoded string is `slnt1…`, ~120–126 chars.

```rust
pub struct MetaAddress { pub version: u8, pub b_spend: [u8;32], pub b_scan: [u8;32],
                         pub label_index: u32, pub flags: u8 }
```

### 4.1 Hand-rolled LEB128

Unsigned LEB128 (DWARF/protobuf style), max 5 bytes for a `u32`:

```rust
fn write_leb128_u32(out: &mut Vec<u8>, mut val: u32) {
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 { byte |= 0x80; }      // continuation bit
        out.push(byte);
        if val == 0 { break; }
    }
}

fn read_leb128_u32(data: &[u8]) -> Result<(u32, usize)> {
    let mut val: u64 = 0; let mut shift = 0;
    for (i, byte) in data.iter().take(5).enumerate() {
        val |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            if val > u32::MAX as u64 { return Err(..("varint exceeds u32")); }
            return Ok((val as u32, i + 1));
        }
        shift += 7;
    }
    Err(..("varint too long"))             // > 5 continuation bytes
}
```

(`leb128_roundtrip` test covers `0, 1, 127, 128, 255, 256, 16384, 1234567, u32::MAX`.)

### 4.2 Encode / decode

`encode_bech32m`: push `version`, extend with `B_spend`, `B_scan`, write LEB128
`label_index`, push `flags`, then `bech32::encode::<Bech32m>(Hrp("slnt"), payload)`.

`decode_bech32m` validation order (each failure is a distinct error):

1. `bech32::decode` (also verifies the bech32m checksum) → `MetaAddressDecode`.
2. HRP must equal `slnt`, else `MetaAddressDecode("expected HRP `slnt`…")`.
3. `data.len() >= 67` (minimum payload), else `MetaAddressDecode("payload too short")`.
4. `version == 0x01`, else `UnsupportedVersion(version)`.
5. Slice `B_spend = data[1..33]`, `B_scan = data[33..65]`.
6. `read_leb128_u32(&data[65..])` → `(label_index, consumed)`; `flags_offset = 65 + consumed`.
7. `data.len() > flags_offset`, else `MetaAddressDecode("missing flags byte")`.
8. `flags = data[flags_offset]`. **No trailing bytes:** `data.len() == flags_offset + 1`,
   else `MetaAddressDecode("N trailing bytes after payload")`.
9. `flags == 0`, else `UnsupportedFlags(flags)`.

Tests cover: unlabeled round-trip (asserts `slnt1` prefix), labeled round-trip
(`label_index = 12345`), wrong HRP rejection, `UnsupportedVersion(0x02)`, and nonzero-flags
rejection.

---

## 5. Labels (§5.2.3)

Labels let one scan key back many meta-addresses, so a recipient can tell which counterparty
paid without linking them. The label tweak scalar `m_i` is derived from the **raw**
(unclamped) scan secret:

```text
m_i = SC25519_reduce(
        HKDF-SHA256(salt = "slnt-v1-label", ikm = b_scan_raw,
                    info = "label-" || leb128(i), length = 32))
```

```rust
pub fn label_tweak_scalar(scan: &ScanKey, label_index: u32) -> Scalar {
    let hk = Hkdf::<Sha256>::new(Some(b"slnt-v1-label"), &scan.raw);
    let mut info = b"label-".to_vec();
    write_leb128_u32(&mut info, label_index);   // same LEB128 used in the codec
    let mut out = [0u8; 32];
    hk.expand(&info, &mut out).expect("32-byte HKDF expand never fails");
    Scalar::from_bytes_mod_order(out)
}
```

**Labeled meta-address (`MetaAddress::for_label`).** `label_index = 0` is identical to
`from_keys` (no tweak). For `i >= 1`:

```text
B_spend_i = B_spend + m_i · G_ed     (encoded in the B_spend field)
label_index = i
B_scan unchanged
```

The scan key is untouched by labeling; senders treat the encoded `B_spend` as opaque.
Tests confirm `m_i` is deterministic and distinct per index, never `Scalar::ZERO`, that
the labeled meta encodes exactly `B_spend + m_i·G_ed`, and that label 0 equals the
unlabeled address.

---

## 6. Sender derivation (`sender.rs`, §5.3)

```rust
pub struct StealthPayment {
    pub stealth_address: Pubkey,   // compress(P_stealth) as a Solana address
    pub ephemeral_pub: [u8; 32],   // R, for the pinboard note
    pub view_tag: u8,              // SHA-256(len||tag||S)[0]
}
```

`derive_payment(meta, rng)` — steps with exact hash inputs:

1. **Validate meta:** `version == 0x01` else `UnsupportedVersion`; `flags == 0` else
   `UnsupportedFlags`.
2. **Decompress** `B_spend` (`CompressedEdwardsY(meta.b_spend).decompress()`), else
   `InvalidPoint`. Reject small-order points (`is_small_order()`) → `InvalidPoint`.
3. **Ephemeral scalar:** `rng.fill_bytes(&mut r_bytes[32])`; `r =
   X25519StaticSecret::from(r_bytes)` (clamped internally); `R = X25519PublicKey::from(&r)`.
4. **ECDH:** `S = r.diffie_hellman(B_scan)`. If `S` is all-zero → `InvalidSharedSecret`
   (catches low-order scan keys before funds move).
5. **view_tag** `= SHA-256(0x0D || "slnt-v1-tweak" || S)[0]` (`compute_view_tag`).
6. **tweak** `t = SC25519_reduce(SHA-256(0x0D || "slnt-v1-tweak" || S || [view_tag]))`
   (`compute_tweak`, `from_bytes_mod_order` on the 32-byte SHA-256 digest).
7. **`P_stealth = B_spend + t · G_ed`**; `stealth_address =
   Pubkey::new_from_array(P_stealth.compress().to_bytes())`.

`compute_view_tag`, `compute_tweak`, and `shared_secret_is_zero` are `pub(crate)` and
**shared verbatim** with the recipient, guaranteeing the two sides hash identical bytes.
Tests: deterministic under a fixed RNG, distinct per call (fresh `r`), rejection of bad
version/flags/small-order spend point/all-zero shared secret.

---

## 7. Recipient scan (`recipient.rs`, §5.4)

```rust
pub struct NoteMatch {
    pub stealth_address: Pubkey,
    pub stealth_scalar: Scalar,   // p_stealth = (b_spend [+ m_i] + t) mod ℓ
    pub label_index: u32,         // 0 = unlabeled
}
```

### 7.1 `scan_note` (unlabeled fast path)

```text
S_candidate = scan.static_secret · R                 (ECDH)
if S_candidate == 0^32: return Ok(None)              (low-order R guard FIRST)
vt = compute_view_tag(S_candidate)
if vt != note_view_tag: return Ok(None)              (~255/256 rejected here)
t  = compute_tweak(S_candidate, note_view_tag)       (uses the NOTE's view tag)
P_stealth      = spend.point + t · G_ed
stealth_scalar = spend.scalar + t                    (= p_stealth, mod ℓ)
```

Returns `Ok(Some(NoteMatch{label_index: 0}))` on a view-tag hit, `Ok(None)` on a miss.
`Err` is reserved for malformed input (currently unreachable — every 32 bytes is a valid
X25519 point — but the `Result` is kept for forward compatibility).

### 7.2 `scan_note_candidates` (labels)

Same ECDH + view-tag prefilter, but on a hit returns the unlabeled candidate **plus one
per known label index**:

```text
unlabeled:  p_stealth = (b_spend + t) mod ℓ
label i≠0:  p_stealth = (b_spend + m_i + t) mod ℓ     where m_i = label_tweak_scalar(scan, i)
```

`label_index = 0` entries in `known_labels` are skipped (already covered). Each candidate's
address is `compress((b_spend [+ m_i] + t) · G_ed)`. The SDK cannot tell which label the
sender used (`B_spend_effective` is opaque to the sender), so the **caller** confirms which
candidate actually received funds on-chain. Returns an empty `Vec` when the view tag
doesn't match.

### 7.3 View-only delegation (`view_tag_matches`, §5.10)

```rust
pub fn view_tag_matches(scan: &ScanKey, ephemeral_pub: &[u8;32], note_view_tag: u8) -> bool {
    let s = scan.static_secret.diffie_hellman(R);
    if shared_secret_is_zero(s) { return false; }
    compute_view_tag(s) == note_view_tag
}
```

A delegated scanner reconstructs a **view-only** `ScanKey` via `ScanKey::from_raw(b_scan_raw)`
(holds the scan material, no spend key) and runs only the ECDH + view-tag filter. It learns
nothing about the stealth address and can never spend; it merely forwards surviving
candidates to the recipient. (Test `view_only_scanner_filters_without_spend_ability`
confirms a foreign scan key rejects >200/256 view tags.)

### 7.4 Hardening checklist (recipient side)

- **All-zero shared secret rejected before the view-tag compare** (`shared_secret_is_zero`).
  Without this, a single crafted low-order `R` would pass the filter for every recipient
  (§5.4). Verified by `low_order_ephemeral_pub_is_ignored`.
- **View-tag false positives never spend.** A ~1/256 coincidental view-tag match still
  yields a different recovered address than the real payment; `unrelated_recipient_does_not_match`
  runs 512 payments to recipient B and asserts recipient A never recovers B's address even
  on a collision.
- **Sender-side hardening** (non-v1 flags/version, small-order spend point, zero shared
  secret) lives in `derive_payment` (§6).

---

## 8. Stealth signing (`stealth_signing.rs`)

The recipient holds the stealth private key as a **scalar** `p_stealth` (from §5.4/§5.9),
not an RFC 8032 32-byte seed. The standard `ed25519_dalek::SigningKey` cannot be used: its
expanded-secret path **clamps** the scalar, which would corrupt the non-clamped
`p_stealth = b_spend + t`. So the SDK implements RFC 8032 signing directly over
`curve25519-dalek` primitives. The output is **bit-identical** to a standard Ed25519 signer
for the same scalar/nonce and verifies cleanly with `ed25519_dalek::VerifyingKey::verify`.

```rust
pub struct StealthSigningKey { scalar: Scalar, public_point: EdwardsPoint, hash_prefix: [u8;32] }

impl StealthSigningKey {
    pub fn new(scalar: Scalar) -> Self {
        // hash_prefix = SHA-512("slnt-v1-nonce" || scalar)[32..64]
        let hash = SHA-512(NONCE_TAG || scalar.to_bytes());
        Self { scalar, public_point: scalar · G_ed, hash_prefix: hash[32..64] }
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        let A = self.public_point.compress();
        r = SC25519_reduce_wide(SHA-512(hash_prefix || message))     // from_bytes_mod_order_wide
        R = (r · G_ed).compress()
        k = SC25519_reduce_wide(SHA-512(R || A || message))
        s = r + k · scalar                                            // mod ℓ
        Signature::from_bytes(R(32) || s(32))                         // 64 bytes
    }
}
```

The nonce derivation `hash_prefix = SHA-512("slnt-v1-nonce" || scalar)[32..64]` keeps
signing deterministic (so the same scalar+message always yields the same signature)
without exposing the raw scalar directly as the nonce seed. `public_bytes()` equals the
stealth account's Solana address (`compress(scalar · G_ed)`). Tests: sign→verify through
`ed25519-dalek`, signature determinism, and `public_bytes == scalar · G_ed`.

---

## 9. Transaction flows (`flows.rs`, §5.7)

These build **decoupled-mode** transfers: pure value movement plus any required account
creation, **no SLNT instruction**, so the transaction is indistinguishable from an ordinary
transfer to a fresh address. The announcement is published separately (§10).

```rust
pub const RENT_EXEMPT_MIN: u64 = 890_880;   // rent-exempt minimum for a bare system account
```

### 9.1 `build_sol_payment` → `Instruction`

Transfers `amount + RENT_EXEMPT_MIN` via the system program. The rent buffer makes the
fresh system account valid and is reclaimed by the recipient on sweep. Overflow is checked:

```rust
let lamports = amount.checked_add(RENT_EXEMPT_MIN)
    .ok_or(SlntError::LamportOverflow { amount, rent_buffer: RENT_EXEMPT_MIN })?;
solana_system_interface::instruction::transfer(sender, stealth_address, lamports)
```

(Test asserts data prefix `[2,0,0,0]` — the bincode system-transfer tag — and lamports =
`amount + 890_880`; `u64::MAX` yields `LamportOverflow`.)

### 9.2 `build_spl_payment` → `Vec<Instruction>` (2 ixs)

Works for SPL Token and Token-2022 by passing the matching `token_program_id`:

1. **Idempotent ATA create** for the stealth owner (`create_associated_token_account_idempotent`,
   sender pays ATA rent). The stealth ATA is
   `get_associated_token_address_with_program_id(stealth, mint, token_program_id)`.
2. **`transfer_checked`** from `sender_token_account` into the stealth ATA, with `amount`
   and `decimals` (instruction tag `12`).

The mint is necessarily on-chain-visible; only the recipient identity is hidden.

### 9.3 `build_nft_payment` → `Vec<Instruction>`

Exactly `build_spl_payment(..., amount = 1, decimals = 0)` — standard and Token-2022 NFTs.
**Programmable NFTs additionally require Metaplex token-record / rule-set accounts**, which
are out of scope here and must be constructed via `mpl-token-metadata` (a follow-up).

---

## 10. Sweep (`sweep.rs`, §5.9)

A stealth account holds value but only the rent-exempt minimum in SOL, so it cannot pay its
own fees. A **relayer** signs as fee payer and is compensated from the swept value. The
sweep transaction is signed by both the relayer (fee payer) and the stealth key (authority
over the funds).

**Close-to-relayer MUST rule (§5.9/§8.3).** Rent reclaimed by closing the stealth
account/ATA MUST go to the relayer or another stealth address the recipient controls —
**never the recipient's main wallet**, which would create a direct `stealth → main` link.
Enforced by:

```rust
pub fn ensure_not_main_wallet(candidate: &Pubkey, main_wallet: Option<&Pubkey>) -> Result<()> {
    if main_wallet == Some(candidate) { return Err(SlntError::CloseToMainWallet); }
    Ok(())
}
```

Pass `main_wallet = None` only when unlinkability is independently guaranteed (e.g. a
known-stealth destination).

### 10.1 `build_sol_sweep` → `Vec<Instruction>` (2 ixs)

Two system transfers from the stealth account: `relayer_take` lamports to the relayer, then
`balance - relayer_take` to `destination`. The account reaches zero and is reclaimed by the
runtime. Guards: `ensure_not_main_wallet(destination, main_wallet)`; `relayer_take < balance`
else `RelayerTakeTooLarge`. Stealth→stealth destinations are allowed (preserves
unlinkability across hops).

### 10.2 `build_spl_sweep` → `Vec<Instruction>` (3 ixs)

All three carry the stealth key as authority:

1. `transfer_checked` `amount - relayer_take` to `destination_ata`.
2. `transfer_checked` `relayer_take` to `relayer_token_account` (in-kind fee).
3. `close_account` the stealth ATA → reclaimed rent to `close_destination` (tag `9`).

Guards: `ensure_not_main_wallet(close_destination, main_wallet)`; `relayer_take <= amount`
else `RelayerTakeTooLarge`. Token-program errors are wrapped as `SlntError::Rpc`. Tests
confirm tags `12,12,9`, the split, main-wallet rejection (SOL and SPL), oversized-take
rejection, and stealth→stealth acceptance.

---

## 11. Announcement layer (`announce.rs`, §5.8)

In v1 the sender **defaults to decoupled mode**: the transfer carries no SLNT instruction,
and the announcement tuple is published separately (ideally by a service).

```rust
pub const MAX_METADATA_LEN: usize = 64;                          // §5.5.1
pub const SELF_ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(60);  // T, §5.8.2

pub struct Announcement { pub scheme_id: u16, pub ephemeral_pub: [u8;32],
                          pub view_tag: u8, pub metadata: Vec<u8> }
```

- `Announcement::from_payment(payment, metadata)` — sets `scheme_id = SCHEME_ID_V1`, copies
  `R` and `view_tag`; rejects `metadata.len() > 64` → `MetadataTooLong(n)`.
- `Announcement::to_post_instruction(pinboard_program_id, fee_payer)` — builds the pinboard
  `post` instruction (coupled mode / self-announce, where the sender pays for the post).

### 11.1 Modes & self-announce

```rust
pub enum AnnounceMode { Decoupled, Coupled }   // Coupled = escape hatch (visible SLNT marker)

pub fn should_self_announce(matching_note_seen: bool, elapsed: Duration, timeout: Duration) -> bool {
    !matching_note_seen && elapsed >= timeout
}
```

After submitting to a service, the wallet watches pinboard for a note with matching `R`; if
none appears within `T = 60s`, it MUST self-announce. `dedup_by_ephemeral_pub` removes
duplicate notes (a service+sender race can publish two notes with the same `R`),
preserving first-seen order via a `HashSet`.

### 11.2 HTTP wire types (§5.8.4)

Binary fields are base58 strings.

```rust
struct AnnounceRequest { scheme_id: u16, ephemeral_pub: String /*bs58 R*/,
                         view_tag: u8, metadata: String /*bs58, "" if none*/,
                         payment_proof: Option<String> /*omitted if None*/ }
struct AnnounceResponse { queued: bool, batch_id: String, expected_slot: u64 }
struct AnnounceStatus   { status: String /*pending|confirmed|failed*/,
                          tx_signature: Option<String> /*omitted if None*/ }
```

`AnnounceRequest::from_announcement` base58-encodes `R` and `metadata`. `Option` fields use
`skip_serializing_if = "Option::is_none"`. (Tests verify JSON round-trip and that
`tx_signature: None` is omitted from the serialized status.)

### 11.3 `[net]` — `AnnounceClient` (`announce_client.rs`)

Thin async `reqwest` client over §5.8.4. URL construction is unit-testable without a server:

- `new(base_url)` / `with_client(base_url, reqwest::Client)` — trailing slashes trimmed.
- `announce_url()` = `{base}/announce`; `status_url(id)` = `{base}/announce/status/{id}`.
- `submit(&AnnounceRequest) -> AnnounceResponse` — `POST /announce`; non-2xx →
  `Rpc("POST /announce: HTTP …")`.
- `status(batch_id) -> AnnounceStatus` — `GET /announce/status/{id}`.

### 11.4 `[net]` — scan stream (`scan_stream.rs`, §5.10)

The REQUIRED baseline scanner subscribes to pinboard program logs over a websocket and
parses `Note` events as they stream:

- `notes_from_log_lines(&[String]) -> Vec<NoteEvent>` — runs `try_parse_note_log` over each
  log line, ignoring non-Note and malformed lines (pure, testable).
- `subscribe_pinboard_notes(ws_url, pinboard_program_id, on_note)` — `PubsubClient`
  `logs_subscribe` with filter `Mentions([program_id])`, commitment `confirmed`; calls
  `on_note(NoteEvent)` for each parsed event. Performs **no key operations** and learns
  nothing about which notes matched — the recipient's `scan_note` runs inside `on_note`.
- `subscribe_pinboard_notes_with_slot(...)` — same, but passes `(slot, NoteEvent)` for
  indexers that serve announcements by slot range.

Offline-gap backfill (`getSignaturesForAddress` + `getTransaction`) is done separately by
the caller (see the lifecycle example and `indexer-service.md`).

---

## 12. Instruction builders re-exported (pinboard / registry)

The SDK hand-builds Anchor instructions (no `anchor-client` dependency) using the 8-byte
discriminator convention `SHA-256("global:<ix_snake>")[..8]` (instructions),
`SHA-256("event:<Event>")[..8]` (events), `SHA-256("account:<Account>")[..8]` (accounts),
followed by borsh-serialized bodies. Each discriminator is re-derived and asserted in a unit
test. **Byte-level detail lives in `pinboard-program.md` and `registry-program.md`;** this
section indexes what the SDK exposes.

### 12.1 `pinboard.rs`

| Constant | Value |
|---|---|
| `POST_DISCRIMINATOR` | `[223, 96, 234, 236, 158, 106, 145, 94]` |
| `POST_BATCH_DISCRIMINATOR` | `[172, 123, 234, 102, 14, 213, 76, 36]` |
| `NOTE_EVENT_DISCRIMINATOR` | `[40, 182, 5, 151, 115, 43, 27, 97]` |

- `build_post_instruction(...)` — `post` ix; one account `AccountMeta::new(fee_payer, true)`;
  data = `POST_DISCRIMINATOR || borsh(PostArgs{scheme_id, ephemeral_pub, view_tag, metadata})`.
- `build_post_batch_instruction(...)` — `post_batch` ix; data =
  `POST_BATCH_DISCRIMINATOR || borsh(Vec<NoteEntry>)`. `entries` must be non-empty (program
  rejects empty); practical size bounded by compute budget.
- `try_parse_note_log(line) -> Result<Option<NoteEvent>, String>` — strips
  `"Program data: "`, base64-decodes, checks the `Note` discriminator, borsh-decodes the
  body. `Ok(None)` for non-matching lines; `Err` only when a Note-tagged line fails to
  deserialize.

`PostArgs`, `NoteEntry`, and `NoteEvent` are identical borsh layouts:
`{ scheme_id: u16, ephemeral_pub: [u8;32], view_tag: u8, metadata: Vec<u8> }`.

### 12.2 `registry.rs`

| Constant | Value |
|---|---|
| `META_SEED` | `b"meta"` |
| `REGISTER_DISCRIMINATOR` | `[211,124,67,15,211,194,178,240]` |
| `UPDATE_DISCRIMINATOR` | `[219,200,88,176,158,63,253,127]` |
| `CLOSE_DISCRIMINATOR` | `[98,165,201,177,108,65,206,96]` |
| `META_ADDRESS_ENTRY_DISCRIMINATOR` | `[165,7,241,154,7,172,74,178]` |

- `registry_pda(program_id, registrant, scheme_id) -> (Pubkey, u8)` —
  `find_program_address([b"meta", registrant, scheme_id.to_le_bytes()], program_id)`.
- `build_register_instruction` — accounts `[registrant(signer,mut), pda(mut),
  system_program(ro)]`; data = `REGISTER_DISCRIMINATOR || borsh(scheme_id:u16) ||
  borsh(MetaAddressPayload)`.
- `build_update_instruction` — accounts `[registrant(signer, **ro**), pda(mut)]` (no system
  program; `update` does not mark registrant writable); same data shape.
- `build_close_instruction` — accounts `[registrant(signer,mut), pda(mut)]`; data =
  `CLOSE_DISCRIMINATOR || borsh(scheme_id:u16)` (no payload).
- `MetaAddressPayload` = `{ version:u8, b_spend:[u8;32], b_scan:[u8;32], flags:u8 }`.
- `MetaAddressEntry` (on-chain account) = `{ registrant:Pubkey, scheme_id:u16, bump:u8,
  version:u8, b_spend:[u8;32], b_scan:[u8;32], flags:u8 }`.
- `try_parse_meta_address_entry(data)` — discriminator check + borsh decode.
- `[rpc] fetch_meta_address(rpc, program_id, registrant, scheme_id)` — derives the PDA,
  `get_account`, maps `AccountNotFound` → `Ok(None)`, decodes the rest. Returns `Err` on
  real RPC failures or malformed data.

---

## 13. Error model (`SlntError`)

Single `thiserror` enum, re-exported at the crate root.

| Variant | When | `Display` summary |
|---|---|---|
| `Derivation` | `b_spend` reduces to zero, or HKDF expand fails (Method 2) | key derivation failed (anomalous scalar) |
| `InvalidPoint` | `B_spend` won't decompress, or is small-order | invalid or non-spendable Ed25519 point |
| `InvalidSharedSecret` | X25519 ECDH yields all-zero `S` (sender side) | invalid X25519 shared secret (all zero) |
| `MetaAddressEncode` | bech32m HRP parse / encode failure | meta-address encoding failed |
| `MetaAddressDecode(String)` | bad checksum, wrong HRP, short payload, bad varint, trailing bytes | meta-address decoding failed: {0} |
| `UnsupportedVersion(u8)` | meta-address / sender sees `version != 0x01` | unsupported meta-address version |
| `UnsupportedFlags(u8)` | meta-address / sender sees `flags != 0` | unsupported meta-address flags |
| `MetadataTooLong(usize)` | announcement metadata > 64 bytes | note metadata exceeds 64 bytes |
| `Base58` | base58 decode failure | base58 decode failed |
| `Rpc(String)` | RPC/HTTP/token-program errors (`[rpc]`/`[net]` + SPL sweep) | rpc error: {0} |
| `CloseToMainWallet` | sweep destination/close target equals main wallet (§8.3) | close/rent destination is the main wallet |
| `RelayerTakeTooLarge { take, balance }` | relayer take ≥ swept balance | relayer take exceeds the swept balance |
| `LamportOverflow { amount, rent_buffer }` | `amount + RENT_EXEMPT_MIN` overflows `u64` | lamport amount overflow |
| `NonDeterministicSignature` | the two Method-2 signatures differ (§8.5) | non-deterministic signature — wallet unusable with Method 2 |

---

## 14. Cost & performance

### 14.1 Scan cost per announcement (§5.4, §8.4)

For each observed note the recipient runs:

1. **One X25519 ECDH** `S = b_scan · R` (~30 µs on commodity hardware) — the dominant cost.
2. **One SHA-256** over `len || "slnt-v1-tweak" || S` (32-byte input, ~sub-µs) to compute
   the view tag.
3. **Compare** the view tag against the note's byte.

Only **~1/256** of notes survive the view-tag filter. On a survivor:

4. **One SHA-256** for the tweak `t` (extra view-tag byte appended), and
5. **One Ed25519 scalar-mult** `B_spend + t·G_ed` (~30–50 µs) for the unlabeled candidate,
   **plus one HKDF + scalar-mult per known label index** (`scan_note_candidates`).

So steady-state scan cost is ≈ `N × (ECDH + SHA-256)`, with a scalar-mult amortized over
~256 notes (×(1 + |known_labels|) on a hit). The all-zero-`S` guard runs **before** the
view-tag compare so a crafted low-order `R` cannot force scalar-mults for every recipient
(the primary scan-cost DoS defense, §8.4). Delegated view-only scanning (§5.10) is
exactly steps 1–3.

### 14.2 Rent buffers & sweep economics

- SOL payments over-fund by `RENT_EXEMPT_MIN = 890_880` lamports so the fresh stealth system
  account is valid; the recipient reclaims this on sweep.
- SPL payments cost the sender the stealth ATA's rent (created idempotently); the recipient
  reclaims it via `close_account` on sweep (to a non-main destination).
- Sweeps require a relayer fee payer because the stealth account holds only the rent
  minimum. The relayer is paid `relayer_take` (lamports for SOL sweeps, in-kind tokens for
  SPL sweeps), bounded by `RelayerTakeTooLarge` (`take < balance` for SOL, `take <= amount`
  for SPL).

---

## 15. Testing

The crate ships **~73 unit tests** (`#[test]` count across `src/`, with the `net`-gated
modules compiled in). Per-module matrix:

| Module | Tests | Highlights |
|---|---|---|
| `keys` | 21 | canonical-message byte-exactness & per-network distinctness; Method-2 determinism; **SLIP-0010 ed25519 KAT** (`m`, `m/0'`); HD determinism / sibling-path / account-index separation; meta-address round-trips (labeled + unlabeled); HRP / version / flags rejection; checked-derivation determinism guard; label tweak determinism & distinctness; LEB128 round-trip |
| `recipient` | 5 | **sender↔recipient round-trip**; view-only delegated filter; labeled payment round-trip; unrelated-recipient false-positive bound; low-order `R` ignored |
| `sender` | 5 | deterministic under fixed RNG; distinct per call; rejection of bad version/flags/small-order spend point/zero shared secret |
| `sweep` | 6 | SOL split; main-wallet rejection (SOL+SPL); oversized take; stealth→stealth allowed; SPL transfer/pay/close tags |
| `flows` | 4 | SOL rent buffer + overflow; SPL ATA-then-transfer; NFT = amount 1/decimals 0 |
| `pinboard` | 7 | discriminator re-derivation (post/post_batch/Note); batch borsh round-trip; log-line parse paths |
| `registry` | 11 | discriminator re-derivation (register/update/close/account); account-shapes; PDA determinism & per-(registrant, scheme_id) uniqueness; account parse round-trip |
| `announce` | 7 | from_payment carries R/view_tag; metadata-length rejection; post-instruction discriminator; self-announce timing; dedup; HTTP JSON round-trip; absent-signature omission |
| `stealth_signing` | 3 | sign→verify via `ed25519-dalek`; signature determinism; `public_bytes == scalar·G_ed` |
| `announce_client` `[net]` | 2 | URL joining / trailing-slash trimming |
| `scan_stream` `[net]` | 2 | extract only Note events from mixed log lines; none when no `Program data` |

Two structural invariants are load-bearing:

- The **SLIP-0010 KAT** pins Method 1 to the official ed25519 test vector — any drift in the
  HMAC-SHA512 master/child math fails immediately.
- The **sender↔recipient round-trip** (and labeled variant) proves the shared
  `compute_view_tag`/`compute_tweak` functions reproduce the sender's `P_stealth` and that
  `stealth_scalar · G_ed == stealth_address`, i.e. the recovered scalar actually signs for
  the derived account.
