# Security Policy

## Status

SLNT is **experimental and has not been independently audited.** The current
devnet/testnet programs remain upgradeable while v1 is draft and unaudited, but
the canonical v1 deployment is intended to become **immutable** (no upgrade
authority) as soon as v1 is finalized and independently audited. After that
point, defects cannot be patched in place; they require a new deployment and a
coordinated migration. Treat all current deployments as testing-only and do not
secure funds you cannot afford to lose.

## Reporting a vulnerability

**Please do not open public issues, pull requests, or discussions for security
problems.** Public disclosure before a fix is available puts users at risk.

Report privately through either channel:

- **GitHub Security Advisories** — open a private advisory at
  <https://github.com/susruth/slnt/security/advisories/new>, or
- **Email** — <susruth@susruth.com> with the subject line `SLNT SECURITY`.

Please include:

- a description of the issue and its impact,
- the affected component(s) and version/commit,
- steps to reproduce or a proof of concept, and
- any suggested remediation.

If you would like acknowledgement, say so and tell us how you'd like to be
credited.

## What counts as a security issue

Because SLNT is a privacy protocol, **privacy weaknesses are security issues**,
not just classic memory/fund-loss bugs. In scope, for example:

- **Unlinkability breaks** — any way for an observer *without the scan key* to
  link a stealth address to a meta-address, link two payments to the same
  meta-address, or identify a decoupled transfer as an SLNT payment.
- **Key-derivation flaws** — weaknesses in Method 1 (HD) or Method 2 (signed
  message) derivation, label derivation, or the spend-scalar reconstruction.
- **Cryptographic errors** — ECDH/view-tag/tweak handling, small-order or
  all-zero point acceptance, domain-separation mistakes, signature handling.
- **On-chain program bugs** — anything in `pinboard` or `registry` that
  allows loss of funds, unauthorized state changes, or denial of service
  beyond the documented spam model.
- **Sweep/relayer issues** — including the close-to-relayer rule that prevents
  a `stealth → main wallet` link.
- **SDK/client bugs** that cause incorrect addresses, leaked key material, or
  divergence between the Rust and TypeScript implementations.

When in doubt, report it.

## Response

This is a volunteer-maintained project, so we cannot promise a fixed SLA, but
we aim to acknowledge a report within a few business days, agree on a
disclosure timeline with you, and credit reporters who wish to be named once a
fix or mitigation is published.

## Supported versions

SLNT is pre-1.0; only the latest `main` is supported. Security fixes land on
`main` and, where applicable, in a coordinated redeployment of the affected
program.
