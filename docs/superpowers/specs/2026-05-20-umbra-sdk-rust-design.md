# Umbra Rust SDK & Lifecycle Demo — Design Spec

**Date:** 2026-05-20
**Status:** Draft for review
**Scope:** A Rust library that implements the Umbra v1 stealth-payment
primitives (spec §§3–5), plus an example binary that runs the full
sender → pinboard → recipient → sweep lifecycle against a local Solana
validator.

**Reference spec:** `docs/superpowers/specs/2026-05-20-umbra-solana-stealth-payments-v1-design.md`

---

## 1. Goals

- A standalone Rust crate (`crates/umbra-sdk`) that implements the v1
  cryptography from spec §§3–5 with a clean public API.
- A runnable `examples/lifecycle.rs` that exercises the entire send →
  scan → sweep flow end-to-end against `solana-test-validator`.
- High enough fidelity to the spec that this serves as the reference Rust
  implementation. Other languages (TS, Go) can mirror this design.

## 2. Non-goals (this iteration)

- **Labels (§3.3).** Demo uses `label_index = 0` only. The meta-address
  format reserves the field; library API exposes only the unlabeled
  flow. Labels are a clean follow-up — the math is in the spec.
- **SPL tokens / NFTs (§7.2, §7.3).** SOL only.
- **Encrypted metadata.** Metadata is opaque bytes; demo uses an empty
  metadata field.
- **Relayer separation (§9).** Demo's sweep is self-paid: the sender's
  initial SOL transfer includes a small fee buffer so the stealth account
  can pay its own sweep tx. Explicitly documented as a privacy
  simplification — real recipients need a relayer to avoid the
  `stealth → main` link via fee payment. Adding a relayer is a follow-up.
- **Hardware wallet integration.** The demo's "wallet" is an in-process
  `ed25519_dalek::SigningKey`. Wallet adapter design is out of scope.
- **Announcement service / indexer.** Demo subscribes directly to the
  pinboard program via `RpcClient::get_signatures_for_address`.
- **TypeScript SDK.** Separate plan; the Rust SDK is the reference
  implementation.

## 3. Crate layout

```
crates/umbra-sdk/
├── Cargo.toml
├── src/
│   ├── lib.rs            — re-exports
│   ├── error.rs          — `UmbraError` enum
│   ├── keys.rs           — key derivation + meta-address codec
│   ├── sender.rs         — stealth-address derivation, sender side
│   ├── recipient.rs      — scan, view-tag filter, scalar reconstruction
│   ├── stealth_signing.rs — Ed25519 scalar-mode signing wrapper
│   └── pinboard.rs       — build the `post` instruction
└── examples/
    └── lifecycle.rs      — end-to-end demo binary
```

Workspace `Cargo.toml` adds `"crates/*"` to `members`.

## 4. Public API sketch

```rust
// crates/umbra-sdk/src/lib.rs

pub use error::UmbraError;
pub use keys::{
    derive_stealth_keys, MetaAddress, ScanKey, SpendKey, MetaAddressVersion,
};
pub use sender::{StealthPayment, derive_payment};
pub use recipient::{NoteMatch, scan_note};
pub use stealth_signing::StealthSigningKey;
pub use pinboard::build_post_instruction;
```

### 4.1 Keys & meta-address

```rust
/// Recipient's scalar form (b_spend = SC25519_reduce(seed[0:32])).
pub struct SpendKey {
    pub scalar: curve25519_dalek::Scalar,   // b_spend
    pub point: curve25519_dalek::EdwardsPoint, // B_spend = b_spend · G_ed
}

/// Recipient's X25519 scan key (b_scan_raw, clamped on use).
pub struct ScanKey {
    pub raw: [u8; 32],                 // b_scan_raw
    pub static_secret: x25519_dalek::StaticSecret,
    pub public: x25519_dalek::PublicKey, // B_scan
}

/// Derive (SpendKey, ScanKey) from a 64-byte Ed25519 signature over the
/// canonical message defined in spec §3.1.
pub fn derive_stealth_keys(signature_64: &[u8; 64]) -> Result<(SpendKey, ScanKey), UmbraError>;

pub struct MetaAddress {
    pub version: u8,         // 0x01 for v1
    pub b_spend: [u8; 32],   // compressed Ed25519
    pub b_scan:  [u8; 32],   // X25519 pubkey
    pub label_index: u32,    // varint on the wire; u32 in memory
    pub flags: u8,           // 0x00 in v1
}

impl MetaAddress {
    pub fn encode_bech32m(&self) -> String;                  // HRP `umbra`
    pub fn decode_bech32m(s: &str) -> Result<Self, UmbraError>;
}
```

