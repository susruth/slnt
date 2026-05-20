# Umbra: Stealth Payments for Solana — v1 Design Spec

**Date:** 2026-05-20
**Status:** Draft for review
**Scope:** Protocol-level design for stealth payments on Solana (SOL, SPL tokens, NFTs)

---

## 1. Overview

Umbra is a stealth-payment protocol for Solana. It is the Solana analog of
ERC-5564 (Ethereum) and BIP-352 (Bitcoin silent payments), adapted to
Solana's account model, key types, and rent economics.

**What it enables:** A recipient publishes a single static *meta-address*.
Anyone can send them funds such that:

- Every payment lands at a different on-chain address (the *stealth address*)
- Only the recipient can identify which addresses belong to them
- An observer cannot link any stealth address to the recipient's meta-address
- An observer cannot link payments from the same recipient's meta-address to
  each other

**Design lineage:**

- **From ERC-5564:** asset-agnostic announcer event, 1-byte view tag, meta-address
  publishing model
- **From BIP-352:** per-sender labels (recipient-side payment-source tagging)
- **Solana-specific:** Ed25519 spend keys (because the stealth address must
  itself be a valid Solana wallet), X25519 scan keys (for clean ECDH),
  rent/relayer economics, decoupled announcement mode (better unlinkability
  than coupled-mode EVM equivalents)

**Goals (v1):**

1. Support SOL, SPL tokens, and NFTs
2. Hardware-wallet compatible (no seed-phrase access required)
3. Better on-chain unlinkability than the ERC-5564 status quo
4. Asset-agnostic at the protocol layer; asset-specific at the SDK layer
5. Extensible to future cryptographic schemes (post-quantum, multi-curve)
   without breaking v1 wallets

**Non-goals (v1):**

- Cross-chain stealth (unified EVM ↔ Solana meta-address)
- Stealth-to-stealth as a separate primitive (it falls out of the sweep flow)
- Post-quantum schemes (future scheme_id slot reserved)
- Multi-recipient announcements (one R fanning out to N recipients)
- Privacy-preserving bridges
- Custom encrypted-metadata standards beyond an opaque bytes field
- A specific announcement-service economic model (the spec defines the
  protocol shape; pricing and payment mechanics are deferred to v1.1+)

---

## 2. Cryptographic Primitives

| Primitive | Choice | Notes |
|---|---|---|
| Spend key | Ed25519 | Stealth address must be signable as a native Solana wallet |
| Scan key  | X25519  | Clean ECDH; sidesteps Ed25519 cofactor/clamping issues |
| Hash      | SHA-256 | Universal, hardware-accelerated, in every wallet env |
| KDF       | HKDF-SHA256 | Standard, well-analyzed |
| Address encoding | bech32m | Stronger error detection than base58 |
| Stealth address on-chain | Ed25519 public key (raw 32 bytes), base58-encoded | Native Solana address |

**Group constants:**

- `G_ed`  = Ed25519 base point
- `G_x`   = X25519 base point (Curve25519 base point in Montgomery form)
- `ℓ`     = Ed25519 group order = 2^252 + 27742317777372353535851937790883648493

**Domain separation tags (used in all hash inputs):**

- `"umbra-v1-derive"` — wallet key derivation
- `"umbra-v1-tweak"` — stealth address tweak
- `"umbra-v1-label"` — label tweak derivation
- `"umbra-v1-memo"` — metadata encryption (if used)

All tags are ASCII bytes, included as a length-prefixed component to prevent
ambiguity:
`H(tag_len || tag || ... inputs ...)`.

---

## 3. Key Generation and Meta-Address

### 3.1 Wallet-key independence (HW-wallet compatible)

Stealth keys are derived deterministically from a signed canonical message,
not from the wallet's seed. This is required because no production wallet
exposes seed material to dApps, and we need hardware-wallet compatibility.

**Canonical message (exact UTF-8, no trailing newline):**

```
Umbra Protocol: Derive Stealth Keys

Version: 1
Network: <Mainnet|Devnet|Testnet>
Warning: Only sign this message in the Umbra wallet or a trusted Umbra integration.
Signing this in any other context will reveal your stealth address scanning ability.
```

`<Mainnet|Devnet|Testnet>` is substituted exactly as written. Different
networks produce different keys, so devnet experimentation cannot leak
mainnet stealth identity.

**Derivation:**

```
sig = WalletSign(canonical_message)    // 64-byte Ed25519 signature
ikm = sig

seed = HKDF-SHA256(
  salt = "umbra-v1-derive",
  ikm  = ikm,
  info = "spend-and-scan",
  length = 64
)

b_spend_raw = seed[0:32]
b_scan_raw  = seed[32:64]

b_spend = SC25519_reduce(b_spend_raw)   // reduce mod ℓ to obtain Ed25519 scalar
b_scan  = X25519_clamp(b_scan_raw)      // standard X25519 clamping

B_spend = b_spend · G_ed                // Ed25519 point, 32 bytes compressed
B_scan  = b_scan  · G_x                 // X25519 point, 32 bytes
```

