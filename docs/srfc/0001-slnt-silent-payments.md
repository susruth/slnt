# sRFC-0001: SLNT — Silent Payments for Solana

| | |
|---|---|
| **sRFC** | 0001 |
| **Title** | SLNT: Silent Payments (Stealth Addresses) for Solana |
| **Authors** | susruth (<susruth@susruth.com>) |
| **Status** | Draft |
| **Type** | Standards Track |
| **Category** | Interface / Application |
| **Created** | 2026-05-31 |
| **Requires** | Ed25519, X25519, SHA-256, HKDF-SHA256, bech32m |
| **Reference deployments** | `pinboard`: `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` · `registry`: `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2` |

---

## 1. Summary

SLNT defines a **silent-payment** (stealth-address) standard for Solana. A
recipient publishes a single, reusable **meta-address**. Any sender can pay
that meta-address such that:

- every payment lands at a fresh, distinct on-chain address (a **stealth
  address**) that is itself a valid native Solana wallet;
- only the recipient can recognize which addresses are theirs;
- no observer without the recipient's scan key can link a stealth address to
  the meta-address, nor link two payments to the same meta-address; and
- in the default (decoupled) mode, the payment transaction is
  **indistinguishable** from an ordinary transfer to a fresh address.

SLNT is the Solana analog of Bitcoin Silent Payments (BIP-352) and Ethereum
Stealth Addresses (ERC-5564 / ERC-6538), re-grounded on Solana's account
model, Ed25519/X25519 key types, and rent/relayer economics. It specifies two
permissionless, immutable on-chain programs — an **announcement** program
(`pinboard`) and a **meta-address registry** (`registry`) — plus the
off-chain cryptography that wallets implement.

This document is normative and self-contained: a conforming SLNT v1
implementation can be built from this sRFC alone.

---

## 2. Motivation

Solana addresses are public and reusable. Receiving funds at a static address
exposes the recipient's entire balance and counterparty graph to anyone. The
standard mitigation — a fresh address per payment — pushes an unsolved key-
management and discovery problem onto users: how does a sender learn the next
fresh address without an interactive round trip, and how does the recipient
find and spend funds that arrive at addresses they did not pre-register?

Stealth addresses solve this with non-interactive key agreement: the sender
derives a one-time address from the recipient's static public material plus
fresh ephemeral randomness; the recipient detects and spends it using a scan
key. Bitcoin (BIP-352) and Ethereum (ERC-5564/6538) have standardized this.
Solana has no comparable standard. Existing schemes do not port directly:

- **Bitcoin's** "silent" construction reuses transaction *input* public keys
  as the sender's ephemeral material. Solana transactions have no equivalent
  per-payment input-key set to repurpose, so SLNT must carry an explicit
  ephemeral public key.
- **Ethereum's** stealth address is an opaque account; the stealth address in
  Solana must be a *spendable Ed25519 wallet* and must hold rent, which
  introduces a sweep/relayer problem absent on the EVM.
- Solana's `secp256k1` ≠ native signing curve. The stealth address must be
  Ed25519 to be a usable wallet, while clean ECDH wants X25519.

SLNT takes the proven ideas from both ecosystems and adapts them. It also
improves on the ERC-5564 status quo: by decoupling the on-chain announcement
from the value transfer, the transfer itself carries no protocol marker —
recovering the "silent" property that BIP-352 has on Bitcoin but that
coupled-mode EVM stealth payments lack.

### 2.1 Goals (v1)

1. Support SOL, SPL tokens, and NFTs.
2. Hardware-wallet compatible — no seed-phrase access required.
3. Stronger on-chain unlinkability than coupled-mode ERC-5564.
4. Asset-agnostic at the protocol layer; asset-specific only at the SDK layer.
5. Extensible to future cryptographic schemes (post-quantum, multi-curve)
   without breaking v1 wallets.

### 2.2 Non-goals (v1)

Amount privacy; asset-type privacy; sender anonymity (the sender's fee-payer
is visible); cross-chain unified meta-addresses; standardized encrypted memos;
multi-recipient announcements; and a specific announcement-service economic
model. See §13.

---

## 3. Prior Art and Lineage

SLNT is a deliberate synthesis. The following table maps each SLNT component
to its closest antecedent.

