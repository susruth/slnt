//! Announcement modes and the announcement-service protocol
//! (sRFC-0042 §5.8).
//!
//! In v1 the sender MUST default to **decoupled mode**: the asset
//! transfer (`flows.rs`) carries no SLNT instruction, and the
//! announcement tuple `(scheme_id, R, view_tag, metadata)` is published
//! separately — ideally by an announcement service so the transfer stays
//! silent on-chain. This module models the announcement itself, the
//! self-announce fallback decision (§5.8.2), the coupled escape hatch
//! (§5.8.3), and the service HTTP wire types (§5.8.4).
//!
//! The networked pieces (HTTP submission, the 60 s watch loop) are driven
//! by the caller; the SDK provides the pure construction/decision logic
//! and the serializable wire types.

use crate::error::SlntError;
use crate::sender::StealthPayment;
use serde::{Deserialize, Serialize};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::time::Duration;

/// Max announcement metadata size (§5.5.1).
pub const MAX_METADATA_LEN: usize = 64;

/// RECOMMENDED self-announce timeout `T` (§5.8.2).
pub const SELF_ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(60);

/// The announcement tuple published on pinboard (§5.4 / §5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub scheme_id: u16,
    /// The sender's ephemeral X25519 public key `R`.
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

impl Announcement {
    /// Build the announcement for a derived [`StealthPayment`], with an
    /// optional opaque `metadata` blob (≤ 64 bytes, §5.5.1).
    pub fn from_payment(payment: &StealthPayment, metadata: Vec<u8>) -> Result<Self, SlntError> {
        if metadata.len() > MAX_METADATA_LEN {
            return Err(SlntError::MetadataTooLong(metadata.len()));
        }
        Ok(Self {
            scheme_id: crate::keys::SCHEME_ID_V1,
            ephemeral_pub: payment.ephemeral_pub,
            view_tag: payment.view_tag,
            metadata,
        })
    }

    /// Build the pinboard `post` instruction for this announcement —
    /// used by coupled mode (§5.8.3) and the self-announce fallback
    /// (§5.8.2), where the sender pays for the post.
    pub fn to_post_instruction(
        &self,
        pinboard_program_id: &Pubkey,
        fee_payer: &Pubkey,
    ) -> Instruction {
        crate::pinboard::build_post_instruction(
            pinboard_program_id,
            fee_payer,
            self.scheme_id,
            self.ephemeral_pub,
            self.view_tag,
            self.metadata.clone(),
        )
    }
}

/// How the announcement reaches pinboard (§5.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnounceMode {
    /// Default: a service publishes the announcement in a tx it pays for,
    /// so the transfer carries no marker.
    Decoupled,
    /// Escape hatch: announcement rides in the same tx as the transfer,
    /// making it visibly an SLNT payment. Surface the trade-off first.
    Coupled,
}

/// Self-announce decision (§5.8.2). After submitting to a service the
/// wallet watches pinboard for a note with matching `R`; if none appears
/// within `T`, it MUST publish the announcement itself.
///
/// Returns `true` when the wallet should now self-announce.
pub fn should_self_announce(
    matching_note_seen: bool,
    elapsed: Duration,
    timeout: Duration,
) -> bool {
    !matching_note_seen && elapsed >= timeout
}

/// Deduplicate observed announcements by `R` (§5.8.2): a service+sender
/// race may publish two notes with the same ephemeral key. Preserves
/// first-seen order.
pub fn dedup_by_ephemeral_pub(announcements: &[Announcement]) -> Vec<Announcement> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for a in announcements {
        if seen.insert(a.ephemeral_pub) {
            out.push(a.clone());
        }
    }
    out
}

// ---- Announcement-service HTTP protocol (§5.8.4) ----

/// `POST /announce` request body. Binary fields are base58 strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnounceRequest {
    pub scheme_id: u16,
    /// `R`, base58-encoded.
    pub ephemeral_pub: String,
    pub view_tag: u8,
    /// `metadata`, base58-encoded (empty string if none).
    pub metadata: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub payment_proof: Option<String>,
}

impl AnnounceRequest {
    pub fn from_announcement(a: &Announcement, payment_proof: Option<String>) -> Self {
        Self {
            scheme_id: a.scheme_id,
            ephemeral_pub: bs58::encode(a.ephemeral_pub).into_string(),
            view_tag: a.view_tag,
            metadata: bs58::encode(&a.metadata).into_string(),
            payment_proof,
        }
    }
}

/// `POST /announce` response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnounceResponse {
    pub queued: bool,
    pub batch_id: String,
    pub expected_slot: u64,
}

/// `GET /announce/status/{batch_id}` response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnounceStatus {
    /// `pending` | `confirmed` | `failed`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tx_signature: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{derive_stealth_keys, MetaAddress};
    use crate::sender::derive_payment;
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    fn sample_payment() -> StealthPayment {
        let (spend, scan) = derive_stealth_keys(&[5u8; 64]).unwrap();
        let meta = MetaAddress::from_keys(&spend, &scan);
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        derive_payment(&meta, &mut rng).unwrap()
    }

    #[test]
    fn from_payment_carries_r_and_view_tag() {
        let p = sample_payment();
        let a = Announcement::from_payment(&p, vec![]).unwrap();
        assert_eq!(a.ephemeral_pub, p.ephemeral_pub);
        assert_eq!(a.view_tag, p.view_tag);
        assert_eq!(a.scheme_id, crate::keys::SCHEME_ID_V1);
    }

    #[test]
    fn from_payment_rejects_oversized_metadata() {
        let p = sample_payment();
        let err = Announcement::from_payment(&p, vec![0u8; 65]);
        assert!(matches!(err, Err(SlntError::MetadataTooLong(65))));
    }

    #[test]
    fn post_instruction_matches_announcement_args() {
        let p = sample_payment();
        let a = Announcement::from_payment(&p, vec![1, 2, 3]).unwrap();
        let program = Pubkey::new_unique();
        let fee_payer = Pubkey::new_unique();
        let ix = a.to_post_instruction(&program, &fee_payer);
        assert_eq!(ix.program_id, program);
        assert_eq!(&ix.data[..8], &crate::pinboard::POST_DISCRIMINATOR);
    }

    #[test]
    fn self_announce_only_after_timeout_without_match() {
        let t = SELF_ANNOUNCE_TIMEOUT;
        // Seen on pinboard → never self-announce.
        assert!(!should_self_announce(true, t, t));
        // Not seen, but timer not elapsed → wait.
        assert!(!should_self_announce(false, Duration::from_secs(30), t));
        // Not seen and timer elapsed → self-announce.
        assert!(should_self_announce(false, t, t));
    }

    #[test]
    fn dedup_removes_duplicate_ephemeral_keys() {
        let p = sample_payment();
        let a = Announcement::from_payment(&p, vec![]).unwrap();
        let deduped = dedup_by_ephemeral_pub(&[a.clone(), a.clone()]);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn announce_request_json_roundtrips() {
        let p = sample_payment();
        let a = Announcement::from_payment(&p, vec![9, 9]).unwrap();
        let req = AnnounceRequest::from_announcement(&a, Some("proof".into()));
        let json = serde_json::to_string(&req).unwrap();
        let back: AnnounceRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
        // ephemeral_pub decodes back to R.
        let decoded = bs58::decode(&back.ephemeral_pub).into_vec().unwrap();
        assert_eq!(decoded, a.ephemeral_pub);
    }

    #[test]
    fn status_response_omits_absent_signature() {
        let s = AnnounceStatus {
            status: "pending".into(),
            tx_signature: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("tx_signature"));
    }
}