**`SC25519_reduce`** is interpret-as-little-endian-integer then mod ℓ. If the
result is zero (negligible probability), abort and surface a derivation
error to the user (do not silently retry — the user's wallet has produced
an anomalous signature that should be investigated).

**`X25519_clamp`** is the standard clamping: `b[0] &= 248; b[31] &= 127; b[31] |= 64;`.

**Determinism caveat:** Ed25519 signatures are deterministic by RFC 8032
when the wallet implements it correctly, which all standard Solana wallets
do. If a wallet ever produces non-deterministic Ed25519 signatures, the
user's stealth identity becomes non-recoverable. Wallets that use
randomized Ed25519 signing MUST NOT be supported by Umbra-compatible
clients.

### 3.2 Meta-address encoding

Meta-addresses are bech32m-encoded with HRP `umbra` and the following
payload:

| Field | Size | Description |
|---|---|---|
| `version` | 1 byte | Meta-address encoding version. `0x01` for v1. |
| `B_spend` | 32 bytes | Ed25519 spend public key (compressed) |
| `B_scan`  | 32 bytes | X25519 scan public key |
| `label_index` | unsigned LEB128 (1-5 bytes) | `0` = unlabeled; `1+` = labelled (see §3.3) |
| `flags` | 1 byte | Reserved. `0x00` in v1. |

Total payload: 67-71 bytes (66 fixed bytes + 1-5 varint bytes for the label
index). Bech32m-encoded length: 120-126 characters (HRP `umbra` + separator
`1` + 108-114 data chars + 6-char checksum).
Example unlabeled: `umbra1qq...` (label_index = 0).

**Varint encoding:** unsigned LEB128 (continuation-bit format used by
DWARF and protobuf). Chosen for ubiquity in TypeScript/Rust ecosystems
and unambiguous parsing.

**`version: u8`** is the *encoding* version of the meta-address. It is
*independent* of the `scheme_id` field in announcements (§6). Encoding
version describes the bytes of the meta-address; scheme_id describes the
cryptographic suite used to derive stealth addresses. v2 schemes can change
either axis independently.

### 3.3 Labels (BIP-352 style)

Labels allow a recipient to publish multiple meta-addresses derived from the
same scan key but with different effective spend keys. Use cases: give
different meta-addresses to different counterparties (employer, friends,
public bio), then identify which counterparty paid you without revealing
the relationship.

**Label tweak derivation:**

```
m_i = SC25519_reduce(
  HKDF-SHA256(
    salt = "umbra-v1-label",
    ikm  = b_scan_raw,
    info = "label-" || varint(i),
    length = 32
  )
)
```

**Labelled spend public key:**

```
B_spend_i = B_spend + m_i · G_ed
```

The meta-address for label `i` encodes `B_spend_i` (not `B_spend`) in the
`B_spend` field, with `label_index = i` in the payload.

**`label_index = 0`** is reserved as the unlabeled default (no tweak applied,
`B_spend_0 = B_spend`). Labels `1+` are user-defined.

Senders do not know they are interacting with a labelled address. The
sender flow is identical regardless of label index.

---

## 4. Stealth Address Derivation — Sender

Given a recipient meta-address with `(B_spend_effective, B_scan, label_index)`:

**Note:** `B_spend_effective` is the spend public key as encoded in the
meta-address. For label_index = 0 it equals the base `B_spend`; for
label_index ≥ 1 it already incorporates the label tweak `m_i · G_ed`. The
sender does NOT need to know `label_index` for the derivation; they treat
the encoded `B_spend_effective` as opaque.

```
// 1. Generate ephemeral X25519 keypair
r = secure_random_32_bytes()
r = X25519_clamp(r)
R = r · G_x                                   // ephemeral public key, 32 bytes

// 2. ECDH against recipient's scan pubkey
S = X25519(r, B_scan)                         // 32-byte shared secret

// 3. View tag (first byte of H(S))
view_tag = SHA-256(
  len("umbra-v1-tweak") || "umbra-v1-tweak" || S
)[0]

// 4. Tweak scalar for stealth-address derivation
t = SC25519_reduce(
  SHA-256(
    len("umbra-v1-tweak") || "umbra-v1-tweak" || S || [view_tag]
  )
)

// 5. Stealth address (Ed25519 public point)
P_stealth = B_spend_effective + t · G_ed

// 6. Solana address bytes are P_stealth's compressed encoding (32 bytes)
stealth_address = base58(compress(P_stealth))
```

