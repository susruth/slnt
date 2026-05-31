//! Key derivation and meta-address codec (sRFC-0042 §5.2).

use crate::error::SlntError;
use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT, EdwardsPoint, Scalar};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha512};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

/// Solana network the stealth identity is bound to. The network name is
/// substituted verbatim into the canonical message (sRFC-0042 §5.2.1.2)
/// so that keys differ per network and devnet experiments cannot leak a
/// mainnet stealth identity.
///
/// `Mainnet`/`Devnet`/`Testnet` are the spec-enumerated values.
/// `Localnet` is a non-spec convenience for local validators and demos;
/// identities derived under it are not portable to a conforming wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Mainnet,
    Devnet,
    Testnet,
    Localnet,
}

impl Network {
    /// The exact token substituted into the `Network:` line.
    pub fn label(self) -> &'static str {
        match self {
            Network::Mainnet => "Mainnet",
            Network::Devnet => "Devnet",
            Network::Testnet => "Testnet",
            Network::Localnet => "Localnet",
        }
    }
}

/// Build the sRFC-0042 §5.2.1.2 canonical message for `network`.
///
/// Exact UTF-8, no trailing newline (the string ends after "ability.").
pub fn canonical_message(network: Network) -> String {
    format!(
        "Slnt Protocol: Derive Stealth Keys\n\nVersion: 1\nNetwork: {}\nWarning: Only sign this message in the Slnt wallet or a trusted Slnt integration.\nSigning this in any other context will reveal your stealth address scanning ability.",
        network.label()
    )
}

/// Convenience: the Localnet canonical message, for demos and tests
/// against a local validator. Not a spec-conformant network.
pub const CANONICAL_MESSAGE_LOCALNET: &str = "Slnt Protocol: Derive Stealth Keys\n\nVersion: 1\nNetwork: Localnet\nWarning: Only sign this message in the Slnt wallet or a trusted Slnt integration.\nSigning this in any other context will reveal your stealth address scanning ability.";

pub const META_ADDRESS_VERSION_V1: u8 = 0x01;
pub const SCHEME_ID_V1: u16 = 0x0001;

const HKDF_SALT_DERIVE: &[u8] = b"slnt-v1-derive";
const HKDF_INFO_SPEND_AND_SCAN: &[u8] = b"spend-and-scan";
const HKDF_SALT_LABEL: &[u8] = b"slnt-v1-label";

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
/// published in sRFC-0042 §5.10 view-key delegation) and the clamped form
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

    /// Reconstruct a (view-only) scan key from the raw 32-byte scan
    /// material `b_scan_raw`. Used by a delegated scanner (sRFC-0042
    /// §5.10): it can run the ECDH + view-tag filter but holds no spend
    /// key, so it can never spend.
    pub fn from_raw(raw: [u8; 32]) -> Self {
        let static_secret = X25519StaticSecret::from(raw);
        let public = X25519PublicKey::from(&static_secret);
        Self {
            raw,
            static_secret,
            public,
        }
    }
}

/// sRFC-0042 §5.2.1.2 (Method 2) derivation:
///   ikm = signature
///   seed = HKDF-SHA256(salt="slnt-v1-derive", ikm, info="spend-and-scan", L=64)
///   b_spend = SC25519_reduce(seed[0..32])
///   b_scan_raw = seed[32..64]
///   B_spend = b_spend * G_ed
///   b_scan = X25519_clamp(b_scan_raw); B_scan = b_scan * G_x
pub fn derive_stealth_keys(signature_64: &[u8; 64]) -> Result<(SpendKey, ScanKey), SlntError> {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT_DERIVE), signature_64);
    let mut seed = [0u8; 64];
    hk.expand(HKDF_INFO_SPEND_AND_SCAN, &mut seed)
        .map_err(|_| SlntError::Derivation)?;

    let mut b_spend_raw = [0u8; 32];
    b_spend_raw.copy_from_slice(&seed[0..32]);
    let mut b_scan_raw = [0u8; 32];
    b_scan_raw.copy_from_slice(&seed[32..64]);

    keys_from_secrets(b_spend_raw, b_scan_raw)
}