| SLNT component | BIP-352 (Bitcoin) | ERC-5564 / ERC-6538 (Ethereum) | SLNT choice |
|---|---|---|---|
| Curve(s) | secp256k1 | secp256k1 | **Ed25519** spend + **X25519** scan |
| Scan/spend key separation | Yes | Yes (viewing/spending) | Yes |
| Meta-address publishing | Silent payment address (`sp1…`) | Stealth meta-address (`st:eth:0x…`) | bech32m `slnt1…` |
| Sender ephemeral material | Reuses tx **input** keys (no extra data) | Explicit `ephemeralPubKey` | Explicit X25519 `R` |
| On-chain announcement | None (recipient scans all txs) | `Announcer` event (ERC-5564) | `pinboard` event program |
| View tag (fast scan filter) | — | 1 byte (first byte of `metadata`) | 1 byte |
| Per-counterparty labels | Yes (label tweaks) | — | Yes (BIP-352-style) |
| Meta-address registry | — | `ERC-6538` registry contract | `registry` PDA program |
| Transfer ↔ announcement coupling | Implicitly silent | Coupled (announcement = the marker) | **Decoupled** (silent) by default |
| Register-on-behalf (gasless) | — | `registerKeysOnBehalf` | Reserved (§7, future) |

What SLNT takes from each:

- **From ERC-5564:** the asset-agnostic, event-based announcement model; the
  1-byte view tag for cheap scan filtering; and the "publish one meta-address"
  user model.
- **From ERC-6538:** an on-chain registry mapping a main wallet to its
  meta-address, so a sender who knows only the recipient's wallet can discover
  their meta-address.
- **From BIP-352:** recipient-side **labels** (deriving multiple meta-addresses
  from one scan key to tag payment sources), and the **silent** property — the
  payment transaction reveals nothing — which SLNT recovers via decoupled
  announcement.
- **Solana-specific and new:** Ed25519 spend keys so the stealth address is a
  real signable wallet; X25519 scan keys for clean ECDH; rent-aware
  **relayer sweep** economics; and a **decoupled-announce with self-announce
  fallback** flow that has no EVM equivalent.

---

## 4. Terminology

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**,
**SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this
document are to be interpreted as described in RFC 2119.

| Term | Meaning |
|---|---|
| Meta-address | A recipient's reusable, publicly shareable SLNT identity (`slnt1…`), encoding a spend public key and a scan public key. |
| Stealth address | A one-time Ed25519 address derived per payment; a native Solana wallet the recipient controls. |
| Spend key | `(b_spend, B_spend)` — Ed25519 keypair authorizing spends from stealth addresses. |
| Scan key | `(b_scan, B_scan)` — X25519 keypair used to detect incoming payments. View-only. |
| Ephemeral key | `(r, R)` — fresh X25519 keypair the sender generates per payment. |
| View tag | 1-byte hash prefix published with each announcement to reject ~255/256 non-matching candidates cheaply. |
| Announcement | The tuple `(scheme_id, R, view_tag, metadata)` published on `pinboard`. |
| Label | A recipient-chosen tweak producing an alternate meta-address from the same scan key (BIP-352 style). |
| Relayer | A fee-payer that sweeps a stealth address on the recipient's behalf and is compensated from the swept value. |

---

## 5. Specification

### 5.1 Cryptographic primitives

| Primitive | Choice |
|---|---|
| Spend key | Ed25519 |
| Scan key | X25519 |
| Hash | SHA-256 |
| KDF | HKDF-SHA256 |
| Meta-address encoding | bech32m, HRP `slnt` |
| Stealth address on-chain | Ed25519 public key (raw 32 bytes), base58 |

**Group constants.** `G_ed` = Ed25519 base point; `G_x` = Curve25519
(Montgomery) base point; `ℓ` = Ed25519 group order
= `2^252 + 27742317777372353535851937790883648493`.

**Domain-separation tags** (ASCII), included length-prefixed in every hash
input as `H(len(tag) || tag || …)` to make inputs unambiguous:

- `slnt-v1-derive` — wallet key derivation
- `slnt-v1-tweak` — stealth-address tweak and view tag
- `slnt-v1-label` — label tweak derivation
- `slnt-v1-memo` — reserved for metadata encryption (not standardized in v1)

`SC25519_reduce(x)` interprets `x` as a little-endian integer and reduces it
mod `ℓ`. `X25519_clamp(b)` performs the standard clamp:
`b[0] &= 248; b[31] &= 127; b[31] |= 64`.

