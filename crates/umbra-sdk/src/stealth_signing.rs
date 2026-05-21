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
