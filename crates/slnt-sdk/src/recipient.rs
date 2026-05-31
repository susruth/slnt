//! Recipient-side scanning (sRFC-0042 §5.4).
//!
//! For each pinboard Note observed, call `scan_note`. If the view tag
//! matches, you get back a `NoteMatch` containing the stealth address
//! and the scalar to sign with.

use crate::error::SlntError;
use crate::keys::{label_tweak_scalar, ScanKey, SpendKey};
use crate::sender::{compute_tweak, compute_view_tag, shared_secret_is_zero};
use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT, Scalar};
use solana_sdk::pubkey::Pubkey;
use x25519_dalek::PublicKey as X25519PublicKey;

#[derive(Debug, Clone)]
pub struct NoteMatch {
    pub stealth_address: Pubkey,
    /// `p_stealth = (b_spend + [m_i +] t) mod ℓ`. Pass this to
    /// `StealthSigningKey::new`.
    pub stealth_scalar: Scalar,
    /// Label index this candidate was derived under (`0` = unlabeled).
    pub label_index: u32,
}

/// sRFC-0042 §5.4. Returns:
/// - `Ok(Some(NoteMatch))` if the view tag matched and a candidate address
///   was derived; the caller still checks on-chain state to confirm funds
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
) -> Result<Option<NoteMatch>, SlntError> {
    // 1. ECDH using recipient's scan private key.
    let r_public = X25519PublicKey::from(*ephemeral_pub);
    let s_candidate = scan.static_secret.diffie_hellman(&r_public);
    if shared_secret_is_zero(s_candidate.as_bytes()) {
        return Ok(None);
    }

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
    let p_stealth_point = spend.point + (t * ED25519_BASEPOINT_POINT);
    let stealth_address = Pubkey::new_from_array(p_stealth_point.compress().to_bytes());
    let stealth_scalar = spend.scalar + t;

    Ok(Some(NoteMatch {
        stealth_address,
        stealth_scalar,
        label_index: 0,
    }))
}

/// Spec §5.4 with labels. Like [`scan_note`], but on a view-tag match
/// returns every candidate stealth address: the unlabeled one plus one
/// per entry in `known_labels`. The caller checks which candidate
/// actually received funds on-chain (the SDK cannot tell which label the
/// sender used — the sender treats `B_spend_effective` as opaque).
///
/// Returns an empty vector when the view tag does not match.
pub fn scan_note_candidates(
    spend: &SpendKey,
    scan: &ScanKey,
    ephemeral_pub: &[u8; 32],
    note_view_tag: u8,
    known_labels: &[u32],
) -> Result<Vec<NoteMatch>, SlntError> {
    let r_public = X25519PublicKey::from(*ephemeral_pub);
    let s_candidate = scan.static_secret.diffie_hellman(&r_public);
    if shared_secret_is_zero(s_candidate.as_bytes()) {
        return Ok(Vec::new());
    }

    if compute_view_tag(s_candidate.as_bytes()) != note_view_tag {
        return Ok(Vec::new());
    }

    let t = compute_tweak(s_candidate.as_bytes(), note_view_tag);

    // Unlabeled candidate (label 0) is always derivable.
    let mut out = Vec::with_capacity(1 + known_labels.len());
    out.push(NoteMatch {
        stealth_address: address_for_scalar(spend.scalar + t),
        stealth_scalar: spend.scalar + t,
        label_index: 0,
    });

    for &i in known_labels {
        if i == 0 {
            continue; // already covered by the unlabeled candidate
        }
        let m_i = label_tweak_scalar(scan, i);
        let scalar = spend.scalar + m_i + t;
        out.push(NoteMatch {
            stealth_address: address_for_scalar(scalar),
            stealth_scalar: scalar,
            label_index: i,
        });
    }

    Ok(out)
}

/// `address = compress(scalar · G_ed)`.
fn address_for_scalar(scalar: Scalar) -> Pubkey {
    Pubkey::new_from_array((scalar * ED25519_BASEPOINT_POINT).compress().to_bytes())
}