### 5.2 Key generation and the meta-address

#### 5.2.1 Wallet-independent derivation (hardware-wallet compatible)

Stealth keys **MUST** be derived from a signed canonical message, not from
wallet seed material, because no production wallet exposes seeds to dApps.

The canonical message is the following exact UTF-8 text, with no trailing
newline:

```
Slnt Protocol: Derive Stealth Keys

Version: 1
Network: <Mainnet|Devnet|Testnet>
Warning: Only sign this message in the Slnt wallet or a trusted Slnt integration.
Signing this in any other context will reveal your stealth address scanning ability.
```

`<Mainnet|Devnet|Testnet>` is substituted exactly as written, so keys differ
per network and devnet experiments cannot leak mainnet stealth identity.

```
sig  = WalletSign(canonical_message)         // 64-byte Ed25519 signature
seed = HKDF-SHA256(salt = "slnt-v1-derive",
                   ikm  = sig,
                   info = "spend-and-scan",
                   length = 64)

b_spend_raw = seed[0:32]
b_scan_raw  = seed[32:64]

b_spend = SC25519_reduce(b_spend_raw)         // Ed25519 scalar
b_scan  = X25519_clamp(b_scan_raw)            // X25519 scalar

B_spend = b_spend · G_ed                      // 32-byte compressed Ed25519 point
B_scan  = b_scan  · G_x                       // 32-byte X25519 point
```

If `b_spend` reduces to zero (negligible probability) the implementation
**MUST** abort and surface a derivation error; it **MUST NOT** silently retry.

**Determinism requirement.** Ed25519 signing is deterministic under RFC 8032.
Wallets that produce randomized Ed25519 signatures **MUST NOT** be supported,
because non-deterministic signatures make the stealth identity unrecoverable.

#### 5.2.2 Meta-address encoding

A meta-address is bech32m-encoded with HRP `slnt` over the payload:

| Field | Size | Description |
|---|---|---|
| `version` | 1 byte | Meta-address encoding version. `0x01` for v1. |
| `B_spend` | 32 bytes | Ed25519 spend public key (compressed). |
| `B_scan` | 32 bytes | X25519 scan public key. |
| `label_index` | unsigned LEB128, 1–5 bytes | `0` = unlabeled; `1+` = labelled (§5.2.3). |
| `flags` | 1 byte | Reserved. `0x00` in v1. |

Total payload 67–71 bytes; encoded length ~120–126 characters
(`slnt1…`). `version` is **independent** of the announcement `scheme_id`
(§5.5): the former describes the meta-address bytes, the latter the
cryptographic suite. Implementations **MUST** reject meta-addresses with
`version != 0x01` in v1.

#### 5.2.3 Labels (BIP-352 style)

Labels let a recipient publish multiple meta-addresses from one scan key and
later identify which counterparty paid, without revealing relationships.

```
m_i = SC25519_reduce(
        HKDF-SHA256(salt = "slnt-v1-label",
                    ikm  = b_scan_raw,
                    info = "label-" || varint(i),
                    length = 32))

B_spend_i = B_spend + m_i · G_ed
```

The meta-address for label `i` encodes `B_spend_i` in the `B_spend` field and
`label_index = i`. `label_index = 0` is the unlabeled default
(`B_spend_0 = B_spend`, no tweak). Senders treat the encoded spend key as
opaque; the sender flow is identical regardless of label index.

### 5.3 Stealth address derivation — sender

Given a meta-address `(B_spend_effective, B_scan, label_index)` — where
`B_spend_effective` already incorporates any label tweak and is treated as
opaque by the sender:

```
r = X25519_clamp(secure_random_32_bytes())
R = r · G_x                                            // ephemeral pubkey (32 bytes)

S = X25519(r, B_scan)                                  // shared secret (32 bytes)

view_tag = SHA-256(len("slnt-v1-tweak") || "slnt-v1-tweak" || S)[0]

t = SC25519_reduce(
      SHA-256(len("slnt-v1-tweak") || "slnt-v1-tweak" || S || [view_tag]))

P_stealth = B_spend_effective + t · G_ed               // Ed25519 point
stealth_address = base58(compress(P_stealth))          // Solana address
```