Sender then sends the asset to `stealth_address` and publishes the
announcement tuple `(scheme_id = 0x0001, R, view_tag, metadata)`.

The `metadata` field is opaque to the announcer. v1 wallets MAY use it for
an encrypted memo (encryption keyed by `S`), but this is not standardized
in v1. Max length: 64 bytes.

---

## 5. Stealth Address Derivation — Recipient

For each announcement `(R, view_tag, metadata)` the recipient observes:

```
// 1. ECDH using recipient scan private key
S_candidate = X25519(b_scan, R)

// 2. Fast view-tag filter (rejects ~255/256 of false positives)
vt_candidate = SHA-256(
  len("umbra-v1-tweak") || "umbra-v1-tweak" || S_candidate
)[0]
if vt_candidate != view_tag:
    continue  // not for us, skip

// 3. Compute tweak (only for view-tag-matched candidates)
t = SC25519_reduce(
  SHA-256(
    len("umbra-v1-tweak") || "umbra-v1-tweak" || S_candidate || [view_tag]
  )
)

// 4. Candidate stealth address (try unlabeled first)
P_candidate = B_spend + t · G_ed
candidate_address = base58(compress(P_candidate))

// 5. If recipient has labels, also try each labelled variant on view-tag hit
for i in known_label_indices:
    P_candidate_i = B_spend + (m_i + t) · G_ed
    // equivalently: (B_spend + m_i · G_ed) + t · G_ed
    candidate_address_i = base58(compress(P_candidate_i))

// 6. Look up on-chain balance / token accounts at each candidate address
for addr in [candidate_address, candidate_address_1, ...]:
    sol_balance = getBalance(addr)
    token_accounts = getTokenAccountsByOwner(addr)
    if sol_balance > rent_exempt_minimum or token_accounts is non-empty:
        // This is a real payment to us
        record_payment(addr, label_index_if_matched, R)
```

**Spend scalar reconstruction** (when recipient sweeps the stealth address):

```
// For unlabeled receipts:
p_stealth = (b_spend + t) mod ℓ

// For labelled receipts (label index i):
p_stealth = (b_spend + m_i + t) mod ℓ
```

`p_stealth · G_ed` must equal `P_stealth` (sanity check). The recipient
uses `p_stealth` as the Ed25519 private key for the stealth address.

**Scan loop performance:** With a 1-byte view tag, ~1/256 announcements
pass the filter. For each pass, the recipient does one scalar mult per
known label index. A recipient with 10 labels processing 1M announcements
does ~256k expensive ops total (still <1 minute on commodity hardware).

---

## 6. Announcer Program

A single deployed Solana program. Anchor IDL or equivalent. Permissionless,
no admin keys.

### 6.1 Instructions

```rust
pub fn announce(
    ctx: Context<Announce>,
    scheme_id: u16,        // 0x0001 for v1
    ephemeral_pub: [u8; 32], // R
    view_tag: u8,
    metadata: Vec<u8>,     // max 64 bytes
) -> Result<()>

pub fn announce_batch(
    ctx: Context<AnnounceBatch>,
    entries: Vec<AnnouncementEntry>,
) -> Result<()>

pub struct AnnouncementEntry {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}
```

**Validation:**

- `scheme_id` is recorded but not validated against a whitelist. v1 clients
  only process `0x0001`. Future schemes will be added by client updates.
- `metadata.len()` ≤ 64 bytes per entry. Exceeding this causes the
  instruction to fail.
- `announce_batch` caps total entries at the tx compute-unit budget allows
  (practical limit: ~50 entries per tx).
- No on-chain storage of announcements. State is emitted via Anchor events
  / `sol_log_data`.

### 6.2 Event format

Each `announce` invocation emits exactly one event. `announce_batch` emits
one event per entry.

```
Event: UmbraAnnouncement
  - discriminator: 8 bytes (Anchor convention)
  - scheme_id:     u16   (2 bytes, little-endian)
  - ephemeral_pub: [u8; 32]
  - view_tag:      u8
  - metadata_len:  u16   (length of metadata, 0-64)
  - metadata:      bytes (metadata_len bytes)
```

Total event size: 45-109 bytes.

### 6.3 Cost

- Single `announce`: ~5,000 lamports base tx fee. No rent (no state account).
- `announce_batch` of N entries: still ~5,000-10,000 lamports for the tx
  (depending on CU consumption). Marginal cost per announcement amortizes
  toward ~100-200 lamports at N=50.

### 6.4 Forward compatibility with compressed accounts

