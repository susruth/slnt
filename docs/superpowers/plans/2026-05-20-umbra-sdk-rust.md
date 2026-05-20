# Umbra Rust SDK & Lifecycle Demo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust SDK (`crates/umbra-sdk`) that implements the Umbra v1 stealth-payment primitives from spec §§3–5, plus an `examples/lifecycle.rs` binary that runs the full sender → pinboard → recipient → sweep flow against a local Solana validator.

**Architecture:** Five small modules (`keys`, `sender`, `recipient`, `stealth_signing`, `pinboard`) compose into the library. Each is testable in isolation; together they implement spec §§3–5 of `docs/superpowers/specs/2026-05-20-umbra-solana-stealth-payments-v1-design.md`. The `lifecycle` example wires them up against an `RpcClient` and exercises every step end-to-end. A shell wrapper (`scripts/demo-lifecycle.sh`) starts `solana-test-validator` with the pinboard preloaded, runs the example, and tears down.

**Reference design spec:** `docs/superpowers/specs/2026-05-20-umbra-sdk-rust-design.md` — read first.

**Tech Stack:**
- Rust 1.91+ (host toolchain — SDK is host-only, no SBF)
- `curve25519-dalek = "4.1"`, `ed25519-dalek = "2.1"`, `x25519-dalek = "2"`
- `sha2 = "0.10"`, `hkdf = "0.12"`, `bech32 = "0.11"`
- `borsh = "1"` (matches pinboard / anchor 0.31)
- `solana-sdk = "2.3"`, `solana-client = "2.3"`
- `rand_core = "0.6"`, `base64 = "0.22"`, `thiserror = "1"`

**Reference cryptographic decisions** (from design spec §6):
- `SC25519_reduce` = `Scalar::from_bytes_mod_order` (curve25519-dalek)
- `X25519_clamp` = done implicitly by `x25519_dalek::StaticSecret::from(...)`
- Domain-separation tag length encoding: **1 byte** (all v1 tags are <256 bytes)
- Stealth Ed25519 `hash_prefix` = `SHA-512("umbra-v1-nonce" || scalar_bytes)[32..64]`
- Anchor discriminator for `post`: `[223, 96, 234, 236, 158, 106, 145, 94]` = `SHA-256("global:post")[..8]`
- Anchor event discriminator for `Note`: `[40, 182, 5, 151, 115, 43, 27, 97]` = `SHA-256("event:Note")[..8]`

---

## Pre-flight

- [ ] **Verify pinboard already builds** (sanity check):
  ```bash
  ls /Users/susruth/Documents/Projects/umbra/target/deploy/pinboard.so \
     /Users/susruth/Documents/Projects/umbra/target/idl/pinboard.json
  ```
  Both files must exist. If not, run `./scripts/build.sh` first.

---

## Task 1: Scaffold the SDK crate