The sender then transfers the asset to `stealth_address` (§5.6) and publishes
the announcement `(scheme_id = 0x0001, R, view_tag, metadata)` (§5.5). The
`metadata` field is opaque to the protocol, **MUST** be ≤ 64 bytes, and **MAY**
carry an implementation-defined encrypted memo keyed by `S` (not standardized
in v1).

### 5.4 Stealth address derivation — recipient

For each observed announcement `(R, view_tag, metadata)`:

```
S_candidate = X25519(b_scan, R)

vt = SHA-256(len("slnt-v1-tweak") || "slnt-v1-tweak" || S_candidate)[0]
if vt != view_tag: continue                            // ~255/256 rejected here

t = SC25519_reduce(
      SHA-256(len("slnt-v1-tweak") || "slnt-v1-tweak" || S_candidate || [view_tag]))

P_candidate = B_spend + t · G_ed                       // unlabeled
for i in known_label_indices:                          // if labels are used
    P_candidate_i = B_spend + (m_i + t) · G_ed

for addr in [P_candidate, P_candidate_1, …]:
    if getBalance(addr) > rent_exempt_min or getTokenAccountsByOwner(addr) nonempty:
        record_payment(addr, label_index_if_matched, R)
```

**Spend-scalar reconstruction** for sweeping (§5.7):

```
p_stealth = (b_spend + t)       mod ℓ      // unlabeled
p_stealth = (b_spend + m_i + t) mod ℓ      // label i
```

`p_stealth · G_ed` **MUST** equal the recorded `P_stealth` (sanity check).
`p_stealth` is the Ed25519 private key of the stealth address.

The view tag bounds scan cost: ~1/256 announcements survive the filter, after
which one scalar-mult per known label index is performed. Implementations
**SHOULD** treat the view-tag filter as the primary defense against scan-cost
DoS (§8.4).

### 5.5 Announcement layer — the `pinboard` program

SLNT announcements are published on **pinboard**, a permissionless, stateless
Solana program that emits opaque tagged notes as Anchor events. Pinboard is
generic (not SLNT-specific) and is SLNT's first consumer. It is the Solana
analog of the ERC-5564 `Announcer`.

**Reference deployment:** `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2`
(immutable, no upgrade authority).

#### 5.5.1 Instructions

```rust
pub fn post(ctx: Context<Post>,
            scheme_id: u16,            // 0x0001 for SLNT v1
            ephemeral_pub: [u8; 32],   // R
            view_tag: u8,
            metadata: Vec<u8>)         // ≤ 64 bytes
    -> Result<()>;

pub fn post_batch(ctx: Context<PostBatch>, entries: Vec<NoteEntry>) -> Result<()>;

pub struct NoteEntry {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}
```

- `metadata.len()` **MUST** be ≤ 64 bytes per entry; exceeding it **MUST**
  fail the instruction (`MetadataTooLong`).
- `post_batch` **MUST** contain ≥ 1 entry (`EmptyBatch`); practical size is
  bounded by the transaction compute budget (~50 entries).
- The program holds **no** state; notes are emitted via events only.
- `scheme_id` is recorded, not whitelisted. SLNT v1 clients **MUST** process
  only `scheme_id = 0x0001` and **MUST** ignore others.

#### 5.5.2 Event format

`post` emits one `Note`; `post_batch` emits one per entry.

```
Note
  discriminator : 8 bytes (Anchor)
  scheme_id     : u16  (LE)
  ephemeral_pub : [u8; 32]
  view_tag      : u8
  metadata_len  : u32  (LE; borsh Vec<u8> length prefix)
  metadata      : metadata_len bytes
```

Total 47–111 bytes. The Anchor 0.31 IDL camelCases the event name; log parsers
**SHOULD** match the string `note`. The fixed-header + variable-tail layout is
intentionally Merkle-tree-friendly for a future compressed-account migration
(§11) without changing the bytes scanners parse.

### 5.6 Meta-address registry — the `registry` program

The **registry** maps a main Solana wallet pubkey to its SLNT meta-address,
closing the discovery gap. It is the Solana analog of ERC-6538. It is
**OPTIONAL**: senders may always share meta-addresses off-chain (QR, profile,
DM). It is deployed independently of pinboard and shares no code or state.

**Reference deployment:** `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2`
(immutable, no upgrade authority, no admin).