The event byte layout (fixed-size header + variable metadata) is
Merkle-tree-friendly. A future v2 announcer can write the same payload as
a Light Protocol compressed account commitment without changing the
serialization format that scanners and indexers parse.

---

## 7. Sender Transaction Flows

In v1 the sender ALWAYS uses **decoupled mode** as the default. The
sender's on-chain transaction contains only the asset transfer (and any
required account creation). The announcement is published in a separate
transaction by an announcement service, or by the sender's own wallet as a
fallback (§8).

### 7.1 SOL transfer

```
Tx contains:
  - SystemProgram::transfer {
      from: sender,
      to: P_stealth,
      lamports: amount + rent_buffer
    }

  rent_buffer = 890_880 lamports (rent-exempt minimum for a system account)
              + optional fee_headroom (0 in pure decoupled mode; the
                relayer covers sweep fees)
```

To an observer the tx looks identical to any other native SOL transfer to a
fresh address.

### 7.2 SPL token transfer

```
Tx contains:
  - CreateAssociatedTokenAccount {
      payer: sender,
      owner: P_stealth,
      mint: token_mint
    }
    // sender pays ATA rent (~2,039,280 lamports)

  - spl_token::transfer {
      from: sender_ata,
      to: stealth_ata,
      amount: amount
    }
```

`stealth_ata` is the deterministic ATA derived from `(P_stealth, mint)`.

**Privacy note:** The ATA creation reveals the mint on-chain. An observer
who later finds the ATA (via `getTokenAccountsByOwner(P_stealth)`) learns
which SPL token was transferred. This is unavoidable for SPL transfers —
the mint must be visible for the SPL program to dispatch correctly. The
*identity* of the recipient still hides; only the asset type is revealed.

### 7.3 NFT transfer (Metaplex Token Metadata and Token-2022)

Same shape as SPL, but the mint is the NFT mint. Additional considerations:

- **Metaplex pNFTs / programmable NFTs:** transfers require additional
  accounts (token records, authorization rule sets, metadata account).
  These are program-mandated and add 3-5 extra accounts to the tx.
  Implementations MUST construct the full account set via Metaplex SDK
  helpers.
- **Token-2022 with transfer hooks:** the hook program executes on
  transfer. If the hook reveals sender data (e.g., royalty distribution to
  a known creator address), this is a leak. SDKs SHOULD warn the user
  before sending such tokens through Umbra.
- **Identifiability:** A 1-of-1 NFT is intrinsically identifiable. If the
  NFT was publicly tied to the sender (e.g., minted by sender or
  previously held by sender), sending it via Umbra leaks the sender
  identity even though the recipient is hidden. Wallets MUST surface a
  warning when sending an NFT the sender has previously held publicly.

### 7.4 Asset-agnostic structure

In all three cases the sender's transaction is a normal Solana transfer to
`stealth_ata` or `P_stealth`. No Umbra-specific instruction appears in the
sender's tx. The Umbra protocol is invisible at the on-chain level until
the announcement is published.

---

## 8. Sender Mode: Decoupled Announce with Self-Announce Fallback

### 8.1 Default flow (decoupled)

1. Sender's wallet computes `(P_stealth, R, view_tag, metadata)`.
2. Sender's wallet submits the asset-transfer tx to the chain (§7).
   This tx does NOT contain any Umbra instruction.
3. Sender's wallet submits `(scheme_id, R, view_tag, metadata)` to an
   announcement service (§8.3).
4. The service publishes the announcement on-chain via `announce(...)` or
   `announce_batch(...)` in a tx paid by the service.

### 8.2 Self-announce fallback

To prevent stranded funds if a service fails or censors:

1. After step 8.1(3), the sender's wallet starts a timer with timeout `T`
   (recommended default: 60 seconds). The wallet retains the announcement
   payload in local state.
2. The wallet subscribes to announcer-program logs and watches for an
   announcement with matching `ephemeral_pub = R`.
3. If such an announcement is observed before timeout `T`: success, the
   wallet discards the payload.
4. If no matching announcement is observed within `T`: the wallet
   constructs a `announce(scheme_id, R, view_tag, metadata)` tx itself,
   paid from the sender's wallet, and submits it.

Idempotency: if the service and the sender both eventually announce (a
race after the timeout), the recipient observes two announcements for the
same `R`. Recipient scan logic deduplicates by `R`.

### 8.3 Announcement service protocol

A minimal HTTP protocol that any service can implement. The spec defines
the request/response shape; pricing, authentication, and reliability SLAs
are out of scope for v1.