**Files:**
- Modify: `/Users/susruth/Documents/Projects/umbra/Cargo.toml` (add `"crates/*"` to workspace `members`)
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/Cargo.toml`
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs`
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/error.rs`

- [ ] **Step 1.1: Add `crates/*` to workspace members**

Open `/Users/susruth/Documents/Projects/umbra/Cargo.toml` and replace the `members` list:

```toml
[workspace]
members = [
    "programs/*",
    "crates/*",
]
resolver = "2"

[profile.release]
overflow-checks = true
lto = "fat"
codegen-units = 1
[profile.release.build-override]
opt-level = 3
incremental = false
codegen-units = 1
```

- [ ] **Step 1.2: Create the SDK crate manifest**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/Cargo.toml`:

```toml
[package]
name = "umbra-sdk"
version = "0.1.0"
edition = "2021"
description = "Rust SDK for the Umbra stealth-payment protocol on Solana (v1)"
license = "Apache-2.0"

[lib]
name = "umbra_sdk"
path = "src/lib.rs"

[dependencies]
curve25519-dalek = { version = "4.1", features = ["digest"] }
ed25519-dalek = { version = "2.1", features = ["rand_core"] }
x25519-dalek = { version = "2", features = ["static_secrets"] }
sha2 = "0.10"
hkdf = "0.12"
bech32 = "0.11"
borsh = { version = "1", features = ["derive"] }
solana-sdk = "2.3"
rand_core = "0.6"
thiserror = "1"

[dev-dependencies]
rand_chacha = "0.3"
hex = "0.4"

[[example]]
name = "lifecycle"
path = "examples/lifecycle.rs"
```

- [ ] **Step 1.3: Create `src/lib.rs`**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs`:

```rust
//! Umbra Rust SDK — v1 stealth-payment primitives on Solana.
//!
//! See the design spec at
//! `docs/superpowers/specs/2026-05-20-umbra-solana-stealth-payments-v1-design.md`.

pub mod error;
pub mod keys;
pub mod pinboard;
pub mod recipient;
pub mod sender;
pub mod stealth_signing;

pub use error::UmbraError;
```

Note: `keys.rs`, `sender.rs`, etc. don't exist yet — `lib.rs` will fail to compile until later tasks create them. That's expected; we'll bring it up gradually.

For now, comment out the modules that don't exist yet to keep the crate compilable after Task 1:

```rust
//! Umbra Rust SDK — v1 stealth-payment primitives on Solana.

pub mod error;
pub use error::UmbraError;
```

We'll add `pub mod keys;` etc. as each module is created.

- [ ] **Step 1.4: Create `src/error.rs`**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UmbraError {
    #[error("key derivation failed (signature produced anomalous scalar)")]
    Derivation,

    #[error("invalid Ed25519 point encoding")]
    InvalidPoint,

    #[error("meta-address encoding failed")]
    MetaAddressEncode,

    #[error("meta-address decoding failed: {0}")]
    MetaAddressDecode(String),

    #[error("unsupported meta-address version: {0:#x}")]
    UnsupportedVersion(u8),

    #[error("note metadata exceeds 64 bytes (got {0})")]
    MetadataTooLong(usize),

    #[error("base58 decode failed")]
    Base58,

    #[error("rpc error: {0}")]
    Rpc(String),
}
```

- [ ] **Step 1.5: Build to verify scaffold**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo build -p umbra-sdk 2>&1 | tail -10
```

Expected: clean build with no warnings. If the lockfile picks new versions of dependencies, that's fine.

- [ ] **Step 1.6: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add Cargo.toml Cargo.lock crates/umbra-sdk/Cargo.toml crates/umbra-sdk/src/lib.rs crates/umbra-sdk/src/error.rs
git commit -m "$(cat <<'EOF'
chore(umbra-sdk): scaffold crate with error enum

Adds crates/* to the workspace and creates an empty umbra-sdk crate
with just an UmbraError enum. Modules added in subsequent commits.
EOF
)"
```

---

## Task 2: Key derivation from signature

**Files:**
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/keys.rs`
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs` (add `pub mod keys;`)

- [ ] **Step 2.1: Add module declaration**

Edit `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs` to add the `keys` module:

```rust
//! Umbra Rust SDK — v1 stealth-payment primitives on Solana.

pub mod error;
pub mod keys;

pub use error::UmbraError;
```

- [ ] **Step 2.2: Create `keys.rs` with the public types and `derive_stealth_keys` (no meta-address codec yet — Task 3)**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/keys.rs`:

```rust
//! Key derivation and meta-address codec (spec §3).

use crate::error::UmbraError;
use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT, EdwardsPoint, Scalar};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

/// Spec §3.1 canonical message. The trailing newline shown here is NOT
/// part of the spec message; the string ends after "ability." with no
/// trailing newline (matching spec §3.1: "exact UTF-8, no trailing
/// newline").
pub const CANONICAL_MESSAGE_LOCALNET: &str = "Umbra Protocol: Derive Stealth Keys\n\nVersion: 1\nNetwork: Localnet\nWarning: Only sign this message in the Umbra wallet or a trusted Umbra integration.\nSigning this in any other context will reveal your stealth address scanning ability.";

pub const META_ADDRESS_VERSION_V1: u8 = 0x01;
pub const SCHEME_ID_V1: u16 = 0x0001;

const HKDF_SALT_DERIVE: &[u8] = b"umbra-v1-derive";
const HKDF_INFO_SPEND_AND_SCAN: &[u8] = b"spend-and-scan";

/// Recipient's spend key in scalar form. `point = scalar * G_ed`.
pub struct SpendKey {
    pub scalar: Scalar,
    pub point: EdwardsPoint,
}

impl SpendKey {
    /// Compressed Ed25519 public point (32 bytes), suitable for embedding
    /// in a meta-address.
    pub fn public_bytes(&self) -> [u8; 32] {
        self.point.compress().to_bytes()
    }
}

/// Recipient's X25519 scan key. Holds both the raw 32 bytes (as
/// published in spec §10.3 view-key delegation) and the clamped form
/// used for ECDH.
pub struct ScanKey {
    pub raw: [u8; 32],
    pub static_secret: X25519StaticSecret,
    pub public: X25519PublicKey,
}

impl ScanKey {
    pub fn public_bytes(&self) -> [u8; 32] {
        self.public.to_bytes()
    }
}

/// Spec §3.1 derivation:
///   ikm = signature
///   seed = HKDF-SHA256(salt="umbra-v1-derive", ikm, info="spend-and-scan", L=64)
///   b_spend = SC25519_reduce(seed[0..32])
///   b_scan_raw = seed[32..64]
///   B_spend = b_spend * G_ed
///   b_scan = X25519_clamp(b_scan_raw); B_scan = b_scan * G_x
pub fn derive_stealth_keys(
    signature_64: &[u8; 64],
) -> Result<(SpendKey, ScanKey), UmbraError> {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT_DERIVE), signature_64);
    let mut seed = [0u8; 64];
    hk.expand(HKDF_INFO_SPEND_AND_SCAN, &mut seed)
        .map_err(|_| UmbraError::Derivation)?;

    let mut b_spend_bytes = [0u8; 32];
    b_spend_bytes.copy_from_slice(&seed[0..32]);
    let mut b_scan_raw = [0u8; 32];
    b_scan_raw.copy_from_slice(&seed[32..64]);

    // SC25519_reduce: 32 bytes → Ed25519 scalar mod ℓ.
    let b_spend = Scalar::from_bytes_mod_order(b_spend_bytes);
    if b_spend == Scalar::ZERO {
        return Err(UmbraError::Derivation);
    }
    let b_spend_point = b_spend * ED25519_BASEPOINT_POINT;

    // X25519 clamping happens inside StaticSecret::from(...).
    let scan_static = X25519StaticSecret::from(b_scan_raw);
    let scan_public = X25519PublicKey::from(&scan_static);

    Ok((
        SpendKey { scalar: b_spend, point: b_spend_point },
        ScanKey { raw: b_scan_raw, static_secret: scan_static, public: scan_public },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canonical test signature (just 64 fixed bytes, not a real
    /// signature). Spec §3.1 takes any 64-byte input as IKM; the HKDF
    /// step doesn't care whether it's a real Ed25519 signature.
    const TEST_SIG: [u8; 64] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
        0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28,
        0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30,
        0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
        0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40,
    ];

    #[test]
    fn derive_is_deterministic() {
        let (s1, sc1) = derive_stealth_keys(&TEST_SIG).unwrap();
        let (s2, sc2) = derive_stealth_keys(&TEST_SIG).unwrap();
        assert_eq!(s1.public_bytes(), s2.public_bytes());
        assert_eq!(sc1.public_bytes(), sc2.public_bytes());
        assert_eq!(s1.scalar.to_bytes(), s2.scalar.to_bytes());
        assert_eq!(sc1.raw, sc2.raw);
    }

    #[test]
    fn different_inputs_give_different_keys() {
        let (s1, _) = derive_stealth_keys(&TEST_SIG).unwrap();
        let mut sig2 = TEST_SIG;
        sig2[0] ^= 0x80;
        let (s2, _) = derive_stealth_keys(&sig2).unwrap();
        assert_ne!(s1.public_bytes(), s2.public_bytes());
    }

    #[test]
    fn b_spend_point_matches_scalar_times_basepoint() {
        let (s, _) = derive_stealth_keys(&TEST_SIG).unwrap();
        let expected = s.scalar * ED25519_BASEPOINT_POINT;
        assert_eq!(s.point.compress(), expected.compress());
    }
}
```

- [ ] **Step 2.3: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo test -p umbra-sdk keys:: 2>&1 | tail -15
```

Expected: 3 tests pass.

- [ ] **Step 2.4: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/src/lib.rs crates/umbra-sdk/src/keys.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): derive (SpendKey, ScanKey) from canonical signature

Implements spec §3.1: HKDF-SHA256 → split into b_spend_raw, b_scan_raw
→ scalar-reduce b_spend, X25519-clamp b_scan, derive public points.
EOF
)"
```

---

## Task 3: Meta-address bech32m codec

**Files:**
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/keys.rs` (add MetaAddress and codec)

- [ ] **Step 3.1: Append `MetaAddress` type, LEB128 helpers, and codec to `keys.rs`**

Append to the bottom of `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/keys.rs` (before the `#[cfg(test)] mod tests { ... }` block — move the tests after the new code, or simply insert above the existing tests module):

```rust
use bech32::{Bech32m, Hrp};

const META_ADDRESS_HRP: &str = "umbra";

/// Spec §3.2 meta-address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaAddress {
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub label_index: u32,
    pub flags: u8,
}

impl MetaAddress {
    /// Build an unlabeled v1 meta-address from a (SpendKey, ScanKey) pair.
    pub fn from_keys(spend: &SpendKey, scan: &ScanKey) -> Self {
        Self {
            version: META_ADDRESS_VERSION_V1,
            b_spend: spend.public_bytes(),
            b_scan: scan.public_bytes(),
            label_index: 0,
            flags: 0,
        }
    }

    pub fn encode_bech32m(&self) -> Result<String, UmbraError> {
        let mut payload = Vec::with_capacity(72);
        payload.push(self.version);
        payload.extend_from_slice(&self.b_spend);
        payload.extend_from_slice(&self.b_scan);
        write_leb128_u32(&mut payload, self.label_index);
        payload.push(self.flags);

        let hrp = Hrp::parse(META_ADDRESS_HRP)
            .map_err(|_| UmbraError::MetaAddressEncode)?;
        bech32::encode::<Bech32m>(hrp, &payload)
            .map_err(|_| UmbraError::MetaAddressEncode)
    }

    pub fn decode_bech32m(s: &str) -> Result<Self, UmbraError> {
        let (hrp, data) = bech32::decode(s)
            .map_err(|e| UmbraError::MetaAddressDecode(format!("{e}")))?;
        if hrp.as_str() != META_ADDRESS_HRP {
            return Err(UmbraError::MetaAddressDecode(format!(
                "expected HRP `umbra`, got `{}`",
                hrp.as_str()
            )));
        }
        // Minimum payload: 1 (version) + 32 (B_spend) + 32 (B_scan)
        //                + 1 (varint label_index = 0) + 1 (flags) = 67 bytes
        if data.len() < 67 {
            return Err(UmbraError::MetaAddressDecode(format!(
                "payload too short: {} bytes",
                data.len()
            )));
        }

        let version = data[0];
        if version != META_ADDRESS_VERSION_V1 {
            return Err(UmbraError::UnsupportedVersion(version));
        }

        let mut b_spend = [0u8; 32];
        b_spend.copy_from_slice(&data[1..33]);
        let mut b_scan = [0u8; 32];
        b_scan.copy_from_slice(&data[33..65]);

        let (label_index, consumed) = read_leb128_u32(&data[65..])?;
        let flags_offset = 65 + consumed;
        if data.len() <= flags_offset {
            return Err(UmbraError::MetaAddressDecode(
                "missing flags byte".into(),
            ));
        }
        let flags = data[flags_offset];
        // Anything trailing is an error.
        if data.len() != flags_offset + 1 {
            return Err(UmbraError::MetaAddressDecode(format!(
                "{} trailing bytes after payload",
                data.len() - flags_offset - 1
            )));
        }

        Ok(Self { version, b_spend, b_scan, label_index, flags })
    }
}

/// Unsigned LEB128 encode (DWARF / protobuf style, max 5 bytes for u32).
fn write_leb128_u32(out: &mut Vec<u8>, mut val: u32) {
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if val == 0 {
            break;
        }
    }
}

/// Unsigned LEB128 decode. Returns (value, bytes_consumed).
fn read_leb128_u32(data: &[u8]) -> Result<(u32, usize), UmbraError> {
    let mut val: u64 = 0;
    let mut shift = 0u32;
    for (i, byte) in data.iter().take(5).enumerate() {
        val |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            if val > u32::MAX as u64 {
                return Err(UmbraError::MetaAddressDecode(
                    "varint exceeds u32".into(),
                ));
            }
            return Ok((val as u32, i + 1));
        }
        shift += 7;
    }
    Err(UmbraError::MetaAddressDecode("varint too long".into()))
}
```

- [ ] **Step 3.2: Add codec tests inside the existing `#[cfg(test)] mod tests` block in `keys.rs`**

Append these tests to the existing `mod tests` block in `keys.rs`:

```rust
    #[test]
    fn meta_address_roundtrip_unlabeled() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta = MetaAddress::from_keys(&spend, &scan);
        let encoded = meta.encode_bech32m().unwrap();
        assert!(encoded.starts_with("umbra1"));
        let decoded = MetaAddress::decode_bech32m(&encoded).unwrap();
        assert_eq!(meta, decoded);
    }

    #[test]
    fn meta_address_roundtrip_labeled() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta = MetaAddress {
            version: META_ADDRESS_VERSION_V1,
            b_spend: spend.public_bytes(),
            b_scan: scan.public_bytes(),
            label_index: 12345,
            flags: 0,
        };
        let encoded = meta.encode_bech32m().unwrap();
        let decoded = MetaAddress::decode_bech32m(&encoded).unwrap();
        assert_eq!(meta, decoded);
        assert_eq!(decoded.label_index, 12345);
    }

    #[test]
    fn meta_address_rejects_wrong_hrp() {
        // "btc1..." instead of "umbra1..."
        let bogus = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080";
        assert!(MetaAddress::decode_bech32m(bogus).is_err());
    }

    #[test]
    fn meta_address_rejects_unsupported_version() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta = MetaAddress {
            version: 0x02,
            b_spend: spend.public_bytes(),
            b_scan: scan.public_bytes(),
            label_index: 0,
            flags: 0,
        };
        let encoded = meta.encode_bech32m().unwrap();
        match MetaAddress::decode_bech32m(&encoded) {
            Err(UmbraError::UnsupportedVersion(0x02)) => {}
            other => panic!("expected UnsupportedVersion(0x02), got {other:?}"),
        }
    }

    #[test]
    fn leb128_roundtrip() {
        for val in [0u32, 1, 127, 128, 255, 256, 16384, 1234567, u32::MAX] {
            let mut buf = Vec::new();
            write_leb128_u32(&mut buf, val);
            let (decoded, consumed) = read_leb128_u32(&buf).unwrap();
            assert_eq!(decoded, val, "varint mismatch at {val}");
            assert_eq!(consumed, buf.len());
        }
    }
```

- [ ] **Step 3.3: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo test -p umbra-sdk keys:: 2>&1 | tail -15
```

Expected: 8 tests pass (3 from Task 2 + 5 new).

- [ ] **Step 3.4: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/src/keys.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): bech32m meta-address codec with LEB128 label index
EOF
)"
```

---

## Task 4: Sender stealth-address derivation

**Files:**
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/sender.rs`
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs` (add `pub mod sender;`)

- [ ] **Step 4.1: Add module declaration**

Edit `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs`:

```rust
//! Umbra Rust SDK — v1 stealth-payment primitives on Solana.

pub mod error;
pub mod keys;
pub mod sender;

pub use error::UmbraError;
```

- [ ] **Step 4.2: Create `sender.rs`**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/sender.rs`:

```rust
//! Sender-side stealth-address derivation (spec §4).

use crate::error::UmbraError;
use crate::keys::MetaAddress;
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_POINT, edwards::CompressedEdwardsY, Scalar,
};
use rand_core::CryptoRngCore;
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

/// Domain-separation tag for the stealth-address tweak hash (spec §4
/// step 3 and step 4). 14 bytes.
const TWEAK_TAG: &[u8] = b"umbra-v1-tweak";

/// Output of `derive_payment`.
#[derive(Debug, Clone)]
pub struct StealthPayment {
    /// The Solana account address to receive funds.
    pub stealth_address: Pubkey,
    /// The ephemeral X25519 public key `R` to include in the pinboard note.
    pub ephemeral_pub: [u8; 32],
    /// First byte of `SHA-256(tag || S)`; included in the pinboard note.
    pub view_tag: u8,
}

/// Spec §4. The sender derives a one-time stealth address for the
/// given meta-address, plus the (R, view_tag) tuple to publish on the
/// pinboard.
pub fn derive_payment(
    meta: &MetaAddress,
    rng: &mut impl CryptoRngCore,
) -> Result<StealthPayment, UmbraError> {
    // Decompress B_spend_effective (already incorporates label tweak if any).
    let b_spend_compressed = CompressedEdwardsY(meta.b_spend);
    let b_spend = b_spend_compressed
        .decompress()
        .ok_or(UmbraError::InvalidPoint)?;

    // 1. Generate ephemeral X25519 scalar r.
    let mut r_bytes = [0u8; 32];
    rng.fill_bytes(&mut r_bytes);
    let r = X25519StaticSecret::from(r_bytes);
    let r_public = X25519PublicKey::from(&r);

    // 2. ECDH: S = r · B_scan
    let b_scan_public = X25519PublicKey::from(meta.b_scan);
    let s = r.diffie_hellman(&b_scan_public);

    // 3. view_tag = SHA-256(len(tag) || tag || S)[0]
    let view_tag = compute_view_tag(s.as_bytes());

    // 4. tweak scalar t = SC25519_reduce(SHA-256(len(tag) || tag || S || view_tag))
    let t = compute_tweak(s.as_bytes(), view_tag);

    // 5. P_stealth = B_spend + t · G_ed
    let p_stealth = b_spend + (t * ED25519_BASEPOINT_POINT);
    let stealth_bytes = p_stealth.compress().to_bytes();

    Ok(StealthPayment {
        stealth_address: Pubkey::new_from_array(stealth_bytes),
        ephemeral_pub: r_public.to_bytes(),
        view_tag,
    })
}

/// `SHA-256(1-byte-len || TWEAK_TAG || S)[0]`. Spec §4 step 3.
pub(crate) fn compute_view_tag(s: &[u8]) -> u8 {
    let mut hasher = Sha256::new();
    hasher.update([TWEAK_TAG.len() as u8]);
    hasher.update(TWEAK_TAG);
    hasher.update(s);
    let out = hasher.finalize();
    out[0]
}

/// `SC25519_reduce(SHA-256(1-byte-len || TWEAK_TAG || S || view_tag))`.
/// Spec §4 step 4.
pub(crate) fn compute_tweak(s: &[u8], view_tag: u8) -> Scalar {
    let mut hasher = Sha256::new();
    hasher.update([TWEAK_TAG.len() as u8]);
    hasher.update(TWEAK_TAG);
    hasher.update(s);
    hasher.update([view_tag]);
    let h = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&h);
    // SHA-256 outputs 32 bytes; mod-ℓ reduce.
    Scalar::from_bytes_mod_order(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{derive_stealth_keys, MetaAddress};
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    const TEST_SIG: [u8; 64] = [7u8; 64];

    #[test]
    fn derive_payment_is_deterministic_under_fixed_rng() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta = MetaAddress::from_keys(&spend, &scan);

        let mut rng1 = ChaCha20Rng::seed_from_u64(42);
        let mut rng2 = ChaCha20Rng::seed_from_u64(42);
        let p1 = derive_payment(&meta, &mut rng1).unwrap();
        let p2 = derive_payment(&meta, &mut rng2).unwrap();

        assert_eq!(p1.stealth_address, p2.stealth_address);
        assert_eq!(p1.ephemeral_pub, p2.ephemeral_pub);
        assert_eq!(p1.view_tag, p2.view_tag);
    }

    #[test]
    fn derive_payment_differs_per_call() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta = MetaAddress::from_keys(&spend, &scan);
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let p1 = derive_payment(&meta, &mut rng).unwrap();
        let p2 = derive_payment(&meta, &mut rng).unwrap();
        // Two consecutive payments to the same meta must produce
        // distinct stealth addresses (ephemeral randomness varies).
        assert_ne!(p1.stealth_address, p2.stealth_address);
        assert_ne!(p1.ephemeral_pub, p2.ephemeral_pub);
    }
}
```

- [ ] **Step 4.3: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo test -p umbra-sdk sender:: 2>&1 | tail -10
```

Expected: 2 tests pass.

- [ ] **Step 4.4: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/src/lib.rs crates/umbra-sdk/src/sender.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): sender-side stealth-address derivation (spec §4)
EOF
)"
```

---

## Task 5: Recipient scan & key recovery

**Files:**
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/recipient.rs`
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs` (add `pub mod recipient;`)

- [ ] **Step 5.1: Add module declaration**

Edit `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs`:

```rust
//! Umbra Rust SDK — v1 stealth-payment primitives on Solana.

pub mod error;
pub mod keys;
pub mod recipient;
pub mod sender;

pub use error::UmbraError;
```

- [ ] **Step 5.2: Create `recipient.rs`**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/recipient.rs`:

```rust
//! Recipient-side scanning (spec §5).
//!
//! For each pinboard Note observed, call `scan_note`. If the view tag
//! matches, you get back a `NoteMatch` containing the stealth address
//! and the scalar to sign with.

use crate::error::UmbraError;
use crate::keys::{ScanKey, SpendKey};
use crate::sender::{compute_tweak, compute_view_tag};
use curve25519_dalek::Scalar;
use solana_sdk::pubkey::Pubkey;
use x25519_dalek::PublicKey as X25519PublicKey;

#[derive(Debug, Clone)]
pub struct NoteMatch {
    pub stealth_address: Pubkey,
    /// `p_stealth = (b_spend + t) mod ℓ`. Pass this to
    /// `StealthSigningKey::new`.
    pub stealth_scalar: Scalar,
}

/// Spec §5. Returns:
/// - `Ok(Some(NoteMatch))` if the view tag matched AND the note is for us
/// - `Ok(None)` if the view tag did not match (fast filter rejection)
/// - `Err` for malformed input (e.g., `ephemeral_pub` is not a valid
///   X25519 point — currently every 32 bytes is valid X25519 so this
///   doesn't happen in practice, but we keep the Result for forward
///   compatibility)
pub fn scan_note(
    spend: &SpendKey,
    scan: &ScanKey,
    ephemeral_pub: &[u8; 32],
    note_view_tag: u8,
) -> Result<Option<NoteMatch>, UmbraError> {
    // 1. ECDH using recipient's scan private key.
    let r_public = X25519PublicKey::from(*ephemeral_pub);
    let s_candidate = scan.static_secret.diffie_hellman(&r_public);

    // 2. Fast view-tag filter.
    let vt_candidate = compute_view_tag(s_candidate.as_bytes());
    if vt_candidate != note_view_tag {
        return Ok(None);
    }

    // 3. Tweak scalar (note: tweak hash includes the *note's* view_tag,
    //    which by this point equals vt_candidate).
    let t = compute_tweak(s_candidate.as_bytes(), note_view_tag);

    // 4. Recover P_stealth = B_spend + t · G_ed (= same as the sender's
    //    derivation), and the corresponding scalar p_stealth.
    let p_stealth_point = spend.point + (t * curve25519_dalek::constants::ED25519_BASEPOINT_POINT);
    let stealth_address = Pubkey::new_from_array(p_stealth_point.compress().to_bytes());
    let stealth_scalar = spend.scalar + t;

    Ok(Some(NoteMatch { stealth_address, stealth_scalar }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{derive_stealth_keys, MetaAddress};
    use crate::sender::derive_payment;
    use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    const TEST_SIG: [u8; 64] = [9u8; 64];

    #[test]
    fn sender_recipient_roundtrip() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta = MetaAddress::from_keys(&spend, &scan);

        let mut rng = ChaCha20Rng::seed_from_u64(0xc0ffee);
        let payment = derive_payment(&meta, &mut rng).unwrap();

        let matched = scan_note(&spend, &scan, &payment.ephemeral_pub, payment.view_tag)
            .unwrap()
            .expect("note should match — same meta");

        assert_eq!(matched.stealth_address, payment.stealth_address);

        // Sanity: stealth_scalar * G_ed must equal the stealth point.
        let recovered_point = matched.stealth_scalar * ED25519_BASEPOINT_POINT;
        assert_eq!(
            recovered_point.compress().to_bytes(),
            payment.stealth_address.to_bytes(),
        );
    }

    #[test]
    fn unrelated_recipient_does_not_match() {
        // Recipient A's keys vs Recipient B's meta.
        let (spend_a, scan_a) = derive_stealth_keys(&[1u8; 64]).unwrap();
        let (spend_b, scan_b) = derive_stealth_keys(&[2u8; 64]).unwrap();
        let meta_b = MetaAddress::from_keys(&spend_b, &scan_b);

        let mut rng = ChaCha20Rng::seed_from_u64(7);
        // Try many payments-to-B; recipient A should fail the view tag
        // for ~255/256 of them, and never match the address even on a
        // 1/256 false-positive collision.
        let mut false_positive_hits = 0;
        for _ in 0..512 {
            let payment = derive_payment(&meta_b, &mut rng).unwrap();
            if let Some(m) = scan_note(&spend_a, &scan_a, &payment.ephemeral_pub, payment.view_tag).unwrap() {
                // View tag matched by coincidence — but the recovered
                // stealth address must NOT equal B's payment address.
                false_positive_hits += 1;
                assert_ne!(m.stealth_address, payment.stealth_address);
            }
        }
        // We expect ~512/256 = 2 false-positive view-tag hits on average.
        // This is non-zero confirmation that the view-tag filter is
        // probabilistic, and crucially each one was filtered out by
        // address mismatch.
        assert!(false_positive_hits < 20, "way more view-tag collisions than expected ({false_positive_hits})");
    }
}
```

- [ ] **Step 5.3: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo test -p umbra-sdk recipient:: 2>&1 | tail -10
```

Expected: 2 tests pass.

- [ ] **Step 5.4: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/src/lib.rs crates/umbra-sdk/src/recipient.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): recipient scan with view-tag filter + key recovery (spec §5)
EOF
)"
```

---

## Task 6: Stealth Ed25519 signing (scalar-mode)

**Files:**
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/stealth_signing.rs`
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs` (add `pub mod stealth_signing;`)

- [ ] **Step 6.1: Add module declaration**

Edit `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs`:

```rust
//! Umbra Rust SDK — v1 stealth-payment primitives on Solana.

pub mod error;
pub mod keys;
pub mod recipient;
pub mod sender;
pub mod stealth_signing;

pub use error::UmbraError;
```

- [ ] **Step 6.2: Create `stealth_signing.rs`**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/stealth_signing.rs`:

```rust
//! Ed25519 signing with a scalar-form private key (no RFC 8032 seed).
//!
//! Umbra's recipient sweep needs to sign Solana transactions from the
//! stealth address. The recipient holds `p_stealth` as a Scalar (per
//! spec §5), not an RFC 8032 seed.
//!
//! `ed25519-dalek` 2.x exposes a `hazmat` module, but its
//! `ExpandedSecretKey` only constructs via `from_bytes` which clamps
//! the scalar — that would corrupt our non-clamped `p_stealth`. So we
//! implement RFC 8032 signing directly here using `curve25519-dalek`
//! primitives. The resulting signature is bit-identical to what a
//! standard Ed25519 signer would produce for the same scalar/nonce,
//! and verifies cleanly against `ed25519_dalek::VerifyingKey::verify`.

use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT, EdwardsPoint, Scalar};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha512};

const NONCE_TAG: &[u8] = b"umbra-v1-nonce";

/// A scalar-form Ed25519 signing key.
pub struct StealthSigningKey {
    scalar: Scalar,
    public_point: EdwardsPoint,
    /// 32-byte input that, combined with the message, derives the
    /// RFC 8032 nonce. We compute it as `SHA-512(NONCE_TAG ||
    /// scalar)[32..64]` so signatures are deterministic but the
    /// scalar isn't directly exposed.
    hash_prefix: [u8; 32],
}

impl StealthSigningKey {
    pub fn new(scalar: Scalar) -> Self {
        let scalar_bytes = scalar.to_bytes();
        let mut hasher = Sha512::new();
        hasher.update(NONCE_TAG);
        hasher.update(scalar_bytes);
        let hash = hasher.finalize();

        let mut hash_prefix = [0u8; 32];
        hash_prefix.copy_from_slice(&hash[32..64]);

        let public_point = scalar * ED25519_BASEPOINT_POINT;

        Self { scalar, public_point, hash_prefix }
    }

    /// Compressed Ed25519 public bytes — equals the Solana address
    /// of the stealth account.
    pub fn public_bytes(&self) -> [u8; 32] {
        self.public_point.compress().to_bytes()
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey::from_bytes(&self.public_bytes())
            .expect("compressed point is valid by construction")
    }

    /// RFC 8032 Ed25519 sign with the scalar.
    ///
    ///   r = SHA-512(hash_prefix || message)  ⤳  reduce mod ℓ
    ///   R = r · G
    ///   k = SHA-512(R || A || message)        ⤳  reduce mod ℓ
    ///   s = r + k · scalar  (mod ℓ)
    ///   signature = R || s   (64 bytes)
    pub fn sign(&self, message: &[u8]) -> Signature {
        let a_compressed = self.public_point.compress();

        // r
        let mut h1 = Sha512::new();
        h1.update(self.hash_prefix);
        h1.update(message);
        let mut r_bytes = [0u8; 64];
        r_bytes.copy_from_slice(&h1.finalize());
        let r = Scalar::from_bytes_mod_order_wide(&r_bytes);

        // R = r · G
        let r_point = (r * ED25519_BASEPOINT_POINT).compress();

        // k
        let mut h2 = Sha512::new();
        h2.update(r_point.as_bytes());
        h2.update(a_compressed.as_bytes());
        h2.update(message);
        let mut k_bytes = [0u8; 64];
        k_bytes.copy_from_slice(&h2.finalize());
        let k = Scalar::from_bytes_mod_order_wide(&k_bytes);

        // s = r + k · scalar  (mod ℓ)
        let s = r + k * self.scalar;

        // signature bytes = R (32) || s (32)
        let mut sig_bytes = [0u8; 64];
        sig_bytes[..32].copy_from_slice(r_point.as_bytes());
        sig_bytes[32..].copy_from_slice(&s.to_bytes());
        Signature::from_bytes(&sig_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;
    use rand_chacha::ChaCha20Rng;
    use rand_core::{RngCore, SeedableRng};

    fn random_scalar(rng: &mut impl RngCore) -> Scalar {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        Scalar::from_bytes_mod_order(bytes)
    }

    #[test]
    fn sign_then_verify_via_dalek() {
        let mut rng = ChaCha20Rng::seed_from_u64(11);
        let scalar = random_scalar(&mut rng);
        let sk = StealthSigningKey::new(scalar);

        let msg = b"a stealth payment sweep tx";
        let sig = sk.sign(msg);
        // Verify using the standard ed25519-dalek path.
        let vk = sk.verifying_key();
        vk.verify(msg, &sig).expect("verification");
    }

    #[test]
    fn signature_is_deterministic() {
        let mut rng = ChaCha20Rng::seed_from_u64(11);
        let scalar = random_scalar(&mut rng);
        let sk1 = StealthSigningKey::new(scalar);
        let sk2 = StealthSigningKey::new(scalar);
        let msg = b"twice signed";
        assert_eq!(
            sk1.sign(msg).to_bytes(),
            sk2.sign(msg).to_bytes(),
            "signatures should be deterministic given the same scalar"
        );
    }

    #[test]
    fn public_bytes_match_scalar_times_basepoint() {
        let mut rng = ChaCha20Rng::seed_from_u64(11);
        let scalar = random_scalar(&mut rng);
        let sk = StealthSigningKey::new(scalar);
        let expected = (scalar * ED25519_BASEPOINT_POINT).compress().to_bytes();
        assert_eq!(sk.public_bytes(), expected);
    }
}
```

- [ ] **Step 6.3: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo test -p umbra-sdk stealth_signing:: 2>&1 | tail -15
```

Expected: 3 tests pass.

Failure mode to watch for: if `Signature::from_bytes(&sig_bytes)` returns a `Result` instead of a `Signature` in your dalek version, unwrap with `.expect("valid 64-byte signature")`. The 2.1.x release I'm pinning returns the `Signature` directly via an infallible constructor.

- [ ] **Step 6.4: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/src/lib.rs crates/umbra-sdk/src/stealth_signing.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): scalar-mode Ed25519 signing via dalek hazmat
EOF
)"
```

---

## Task 7: Pinboard `post` instruction builder

**Files:**
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/pinboard.rs`
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs` (add `pub mod pinboard;`)

- [ ] **Step 7.1: Add module declaration**

Edit `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/lib.rs`:

```rust
//! Umbra Rust SDK — v1 stealth-payment primitives on Solana.

pub mod error;
pub mod keys;
pub mod pinboard;
pub mod recipient;
pub mod sender;
pub mod stealth_signing;

pub use error::UmbraError;
```

- [ ] **Step 7.2: Create `pinboard.rs`**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/src/pinboard.rs`:

```rust
//! Build instructions and parse events for the pinboard program.
//!
//! We don't depend on `anchor-client` (heavy, version-coupled). We
//! hand-build the instruction using Anchor's 8-byte discriminator
//! (`SHA-256("global:<instruction_snake>")[..8]`) followed by borsh-
//! serialized args. For events, the on-chain logs contain
//! `Program data: <base64>` where the payload is
//! `SHA-256("event:<EventName>")[..8] || borsh(event)`.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

/// `SHA-256("global:post")[..8]`. Verified against `target/idl/pinboard.json`.
pub const POST_DISCRIMINATOR: [u8; 8] =
    [223, 96, 234, 236, 158, 106, 145, 94];

/// `SHA-256("event:Note")[..8]`.
pub const NOTE_EVENT_DISCRIMINATOR: [u8; 8] =
    [40, 182, 5, 151, 115, 43, 27, 97];

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PostArgs {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

/// On-chain `Note` event, matching `programs/pinboard/src/lib.rs`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NoteEvent {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

/// Build a `pinboard.post(...)` instruction.
pub fn build_post_instruction(
    pinboard_program_id: &Pubkey,
    fee_payer: &Pubkey,
    scheme_id: u16,
    ephemeral_pub: [u8; 32],
    view_tag: u8,
    metadata: Vec<u8>,
) -> Instruction {
    let args = PostArgs { scheme_id, ephemeral_pub, view_tag, metadata };
    let mut data = Vec::with_capacity(8 + 2 + 32 + 1 + 4 + args.metadata.len());
    data.extend_from_slice(&POST_DISCRIMINATOR);
    borsh::to_writer(&mut data, &args).expect("borsh serialize PostArgs");
    Instruction {
        program_id: *pinboard_program_id,
        accounts: vec![AccountMeta::new(*fee_payer, true)],
        data,
    }
}

/// Parse a `Program data: <base64>` log line into a `NoteEvent`.
///
/// Returns `Ok(None)` if the line is not a `Program data:` line or if
/// the discriminator doesn't match `Note`. Returns `Err` if the line
/// looks like a Note event but fails to deserialize.
pub fn try_parse_note_log(line: &str) -> Result<Option<NoteEvent>, String> {
    const PREFIX: &str = "Program data: ";
    let Some(b64) = line.strip_prefix(PREFIX) else {
        return Ok(None);
    };
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("base64 decode: {e}"))?;
    if raw.len() < 8 {
        return Ok(None);
    }
    if &raw[..8] != NOTE_EVENT_DISCRIMINATOR {
        return Ok(None);
    }
    let event = NoteEvent::try_from_slice(&raw[8..])
        .map_err(|e| format!("borsh deserialize NoteEvent: {e}"))?;
    Ok(Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn post_discriminator_matches_anchor_convention() {
        let mut h = Sha256::new();
        h.update(b"global:post");
        let computed = h.finalize();
        assert_eq!(&computed[..8], &POST_DISCRIMINATOR);
    }

    #[test]
    fn note_event_discriminator_matches_anchor_convention() {
        let mut h = Sha256::new();
        h.update(b"event:Note");
        let computed = h.finalize();
        assert_eq!(&computed[..8], &NOTE_EVENT_DISCRIMINATOR);
    }

    #[test]
    fn note_event_roundtrip_through_log() {
        let original = NoteEvent {
            scheme_id: 1,
            ephemeral_pub: [7u8; 32],
            view_tag: 0x42,
            metadata: vec![0xab, 0xcd],
        };
        // Build a synthetic Program data line.
        let mut payload = Vec::new();
        payload.extend_from_slice(&NOTE_EVENT_DISCRIMINATOR);
        borsh::to_writer(&mut payload, &original).unwrap();
        use base64::Engine;
        let line = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(&payload),
        );
        let parsed = try_parse_note_log(&line).unwrap().unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn non_program_data_line_is_none() {
        let result = try_parse_note_log("Program log: hello").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn other_event_discriminator_is_none() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8; 8]); // wrong discriminator
        payload.extend_from_slice(&[1, 2, 3, 4]);
        use base64::Engine;
        let line = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(&payload),
        );
        assert!(try_parse_note_log(&line).unwrap().is_none());
    }
}
```

- [ ] **Step 7.3: Add `base64` to the SDK's `Cargo.toml` deps**

Edit `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/Cargo.toml`'s `[dependencies]` block to add `base64`:

```toml
[dependencies]
curve25519-dalek = { version = "4.1", features = ["digest"] }
ed25519-dalek = { version = "2.1", features = ["hazmat", "rand_core"] }
x25519-dalek = { version = "2", features = ["static_secrets"] }
sha2 = "0.10"
hkdf = "0.12"
bech32 = "0.11"
borsh = { version = "1", features = ["derive"] }
solana-sdk = "2.3"
rand_core = "0.6"
thiserror = "1"
base64 = "0.22"
```

- [ ] **Step 7.4: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo test -p umbra-sdk pinboard:: 2>&1 | tail -15
```

Expected: 5 tests pass.

- [ ] **Step 7.5: Run the entire library test suite to verify no module is broken**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo test -p umbra-sdk 2>&1 | tail -10
```

Expected: 18 tests pass (3 + 5 + 2 + 2 + 3 + 5 — adjust if counts differ slightly).

- [ ] **Step 7.6: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/src/lib.rs crates/umbra-sdk/src/pinboard.rs crates/umbra-sdk/Cargo.toml Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): pinboard post instruction builder + Note event parser
EOF
)"
```

---

## Task 8: Demo binary — setup phase (keypairs, airdrops, helpers)

**Files:**
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/examples/lifecycle.rs`
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/Cargo.toml` (add `solana-client` to deps under a `[dev-dependencies]` or move it up — see step 8.1)

- [ ] **Step 8.1: Add `solana-client` to the SDK's deps**

Examples are dev-targets in Cargo and inherit dev-dependencies. Edit `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/Cargo.toml` to extend `[dev-dependencies]`:

```toml
[dev-dependencies]
rand_chacha = "0.3"
hex = "0.4"
solana-client = "2.3"
solana-system-interface = "1"
```

(`solana-system-interface` is where `SystemInstruction::transfer` lives in solana 2.3.x — we'll need it for the demo.)

- [ ] **Step 8.2: Create `examples/lifecycle.rs` with just the setup phase**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/examples/lifecycle.rs`:

```rust
//! End-to-end stealth-payment lifecycle demo.
//!
//! Run against a fresh `solana-test-validator` with the pinboard program
//! preloaded. See `scripts/demo-lifecycle.sh` for the orchestration.
//!
//! Stages:
//!   1. Setup        — create keypairs, airdrop SOL
//!   2. Recipient    — derive stealth keys, emit meta-address
//!   3. Sender       — derive stealth address, transfer SOL, post note
//!   4. Recipient    — scan pinboard logs, recover scalar
//!   5. Recipient    — sweep stealth address to main wallet
//!   6. Verification — assert balances

use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{str::FromStr, time::Duration};

const RPC_URL: &str = "http://127.0.0.1:8899";

/// Pinboard program ID (current dev keypair; baked in for demo
/// reproducibility — the shell wrapper deploys it).
const PINBOARD_PROGRAM_ID: &str = "G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P";

const ONE_SOL: u64 = 1_000_000_000;

fn main() {
    let rpc = RpcClient::new_with_commitment(
        RPC_URL.to_string(),
        CommitmentConfig::confirmed(),
    );

    println!("== Umbra lifecycle demo ==");
    println!("RPC: {}", RPC_URL);
    let pinboard_id = Pubkey::from_str(PINBOARD_PROGRAM_ID)
        .expect("PINBOARD_PROGRAM_ID parse");
    println!("pinboard program: {pinboard_id}");

    // 1. Setup: keypairs + airdrops.
    println!("\n[1/6] setup: creating keypairs");
    let sender_wallet = Keypair::new();
    let recipient_wallet = Keypair::new();
    println!("  sender:    {}", sender_wallet.pubkey());
    println!("  recipient: {}", recipient_wallet.pubkey());

    airdrop_blocking(&rpc, &sender_wallet.pubkey(), 10 * ONE_SOL);
    airdrop_blocking(&rpc, &recipient_wallet.pubkey(), 10 * ONE_SOL);
    println!("  airdropped 10 SOL to each");

    // Sanity check: balances are visible.
    println!(
        "  sender balance after airdrop:    {} lamports",
        rpc.get_balance(&sender_wallet.pubkey()).expect("get_balance")
    );
    println!(
        "  recipient balance after airdrop: {} lamports",
        rpc.get_balance(&recipient_wallet.pubkey()).expect("get_balance")
    );

    // Suppress unused warnings until later tasks wire these in.
    let _ = ChaCha20Rng::seed_from_u64(0);
    let _ = pinboard_id;
}

/// Request an airdrop and poll until the balance is at least
/// `min_lamports`. Panics on RPC error or 30s timeout.
fn airdrop_blocking(rpc: &RpcClient, recipient: &Pubkey, lamports: u64) {
    let sig = rpc
        .request_airdrop(recipient, lamports)
        .expect("request_airdrop");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if rpc.confirm_transaction(&sig).unwrap_or(false) {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("airdrop {sig} did not confirm within 30s");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    // Confirmation does not always mean the balance updated; poll.
    let bal_deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let bal = rpc.get_balance(recipient).unwrap_or(0);
        if bal >= lamports {
            return;
        }
        if std::time::Instant::now() > bal_deadline {
            panic!("airdrop balance did not appear within 10s");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
```

- [ ] **Step 8.3: Build the example (no run yet)**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo build -p umbra-sdk --example lifecycle 2>&1 | tail -10
```

Expected: clean build. If a Solana-SDK-version mismatch surfaces in error messages, run `cargo tree -p umbra-sdk` to inspect.

- [ ] **Step 8.4: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/Cargo.toml crates/umbra-sdk/examples/lifecycle.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): lifecycle demo scaffolding (RPC setup + airdrops)
EOF
)"
```

---

## Task 9: Demo binary — recipient setup, sender payment, on-chain post

**Files:**
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/examples/lifecycle.rs`

- [ ] **Step 9.1: Replace `main()` with the full sender-side flow**

Edit `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/examples/lifecycle.rs`. Replace the `main()` function (keep `airdrop_blocking` as-is at the bottom) with:

```rust
fn main() {
    let rpc = RpcClient::new_with_commitment(
        RPC_URL.to_string(),
        CommitmentConfig::confirmed(),
    );

    println!("== Umbra lifecycle demo ==");
    println!("RPC: {RPC_URL}");
    let pinboard_id = Pubkey::from_str(PINBOARD_PROGRAM_ID)
        .expect("PINBOARD_PROGRAM_ID parse");
    println!("pinboard program: {pinboard_id}");

    // ---- 1. Setup ----
    println!("\n[1/6] setup: creating keypairs");
    let sender_wallet = Keypair::new();
    let recipient_wallet = Keypair::new();
    println!("  sender:    {}", sender_wallet.pubkey());
    println!("  recipient: {}", recipient_wallet.pubkey());

    airdrop_blocking(&rpc, &sender_wallet.pubkey(), 10 * ONE_SOL);
    airdrop_blocking(&rpc, &recipient_wallet.pubkey(), 10 * ONE_SOL);

    // ---- 2. Recipient: derive stealth keys + meta-address ----
    println!("\n[2/6] recipient: deriving stealth keys");
    // For the demo, "sign" the canonical message with a fresh Ed25519
    // keypair derived from a fixed seed. In production this would be
    // a user wallet signature.
    let canonical_msg = umbra_sdk::keys::CANONICAL_MESSAGE_LOCALNET.as_bytes();
    let recipient_id_seed: [u8; 32] = [
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
    ];
    let recipient_id_sk =
        ed25519_dalek::SigningKey::from_bytes(&recipient_id_seed);
    // `SigningKey` has an inherent `sign` method in ed25519-dalek 2.x.
    let signature: ed25519_dalek::Signature = recipient_id_sk.sign(canonical_msg);
    let sig_bytes: [u8; 64] = signature.to_bytes();

    let (spend, scan) = umbra_sdk::keys::derive_stealth_keys(&sig_bytes)
        .expect("derive_stealth_keys");
    let meta = umbra_sdk::keys::MetaAddress::from_keys(&spend, &scan);
    let meta_str = meta.encode_bech32m().expect("encode meta-address");
    println!("  meta-address: {meta_str}");

    // ---- 3. Sender: derive stealth address ----
    println!("\n[3/6] sender: deriving stealth address");
    let decoded_meta =
        umbra_sdk::keys::MetaAddress::decode_bech32m(&meta_str)
            .expect("decode meta-address");
    // Use a strong RNG in production. Seeded here so demo output is
    // reproducible across runs.
    let mut sender_rng = ChaCha20Rng::seed_from_u64(0xdeadbeef);
    let payment = umbra_sdk::sender::derive_payment(&decoded_meta, &mut sender_rng)
        .expect("derive_payment");
    println!("  stealth address: {}", payment.stealth_address);
    println!("  ephemeral_pub:   {}", hex::encode(payment.ephemeral_pub));
    println!("  view_tag:        0x{:02x}", payment.view_tag);

    // ---- 4. Sender: transfer SOL + post pinboard Note in one tx ----
    println!("\n[4/6] sender: sending 1 SOL + posting note");
    let transfer_ix = solana_system_interface::instruction::transfer(
        &sender_wallet.pubkey(),
        &payment.stealth_address,
        ONE_SOL,
    );
    let post_ix = umbra_sdk::pinboard::build_post_instruction(
        &pinboard_id,
        &sender_wallet.pubkey(),
        umbra_sdk::keys::SCHEME_ID_V1,
        payment.ephemeral_pub,
        payment.view_tag,
        vec![], // metadata: empty for demo
    );
    let latest_blockhash =
        rpc.get_latest_blockhash().expect("get_latest_blockhash");
    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[transfer_ix, post_ix],
        Some(&sender_wallet.pubkey()),
        &[&sender_wallet],
        latest_blockhash,
    );
    let sig = rpc
        .send_and_confirm_transaction(&tx)
        .expect("send_and_confirm_transaction (payment + post)");
    println!("  payment tx: {sig}");
    let stealth_balance = rpc
        .get_balance(&payment.stealth_address)
        .expect("get_balance stealth");
    println!("  stealth balance: {stealth_balance} lamports");
    assert_eq!(stealth_balance, ONE_SOL);

    // Defer task 10 stages — placeholders.
    println!("\n[5/6] recipient: scanning … (next task)");
    println!("[6/6] recipient: sweeping  … (next task)");
    let _ = (recipient_wallet, spend, scan); // suppress unused warnings
}
```

- [ ] **Step 9.2: Add imports near the top of the file**

Replace the existing `use` block at the top with:

```rust
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{str::FromStr, time::Duration};
```

(Same as before; restate to keep the file consistent.)

- [ ] **Step 9.3: Build to verify**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo build -p umbra-sdk --example lifecycle 2>&1 | tail -10
```

Expected: clean build (warnings about `recipient_wallet` etc. are fine; they'll be used in Task 10).

If `solana_system_interface::instruction::transfer` fails to resolve, replace with `solana_sdk::system_instruction::transfer(...)` (older path; works in 2.x) and remove `solana-system-interface` from dev-deps.

- [ ] **Step 9.4: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/Cargo.toml crates/umbra-sdk/examples/lifecycle.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): lifecycle demo — sender flow (transfer + post)
EOF
)"
```

---

## Task 10: Demo binary — recipient scan + sweep

**Files:**
- Modify: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/examples/lifecycle.rs`

- [ ] **Step 10.1: Replace the Task-9 placeholder section with full scan-and-sweep**

In `examples/lifecycle.rs`, replace this block:

```rust
    // Defer task 10 stages — placeholders.
    println!("\n[5/6] recipient: scanning … (next task)");
    println!("[6/6] recipient: sweeping  … (next task)");
    let _ = (recipient_wallet, spend, scan); // suppress unused warnings
}
```

with:

```rust
    // ---- 5. Recipient: scan pinboard logs ----
    println!("\n[5/6] recipient: scanning pinboard logs");
    let matched = scan_pinboard_for_match(&rpc, &pinboard_id, &spend, &scan)
        .expect("scan returned a match");
    println!("  found match: stealth address {}", matched.stealth_address);
    assert_eq!(matched.stealth_address, payment.stealth_address);

    // ---- 6. Recipient: sweep stealth address ----
    println!("\n[6/6] recipient: sweeping stealth balance to main wallet");
    let stealth_signing_key =
        umbra_sdk::stealth_signing::StealthSigningKey::new(matched.stealth_scalar);
    // Sanity check before we sign anything: the signing key's public
    // bytes must equal the stealth address bytes.
    assert_eq!(
        stealth_signing_key.public_bytes(),
        payment.stealth_address.to_bytes(),
    );

    let recipient_before = rpc
        .get_balance(&recipient_wallet.pubkey())
        .expect("recipient balance before");
    let stealth_before = rpc
        .get_balance(&payment.stealth_address)
        .expect("stealth balance before");
    const TX_FEE: u64 = 5_000;
    let sweep_amount = stealth_before - TX_FEE;

    let sweep_ix = solana_system_interface::instruction::transfer(
        &payment.stealth_address,
        &recipient_wallet.pubkey(),
        sweep_amount,
    );
    let latest_blockhash = rpc
        .get_latest_blockhash()
        .expect("get_latest_blockhash for sweep");

    // Build the message and sign it manually using our scalar-mode key.
    let message = solana_sdk::message::Message::new_with_blockhash(
        &[sweep_ix],
        Some(&payment.stealth_address),
        &latest_blockhash,
    );
    let message_bytes = message.serialize();
    let ed_sig = stealth_signing_key.sign(&message_bytes);
    let signature = solana_sdk::signature::Signature::from(ed_sig.to_bytes());

    let mut sweep_tx = solana_sdk::transaction::Transaction {
        signatures: vec![signature],
        message,
    };
    // Solana's local validator double-checks signatures during simulation;
    // verify ours locally too to fail fast if anything is off.
    sweep_tx
        .verify()
        .expect("locally-built sweep tx must verify");

    let sweep_sig = rpc
        .send_and_confirm_transaction(&sweep_tx)
        .expect("send_and_confirm_transaction (sweep)");
    println!("  sweep tx: {sweep_sig}");

    let recipient_after = rpc
        .get_balance(&recipient_wallet.pubkey())
        .expect("recipient balance after");
    let stealth_after = rpc
        .get_balance(&payment.stealth_address)
        .expect("stealth balance after");
    println!("  recipient balance: {recipient_before} → {recipient_after}");
    println!("  stealth balance:   {stealth_before} → {stealth_after}");

    // ---- Verification ----
    assert_eq!(stealth_after, 0, "stealth account should drain to 0");
    let recipient_gain = recipient_after - recipient_before;
    assert_eq!(
        recipient_gain, sweep_amount,
        "recipient should gain exactly the swept lamports"
    );
    println!("\n== SUCCESS: stealth payment delivered and swept ==");
    println!("   {} lamports moved to recipient through a stealth address", recipient_gain);
}

/// Scan recent pinboard transactions, parse Note events, and try
/// `scan_note` until we find one for the given (spend, scan) pair.
/// Times out after ~10 seconds of polling.
fn scan_pinboard_for_match(
    rpc: &RpcClient,
    pinboard_id: &Pubkey,
    spend: &umbra_sdk::keys::SpendKey,
    scan: &umbra_sdk::keys::ScanKey,
) -> Option<umbra_sdk::recipient::NoteMatch> {
    use solana_client::rpc_config::GetConfirmedSignaturesForAddress2Config;
    use solana_sdk::commitment_config::CommitmentConfig;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let cfg = GetConfirmedSignaturesForAddress2Config {
            before: None,
            until: None,
            limit: Some(100),
            commitment: Some(CommitmentConfig::confirmed()),
        };
        let sigs = rpc
            .get_signatures_for_address_with_config(pinboard_id, cfg)
            .unwrap_or_default();
        for sig_info in &sigs {
            let sig = sig_info
                .signature
                .parse::<solana_sdk::signature::Signature>()
                .ok();
            let Some(sig) = sig else { continue };
            let tx = rpc.get_transaction_with_config(
                &sig,
                solana_client::rpc_config::RpcTransactionConfig {
                    encoding: Some(solana_transaction_status::UiTransactionEncoding::Json),
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(0),
                },
            );
            let Ok(tx) = tx else { continue };
            // log_messages is `OptionSerializer<Vec<String>>` in current
            // solana-transaction-status. `Into<Option<...>>` is implemented;
            // failing that, see the fallback in Step 10.3.
            let logs: Vec<String> = tx
                .transaction
                .meta
                .map(|m| {
                    let opt: Option<Vec<String>> = m.log_messages.into();
                    opt.unwrap_or_default()
                })
                .unwrap_or_default();
            for line in logs {
                if let Ok(Some(note)) = umbra_sdk::pinboard::try_parse_note_log(&line) {
                    if let Ok(Some(m)) = umbra_sdk::recipient::scan_note(
                        spend, scan, &note.ephemeral_pub, note.view_tag,
                    ) {
                        return Some(m);
                    }
                }
            }
        }
        if std::time::Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
```

- [ ] **Step 10.2: Add the missing crate dep for transaction-status parsing**

The new `scan_pinboard_for_match` uses `solana_transaction_status`. Add it to `[dev-dependencies]` in `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/Cargo.toml`:

```toml
[dev-dependencies]
rand_chacha = "0.3"
hex = "0.4"
solana-client = "2.3"
solana-system-interface = "1"
solana-transaction-status = "2.3"
ed25519-dalek = { version = "2.1", features = ["hazmat", "rand_core"] }
```

- [ ] **Step 10.3: Build**

```bash
cd /Users/susruth/Documents/Projects/umbra
cargo build -p umbra-sdk --example lifecycle 2>&1 | tail -10
```

Expected: clean build.

If `m.log_messages.into()` doesn't compile (no `Into<Option<Vec<String>>>` impl in your `solana-transaction-status` version), pattern-match on `OptionSerializer` directly:

```rust
use solana_transaction_status::option_serializer::OptionSerializer;

let logs: Vec<String> = tx
    .transaction
    .meta
    .map(|m| match m.log_messages {
        OptionSerializer::Some(v) => v,
        _ => Vec::new(),
    })
    .unwrap_or_default();
```

- [ ] **Step 10.4: Commit** (do NOT run the demo yet — that's the shell-script task)

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/Cargo.toml crates/umbra-sdk/examples/lifecycle.rs Cargo.lock
git commit -m "$(cat <<'EOF'
feat(umbra-sdk): lifecycle demo — recipient scan + sweep with stealth signing
EOF
)"
```

---

## Task 11: Shell wrapper — start validator, deploy pinboard, run demo

**Files:**
- Create: `/Users/susruth/Documents/Projects/umbra/scripts/demo-lifecycle.sh`

- [ ] **Step 11.1: Create the wrapper script**

Create `/Users/susruth/Documents/Projects/umbra/scripts/demo-lifecycle.sh`:

```bash
#!/usr/bin/env bash
# End-to-end Umbra stealth-payment lifecycle demo.
#
# Starts a fresh solana-test-validator with the pinboard program loaded
# at G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P, runs the
# `lifecycle` example, then tears down.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROGRAM_ID="G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P"
PROGRAM_SO="target/deploy/pinboard.so"

# 1. Ensure pinboard is built.
if [[ ! -f "$PROGRAM_SO" ]]; then
  echo "Pinboard .so not found; building..."
  ./scripts/build.sh
fi

# 2. Stop any stray validator from a previous run.
pkill -f solana-test-validator 2>/dev/null || true
rm -rf test-ledger 2>/dev/null || true

# 3. Start the validator with pinboard preloaded.
echo "Starting solana-test-validator..."
solana-test-validator \
  --bpf-program "$PROGRAM_ID" "$PROGRAM_SO" \
  --reset \
  --quiet \
  > /tmp/umbra-lifecycle-validator.log 2>&1 &
VALIDATOR_PID=$!
trap 'kill "$VALIDATOR_PID" 2>/dev/null || true; rm -rf test-ledger 2>/dev/null || true' EXIT

# 4. Wait for it to be ready.
echo "Waiting for validator (max 60s)..."
for i in {1..120}; do
  if solana --url http://127.0.0.1:8899 cluster-version > /dev/null 2>&1; then
    echo "Validator ready (after ~$((i / 2))s)"
    break
  fi
  if [[ $i -eq 120 ]]; then
    echo "Validator did not become ready in 60s. Last log lines:"
    tail -20 /tmp/umbra-lifecycle-validator.log
    exit 1
  fi
  sleep 0.5
done

# 5. Run the lifecycle.
echo
cargo run --release \
  --manifest-path crates/umbra-sdk/Cargo.toml \
  --example lifecycle

# 6. Cleanup happens via the EXIT trap.
echo
echo "Demo complete — tearing down validator."
```

- [ ] **Step 11.2: Make the script executable**

```bash
chmod +x /Users/susruth/Documents/Projects/umbra/scripts/demo-lifecycle.sh
```

- [ ] **Step 11.3: Run it end-to-end**

```bash
cd /Users/susruth/Documents/Projects/umbra
./scripts/demo-lifecycle.sh 2>&1 | tail -50
```

Expected: The validator boots, the example runs, prints `== SUCCESS: stealth payment delivered and swept ==`, and a non-zero `recipient_gain` line.

Possible failure modes and remedies:
- "Validator did not become ready in 60s" → check whether `solana-test-validator` is on `PATH`. Run `which solana-test-validator`.
- "Program not deployed" inside the example → the `--bpf-program` flag's program ID and `.so` path must both exist. Verify `target/deploy/pinboard.so` exists and matches the keypair at `target/deploy/pinboard-keypair.json`.
- "InstructionError(0, IncorrectProgramId)" → the post discriminator is wrong (check Task 7's constants against `target/idl/pinboard.json`).
- "InstructionError(SignatureFailure)" → the stealth-signing path produced an invalid Ed25519 signature for the message bytes. Check that you serialized the `Message` and not the full `Transaction`.
- "AccountNotFound" or "insufficient lamports" on the sweep → make sure the stealth balance equals `ONE_SOL` before sweeping; if it doesn't, the transfer instruction in the sender's tx didn't land. Inspect the tx with `solana confirm <tx_sig>`.

- [ ] **Step 11.4: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add scripts/demo-lifecycle.sh
git commit -m "$(cat <<'EOF'
chore(umbra-sdk): shell wrapper for end-to-end lifecycle demo
EOF
)"
```

---

## Task 12: SDK README

**Files:**
- Create: `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/README.md`

- [ ] **Step 12.1: Write the README**

Create `/Users/susruth/Documents/Projects/umbra/crates/umbra-sdk/README.md`:

```markdown
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
```

- [ ] **Step 12.2: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add crates/umbra-sdk/README.md
git commit -m "$(cat <<'EOF'
docs(umbra-sdk): add README with module map and demo instructions
EOF
)"
```

---

## Summary

After all 12 tasks:

- **`crates/umbra-sdk/`** — a Rust library implementing Umbra v1 §§3–6
- **`examples/lifecycle.rs`** — runnable demo of the full lifecycle
- **`scripts/demo-lifecycle.sh`** — end-to-end shell orchestration
- **~21 unit tests** + a passing live lifecycle
- **12 commits** marking each milestone
- **0 placeholders** — every step has exact code or commands

What's next (separate plans, in priority order):
1. Relayer-paid sweep (close spec §9.2's gap; remove the self-pay simplification)
2. Labels (BIP-352 style multi-meta-address per §3.3)
3. SPL token + NFT sweeps (§7.2, §7.3, §9.1 SPL variant)
4. TypeScript SDK mirroring this Rust shape
5. Reference indexer (§10.2)
6. Reference announcement-publishing service (§8.3)
7. Hardware-wallet adapter
