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
}
