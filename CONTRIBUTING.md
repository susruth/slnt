# Contributing to SLNT

Thanks for your interest in SLNT — silent payments for Solana. This document
explains how to propose changes, set up your environment, and get a pull
request merged.

By participating you agree to our [Code of Conduct](CODE_OF_CONDUCT.md).

## Two kinds of change

SLNT separates the **protocol** from its **implementation**, and they evolve
through different channels.

### 1. Protocol changes (the sRFC)

[sRFC-0042](docs/srfc/0001-slnt-silent-payments.md) is the normative
specification: wire formats, cryptographic derivations, and the
MUST/SHOULD/MAY requirements. Anything that changes how independent
implementations interoperate — byte layouts, derivation paths, domain tags,
scheme IDs, the meta-address encoding — is a **protocol** change.

Propose these in the **sRFC discussion thread first**, not as a code PR. A
spec change should reach rough consensus before implementation, because every
conforming wallet, indexer, and program has to agree on it. Open a GitHub
Discussion describing the motivation, the proposed change, and its
compatibility impact.

> **The sRFC governs.** Where this codebase and the sRFC disagree on a
> wire/byte format, the sRFC is correct and the code is the bug.

### 2. Implementation changes (pull requests)

Bug fixes, new tests, performance work, docs, additional SDK ergonomics,
service hardening, and anything that makes the code match the sRFC more
faithfully are normal **pull requests**. No discussion is required for small,
self-evident fixes; for larger work, open an issue first so we can agree on
the approach.

If you change behavior, update [the implementation-status
tracker](docs/srfc/IMPLEMENTATION-STATUS.md) and cite the relevant sRFC §
section in your code and PR.

## Development setup

See the [README quickstart](README.md#quickstart) for the toolchain versions
(Rust stable, Solana CLI 2.3.0, Anchor 0.31.1, Node 20+).

```bash
git clone https://github.com/susruth/slnt
cd slnt

./scripts/build.sh                 # build the on-chain programs
cargo test                         # Rust unit tests
anchor test                        # on-chain integration tests (boots a validator)
cd clients/typescript && npm install && npm test
```

## Before you open a PR

Run the full local check set and make sure it is green:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test -p slnt-sdk --features net
npm run lint                       # from the repo root (prettier)
( cd clients/typescript && npm test )
```

### Coding standards

- **Rust** — formatted with `rustfmt`; must pass `clippy` with no warnings
  (`-D warnings`). Public items carry doc-comments; cryptographic and wire
  code cites the governing sRFC section (e.g. `// §5.3`).
- **TypeScript** — formatted with `prettier` (`npm run lint:fix`). The
  `@slnt/sdk` client must stay **byte-compatible** with the Rust reference;
  cross-implementation known-answer tests (meta-address, stealth recovery, HD
  derivation) must pass.
- **Tests are required.** New behavior needs tests; bug fixes need a
  regression test that fails before the fix. Cryptographic changes should add
  a known-answer test vector where possible.
- **No silent format drift.** If you touch a serialized layout, a derivation,
  or a domain-separation tag, that is a protocol change — see above.

### Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(registry): implement close instruction
fix(sdk): reject all-zero X25519 shared secret
docs(srfc): clarify the view-tag derivation
```

Common scopes: `pinboard`, `registry`, `sdk`, `cli`, `announcer`, `indexer`,
`ts`, `srfc`, `docs`, `ci`. Keep commits focused; one logical change per
commit where practical.

### Pull request checklist

- [ ] Targets a single, clearly-described change.
- [ ] `cargo fmt`, `cargo clippy -D warnings`, and the test suites pass.
- [ ] New/changed behavior is tested.
- [ ] Docs and [IMPLEMENTATION-STATUS.md](docs/srfc/IMPLEMENTATION-STATUS.md)
      updated if relevant; sRFC section cited.
- [ ] No wire/byte-format change without a corresponding sRFC discussion.

## Reporting bugs and requesting features

Use the GitHub issue templates. For anything with security or privacy impact,
**do not open a public issue** — follow [SECURITY.md](SECURITY.md).

## License of contributions

Unless you state otherwise, contributions you submit are licensed under the
project's [Apache License 2.0](LICENSE). Per the Apache-2.0 grant, you confirm
you have the right to contribute the work under that license.
