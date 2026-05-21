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