#### 5.6.1 PDA and account

One PDA per `(registrant, scheme_id)`:

```
seeds = [ b"meta", registrant.key().as_ref(), &scheme_id.to_le_bytes() ]
```

```rust
#[account]
pub struct MetaAddressEntry {
    pub registrant: Pubkey,   // 32
    pub scheme_id:  u16,      // 2
    pub bump:       u8,       // 1
    pub version:    u8,       // 1  (meta-address encoding version)
    pub b_spend:    [u8; 32], // 32 (Ed25519 spend pubkey)
    pub b_scan:     [u8; 32], // 32 (X25519 scan pubkey)
    pub flags:      u8,       // 1  (reserved, 0x00)
}
```

Fixed 101-byte payload (109 with discriminator). A sender derives the PDA from
the recipient's wallet and a single `getAccountInfo` retrieves it — no scans.

#### 5.6.2 Instructions and validation

Three registrant-signed instructions; no admin path:

- `register(scheme_id, payload)` — creates the PDA; registrant pays rent;
  fails if it already exists.
- `update(scheme_id, payload)` — overwrites in place; `has_one = registrant`
  ensures only the owner can update; fails if the PDA does not exist.
- `close(scheme_id)` — closes the PDA, returning rent to the registrant; the
  pair may be re-registered later.

```rust
pub struct MetaAddressPayload { pub version: u8, pub b_spend: [u8;32], pub b_scan: [u8;32], pub flags: u8 }
```

Validation, applied to `register` and `update`:

| Rule | Error |
|---|---|
| `scheme_id != 0` | `InvalidSchemeId` |
| `version == 0x01` | `InvalidVersion` |
| `flags == 0` | `InvalidFlags` |

The registry **MUST** accept only unlabeled meta-addresses (`label_index` is
not on the wire; the payload has no such field). Publishing a labelled
meta-address would leak relationship metadata and is therefore impossible by
construction. The registry **MUST NOT** be required to validate that `b_spend`
/ `b_scan` are valid curve points (garbage fails at sender derivation, never
risks funds). Each instruction emits a corresponding event
(`MetaAddressRegistered` / `MetaAddressUpdated` / `MetaAddressClosed`) so
indexers can maintain a `(registrant, scheme_id) → entry` map without
`getProgramAccounts`.

Sender prelude (OPTIONAL): given a recipient wallet `A`, look up
`(A, 0x0001)`; on hit, decode the meta-address and proceed with §5.3; on miss,
fall back to off-chain meta-address entry. The registry is an enhancement,
never a hard dependency.

### 5.7 Sender transaction flows

In v1 the sender **MUST** default to **decoupled mode** (§5.8): the on-chain
transfer contains only the asset movement and any required account creation —
no SLNT instruction — so it is indistinguishable from a normal transfer to a
fresh address.

- **SOL:** `SystemProgram::transfer { to: P_stealth, lamports: amount +
  rent_buffer }`, where `rent_buffer` is the rent-exempt minimum (890,880
  lamports) so the fresh system account is valid.
- **SPL token:** create the recipient ATA for `(P_stealth, mint)` (sender pays
  ATA rent ≈ 2,039,280 lamports), then `spl_token::transfer` into it. The mint
  is necessarily visible on-chain; only the recipient *identity* is hidden,
  not the asset type.
- **NFT (Metaplex / Token-2022):** same shape with the NFT mint, plus
  program-mandated extra accounts (token records, rule sets) for pNFTs and
  transfer-hook execution for Token-2022. Implementations **MUST** construct
  the full account set via the relevant SDK and **SHOULD** warn that a
  publicly-tied NFT can deanonymize the sender (§8.3).

### 5.8 Announcement modes

#### 5.8.1 Decoupled announce (default)

1. The wallet computes `(P_stealth, R, view_tag, metadata)`.
2. The wallet submits the asset-transfer tx (§5.7) — containing no SLNT
   instruction.
3. The wallet submits `(scheme_id, R, view_tag, metadata)` to an announcement
   service, which publishes it on pinboard in a tx the service pays for.

The service learns only the announcement tuple. Without `b_scan` it **cannot**
determine the recipient, `P_stealth`, or the sender's identity.

#### 5.8.2 Self-announce fallback

