# sRFC-0042: SLNT - Silent Payments for Solana

## Summary

Solana does not currently have a standard way for a user to publish one reusable receiving identifier while keeping individual incoming payments unlinkable from that identifier.

Today, users have two unsatisfying options:

1. Reuse one public Solana address and expose their balance, counterparties, and payment history.
2. Coordinate fresh addresses out of band for every payment, which is poor UX and difficult for wallets and applications to support consistently.

SLNT proposes a Solana-native silent payment standard. A recipient publishes a reusable SLNT meta-address. A sender uses that meta-address to derive a fresh one-time Solana address for each payment. The sender transfers SOL, SPL tokens, or NFTs using normal Solana transfer flows, then posts a small announcement that lets the recipient discover the payment.

The supporting on-chain components are:

- **Pinboard:** a permissionless announcement program that emits discovery events.
- **Registry:** an optional meta-address registry that maps an existing Solana wallet to an SLNT meta-address.

The goal of this sRFC is to standardize the wallet and application interface for silent payments on Solana. It is not intended to make any hosted service, registry deployment, relayer, indexer, or implementation repository canonical.

## Motivation

```mermaid
flowchart LR
  subgraph TODAY["Today: reusable public address"]
    direction TB
    A1["Alice pays"] --> W["Recipient public wallet"]
    B1["Bob pays"] --> W
    C1["Carol pays"] --> W
    O1["Public observer"] -. "links payments, balance, and counterparty graph" .-> W
  end

  subgraph SLNT["SLNT: reusable meta-address, fresh receive addresses"]
    direction TB
    M["Recipient publishes SLNT meta-address"]
    M --> A2["Alice derives P1"]
    M --> B2["Bob derives P2"]
    M --> C2["Carol derives P3"]

    A2 --> P1["Transfer to fresh stealth address P1"]
    B2 --> P2["Transfer to fresh stealth address P2"]
    C2 --> P3["Transfer to fresh stealth address P3"]

    O2["Public observer"] -. "sees separate transfers, but lacks scan key" .-> P2
  end

  TODAY ~~~ SLNT

  classDef problem fill:#fff1f2,stroke:#e11d48,color:#111827;
  classDef slnt fill:#ecfdf5,stroke:#059669,color:#111827;
  classDef neutral fill:#f8fafc,stroke:#64748b,color:#111827;

  class W,O1 problem;
  class M,A2,B2,C2,P1,P2,P3,O2 slnt;
  class A1,B1,C1 neutral;
```

Reusable Solana addresses create a privacy and UX tradeoff. If a creator, merchant, DAO contributor, or employee publishes one wallet address, every incoming payment to that address becomes part of the same public graph. Observers can inspect balances, counterparties, payment timing, token types, and historical activity.

Generating a fresh address for every payment improves privacy, but requires coordination between sender and recipient. That does not compose well with wallets, payment links, profiles, checkout flows, payroll systems, or donation pages.

SLNT tries to preserve the useful part of a public receiving address: one stable identifier that people and apps can use. The difference is that this identifier is not itself the destination of funds. Instead, it lets each sender derive a unique Solana address for that payment.

## What This Draft Standardizes

This draft proposes a standard for:

- The SLNT meta-address format.
- Sender-side derivation of a fresh stealth address.
- Recipient-side scanning and recognition using a scan key.
- The Pinboard announcement model.
- Sweep behavior for moving funds out of one-time addresses.
- Optional Registry support for mapping known Solana wallets to SLNT meta-addresses.
- The minimal on-chain program interface for Pinboard and Registry.

The intent is to define a common wallet and application interface, not to require one specific product or infrastructure provider.

## Core Flow

```mermaid
flowchart LR
  R["Recipient wallet"] --> M["Creates SLNT meta-address"]
  M --> SHARE["Shares directly<br/>or registers in Registry"]
  SHARE --> S["Sender obtains meta-address"]
  S --> D["Derives stealth address, R, and view tag"]

  subgraph payment["Payment path"]
    direction LR
    TX["Normal asset transfer<br/>no SLNT instruction"] --> P["Fresh stealth address<br/>holds SOL / SPL / NFT"]
  end

  subgraph discovery["Discovery path"]
    direction LR
    N["Announcement tuple<br/>scheme_id + R + view_tag + metadata"] --> PB["Pinboard event"]
    N -. "fallback" .-> SELF["Sender self-announces"] -.-> PB
  end

  D --> TX
  D --> N

  PB --> SCAN["Recipient scans with scan key"]
  P --> SCAN
  SCAN --> SWEEP["Recipient sweeps funds"]

  OBS["Public observer"] -. "sees transfer" .-> P
  OBS -. "sees announcement" .-> PB
  OBS -. "without scan key, cannot link them to recipient" .-> SCAN

  classDef recipient fill:#ecfdf5,stroke:#059669,color:#111827;
  classDef sender fill:#eef2ff,stroke:#4f46e5,color:#111827;
  classDef paymentNode fill:#eff6ff,stroke:#2563eb,color:#111827;
  classDef discoveryNode fill:#fff7ed,stroke:#f97316,color:#111827;
  classDef observer fill:#f8fafc,stroke:#64748b,color:#111827;

  class R,M,SHARE,SCAN,SWEEP recipient;
  class S,D sender;
  class TX,P paymentNode;
  class N,PB,SELF discoveryNode;
  class OBS observer;

  style payment fill:#eff6ff,stroke:#2563eb,stroke-width:1px;
  style discovery fill:#fff7ed,stroke:#f97316,stroke-width:1px;
```