```
POST /announce
Content-Type: application/json
Body: {
  "scheme_id": 1,
  "ephemeral_pub": "<base58 32 bytes>",
  "view_tag": <0-255>,
  "metadata": "<base58 encoded bytes>",
  "payment_proof": <opaque>           // service-defined
}
Response: {
  "queued": true,
  "batch_id": "<opaque>",
  "expected_slot": <u64>
}

GET /announce/status/{batch_id}
Response: {
  "status": "pending" | "confirmed" | "failed",
  "tx_signature": "<base58>"          // present if confirmed
}
```

`payment_proof` shape is service-defined in v1 (prepaid credit token, micro-
payment signature, etc.). The protocol does not standardize payment.

### 8.4 Trust and privacy properties of decoupled mode

- **What the service learns:** the announcement tuple
  `(R, view_tag, metadata)`. The service does NOT learn `P_stealth`,
  `B_spend`, `B_scan`, or anything about the sender's main identity unless
  the sender's wallet authenticates to the service from a known address.
- **What the service can do maliciously:** drop or delay announcements
  (mitigated by the self-announce fallback); collude with chain observers
  to attempt to correlate received tuples with on-chain transfers (the
  service has no advantage here over any other observer who watches both
  the service and the chain — the announcement payload contains no
  sender-identifying information).
- **Critical:** the announcement tuple does NOT reveal the recipient's
  identity to anyone without `b_scan`. A service operator without the
  scan key cannot tell who any announcement is for.

### 8.5 Coupled mode (escape hatch)

Wallets MAY support a coupled mode where the announcement is published in
the same tx as the transfer. This is useful when no announcement service
is reachable and the user prefers a single-tx flow over the
self-announce-fallback retry pattern.

Coupled mode trades on-chain unlinkability (the tx is visibly an Umbra
payment) for trustless single-tx atomicity. Wallets MUST surface this
trade-off to the user before using coupled mode.

---

## 9. Recipient Sweep Flow

The stealth address holds value. To use it, the recipient must move funds
out. The stealth address holds essentially no SOL beyond the rent-exempt
minimum and cannot pay its own transaction fees.

### 9.1 Relayer-based sweep (default)

The recipient uses Solana's native fee-payer separation: a relayer pays the
tx base fee, and is compensated from the swept value.

**Sweep tx structure (SOL receipt):**

```
Tx signers:
  - stealth_key (signs both transfers below)
  - relayer (signs as fee payer)

Let:
  balance       = lamports currently held by the stealth account
                  (= received_amount + rent_reserve)
  relayer_take  = relayer_margin + rent_reserve
                  (relayer keeps the rent as part of their fee)

Tx instructions:
  1. SystemProgram::transfer {
       from: stealth_address,
       to: relayer_address,
       lamports: relayer_take
     }
  2. SystemProgram::transfer {
       from: stealth_address,
       to: recipient_destination,
       lamports: balance - relayer_take
     }
  // After both instructions, stealth_address balance = 0; the system
  // account is then garbage-collected by the runtime (no explicit close
  // instruction is needed for system accounts).
```

The relayer's margin must cover the base tx fee (5,000 lamports minimum)
plus their profit. Recipients should expect total relayer compensation
in the 900,000-940,000 lamports range for SOL sweeps (dominated by the
~890k rent reserve being absorbed by the relayer).

**Sweep tx structure (SPL receipt):**

```
Tx signers:
  - stealth_key
  - relayer (fee payer)

Tx instructions:
  1. spl_token::transfer {
       from: stealth_ata,
       to: recipient_destination_ata,
       amount: balance - relayer_token_compensation
     }
  2. spl_token::transfer {
       from: stealth_ata,
       to: relayer_ata,
       amount: relayer_token_compensation
     }
  3. spl_token::CloseAccount {
       account: stealth_ata,
       destination: relayer_address,    // rent goes to relayer
       authority: stealth_key
     }
  4. (optionally) close the stealth system account with destination =
     relayer to reclaim system-account rent
```

Relayer compensation in SPL sweeps is paid in-kind (in the token being
swept). The relayer accepts the base SOL fee out-of-pocket and is repaid
in tokens.

### 9.2 Close-to-relayer rent disposal

**Critical privacy property:** the rent reclaimed from closing the stealth
account (or ATA) MUST be sent to the relayer, not to the recipient's main
wallet. Closing rent to the recipient's main wallet would create a
direct on-chain link `stealth_address → main_wallet`, defeating the
protocol's privacy guarantee.

The relayer absorbs the rent as part of their fee. This is acceptable
because the rent (~890k-2M lamports per closed account) is significant
compared to the relayer's base cost (~5k lamports tx fee), so relayer
economics work out in their favor for sweeps that close accounts.

Wallets MUST refuse to construct sweep transactions whose close-account
destination is anything other than the relayer (or another stealth
address controlled by the recipient — see §9.3).

### 9.3 Stealth-to-stealth as a sweep variant

