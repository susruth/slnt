//! Sender-side stealth-address derivation (sRFC-0042 §5.3).

use crate::error::SlntError;
use crate::keys::{MetaAddress, META_ADDRESS_VERSION_V1};
use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT, edwards::CompressedEdwardsY, Scalar};
use rand_core::CryptoRngCore;
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

/// Domain-separation tag for the stealth-address tweak hash
/// (sRFC-0042 §5.3). 14 bytes.
const TWEAK_TAG: &[u8] = b"slnt-v1-tweak";

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

/// sRFC-0042 §5.3. The sender derives a one-time stealth address for the
/// given meta-address, plus the (R, view_tag) tuple to publish on the
/// pinboard.
pub fn derive_payment(
    meta: &MetaAddress,
    rng: &mut impl CryptoRngCore,
) -> Result<StealthPayment, SlntError> {
    if meta.version != META_ADDRESS_VERSION_V1 {
        return Err(SlntError::UnsupportedVersion(meta.version));
    }
    if meta.flags != 0 {
        return Err(SlntError::UnsupportedFlags(meta.flags));
    }

    // Decompress B_spend_effective (already incorporates label tweak if any).
    let b_spend_compressed = CompressedEdwardsY(meta.b_spend);
    let b_spend = b_spend_compressed
        .decompress()
        .ok_or(SlntError::InvalidPoint)?;
    if b_spend.is_small_order() {
        return Err(SlntError::InvalidPoint);
    }

    // 1. Generate ephemeral X25519 scalar r.
    let mut r_bytes = [0u8; 32];
    rng.fill_bytes(&mut r_bytes);
    let r = X25519StaticSecret::from(r_bytes);
    let r_public = X25519PublicKey::from(&r);

    // 2. ECDH: S = r · B_scan
    let b_scan_public = X25519PublicKey::from(meta.b_scan);
    let s = r.diffie_hellman(&b_scan_public);
    if shared_secret_is_zero(s.as_bytes()) {
        return Err(SlntError::InvalidSharedSecret);
    }

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

pub(crate) fn shared_secret_is_zero(s: &[u8; 32]) -> bool {
    s.iter().all(|b| *b == 0)
}

/// `SHA-256(1-byte-len || TWEAK_TAG || S)[0]`. sRFC-0042 §5.3.
pub(crate) fn compute_view_tag(s: &[u8]) -> u8 {
    let mut hasher = Sha256::new();
    hasher.update([TWEAK_TAG.len() as u8]);
    hasher.update(TWEAK_TAG);
    hasher.update(s);
    let out = hasher.finalize();
    out[0]
}

/// `SC25519_reduce(SHA-256(1-byte-len || TWEAK_TAG || S || view_tag))`.
/// sRFC-0042 §5.3.
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

    #[test]
    fn derive_payment_rejects_unsupported_meta_fields() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let mut bad_version = MetaAddress::from_keys(&spend, &scan);
        bad_version.version = 0x02;
        let mut bad_flags = MetaAddress::from_keys(&spend, &scan);
        bad_flags.flags = 0x01;
        let mut rng = ChaCha20Rng::seed_from_u64(7);

        assert!(derive_payment(&bad_version, &mut rng).is_err());
        assert!(derive_payment(&bad_flags, &mut rng).is_err());
    }

    #[test]
    fn derive_payment_rejects_small_order_spend_key() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let mut meta = MetaAddress::from_keys(&spend, &scan);
        meta.b_spend = [0u8; 32];
        meta.b_spend[0] = 1; // compressed Ed25519 identity point
        let mut rng = ChaCha20Rng::seed_from_u64(7);

        assert!(derive_payment(&meta, &mut rng).is_err());
    }

    #[test]
    fn derive_payment_rejects_all_zero_shared_secret() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let mut meta = MetaAddress::from_keys(&spend, &scan);
        meta.b_scan = [0u8; 32]; // low-order X25519 point
        let mut rng = ChaCha20Rng::seed_from_u64(7);

        assert!(derive_payment(&meta, &mut rng).is_err());
    }
}