/// View-only scanner primitive (sRFC-0042 §5.10).
///
/// Runs one ECDH + view-tag hash for a delegated scanner that holds only
/// the scan material (a [`ScanKey`] built via [`ScanKey::from_raw`]).
/// Returns `true` when the announcement survives the ~1/256 view-tag
/// filter. The scanner cannot derive the stealth address or spend — it
/// only forwards surviving candidates to the recipient.
pub fn view_tag_matches(scan: &ScanKey, ephemeral_pub: &[u8; 32], note_view_tag: u8) -> bool {
    let r_public = X25519PublicKey::from(*ephemeral_pub);
    let s_candidate = scan.static_secret.diffie_hellman(&r_public);
    if shared_secret_is_zero(s_candidate.as_bytes()) {
        return false;
    }
    compute_view_tag(s_candidate.as_bytes()) == note_view_tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{derive_stealth_keys, MetaAddress};
    use crate::sender::{compute_view_tag, derive_payment};
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
    fn view_only_scanner_filters_without_spend_ability() {
        // A delegated scanner holds only b_scan_raw (view-only, §5.10).
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let meta = MetaAddress::from_keys(&spend, &scan);
        let mut rng = ChaCha20Rng::seed_from_u64(0xfeed);
        let payment = derive_payment(&meta, &mut rng).unwrap();

        // Scanner reconstructs the scan key from raw material alone.
        let scanner_scan = ScanKey::from_raw(scan.raw);
        assert!(view_tag_matches(
            &scanner_scan,
            &payment.ephemeral_pub,
            payment.view_tag,
        ));

        // An unrelated scan key rejects the view tag (almost always).
        let (_, other_scan) = derive_stealth_keys(&[2u8; 64]).unwrap();
        let mut rejects = 0;
        let mut rng2 = ChaCha20Rng::seed_from_u64(0xfeed);
        for _ in 0..256 {
            let p = derive_payment(&meta, &mut rng2).unwrap();
            if !view_tag_matches(&other_scan, &p.ephemeral_pub, p.view_tag) {
                rejects += 1;
            }
        }
        assert!(
            rejects > 200,
            "view-tag filter should reject most ({rejects}/256)"
        );
    }

    #[test]
    fn labeled_payment_roundtrip() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let label_index = 7u32;
        // Recipient hands out a labeled meta-address; sender pays it
        // treating B_spend_effective as opaque.
        let meta = MetaAddress::for_label(&spend, &scan, label_index);

        let mut rng = ChaCha20Rng::seed_from_u64(0xabc123);
        let payment = derive_payment(&meta, &mut rng).unwrap();

        // Scanning without knowing the label must NOT recover the address.
        let unlabeled = scan_note(&spend, &scan, &payment.ephemeral_pub, payment.view_tag).unwrap();
        if let Some(m) = unlabeled {
            assert_ne!(m.stealth_address, payment.stealth_address);
        }

        // Scanning with the known label index recovers it.
        let candidates = scan_note_candidates(
            &spend,
            &scan,
            &payment.ephemeral_pub,
            payment.view_tag,
            &[label_index],
        )
        .unwrap();
        let hit = candidates
            .iter()
            .find(|m| m.stealth_address == payment.stealth_address)
            .expect("labeled candidate must match the payment address");
        assert_eq!(hit.label_index, label_index);

        // Reconstructed scalar must sign for the stealth address.
        let recovered = hit.stealth_scalar * ED25519_BASEPOINT_POINT;
        assert_eq!(
            recovered.compress().to_bytes(),
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
            if let Some(m) =
                scan_note(&spend_a, &scan_a, &payment.ephemeral_pub, payment.view_tag).unwrap()
            {
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
        assert!(
            false_positive_hits < 20,
            "way more view-tag collisions than expected ({false_positive_hits})"
        );
    }

    #[test]
    fn low_order_ephemeral_pub_is_ignored() {
        let (spend, scan) = derive_stealth_keys(&TEST_SIG).unwrap();
        let zero_secret = [0u8; 32];
        let matching_zero_view_tag = compute_view_tag(&zero_secret);
        let low_order_r = [0u8; 32];

        assert!(
            scan_note(&spend, &scan, &low_order_r, matching_zero_view_tag)
                .unwrap()
                .is_none()
        );

        let candidates = scan_note_candidates(
            &spend,
            &scan,
            &low_order_r,
            matching_zero_view_tag,
            &[1, 2, 3],
        )
        .unwrap();
        assert!(candidates.is_empty());
    }
}