Stealth-to-stealth payments are not a separate primitive. They are simply
a sweep where the destination is another stealth address (derived from
some recipient's meta-address), rather than the recipient's main wallet.

The sweep tx structure is identical to §9.1, with:

- `recipient_destination` = a stealth address derived from the target
  meta-address (the "next hop" recipient)
- The relayer also publishes an announcement for the new stealth address
  (or the stealth-to-stealth sender's wallet self-announces with fallback,
  same as §8)
- Closing rent still goes to the relayer

This preserves privacy through the chain of payments: the original sender
cannot be linked to the second-hop recipient on-chain.

### 9.4 NFT sweep specifics

NFT sweeps follow the SPL pattern, with Metaplex/Token-2022 program-
mandated extra accounts. NFTs cannot be partially swept; the full token
moves. The relayer compensation must therefore come either from SOL the
recipient added separately (uncommon in stealth flow), or from a side
agreement with the relayer (e.g., the relayer is willing to sweep NFTs for
a fixed SOL fee paid out-of-band).

V1 SDKs SHOULD warn users that NFT sweeps are more expensive in practice
than SOL/SPL sweeps due to the relayer-compensation challenge.

---

## 10. Discovery and Scanning

### 10.1 Self-scan via logs (trustless)

Recipient wallets subscribe to `logsSubscribe` for the announcer program
ID and process new announcements live. For historical backfill (e.g.,
first sync), wallets use `getSignaturesForAddress` against the announcer
program followed by `getTransaction` to retrieve logs.

**Limitation:** default Solana RPCs prune logs aggressively (hours, not
days). Recipients who go offline for more than ~24 hours may need an
alternative discovery path (§10.2 or §10.3).

### 10.2 Self-scan via indexer (HTTP protocol)

Any party can host an indexer that retains Umbra announcements indefinitely
and exposes them over HTTP. The indexer does NOT receive any scan keys;
the recipient downloads announcements and runs the scan loop locally.

**Minimal indexer HTTP protocol:**

```
GET /announcements?since_slot=<u64>&limit=<u32>
Response: {
  "announcements": [
    {
      "slot": <u64>,
      "tx_signature": "<base58>",
      "scheme_id": <u16>,
      "ephemeral_pub": "<base58>",
      "view_tag": <u8>,
      "metadata": "<base58>"
    },
    ...
  ],
  "next_slot": <u64>
}

GET /health
Response: { "indexed_through_slot": <u64>, "lag_seconds": <u32> }
```

Indexers commit to retaining all announcements they ingest. The protocol
defines correctness (faithfully report all announcer-program invocations)
but not SLA. Multiple competing indexers prevent ecosystem capture.

**Privacy of the indexer query:** the indexer sees that someone polled
slot ranges. It does NOT see which announcements matched (matching is
recipient-local). Polling slot ranges leaks no information about which
announcements are interesting to the caller.

### 10.3 View-key delegated scanning (opt-in)

A recipient who wants near-instant low-power scanning can publish their
scan key material to a trusted scanner service. The scanner runs the
per-announcement ECDH and view-tag filter on the recipient's behalf and
returns only the small set of view-tag-matched announcements.

**What gets published:** `b_scan_raw` — the 32-byte pre-clamping scan key
material from §3.1. The scanner needs both the clamped form (for X25519
ECDH) and the raw form (for deriving label tweaks `m_i` if the recipient
uses labels). The raw form is sufficient to derive both, so only it is
published.

**Critical security property:** scan key material is **view-only**. It
cannot sign Solana transactions, cannot recover `b_spend`, and cannot
spend any funds. Compromise exposes incoming-payment metadata (R values,
view tags, eventually amounts and senders via on-chain lookup), but does
not allow theft.

**Minimal view-key-scanner HTTP protocol:**

```
POST /register
Body: {
  "scan_priv_raw": "<base58 32 bytes>",  // b_scan_raw, sent over TLS
  "label_indices": [0, 1, 5, 7],         // labels the recipient uses
  "since_slot": <u64>
}
Response: { "subscription_id": "<opaque>" }

GET /matches/{subscription_id}?since_slot=<u64>
Response: {
  "matches": [
    {
      "slot": <u64>,
      "tx_signature": "<base58>",
      "scheme_id": <u16>,
      "ephemeral_pub": "<base58>",
      "view_tag": <u8>,
      "metadata": "<base58>",
      "label_index_hint": <u32>      // optional, if scanner did the label loop
    },
    ...
  ]
}
```

**Threat model warning for view-key delegation:**

- The scanner sees every payment to the recipient (R, view tag, timing,
  on-chain destination, eventually amounts and senders).
- A compromised or malicious scanner can sell or publish this data.
- Recipients SHOULD only use scanners they trust. Recipients SHOULD NOT
  use view-key delegation for high-stakes financial privacy.
- v1 does not support multiple scan keys per meta-address, so a
  recipient using N scanners exposes their full incoming-payment view
  to all N. Use one scanner at a time, or accept the aggregate
  exposure.

### 10.4 Discovery is not part of the protocol

The v1 spec defines indexer and scanner *protocols* but does not mandate
either. Wallets MUST support §10.1 (self-scan via logs) as a baseline.
Wallets MAY additionally support §10.2 and §10.3.

---

## 11. Versioning and Scheme IDs

### 11.1 Two independent version axes

1. **Meta-address encoding version** (`version: u8` field in §3.2):
   describes the bytes of the meta-address. v1 = `0x01`.
2. **Cryptographic scheme ID** (`scheme_id: u16` field in announcer
   instruction): describes the cryptographic derivation used for the
   announcement. v1 = `0x0001`.

These can evolve independently:

- A v2 meta-address encoding (e.g., adding new flag bits) but reusing the
  v1 scheme: `version = 0x02`, `scheme_id = 0x0001`.
- A v2 scheme (e.g., post-quantum) but v1 meta-address encoding:
  `version = 0x01`, `scheme_id = 0x0002`.
- Multi-curve meta-addresses (EVM + Solana unified): `version = 0x03`
  with extended payload, new scheme_id for cross-curve derivation.

### 11.2 Reserved values

| Field | Value | Meaning |
|---|---|---|
| `version` | `0x00` | Reserved (invalid) |
| `version` | `0x01` | v1 encoding (this spec) |
| `version` | `0x02` - `0xEF` | Reserved for future canonical schemes |
| `version` | `0xF0` - `0xFF` | Experimental / non-canonical |
| `scheme_id` | `0x0000` | Reserved (invalid) |
| `scheme_id` | `0x0001` | v1 scheme: Ed25519 spend, X25519 scan, SHA-256, BIP-352 labels |
| `scheme_id` | `0x0002` - `0xFEFF` | Reserved for future canonical schemes |
| `scheme_id` | `0xFF00` - `0xFFFF` | Experimental / non-canonical |

v1 wallets MUST reject meta-addresses with `version != 0x01` and MUST
ignore announcements with `scheme_id != 0x0001`. Forward-compatible
wallets MAY surface "unsupported scheme — please update" UX.

---

## 12. Threat Model

### 12.1 On-chain unlinkability properties

**What the protocol guarantees against an on-chain observer with no scan
key:**

- Cannot link a stealth address to the recipient's meta-address.
- Cannot link two stealth addresses derived from the same meta-address to
  each other.
- In decoupled mode, cannot identify a transfer as being an Umbra payment
  (the transfer is indistinguishable from a normal transfer to a fresh
  address).
- Even with the announcement event visible (any mode), cannot tell which
  R corresponds to which on-chain transfer without `b_scan`.

**What the protocol does NOT guarantee:**

- Cannot hide the amount of an asset transfer. Amounts are visible
  on-chain.
- Cannot hide the type of asset transferred (SOL vs SPL vs NFT, and which
  mint).
- Cannot prevent sender deanonymization via the sender's own tx (the
  sender's fee-payer is visible; sender SHOULD use a dedicated funding
  wallet or another privacy layer if sender anonymity is required).