### 4.2 Sender

```rust
pub struct StealthPayment {
    /// Solana address bytes (compressed P_stealth).
    pub stealth_address: solana_sdk::pubkey::Pubkey,
    /// R (ephemeral X25519 pubkey).
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
}

/// Per spec §4. `rng` is a CSPRNG (test code passes a seeded one for
/// determinism; production passes `OsRng`).
pub fn derive_payment(
    meta_address: &MetaAddress,
    rng: &mut impl rand_core::CryptoRngCore,
) -> Result<StealthPayment, UmbraError>;
```

### 4.3 Recipient

```rust
pub struct NoteMatch {
    /// Reconstructed Solana address for this payment.
    pub stealth_address: solana_sdk::pubkey::Pubkey,
    /// Scalar to use with `StealthSigningKey::new(...)`.
    pub stealth_scalar: curve25519_dalek::Scalar,
}

/// Per spec §5. Returns `Ok(Some(_))` if the note is for this recipient,
/// `Ok(None)` if the view tag rejected it, `Err` on malformed input.
pub fn scan_note(
    spend: &SpendKey,
    scan: &ScanKey,
    ephemeral_pub: &[u8; 32],
    view_tag: u8,
) -> Result<Option<NoteMatch>, UmbraError>;
```

### 4.4 Stealth signing (the load-bearing detail)

```rust
/// An Ed25519 signing key constructed from a scalar (not a seed).
/// Required because Umbra produces `p_stealth` as a Scalar; standard
/// `ed25519_dalek::SigningKey` derives the scalar from a seed via SHA-512.
pub struct StealthSigningKey {
    scalar: curve25519_dalek::Scalar,
    verifying_key: ed25519_dalek::VerifyingKey,
    /// Deterministic hash-prefix for RFC 8032 nonce derivation. We
    /// derive it from the scalar via SHA-512("umbra-v1-nonce" || scalar)
    /// so signatures are deterministic without exposing the scalar.
    hash_prefix: [u8; 32],
}

impl StealthSigningKey {
    pub fn new(scalar: curve25519_dalek::Scalar) -> Self;
    pub fn sign(&self, message: &[u8]) -> ed25519_dalek::Signature;
}
```

Internally `sign` uses `ed25519_dalek::hazmat::raw_sign` with an
`ExpandedSecretKey { scalar, hash_prefix }`. This is the API path that
ed25519-dalek explicitly provides for scalar-form keys.

### 4.5 Pinboard client

```rust
/// Build the on-the-wire instruction to call `pinboard.post`.
/// `pinboard_program_id` is the deployed pinboard pubkey.
pub fn build_post_instruction(
    pinboard_program_id: &solana_sdk::pubkey::Pubkey,
    fee_payer: &solana_sdk::pubkey::Pubkey,
    scheme_id: u16,
    ephemeral_pub: [u8; 32],
    view_tag: u8,
    metadata: Vec<u8>,
) -> solana_sdk::instruction::Instruction;
```

The instruction is hand-built with Anchor's 8-byte discriminator
(`sha256("global:post")[..8]`) + borsh-serialized args. Avoids pulling
`anchor-client` (heavy and version-coupled).

## 5. Demo binary (`examples/lifecycle.rs`)

```
1. RPC client connects to http://127.0.0.1:8899
2. Generate three Solana keypairs:
   - sender_wallet     — funded
   - recipient_wallet  — funded (the "main wallet" the recipient sweeps to)
   - relayer (unused this iteration)

3. Airdrop SOL to sender_wallet and recipient_wallet (10 SOL each).

4. Recipient flow:
   a. Use ed25519-dalek to sign the canonical message
      (with a deterministic seed for reproducibility).
   b. derive_stealth_keys(signature) → (spend, scan).
   c. Build MetaAddress and encode to bech32m. Print.

5. Sender flow:
   a. Decode the meta-address.
   b. derive_payment(meta) → (stealth_address, R, view_tag).
   c. Compose tx with:
      - SystemProgram::transfer(sender_wallet → stealth_address, 1.0 SOL)
      - pinboard::post(scheme_id=1, R, view_tag, metadata=[])
      Signed by sender_wallet. Send and confirm.

6. Recipient scan:
   a. RpcClient::get_signatures_for_address(pinboard_program_id, limit=100)
   b. For each tx, fetch logs, parse Note events.
   c. For each note: scan_note(spend, scan, R, view_tag).
   d. On a match → record (stealth_address, stealth_scalar).

7. Recipient sweep:
   a. RpcClient::get_balance(stealth_address) → balance B
   b. Build sweep tx:
      - stealth_address is the sole signer AND fee payer
      - SystemProgram::transfer(stealth_address → recipient_wallet, B − 5_000)
        (subtracts the 5,000-lamport base tx fee the runtime will deduct)
      After execution, stealth balance = 0, the system runtime
      garbage-collects the account; no explicit close needed.
      Send via `StealthSigningKey::sign` for the stealth signature.

8. Verification: print balances. Recipient should have gained
   ~999,995,000 lamports (1.0 SOL minus the 5,000-lamport sweep fee).
```