/// Method 2 with a determinism guard (sRFC-0042 §8.5).
///
/// Method 2 recoverability depends on deterministic Ed25519 signing of
/// the canonical message. Callers obtain two independent signatures of
/// the **same** canonical message; if they differ, the wallet is a
/// randomized signer and MUST NOT be used. On a match, derives keys from
/// the (deterministic) signature.
pub fn derive_stealth_keys_checked(
    signature_64: &[u8; 64],
    confirmation_64: &[u8; 64],
) -> Result<(SpendKey, ScanKey), SlntError> {
    if signature_64 != confirmation_64 {
        return Err(SlntError::NonDeterministicSignature);
    }
    derive_stealth_keys(signature_64)
}

/// SLNT HD purpose: ASCII bytes "SLNT" (sRFC-0042 §5.2.1.1).
pub const SLNT_HD_PURPOSE: u32 = 0x534C_4E54;
/// Solana SLIP-0044 coin type.
pub const SOLANA_COIN_TYPE: u32 = 501;

/// Method 1 — wallet-native HD derivation (sRFC-0042 §5.2.1.1).
///
/// Derives the spend/scan secrets directly from a BIP-39 `seed` (the
/// 64-byte output of the mnemonic), at the dedicated SLNT branch:
///
/// ```text
/// spend:  m / 0x534C4E54' / 501' / account' / 0'
/// scan:   m / 0x534C4E54' / 501' / account' / 1'
/// ```
///
/// using SLIP-0010 for ed25519 (every level hardened). The two 32-byte
/// node values are used directly as the spend/scan secrets — no HKDF
/// step — then mapped to keys by §5.2.1.3.
pub fn derive_stealth_keys_hd(seed: &[u8], account: u32) -> Result<(SpendKey, ScanKey), SlntError> {
    if seed.len() < 16 || seed.len() > 64 {
        return Err(SlntError::InvalidSeedLength(seed.len()));
    }
    let base = [
        harden(SLNT_HD_PURPOSE),
        harden(SOLANA_COIN_TYPE),
        harden(account),
    ];
    let mut spend_path = base.to_vec();
    spend_path.push(harden(0)); // 0' = spend
    let mut scan_path = base.to_vec();
    scan_path.push(harden(1)); // 1' = scan

    let b_spend_raw = slip10_ed25519_node(seed, &spend_path);
    let b_scan_raw = slip10_ed25519_node(seed, &scan_path);

    keys_from_secrets(b_spend_raw, b_scan_raw)
}

/// Map the two 32-byte secrets to a `(SpendKey, ScanKey)` pair, common
/// to both derivation methods (sRFC-0042 §5.2.1.3).
fn keys_from_secrets(
    b_spend_raw: [u8; 32],
    b_scan_raw: [u8; 32],
) -> Result<(SpendKey, ScanKey), SlntError> {
    // SC25519_reduce: 32 bytes → Ed25519 scalar mod ℓ.
    let b_spend = Scalar::from_bytes_mod_order(b_spend_raw);
    if b_spend == Scalar::ZERO {
        return Err(SlntError::Derivation);
    }
    let b_spend_point = b_spend * ED25519_BASEPOINT_POINT;

    // X25519 clamping happens inside StaticSecret::from(...).
    let scan_static = X25519StaticSecret::from(b_scan_raw);
    let scan_public = X25519PublicKey::from(&scan_static);

    Ok((
        SpendKey {
            scalar: b_spend,
            point: b_spend_point,
        },
        ScanKey {
            raw: b_scan_raw,
            static_secret: scan_static,
            public: scan_public,
        },
    ))
}

/// Apply the SLIP-0010 hardened-derivation offset to a raw index.
fn harden(index: u32) -> u32 {
    index | 0x8000_0000
}

type HmacSha512 = Hmac<Sha512>;

/// SLIP-0010 ed25519 key derivation. `path` entries are already-hardened
/// indices (ed25519 supports only hardened derivation). Returns the
/// 32-byte private key (`I_L`) at the requested node.
fn slip10_ed25519_node(seed: &[u8], path: &[u32]) -> [u8; 32] {
    // Master: I = HMAC-SHA512(key = "ed25519 seed", data = seed)
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed").expect("hmac accepts any key length");
    mac.update(seed);
    let i = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    let mut chain = [0u8; 32];
    key.copy_from_slice(&i[0..32]);
    chain.copy_from_slice(&i[32..64]);

    // Each child: I = HMAC-SHA512(key = chain, data = 0x00 || key || ser32(i))
    for &index in path {
        let mut mac = HmacSha512::new_from_slice(&chain).expect("32-byte key");
        mac.update(&[0u8]);
        mac.update(&key);
        mac.update(&index.to_be_bytes());
        let i = mac.finalize().into_bytes();
        key.copy_from_slice(&i[0..32]);
        chain.copy_from_slice(&i[32..64]);
    }
    key
}