- Cannot prevent timing correlation by an adversary watching both the
  sender's network traffic and the chain.

### 12.2 Sender deanonymization risks

- **NFT sender deanonymization:** sending an NFT publicly tied to the
  sender (minted by, previously held by) leaks sender identity.
- **Coupled-mode tx pattern:** the sender's tx in coupled mode is visibly
  an Umbra payment, narrowing the anonymity set.
- **Fee-payer correlation:** the sender's wallet pays the fee for the
  transfer tx. Clustering attacks on fee-payer history can link the
  sender's stealth payments to the rest of their on-chain activity.
- **Announcement-service correlation:** if the sender authenticates to an
  announcement service from a known identity, the service can build a
  log linking the sender to each `R` they announce. This does not
  deanonymize the recipient, but it does link the sender to their own
  stealth-payment history.

### 12.3 Recipient deanonymization risks

- **Stealth → main link via rent close:** closing a stealth account with
  destination = main wallet creates a direct on-chain link. v1 wallets
  MUST refuse this (§9.2).
- **View-key scanner compromise:** §10.3.
- **Multi-payment cluster analysis:** if a recipient consolidates
  multiple stealth payments into a single sweep destination, that
  destination becomes a cluster point. Recipients SHOULD vary sweep
  destinations or use stealth-to-stealth chains.
- **NFT identifiability:** the NFT itself is the de-anonymizer; if Alice
  is publicly known to own NFT X and she sweeps it from a stealth
  address, the world learns she received that stealth payment.

