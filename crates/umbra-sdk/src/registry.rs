//! Helpers for the Umbra registry program.
//!
//! Provides PDA derivation matching the on-chain seeds and a borsh
//! decoder for the `MetaAddressEntry` account. RPC fetching lives in
//! `fetch_meta_address` (added in the next task).

use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::pubkey::Pubkey;

/// Seed prefix used by the registry program for meta-address PDAs.
pub const META_SEED: &[u8] = b"meta";

/// Anchor account discriminator for `MetaAddressEntry`:
/// `SHA-256("account:MetaAddressEntry")[..8]`.
///
/// The unit test below re-derives and checks equality. If this constant
/// is wrong, the test will fail and print the expected bytes; copy the
/// expected bytes here and re-run.
pub const META_ADDRESS_ENTRY_DISCRIMINATOR: [u8; 8] =
    [165, 7, 241, 154, 7, 172, 74, 178];

/// On-chain `MetaAddressEntry` layout. Mirrors
/// `programs/registry/src/lib.rs` exactly.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct MetaAddressEntry {
    pub registrant: Pubkey,
    pub scheme_id: u16,
    pub bump: u8,
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub flags: u8,
}

/// Derive the registry PDA for a `(registrant, scheme_id)` pair.
pub fn registry_pda(
    program_id: &Pubkey,
    registrant: &Pubkey,
    scheme_id: u16,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            META_SEED,
            registrant.as_ref(),
            &scheme_id.to_le_bytes(),
        ],
        program_id,
    )
}

/// Parse the raw bytes of a registry account into a `MetaAddressEntry`.
///
/// Validates the 8-byte Anchor discriminator and borsh-decodes the rest.
/// Returns `Ok(None)` if the bytes do not start with the expected
/// discriminator. Returns `Err` if the discriminator matches but the
/// body fails to deserialize.
pub fn try_parse_meta_address_entry(
    data: &[u8],
) -> Result<Option<MetaAddressEntry>, String> {
    if data.len() < 8 {
        return Ok(None);
    }
    if &data[..8] != META_ADDRESS_ENTRY_DISCRIMINATOR {
        return Ok(None);
    }
    let entry = MetaAddressEntry::try_from_slice(&data[8..])
        .map_err(|e| format!("borsh deserialize MetaAddressEntry: {e}"))?;
    Ok(Some(entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn account_discriminator_matches_anchor_convention() {
        let mut h = Sha256::new();
        h.update(b"account:MetaAddressEntry");
        let computed = h.finalize();
        assert_eq!(&computed[..8], &META_ADDRESS_ENTRY_DISCRIMINATOR);
    }

    #[test]
    fn pda_is_deterministic_for_same_inputs() {
        let program_id = Pubkey::new_unique();
        let registrant = Pubkey::new_unique();
        let (pda1, bump1) = registry_pda(&program_id, &registrant, 1);
        let (pda2, bump2) = registry_pda(&program_id, &registrant, 1);
        assert_eq!(pda1, pda2);
        assert_eq!(bump1, bump2);
    }

    #[test]
    fn pda_differs_by_scheme_id() {
        let program_id = Pubkey::new_unique();
        let registrant = Pubkey::new_unique();
        let (pda_scheme1, _) = registry_pda(&program_id, &registrant, 1);
        let (pda_scheme2, _) = registry_pda(&program_id, &registrant, 2);
        assert_ne!(pda_scheme1, pda_scheme2);
    }

    #[test]
    fn pda_differs_by_registrant() {
        let program_id = Pubkey::new_unique();
        let registrant1 = Pubkey::new_unique();
        let registrant2 = Pubkey::new_unique();
        let (pda1, _) = registry_pda(&program_id, &registrant1, 1);
        let (pda2, _) = registry_pda(&program_id, &registrant2, 1);
        assert_ne!(pda1, pda2);
    }

    #[test]
    fn parse_roundtrip() {
        let original = MetaAddressEntry {
            registrant: Pubkey::new_unique(),
            scheme_id: 1,
            bump: 254,
            version: 1,
            b_spend: [0xaa; 32],
            b_scan: [0xbb; 32],
            flags: 0,
        };
        let mut data = Vec::new();
        data.extend_from_slice(&META_ADDRESS_ENTRY_DISCRIMINATOR);
        borsh::to_writer(&mut data, &original).unwrap();
        let parsed = try_parse_meta_address_entry(&data).unwrap().unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn parse_short_data_is_none() {
        assert!(try_parse_meta_address_entry(&[1, 2, 3]).unwrap().is_none());
    }

    #[test]
    fn parse_wrong_discriminator_is_none() {
        let data = vec![0u8; 8 + 101];
        assert!(try_parse_meta_address_entry(&data).unwrap().is_none());
    }
}