To prevent stranded funds if a service fails or censors, after step 3 the
wallet **SHOULD** start a timer (RECOMMENDED `T` = 60 s) and watch pinboard
logs for a note with matching `R`. If none appears before `T`, the wallet
**MUST** publish `post(scheme_id, R, view_tag, metadata)` itself, paid from the
sender's wallet. Recipients **MUST** deduplicate announcements by `R` (a
service+sender race may publish two).

#### 5.8.3 Coupled mode (escape hatch)

Wallets **MAY** publish the announcement in the same tx as the transfer when
no service is reachable. Coupled mode makes the tx visibly an SLNT payment,
trading on-chain unlinkability for single-tx atomicity. Wallets **MUST**
surface this trade-off before using it.

#### 5.8.4 Announcement-service HTTP protocol

A minimal shape any service **MAY** implement (pricing/auth/SLA out of scope):

```
POST /announce  { scheme_id, ephemeral_pub(b58), view_tag, metadata(b58), payment_proof }
             ->  { queued, batch_id, expected_slot }
GET  /announce/status/{batch_id}  ->  { status: pending|confirmed|failed, tx_signature? }
```

### 5.9 Recipient sweep

A stealth address holds value but only the rent-exempt minimum in SOL, so it
cannot pay its own fees. The recipient uses Solana's fee-payer separation: a
**relayer** signs as fee payer and is compensated from the swept value.

- **SOL sweep:** two `SystemProgram::transfer`s from the stealth account — one
  paying the relayer (`relayer_margin + rent_reserve`), one paying the
  recipient destination (`balance − relayer_take`). The account reaches zero
  and is reclaimed by the runtime.
- **SPL sweep:** transfer the token to the destination ATA, pay the relayer
  in-kind from the same token, then `CloseAccount` the stealth ATA. Relayer
  compensation is paid in the swept token; the relayer fronts the SOL fee.

**Close-to-relayer (critical privacy rule).** Rent reclaimed by closing the
stealth account or ATA **MUST** be sent to the relayer (or to another stealth
address the recipient controls), **never** to the recipient's main wallet —
doing so would create a direct `stealth → main` link. Wallets **MUST** refuse
to build a sweep whose close destination is the recipient's main wallet.

**Stealth-to-stealth** is not a separate primitive: it is a sweep whose
destination is a stealth address derived from another meta-address (§5.9 rules
unchanged), preserving unlinkability across hops.

### 5.10 Discovery and scanning

- **Self-scan via logs (baseline, REQUIRED).** Wallets **MUST** support
  scanning by subscribing to pinboard program logs (`logsSubscribe`) and
  backfilling via `getSignaturesForAddress` + `getTransaction`. Default RPCs
  prune logs within hours, so wallets **SHOULD** offer an alternative for
  recipients offline beyond ~24 h.
- **Indexer (OPTIONAL).** Any party **MAY** host an indexer that retains
  announcements and serves them over HTTP
  (`GET /announcements?since_slot&limit`). The indexer receives **no** scan
  keys; matching is recipient-local. Polling slot ranges leaks nothing about
  which announcements matched.
- **View-key delegated scanning (OPTIONAL, opt-in).** A recipient **MAY**
  publish `b_scan_raw` (the pre-clamp scan material) to a trusted scanner that
  runs the ECDH + view-tag filter and returns only matches. Scan material is
  **view-only**: it cannot sign, cannot recover `b_spend`, and cannot spend.
  Implementations **MUST** warn that the scanner learns all incoming-payment
  metadata, and **SHOULD** discourage view-key delegation for high-stakes
  privacy.

### 5.11 Versioning and scheme IDs

Two independent axes:

1. **Meta-address encoding version** (`version: u8`, §5.2.2) — describes the
   meta-address bytes. v1 = `0x01`.
2. **Cryptographic scheme ID** (`scheme_id: u16`, §5.5) — describes the
   derivation suite. v1 = `0x0001` (Ed25519 spend, X25519 scan, SHA-256,
   BIP-352-style labels).

| Field | Value | Meaning |
|---|---|---|
| `version` | `0x00` | Reserved (invalid) |
| `version` | `0x01` | v1 encoding (this sRFC) |
| `version` | `0x02`–`0xEF` | Reserved (future canonical) |
| `version` | `0xF0`–`0xFF` | Experimental |
| `scheme_id` | `0x0000` | Reserved (invalid) |
| `scheme_id` | `0x0001` | v1 scheme |
| `scheme_id` | `0x0002`–`0xFEFF` | Reserved (future canonical) |
| `scheme_id` | `0xFF00`–`0xFFFF` | Experimental |