### 12.4 Spam and DoS

The announcer program is permissionless. An attacker can submit garbage
announcements to inflate recipient scan workloads.

**Cost analysis:**
- Per-announcement cost to attacker: ~5,000 lamports (~$0.001) for single
  announces; amortizes to ~100 lamports (~$0.00002) per entry at full
  batches.
- Per-announcement cost to scanner: one X25519 ECDH (~30 microseconds on
  commodity hardware) plus a SHA-256.
- 1 million spam announcements = ~$1,000 attacker cost; ~30 CPU-seconds
  total scan work per recipient. Not a meaningful attack.
- 100 million spam announcements = ~$100,000 attacker cost; ~3,000
  CPU-seconds (~50 minutes) per recipient. Annoying but not crippling.

The 1-byte view tag is the structural defense: it rejects ~255/256 spam
announcements after just an ECDH and a SHA-256, before any scalar mult.

v1 does not include further DoS mitigations. Future schemes may add
sender-attached anti-spam stamps (e.g., a small fixed SOL burn per
announcement) if attack patterns warrant.

### 12.5 Announcement-service trust surface

See §8.4. The service learns the announcement payload but not the
recipient's identity. The biggest risk is censorship/dropping, mitigated
by the self-announce fallback. Recipients are not exposed to the service
at all (the service interacts only with senders).

---

## 13. Out of Scope (v1)

Explicitly NOT in v1, but the design accommodates these as future work:

- **Cross-chain stealth payments** (sender on EVM, recipient on Solana, or
  unified meta-addresses). Future schemes can use a new meta-address
  `version` byte to encode multi-curve keys.
- **Stealth-to-stealth as a separate primitive.** It is a documented
  pattern but uses the existing sweep flow (§9.3).
- **Post-quantum cryptographic schemes.** Future `scheme_id` slot
  reserved.
- **Multi-recipient announcements** (one R fanning out to N stealth
  addresses).
- **Standardized encrypted-metadata schemes.** The `metadata` field is
  opaque in v1.
- **Announcement-service payment economics.** v1 defines the protocol
  shape; pricing, prepaid credits, anonymous payment for service, and
  competing-service routing are all v1.1+ work.
- **On-chain anti-spam stamps.** Not needed at projected attack scale.
- **Light Protocol compressed-account migration.** Event byte layout is
  forward-compatible; the actual migration is v2 work.

---

## 14. Reference Implementations to Ship

The v1 release will include:

1. **Anchor program (`programs/umbra-announcer`)**: the on-chain announcer
   with `announce` and `announce_batch` instructions. ~150 lines of Rust.
2. **Wallet SDK (`packages/umbra-sdk`, TypeScript)**:
   - Key derivation from canonical signed message
   - Meta-address encode/decode (bech32m + label support)
   - Sender flow: derive stealth address, build asset-transfer tx
   - Decoupled announce + self-announce fallback
   - Recipient scan loop (logs and indexer modes)
   - Sweep tx construction with relayer integration
3. **Reference indexer (`services/umbra-indexer`, Rust)**:
   - Subscribes to announcer program logs
   - Serves the HTTP protocol from §10.2
   - SQLite-backed; runnable as a single binary
4. **Reference announcement service (`services/umbra-announcer-service`,
   Rust or TypeScript)**:
   - Receives announcement payloads, batches, submits on-chain
   - HTTP protocol from §8.3
   - Stub payment model (no real economics; documented as not
     production-ready)
5. **CLI tool (`bin/umbra`)**:
   - Generate meta-address from a wallet signature
   - Send via meta-address (SOL, SPL, NFT)
   - Scan for receipts
   - Sweep to a destination
   - Power-user friendly; not a replacement for wallet integrations

Reference implementations are NOT part of the protocol spec. They are
intended to bootstrap an ecosystem of wallet integrations and indexer
operators.

---

## 15. Open Questions

The following questions are explicitly deferred and not resolved by this
spec:

1. **Announcer program deploy address and authority key.** Who deploys
   and upgrades the canonical announcer program? Probably immutable (no
   upgrade authority) once deployed.
2. **Bech32m HRP collision.** The HRP `umbra` is not formally registered.
   Should be verified against the SLIP-0173 registry.
3. **Standard relayer pricing schema.** Whether the v1 spec should
   include a "relayer quote" RPC for wallets to discover relayers and
   their fees. Deferred to v1.1+.
4. **Encrypted-metadata standardization.** Whether v1.1 should include a
   memo encryption scheme keyed by `S`. Not blocking v1.
5. **Migration path for users with existing stealth keys from
   competing-protocol experiments.** Not blocking v1.