SLNT separates payment from discovery.

The asset transfer is a normal transfer to a fresh Solana address. The announcement carries only discovery data. The recipient uses their scan key to connect the two paths and sweep the funds.

This does **not** provide amount privacy, asset-type privacy, sender anonymity, network-level privacy, or complete protection against timing correlation. The narrower privacy claim is: the payment transaction itself does not reveal the recipient's reusable meta-address, and observers without the scan key cannot directly link stealth addresses to that meta-address.

## Pinboard

SLNT needs a way for recipients to discover payments sent to fresh stealth addresses. The sender can transfer assets normally, but the recipient still needs discovery data in order to recognize which stealth addresses belong to them.

Pinboard is the proposed announcement surface for this discovery data.

A Pinboard announcement contains the public values needed for recipient scanning:

- `scheme_id`, identifying the cryptographic scheme.
- `ephemeral_pub`, the sender's ephemeral public value `R`.
- `view_tag`, a short filter used to reject non-matching announcements cheaply.
- `metadata`, an opaque field for minimal implementation-defined context.

Pinboard does not custody funds, route payments, or act as a relayer for the asset transfer. The asset transfer and the announcement are separate.

This separation gives SLNT two useful properties:

- The payment transaction can remain a normal Solana transfer.
- Wallets and indexers can scan a common announcement stream instead of searching the entire chain for possible stealth payments.

Pinboard should be treated as an announcement layer, not as the privacy mechanism itself. Privacy comes from the derivation scheme and the recipient's scan key. Pinboard only makes discovery practical.

The reference implementation uses a small, stateless Anchor program for Pinboard. The candidate program ID is:

```text
SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2
```

This ID is useful for discussion and testing, but this sRFC should not depend on a final mainnet deployment until the interface is reviewed, audited, and finalized.

Pinboard instructions:

| Instruction | Arguments | Accounts | Behavior |
|---|---|---|---|
| `post` | `scheme_id`, `ephemeral_pub`, `view_tag`, `metadata` | `fee_payer: Signer` | Emits one announcement. |
| `post_batch` | `entries: Vec<NoteEntry>` | `fee_payer: Signer` | Emits one announcement per entry. |

Announcement shape:

```rust
Note {
  scheme_id: u16,
  ephemeral_pub: [u8; 32],
  view_tag: u8,
  metadata: Vec<u8>,
}
```

Limits and validation:

- `metadata` is capped at 64 bytes per announcement.
- `post_batch` requires 1 to 50 entries.
- `scheme_id` is recorded but not validated by the program. v1 clients process `0x0001` and ignore unsupported schemes.

## Registry

A recipient can share their SLNT meta-address directly, but direct sharing is not always good UX. In many cases, a sender may only know a recipient's normal Solana wallet address, profile, or application identity.

Registry is an optional discovery layer that maps a known Solana wallet to an SLNT meta-address.

For example:

```text
normal Solana wallet -> SLNT meta-address
```

A sender wallet can look up the recipient's SLNT meta-address before deriving a stealth address.

Registry is not required for silent payments to work. A sender who already has the recipient's SLNT meta-address can derive a stealth address without using Registry. This distinction is important: Registry improves discoverability, but it should not become a mandatory dependency for the core protocol.

Registry should support basic wallet expectations:

- Recipients can publish a meta-address.
- Recipients can rotate a meta-address.
- Recipients can remove a meta-address.
- Senders can verify that a registry entry was authorized by the owning wallet.
- Wallets can distinguish direct meta-address input from registry-based lookup.

The reference implementation uses a small Anchor program for Registry. The candidate program ID is:

```text
SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2
```

Registry stores one meta-address entry per `(registrant, scheme_id)` pair.

Entry PDA:

```text
["meta", registrant, scheme_id_le_bytes]
```

Instructions:

| Instruction | Arguments | Accounts | Behavior |
|---|---|---|---|
| `register` | `scheme_id`, `payload` | `registrant: Signer`, `entry`, `system_program` | Creates the registrant's meta-address entry. |
| `update` | `scheme_id`, `payload` | `registrant: Signer`, `entry` | Replaces the stored meta-address payload. |
| `close` | `scheme_id` | `registrant: Signer`, `entry` | Removes the entry and returns rent to the registrant. |

