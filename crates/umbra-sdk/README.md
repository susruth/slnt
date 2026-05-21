# umbra-sdk

Rust SDK for the [Umbra](../../docs/superpowers/specs/2026-05-20-umbra-solana-stealth-payments-v1-design.md)
stealth-payment protocol on Solana, v1.

Implements spec §§3–5: key derivation from a wallet signature,
bech32m meta-address encoding, sender-side stealth-address derivation,
recipient-side scan/recover, and scalar-mode Ed25519 signing for sweeps.
Plus a thin pinboard-program client and event parser.

## Modules

| Module | What it does | Spec section |
|---|---|---|
| `keys` | Derive `(SpendKey, ScanKey)` from a signature; encode/decode meta-addresses | §3 |
| `sender` | `derive_payment(meta, rng) -> StealthPayment` | §4 |
| `recipient` | `scan_note(spend, scan, R, view_tag) -> Option<NoteMatch>` | §5 |
| `stealth_signing` | Ed25519 signing with a Scalar (via dalek hazmat) | §5 (signing) |
| `pinboard` | Build `post` instruction; parse `Note` event log lines | §6 |

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
cargo test -p umbra-sdk
```

Unit tests cover deterministic key derivation, meta-address roundtrip,
sender/recipient roundtrip (with a false-positive view-tag stress test),
scalar-mode Ed25519 sign+verify, Anchor discriminator equality, and
Note-event log parsing.

## What's out of scope

Currently this SDK covers SOL only, no labels, no relayer-paid sweeps,
no encrypted metadata, and no hardware wallet integration. Each of
those is a documented follow-up in the design spec at
`docs/superpowers/specs/2026-05-20-umbra-sdk-rust-design.md`.
