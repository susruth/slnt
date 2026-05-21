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