Stored entry:

```rust
MetaAddressEntry {
  registrant: Pubkey,
  scheme_id: u16,
  bump: u8,
  version: u8,
  b_spend: [u8; 32],
  b_scan: [u8; 32],
  flags: u8,
}
```

Payload for `register` and `update`:

```rust
MetaAddressPayload {
  version: u8,
  b_spend: [u8; 32],
  b_scan: [u8; 32],
  flags: u8,
}
```

Validation:

- `scheme_id` must be non-zero.
- `payload.version` must be `1`.
- `payload.flags` must be `0`.
- `update` and `close` require the entry PDA to match the signing registrant.

Events mirror state changes: `MetaAddressRegistered`, `MetaAddressUpdated`, and `MetaAddressClosed`.

## Privacy Model

The main privacy goal is recipient unlinkability from the reusable meta-address.

A public observer may see:

- the asset transfer,
- the destination stealth address,
- the Pinboard announcement,
- token type and amount,
- transaction timing,
- later sweep activity.

The observer should not learn, from the transfer alone, which reusable SLNT meta-address the payment was derived from.

This is a limited privacy claim. SLNT does not provide amount privacy, asset-type privacy, sender anonymity, network-level privacy, or complete protection against timing correlation. If a sender announces immediately and the recipient sweeps immediately, an observer may still make probabilistic inferences.

The standard should be judged on whether it gives wallets a practical recipient-privacy primitive that is meaningfully better than address reuse while still fitting Solana's existing account and token model.

## Design Choices

### Decoupled Announcements

The default design separates the payment transaction from the announcement.

This avoids marking the asset transfer itself as an SLNT payment. It also allows different announcement services, indexers, and wallet backends to exist without changing how assets are transferred.

The tradeoff is operational complexity. If announcements are delayed, censored, or lost, the recipient may not discover the payment promptly. The draft therefore includes a self-announce fallback.

### Self-Announce Fallback

If the sender cannot use an announcement service, they can publish the announcement directly to Pinboard.

This is less ideal for privacy because timing correlation may become easier, but it avoids a worse failure mode where funds arrive at a stealth address and the recipient never discovers them.

Feedback is needed on whether this fallback should be required, optional, or handled entirely outside the standard.

### Optional Registry

Registry improves UX when a sender knows only the recipient's normal wallet address. However, direct sharing of SLNT meta-addresses should remain valid.

Registry should be treated as a discovery convenience, not as the core privacy mechanism.

### Normal Solana Receive Addresses

The derived stealth address is a normal Solana address.

This allows SLNT to support SOL, SPL tokens, and NFTs without requiring new token programs or special transfer instructions. The complexity is pushed into sender derivation, recipient scanning, and sweeping.

## What This Draft Does Not Standardize

This draft does not attempt to standardize:

- Amount privacy.
- Asset-type privacy.
- Sender anonymity.
- Encrypted memos.
- Relayer incentives.
- A canonical hosted announcement service.
- A final mainnet deployment address.
- Cross-chain meta-addresses.
- Multi-recipient payments.

Some of these may be useful future extensions, but including them in the first standard would make the core proposal harder to evaluate.

## Feedback Requested

I would especially like feedback on the following questions:

1. **Wallet integration:** Is this derivation and scanning model practical for Solana wallets, given current key-management constraints?
2. **Pinboard interface:** Is the `Note` event shape sufficient for wallet discovery without leaking unnecessary linkage information?
3. **Announcement reliability:** Should the standard require self-announcement as a fallback, or leave fallback behavior to implementations?
4. **Registry scope:** Should Registry be part of this sRFC, or split into a separate discovery sRFC?
5. **Privacy claims:** Are the privacy guarantees stated narrowly enough? Are there realistic linkage attacks the draft should address more directly?
6. **Sweep behavior:** What rules should wallets follow to avoid making sweep transactions the easiest point of linkage?
7. **Indexer support:** Is scanning Pinboard events practical for RPC providers, indexers, and wallet backends at scale?
8. **Contract interface:** Are the Pinboard and Registry program interfaces minimal enough to freeze after review, or should any fields/instructions change before audit?

## Reference Material

- Reference implementation: <https://github.com/susruth/slnt>
- Pinboard program source: <https://github.com/susruth/slnt/blob/main/programs/pinboard/src/lib.rs>
- Registry program source: <https://github.com/susruth/slnt/blob/main/programs/registry/src/lib.rs>
- Full technical draft: <https://github.com/susruth/slnt/blob/main/docs/srfc/0001-slnt-silent-payments.md>
- Test vectors: <https://github.com/susruth/slnt/blob/main/test-vectors.json>