use bech32::{Bech32m, Hrp};

const META_ADDRESS_HRP: &str = "slnt";

/// sRFC-0042 §5.2.2 meta-address.
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

    /// Build a labeled v1 meta-address (sRFC-0042 §5.2.3).
    ///
    /// `label_index = 0` is the unlabeled default (identical to
    /// [`MetaAddress::from_keys`]). For `i >= 1` the encoded spend key is
    /// `B_spend_i = B_spend + m_i · G_ed`, and `label_index = i`. The scan
    /// key is unchanged. Senders treat the encoded spend key as opaque.
    pub fn for_label(spend: &SpendKey, scan: &ScanKey, label_index: u32) -> Self {
        if label_index == 0 {
            return Self::from_keys(spend, scan);
        }
        let m_i = label_tweak_scalar(scan, label_index);
        let b_spend_i = spend.point + m_i * ED25519_BASEPOINT_POINT;
        Self {
            version: META_ADDRESS_VERSION_V1,
            b_spend: b_spend_i.compress().to_bytes(),
            b_scan: scan.public_bytes(),
            label_index,
            flags: 0,
        }
    }

    pub fn encode_bech32m(&self) -> Result<String, SlntError> {
        let mut payload = Vec::with_capacity(72);
        payload.push(self.version);
        payload.extend_from_slice(&self.b_spend);
        payload.extend_from_slice(&self.b_scan);
        write_leb128_u32(&mut payload, self.label_index);
        payload.push(self.flags);

        let hrp = Hrp::parse(META_ADDRESS_HRP).map_err(|_| SlntError::MetaAddressEncode)?;
        bech32::encode::<Bech32m>(hrp, &payload).map_err(|_| SlntError::MetaAddressEncode)
    }

    pub fn decode_bech32m(s: &str) -> Result<Self, SlntError> {
        let (hrp, data) =
            bech32::decode(s).map_err(|e| SlntError::MetaAddressDecode(format!("{e}")))?;
        if hrp.as_str() != META_ADDRESS_HRP {
            return Err(SlntError::MetaAddressDecode(format!(
                "expected HRP `slnt`, got `{}`",
                hrp.as_str()
            )));
        }
        // Minimum payload: 1 (version) + 32 (B_spend) + 32 (B_scan)
        //                + 1 (varint label_index = 0) + 1 (flags) = 67 bytes
        if data.len() < 67 {
            return Err(SlntError::MetaAddressDecode(format!(
                "payload too short: {} bytes",
                data.len()
            )));
        }

        let version = data[0];
        if version != META_ADDRESS_VERSION_V1 {
            return Err(SlntError::UnsupportedVersion(version));
        }

        let mut b_spend = [0u8; 32];
        b_spend.copy_from_slice(&data[1..33]);
        let mut b_scan = [0u8; 32];
        b_scan.copy_from_slice(&data[33..65]);

        let (label_index, consumed) = read_leb128_u32(&data[65..])?;
        let flags_offset = 65 + consumed;
        if data.len() <= flags_offset {
            return Err(SlntError::MetaAddressDecode("missing flags byte".into()));
        }
        let flags = data[flags_offset];
        // Anything trailing is an error.
        if data.len() != flags_offset + 1 {
            return Err(SlntError::MetaAddressDecode(format!(
                "{} trailing bytes after payload",
                data.len() - flags_offset - 1
            )));
        }
        if flags != 0 {
            return Err(SlntError::UnsupportedFlags(flags));
        }

        Ok(Self {
            version,
            b_spend,
            b_scan,
            label_index,
            flags,
        })
    }
}