v1 wallets **MUST** reject `version != 0x01` and **MUST** ignore
`scheme_id != 0x0001`, and **MAY** show "unsupported scheme — please update".

---

## 6. Rationale

- **Ed25519 spend + X25519 scan.** The stealth address must be a spendable
  native Solana wallet, which forces Ed25519 for spends. ECDH on the
  twisted-Edwards curve is error-prone (cofactor, clamping), so the scan path
  uses X25519, the Montgomery form purpose-built for clean Diffie-Hellman. The
  two keys are independent and serve different trust tiers (scan = view-only).
- **Decoupled by default.** Coupling the announcement to the transfer (the
  ERC-5564 status quo) makes every stealth payment self-identifying on-chain.
  Decoupling restores BIP-352's silent property: the transfer is an ordinary
  transfer. The self-announce fallback removes the liveness dependency that
  decoupling would otherwise introduce.
- **Two programs, not one.** Announcement (ephemeral, high-volume) and registry
  (long-lived key material) have different lifecycles, threat surfaces, and
  state models. Keeping pinboard stateless and generic lets other protocols
  reuse it; keeping the registry optional means privacy-maximizing users pay no
  metadata cost.
- **bech32m over base58.** Stronger error detection for a long, hand-shareable
  identity string, matching the direction BIP-352 (`sp1…`) took.
- **1-byte view tag.** Directly from ERC-5564: it rejects ~255/256 candidates
  after one ECDH + one SHA-256, before any scalar multiplication, making the
  scan loop cheap enough to run on commodity hardware and structurally
  defusing scan-cost spam.
- **Wallet-signature key derivation.** Hardware wallets never expose seeds;
  deriving from a deterministic signed message is the only portable path to
  stealth keys, and binding the network name into the message isolates
  devnet from mainnet identities.

---

## 7. Backwards Compatibility

SLNT introduces new programs and an off-chain key scheme; it changes no
existing Solana behavior and has no prior on-chain version to break.

- **Sender transactions** are ordinary transfers; existing wallets, explorers,
  and programs interact with stealth addresses as with any other address.
- **Registry is optional**; its absence degrades gracefully to off-chain
  meta-address sharing.
- **Forward compatibility** is built in via the two version axes (§5.11). New
  schemes add a `scheme_id` and coexist; a new meta-address *encoding* (e.g.,
  multi-curve cross-chain keys) requires a new immutable registry deployment,
  with clients trying the newest supported scheme first and falling back.
- **`register_on_behalf`** (gasless registration, the ERC-6538
  `registerKeysOnBehalf` analog) is reserved: a future instruction can be added
  without altering the existing ones.

---

## 8. Security Considerations

### 8.1 Guarantees (observer without `b_scan`)

- Cannot link a stealth address to a meta-address.
- Cannot link two stealth addresses from the same meta-address.
- In decoupled mode, cannot identify a transfer as an SLNT payment.
- Even with the announcement event visible, cannot match `R` to a transfer
  without `b_scan`.

### 8.2 Non-guarantees

Amounts and asset types are visible on-chain. Sender anonymity is **not**
provided — the sender's fee-payer is public; senders needing anonymity
**SHOULD** use a dedicated funding wallet or an additional privacy layer.
Timing correlation by an adversary watching both the sender's network and the
chain is out of scope.

### 8.3 Deanonymization risks and required mitigations

- **Rent-close link (recipient).** Closing a stealth account to the main
  wallet links them; wallets **MUST** enforce close-to-relayer (§5.9).
- **NFT identifiability (sender/recipient).** A publicly-tied NFT is its own
  deanonymizer; wallets **MUST** warn before sending/sweeping such NFTs.
- **Fee-payer clustering (sender).** Repeated stealth payments from one
  funding wallet are linkable to each other; wallets **SHOULD** advise
  dedicated funding wallets.
- **Service correlation (sender).** Authenticating to an announcement service
  from a known identity lets the service link the sender to each `R` (not to
  the recipient).
- **View-key scanner compromise (recipient).** A delegated scanner sees all
  incoming-payment metadata; it can never spend (§5.10). Use sparingly.
