# slnt-sdk

Rust SDK for [sRFC-0042: SLNT silent payments](../../docs/srfc/0001-slnt-silent-payments.md)
on Solana.

The SDK implements the v1 cryptographic and instruction-building surface:
Method 1 and Method 2 key derivation, bech32m meta-address encoding with
labels, sender stealth-address derivation, recipient scanning and
spend-scalar recovery, scalar-mode Ed25519 signing for sweeps, pinboard
and registry instruction builders/parsers, announcement wire types, and
SOL/SPL/NFT payment and sweep builders.

## Modules

| Module | What it does | Spec section |
|---|---|---|
| `keys` | Method 1/2 derivation, scan-key reconstruction, meta-address encode/decode, labels | §5.2 |
| `sender` | `derive_payment(meta, rng) -> Result<StealthPayment, SlntError>` with v1/meta-key validation | §5.3 |
| `recipient` | `scan_note`, `scan_note_candidates`, and view-only `view_tag_matches` | §5.4, §5.10 |
| `announce` | Decoupled/coupled announcement helpers, self-announce decision, HTTP wire types | §5.8 |
| `flows` | SOL, SPL, and NFT payment instruction builders | §5.7 |
| `sweep` | SOL/SPL relayer sweep builders with close-to-main-wallet protection | §5.9 |
| `pinboard` | Build `post`/`post_batch`; parse `Note` event logs | §5.5 |
| `registry` | Build `register`/`update`/`close`; derive PDA; parse/fetch entries | §5.6 |
| `stealth_signing` | Ed25519 signing from recovered scalar-form stealth keys | §5.9 |

## Demo

```bash
./scripts/demo-lifecycle.sh
```

The shell wrapper boots `solana-test-validator` with the pinboard
program preloaded, runs `examples/lifecycle.rs`, then tears down. The
example performs the full sender → post → scan → sweep flow with two
fresh keypairs and asserts the recipient ends up with the swept funds.

Read the source at `examples/lifecycle.rs` — it doubles as the most
faithful "how to use this SDK" reference.

## Testing

```bash
cargo test -p slnt-sdk
```

Unit tests cover deterministic key derivation, meta-address roundtrip,
HD derivation vectors, labels, sender/recipient roundtrip, low-order
X25519 rejection, reserved meta-address flags, lamport overflow handling,
scalar-mode Ed25519 sign+verify, Anchor discriminator equality, and
event/account parsing.

## What's out of scope

Networked services themselves, a reference indexer, a reference
announcement service, a TypeScript wallet SDK, on-chain CLI commands,
standardized encrypted metadata, and wallet UI/hardware-wallet
integration remain separate deliverables tracked in
`docs/srfc/IMPLEMENTATION-STATUS.md`.