The shell wrapper (`scripts/demo-lifecycle.sh`) starts
`solana-test-validator`, deploys pinboard via `solana program deploy`,
runs the example, then tears down.

## 6. Cryptographic decisions

These resolve ambiguities the spec leaves open or punts to the
implementation:

| Decision | Choice | Rationale |
|---|---|---|
| `SC25519_reduce` | `curve25519_dalek::Scalar::from_bytes_mod_order` | Standard, audited, exactly mod-ℓ reduction |
| `X25519_clamp` | Done implicitly by `x25519_dalek::StaticSecret::from(...)` | The crate clamps on construction |
| Edwards point add | `curve25519_dalek::EdwardsPoint` arithmetic | Standard, well-tested |
| Stealth nonce derivation | `SHA-512("umbra-v1-nonce" \|\| scalar.to_bytes())[32..64]` as `hash_prefix` | Deterministic; doesn't leak scalar; stable across signatures |
| Compressed Ed25519 → point | `CompressedEdwardsY::decompress()` returns `Option<EdwardsPoint>`; treat `None` as invalid input | Spec says abort on derivation error |
| `version` byte location in bech32m | First byte of data part, before B_spend | Matches spec §3.2 table order |
| LEB128 varint | Hand-roll (~20 LOC) — encoding-only, all values ≤ u32 in v1 | Avoids pulling a varint crate for one site |

## 7. Risks and mitigations

1. **Ed25519 scalar signing via hazmat**. Primary risk. ed25519-dalek's
   hazmat module is intentionally restricted; the exact API
   (`raw_sign` vs `raw_sign_byupdate` vs constructing `ExpandedSecretKey`
   manually) varies by minor version. Mitigation: pin
   `ed25519-dalek = "2.1"`, verify the hazmat path works with a unit
   test (sign + verify-against-ed25519-dalek-VerifyingKey) before
   plumbing into the lifecycle.

2. **Solana SDK + curve25519-dalek version conflict**. `solana-sdk`
   pulls a specific curve25519-dalek; if our direct dep diverges, cargo
   may fail to unify. Mitigation: align to whichever `curve25519-dalek`
   version `solana-program 2.3.0` uses (4.x as of writing). Use
   `cargo tree` to verify a single version is selected.

3. **Edition2024 / SBF toolchain**. The SDK is a host-only crate (not
   compiled for SBF), so the `--tools-version v1.54` workaround that
   the pinboard build needs does NOT apply here. Standard `cargo build`
   on host Rust (1.91) just works.

4. **Test validator startup race**. The shell wrapper must wait for
   the validator to be ready before deploying. Mitigation: poll
   `solana cluster-version` with a short timeout loop.

5. **Anchor discriminator mismatch**. The 8-byte discriminator for the
   `post` instruction is `sha256("global:post")[..8]`. If we get this
   wrong the on-chain program rejects the tx with an opaque error.
   Mitigation: a unit test that compares our computed discriminator
   against the value in `target/idl/pinboard.json`.

## 8. Testing

- Unit tests in `keys.rs`: round-trip meta-address encode/decode; derive
  from a known signature and verify deterministic key bytes.
- Unit tests in `sender.rs` + `recipient.rs`: sender derivation followed
  by recipient scan recovers the same stealth address; recipient
  reconstructs a scalar that, when multiplied by `G_ed`, equals the
  sender's `P_stealth`.
- Unit test in `stealth_signing.rs`: scalar-mode sign + verify-via-pubkey
  round-trip.
- Unit test in `pinboard.rs`: discriminator matches the IDL.
- The `examples/lifecycle.rs` binary IS the integration test for the
  on-chain path.

## 9. What's the next iteration after this

Roughly in priority order:

1. Add a real relayer to the sweep (close §9.2's gap)
2. Add label support (§3.3) — multi-meta-address
3. Add SPL token sweep (§7.2 + §9.1 SPL variant)
4. TypeScript SDK mirroring this Rust shape
5. Reference indexer (§10.2)
6. Reference announcement service (§8.3)
7. Hardware wallet adapter

Each is its own plan; this SDK is the foundation they all stand on.