/// Derive the label tweak scalar `m_i` for `label_index` (sRFC-0042 §5.2.3):
///
/// ```text
/// m_i = SC25519_reduce(
///         HKDF-SHA256(salt="slnt-v1-label", ikm=b_scan_raw,
///                     info="label-" || varint(i), length=32))
/// ```
///
/// Only meaningful for `label_index >= 1`; the unlabeled default (`0`)
/// applies no tweak.
pub fn label_tweak_scalar(scan: &ScanKey, label_index: u32) -> Scalar {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT_LABEL), &scan.raw);
    let mut info = Vec::with_capacity(6 + 5);
    info.extend_from_slice(b"label-");
    write_leb128_u32(&mut info, label_index);
    let mut out = [0u8; 32];
    hk.expand(&info, &mut out)
        .expect("HKDF expand of 32 bytes never fails");
    Scalar::from_bytes_mod_order(out)
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
fn read_leb128_u32(data: &[u8]) -> Result<(u32, usize), SlntError> {
    let mut val: u64 = 0;
    let mut shift = 0u32;
    for (i, byte) in data.iter().take(5).enumerate() {
        val |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            if val > u32::MAX as u64 {
                return Err(SlntError::MetaAddressDecode("varint exceeds u32".into()));
            }
            return Ok((val as u32, i + 1));
        }
        shift += 7;
    }
    Err(SlntError::MetaAddressDecode("varint too long".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_message_mainnet_is_exact_spec_bytes() {
        // Spec §5.2.1.2: exact UTF-8, no trailing newline, "Mainnet"
        // substituted verbatim into the Network line.
        let expected = "Slnt Protocol: Derive Stealth Keys\n\nVersion: 1\nNetwork: Mainnet\nWarning: Only sign this message in the Slnt wallet or a trusted Slnt integration.\nSigning this in any other context will reveal your stealth address scanning ability.";
        assert_eq!(canonical_message(Network::Mainnet), expected);
        assert!(!canonical_message(Network::Mainnet).ends_with('\n'));
    }

    #[test]
    fn canonical_message_differs_per_network() {
        // Distinct networks MUST yield distinct messages so devnet
        // experiments cannot leak mainnet stealth identity (§5.2.1.2).
        let m = canonical_message(Network::Mainnet);
        let d = canonical_message(Network::Devnet);
        let t = canonical_message(Network::Testnet);
        assert_ne!(m, d);
        assert_ne!(m, t);
        assert_ne!(d, t);
        assert!(d.contains("Network: Devnet"));
        assert!(t.contains("Network: Testnet"));
    }

    /// A canonical test signature (just 64 fixed bytes, not a real
    /// signature). sRFC-0042 §5.2.1.2 takes any 64-byte input as IKM; the HKDF
    /// step doesn't care whether it's a real Ed25519 signature.
    const TEST_SIG: [u8; 64] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d,
        0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c,
        0x3d, 0x3e, 0x3f, 0x40,
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
        assert!(encoded.starts_with("slnt1"));
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
        // "btc1..." instead of "slnt1..."
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
            Err(SlntError::UnsupportedVersion(0x02)) => {}
            other => panic!("expected UnsupportedVersion(0x02), got {other:?}"),
        }
    }

    #[test]
    fn meta_address_rejects_nonzero_flags() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta = MetaAddress {
            version: META_ADDRESS_VERSION_V1,
            b_spend: spend.public_bytes(),
            b_scan: scan.public_bytes(),
            label_index: 0,
            flags: 0x01,
        };
        let encoded = meta.encode_bech32m().unwrap();
        assert!(MetaAddress::decode_bech32m(&encoded).is_err());
    }

    #[test]
    fn checked_derivation_rejects_nondeterministic_signatures() {
        // Two signings of the same canonical message that differ ⇒ the
        // wallet is a randomized signer and MUST be rejected (§8.5).
        let mut other = TEST_SIG;
        other[0] ^= 0x01;
        let err = derive_stealth_keys_checked(&TEST_SIG, &other);
        assert!(matches!(err, Err(SlntError::NonDeterministicSignature)));
    }

    #[test]
    fn checked_derivation_accepts_matching_signatures() {
        let (a, _) = derive_stealth_keys_checked(&TEST_SIG, &TEST_SIG).unwrap();
        let (b, _) = derive_stealth_keys(&TEST_SIG).unwrap();
        assert_eq!(a.public_bytes(), b.public_bytes());
    }

    #[test]
    fn label_tweak_is_deterministic_and_distinct() {
        let (_, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let m1a = label_tweak_scalar(&scan, 1);
        let m1b = label_tweak_scalar(&scan, 1);
        let m2 = label_tweak_scalar(&scan, 2);
        assert_eq!(m1a.to_bytes(), m1b.to_bytes());
        assert_ne!(m1a.to_bytes(), m2.to_bytes());
        assert_ne!(m1a, Scalar::ZERO);
    }

    #[test]
    fn labeled_meta_address_encodes_tweaked_spend_key() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let label_index = 5u32;
        let meta = MetaAddress::for_label(&spend, &scan, label_index);

        // B_spend_i = B_spend + m_i · G_ed
        let m_i = label_tweak_scalar(&scan, label_index);
        let expected = (spend.point + m_i * ED25519_BASEPOINT_POINT)
            .compress()
            .to_bytes();

        assert_eq!(meta.label_index, label_index);
        assert_eq!(meta.b_spend, expected);
        // The scan key is unchanged by labeling.
        assert_eq!(meta.b_scan, scan.public_bytes());
    }

    #[test]
    fn label_zero_is_unlabeled() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta0 = MetaAddress::for_label(&spend, &scan, 0);
        let plain = MetaAddress::from_keys(&spend, &scan);
        assert_eq!(meta0, plain);
    }

    #[test]
    fn slip10_matches_official_ed25519_vector() {
        // SLIP-0010 test vector 1 for ed25519.
        // seed = 000102030405060708090a0b0c0d0e0f
        let seed = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        // m  → 2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7
        let m = slip10_ed25519_node(&seed, &[]);
        assert_eq!(
            hex_lower(&m),
            "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7",
        );
        // m/0' → 68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3
        let m0 = slip10_ed25519_node(&seed, &[harden(0)]);
        assert_eq!(
            hex_lower(&m0),
            "68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3",
        );
    }

    #[test]
    fn hd_rejects_out_of_range_seed_length() {
        // SLIP-0010 mandates a 128–512 bit (16–64 byte) seed.
        assert!(matches!(
            derive_stealth_keys_hd(&[0u8; 15], 0),
            Err(SlntError::InvalidSeedLength(15))
        ));
        assert!(matches!(
            derive_stealth_keys_hd(&[0u8; 65], 0),
            Err(SlntError::InvalidSeedLength(65))
        ));
        // Boundaries are accepted.
        assert!(derive_stealth_keys_hd(&[0u8; 16], 0).is_ok());
        assert!(derive_stealth_keys_hd(&[0u8; 64], 0).is_ok());
    }

    #[test]
    fn hd_derivation_is_deterministic() {
        let seed = [0x42u8; 64];
        let (s1, sc1) = derive_stealth_keys_hd(&seed, 0).unwrap();
        let (s2, sc2) = derive_stealth_keys_hd(&seed, 0).unwrap();
        assert_eq!(s1.public_bytes(), s2.public_bytes());
        assert_eq!(sc1.public_bytes(), sc2.public_bytes());
    }

    #[test]
    fn hd_spend_and_scan_use_sibling_paths() {
        // The 0' (spend) and 1' (scan) selectors must produce different
        // underlying material — spend pubkey must not equal scan pubkey.
        let seed = [0x42u8; 64];
        let (spend, scan) = derive_stealth_keys_hd(&seed, 0).unwrap();
        assert_ne!(spend.public_bytes(), scan.public_bytes());
    }

    #[test]
    fn hd_account_index_changes_identity() {
        let seed = [0x42u8; 64];
        let (s0, _) = derive_stealth_keys_hd(&seed, 0).unwrap();
        let (s1, _) = derive_stealth_keys_hd(&seed, 1).unwrap();
        assert_ne!(s0.public_bytes(), s1.public_bytes());
    }

    #[test]
    fn hd_spend_point_matches_scalar() {
        let seed = [0x07u8; 64];
        let (spend, _) = derive_stealth_keys_hd(&seed, 0).unwrap();
        assert_eq!(
            (spend.scalar * ED25519_BASEPOINT_POINT).compress(),
            spend.point.compress(),
        );
    }

    fn hex_lower(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
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