- **Consolidation clustering (recipient).** Sweeping many stealth receipts to
  one destination creates a cluster point; recipients **SHOULD** vary sweep
  destinations or chain stealth-to-stealth.

### 8.4 Spam / DoS

Pinboard is permissionless, so an attacker can post garbage to inflate scan
work. The 1-byte view tag is the structural defense: each garbage note costs a
scanner one X25519 ECDH (~30 µs) + one SHA-256. At ~5,000 lamports per
announcement, 100M spam notes cost the attacker ~$100k for ~50 CPU-minutes of
scan work per recipient — annoying, not crippling. v1 adds no further
mitigation; future schemes **MAY** attach an anti-spam stamp (e.g., a small
fixed burn per announcement) if attack patterns warrant.

### 8.5 Key-derivation integrity

Stealth identity recoverability depends entirely on deterministic Ed25519
signing of the canonical message (§5.2.1). Randomized signers **MUST NOT** be
supported. The canonical message warns the user that signing it elsewhere
exposes scan ability; integrations **MUST** present it as a derivation step,
not a generic authentication signature.

---

## 9. Reference Implementation

The reference implementation lives in this repository and is **not** part of
the normative standard; it bootstraps the ecosystem:

- **`programs/pinboard`** — the on-chain announcement program (`post`,
  `post_batch`, `Note` event). Deployed at
  `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2`.
- **`programs/registry`** — the meta-address registry (`register`, `update`,
  `close`, events). Deployed at
  `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2`.
- **`crates/slnt-sdk`** — Rust SDK: canonical-message key derivation, bech32m
  meta-address encode/decode with labels, sender stealth-address derivation,
  recipient scan + spend-scalar reconstruction, stealth signing, and registry
  PDA / fetch helpers.

Planned: a TypeScript wallet SDK, a reference indexer (§5.10), a reference
announcement service (§5.8.4), and a `slnt` CLI.

Conformance: an implementation is SLNT-v1-conforming if it produces and
consumes meta-addresses, announcements, and stealth addresses byte-compatible
with §5, processes `scheme_id = 0x0001` / `version = 0x01`, supports self-scan
via logs (§5.10), and enforces the close-to-relayer rule (§5.9) and the
deterministic-signing requirement (§5.2.1).

---

## 10. Internal design references

This sRFC consolidates the following internal design documents, which retain
the deepest byte-level detail, derivations, and cost analyses:

- `docs/superpowers/specs/2026-05-20-umbra-solana-stealth-payments-v1-design.md`
  — core protocol.
- `docs/superpowers/specs/2026-05-26-umbra-registry-program-design.md`
  — registry program.
- `docs/superpowers/specs/2026-05-20-umbra-sdk-rust-design.md` — SDK shape.

(Filenames retain their original date-stamped slugs.)

---

## 11. Future work

Cross-chain unified meta-addresses (new encoding `version`); post-quantum
scheme (`scheme_id` slot reserved); multi-recipient announcements; standardized
encrypted-memo scheme keyed by `S` (`slnt-v1-memo` reserved); gasless
`register_on_behalf`; Light Protocol compressed-account migration for pinboard
(event layout already forward-compatible); and a standardized relayer-discovery
/ pricing RPC.

---

## 12. Open Questions

1. **Canonical deploy authority.** Both programs are intended to be immutable
   (no upgrade authority) with vanity `SLNT…` prefixes for on-sight
   recognition. Final mainnet addresses and the renounce procedure are TBD.
2. **HRP registration.** The bech32m HRP `slnt` should be registered against
   SLIP-0173 to avoid collision.
3. **Relayer discovery/pricing.** Whether v1.1 should standardize a relayer
   quote RPC.
4. **Encrypted-metadata standard.** Whether to standardize a memo scheme keyed
   by `S` (`slnt-v1-memo`) in v1.1.
5. **SIMD track.** Whether and when to promote this sRFC to a formal SIMD.

---

## 13. Out of scope (v1)

Amount privacy; asset-type privacy; sender anonymity; cross-chain meta-
addresses; stealth-to-stealth as a distinct primitive; post-quantum schemes;
multi-recipient announcements; standardized encrypted memos; announcement-
service economics; on-chain anti-spam stamps; and compressed-account
migration.

---

## Copyright

This document is placed under Apache-2.0, consistent with the repository
license.
