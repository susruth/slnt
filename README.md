# SLNT — Silent Payments for Solana

[![Status](https://img.shields.io/badge/status-experimental-orange)](docs/srfc/IMPLEMENTATION-STATUS.md)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
[![Anchor](https://img.shields.io/badge/Anchor-0.31-9945FF)](https://www.anchor-lang.com/)
[![Spec](https://img.shields.io/badge/spec-sRFC--0042-informational)](docs/srfc/0001-slnt-silent-payments.md)

**SLNT** is a silent-payment (stealth-address) protocol for Solana — the Solana
analog of [Bitcoin Silent Payments (BIP-352)](https://github.com/bitcoin/bips/blob/master/bip-0352.mediawiki)
and [Ethereum Stealth Addresses (ERC-5564 / ERC-6538)](https://eips.ethereum.org/EIPS/eip-5564).

A recipient publishes a single, reusable **meta-address**. Anyone can pay it
such that:

- every payment lands at a fresh, distinct on-chain address (a **stealth
  address**) that is itself a normal, spendable Solana wallet;
- only the recipient can recognize which addresses are theirs;
- no observer without the recipient's scan key can link a stealth address to
  the meta-address, or link two payments to the same meta-address; and
- in the default (decoupled) mode, the payment transaction is
  **indistinguishable** from an ordinary transfer to a fresh address.

The protocol is specified in **[sRFC-0042](docs/srfc/0001-slnt-silent-payments.md)**,
which is normative. This repository is the reference implementation.

> [!WARNING]
> **Experimental and unaudited.** SLNT has not been security-audited. The
> on-chain programs are intended to be deployed immutably, so bugs cannot be
> patched after launch. Do not use with funds you cannot afford to lose. See
> [SECURITY.md](SECURITY.md).

---

## How it works

```
            meta-address (slnt1…)                published once, off-chain or
        ┌──────────────────────────┐            via the on-chain registry
        │  B_spend (Ed25519)        │
        │  B_scan  (X25519)         │
        └──────────────────────────┘
                    │
  sender: ECDH(ephemeral r, B_scan) ─► shared secret S ─► tweak t, view_tag
                    │
                    ▼
        stealth address  P = B_spend + t·G        ◄── a real Solana wallet
                    │
   1) transfer asset to P            (looks like any transfer to a fresh address)
   2) announce (R, view_tag) on the `pinboard` program
                    │
                    ▼
  recipient: scans announcements, view-tag filter, ECDH(b_scan, R),
             recomputes P, recovers spend scalar, sweeps via a relayer
```

The cryptography (Ed25519 spend keys, X25519 scan keys, HKDF-SHA256, bech32m
meta-addresses, BIP-352-style labels, the 1-byte view tag) and the on-chain
wire formats are defined end-to-end in the [sRFC](docs/srfc/0001-slnt-silent-payments.md).

## Repository layout

| Path | What it is |
|---|---|
| [`programs/pinboard`](programs/pinboard) | Permissionless, stateless **announcement** program (`post`, `post_batch`, `Note` event). The ERC-5564 `Announcer` analog. |
| [`programs/registry`](programs/registry) | Optional **meta-address registry** (`register`, `update`, `close`). The ERC-6538 analog. |
| [`crates/slnt-sdk`](crates/slnt-sdk) | Canonical **Rust SDK** and cryptographic reference: key derivation, codec, sender/recipient, flows, sweep, announce. |
| [`crates/slnt-cli`](crates/slnt-cli) | Offline **`slnt` CLI** — key derivation, meta-address encode/decode, sender derivation. |
| [`crates/slnt-announcer`](crates/slnt-announcer) | Reference **announcement service** (HTTP → `post_batch`). |
| [`crates/slnt-indexer`](crates/slnt-indexer) | Reference **indexer** (retains announcements, serves by slot range). |
| [`clients/typescript`](clients/typescript) | **`@slnt/sdk`** — TypeScript wallet SDK, byte-compatible with the Rust SDK. |
| [`docs/srfc`](docs/srfc) | The sRFC, per-component design documents, and the implementation-status tracker. |
| [`tests`](tests) | Anchor/Mocha on-chain integration tests. |
| [`scripts`](scripts) | `build.sh` (program build) and `demo-lifecycle.sh` (end-to-end demo). |

## On-chain programs

| Program | ID (localnet & devnet) |
|---|---|
| `pinboard` | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` |
| `registry` | `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2` |

The vanity prefixes are mnemonic: `SLNTP…` = **P**inboard, `SLNTR…` =
**R**egistry.

## Quickstart

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Solana CLI](https://docs.solanalabs.com/cli/install) **2.3.0**
- [Anchor](https://www.anchor-lang.com/docs/installation) **0.31.1** (via `avm`)
- [Node.js](https://nodejs.org/) **20+** and npm/yarn

### Build the programs

```bash
./scripts/build.sh
```

> The script passes `--tools-version v1.54` to the Solana build toolchain.
> Solana CLI 2.3.0 ships platform-tools v1.48 (cargo 1.84), which predates
> Rust `edition2024` required by some transitive dependencies; v1.54 ships
> cargo 1.89, which supports it. See the comments in
> [`scripts/build.sh`](scripts/build.sh).

### Run the tests

```bash
# Rust SDK + service unit tests (no validator needed)
cargo test
cargo test -p slnt-sdk --features net      # include networked client/scanner tests

# On-chain integration tests (boots a local validator)
anchor test

# TypeScript SDK
cd clients/typescript && npm install && npm test
```

### Run the end-to-end demo

```bash
./scripts/demo-lifecycle.sh
```

Boots a local validator with the `pinboard` program, then runs the
[`lifecycle`](crates/slnt-sdk/examples/lifecycle.rs) example: derive keys →
emit a meta-address → send → scan → recover → sweep.

### Lint & format

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
npm run lint            # prettier over JS/TS
```

## Using the SDKs

- **Rust** — add `slnt-sdk` from this workspace; see its
  [README](crates/slnt-sdk/README.md) and the
  [`lifecycle` example](crates/slnt-sdk/examples/lifecycle.rs).
- **TypeScript** — [`@slnt/sdk`](clients/typescript) is feature- and
  byte-equivalent with the Rust SDK (verified by cross-implementation
  known-answer tests).

## Documentation

- **[sRFC-0042 — the normative specification](docs/srfc/0001-slnt-silent-payments.md)**
- [Per-component design documents](docs/srfc/design/README.md)
- [Implementation status / conformance tracker](docs/srfc/IMPLEMENTATION-STATUS.md)

Protocol discussion happens in the sRFC thread; see
[CONTRIBUTING.md](CONTRIBUTING.md) for how to propose changes.

## Contributing

Contributions are welcome — please read **[CONTRIBUTING.md](CONTRIBUTING.md)**.
By participating you agree to abide by our
[Code of Conduct](CODE_OF_CONDUCT.md).

## Security

This is experimental, unaudited software. Found a vulnerability — including a
privacy/unlinkability weakness? Please follow the responsible-disclosure
process in **[SECURITY.md](SECURITY.md)** rather than opening a public issue.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
