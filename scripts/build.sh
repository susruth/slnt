#!/usr/bin/env bash
# Build the pinboard program, its IDL, and its TS types.
#
# Why this script exists: as of Solana CLI 2.3.0, the bundled
# `cargo-build-sbf` defaults to platform-tools v1.48 (cargo 1.84.0), which
# predates Rust edition2024. Several transitive dependencies of
# `solana-program 2.3.0` and `anchor-lang 0.31.1` now require edition2024
# and fail to parse. Passing `--tools-version v1.54` selects platform-
# tools with cargo 1.89, which supports edition2024.
#
# The IDL build step uses host cargo (which is modern, no toolchain
# override needed), so it is invoked separately to avoid the unknown
# `--tools-version` flag being forwarded to it.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROGRAM=pinboard
TOOLS_VERSION=v1.54

anchor build --no-idl -- --tools-version "$TOOLS_VERSION"
anchor idl build -p "$PROGRAM" -o "target/idl/${PROGRAM}.json"
anchor idl type "target/idl/${PROGRAM}.json" -o "target/types/${PROGRAM}.ts"
