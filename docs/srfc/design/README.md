# SLNT — Component Design Documents

These are the **service-level design documents** for the SLNT reference
implementation. They are non-normative: the normative standard is
[sRFC-0042](../0001-slnt-silent-payments.md). Where a design document and
the sRFC disagree, the sRFC governs the wire/byte format; the design
documents carry the deepest implementation detail — byte-level layouts,
cryptographic derivations, cost analyses, and operational notes — for an
engineer reimplementing a component from scratch.

Each document maps to a single deployable or publishable component.

## On-chain programs

| Document | Component | Spec | Deployment |
|---|---|---|---|
| [pinboard-program.md](pinboard-program.md) | Announcement program — permissionless, stateless; emits opaque tagged `Note` events. | §5.5 | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` (devnet/testnet upgradeable; v1 immutable after finalization and audit) |
| [registry-program.md](registry-program.md) | Meta-address registry — maps a wallet pubkey → meta-address (ERC-6538 analog); optional. | §5.6 | `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2` (devnet/testnet upgradeable; v1 immutable after finalization and audit) |

## Client libraries

| Document | Component | Spec |
|---|---|---|
| [rust-sdk.md](rust-sdk.md) | `slnt-sdk` — the canonical Rust SDK: cryptography, codecs, instruction builders, transaction flows, sweep, announcement, and scanning. **The cryptographic reference; all other clients mirror it.** | §5.2–§5.10 |
| [typescript-sdk.md](typescript-sdk.md) | `@slnt/sdk` — browser/wallet TypeScript SDK, byte-compatible with the Rust SDK (proven by a cross-impl known-answer test). | §5.2–§5.4, §5.10 |
| [cli.md](cli.md) | `slnt` — offline command-line tool: key derivation, meta-address codec, sender derivation. | §9 |

## Off-chain services

| Document | Component | Spec |
|---|---|---|
| [announcer.md](announcer.md) | `slnt-announcer` — accepts announcement tuples over HTTP and publishes them to pinboard in service-paid `post_batch` transactions (enables decoupled/silent transfers). | §5.8 |
| [indexer-service.md](indexer-service.md) | `slnt-indexer` — subscribes to pinboard logs and serves retained announcements by slot range over HTTP; holds no scan keys. | §5.10 |

## Reading order

For a top-down tour: start with the [pinboard program](pinboard-program.md)
(the announcement primitive everything else is built around), then the
[Rust SDK](rust-sdk.md) (the full off-chain protocol), then the services
([announcer](announcer.md), [indexer](indexer-service.md)),
the [registry](registry-program.md), and finally the client surfaces
([CLI](cli.md), [TypeScript SDK](typescript-sdk.md)).

Implementation status for the whole protocol is tracked in
[../IMPLEMENTATION-STATUS.md](../IMPLEMENTATION-STATUS.md).
