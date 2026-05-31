//! Helpers for the Slnt registry program.
//!
//! Provides PDA derivation matching the on-chain seeds, a borsh
//! decoder for the `MetaAddressEntry` account, and (behind the `rpc`
//! feature) an async `fetch_meta_address` helper.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

/// Seed prefix used by the registry program for meta-address PDAs.
pub const META_SEED: &[u8] = b"meta";

/// `SHA-256("global:register")[..8]`.
pub const REGISTER_DISCRIMINATOR: [u8; 8] = [211, 124, 67, 15, 211, 194, 178, 240];
/// `SHA-256("global:update")[..8]`.
pub const UPDATE_DISCRIMINATOR: [u8; 8] = [219, 200, 88, 176, 158, 63, 253, 127];
/// `SHA-256("global:close")[..8]`.
pub const CLOSE_DISCRIMINATOR: [u8; 8] = [98, 165, 201, 177, 108, 65, 206, 96];

/// Registry instruction argument for `register` / `update`. Mirrors
/// `MetaAddressPayload` in `programs/registry/src/lib.rs`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct MetaAddressPayload {
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub flags: u8,
}

/// Anchor account discriminator for `MetaAddressEntry`:
/// `SHA-256("account:MetaAddressEntry")[..8]`.
///
/// The unit test below re-derives and checks equality. If this constant
/// is wrong, the test will fail and print the expected bytes; copy the
/// expected bytes here and re-run.
pub const META_ADDRESS_ENTRY_DISCRIMINATOR: [u8; 8] = [165, 7, 241, 154, 7, 172, 74, 178];

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
pub fn registry_pda(program_id: &Pubkey, registrant: &Pubkey, scheme_id: u16) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[META_SEED, registrant.as_ref(), &scheme_id.to_le_bytes()],
        program_id,
    )
}

/// Build a `registry.register(scheme_id, payload)` instruction
/// (sRFC-0042 §5.6.2). Creates the PDA; the registrant pays rent and
/// signs. Fails on-chain if the `(registrant, scheme_id)` entry exists.
pub fn build_register_instruction(
    program_id: &Pubkey,
    registrant: &Pubkey,
    scheme_id: u16,
    payload: MetaAddressPayload,
) -> Instruction {
    let (pda, _) = registry_pda(program_id, registrant, scheme_id);
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*registrant, true),
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        ],
        data: encode_payload_ix(&REGISTER_DISCRIMINATOR, scheme_id, Some(&payload)),
    }
}

/// Build a `registry.update(scheme_id, payload)` instruction. Overwrites
/// the existing entry in place; only the owning registrant may sign.
pub fn build_update_instruction(
    program_id: &Pubkey,
    registrant: &Pubkey,
    scheme_id: u16,
    payload: MetaAddressPayload,
) -> Instruction {
    let (pda, _) = registry_pda(program_id, registrant, scheme_id);
    Instruction {
        program_id: *program_id,
        accounts: vec![
            // `update` does not mark the registrant `mut` on-chain.
            AccountMeta::new_readonly(*registrant, true),
            AccountMeta::new(pda, false),
        ],
        data: encode_payload_ix(&UPDATE_DISCRIMINATOR, scheme_id, Some(&payload)),
    }
}

/// Build a `registry.close(scheme_id)` instruction. Closes the PDA,
/// returning rent to the registrant; the pair may be re-registered later.
pub fn build_close_instruction(
    program_id: &Pubkey,
    registrant: &Pubkey,
    scheme_id: u16,
) -> Instruction {
    let (pda, _) = registry_pda(program_id, registrant, scheme_id);
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*registrant, true),
            AccountMeta::new(pda, false),
        ],
        data: encode_payload_ix(&CLOSE_DISCRIMINATOR, scheme_id, None),
    }
}

/// Anchor instruction data: `discriminator || borsh(scheme_id) ||
/// borsh(payload)?`.
fn encode_payload_ix(
    discriminator: &[u8; 8],
    scheme_id: u16,
    payload: Option<&MetaAddressPayload>,
) -> Vec<u8> {
    let mut data = Vec::with_capacity(8 + 2 + 66);
    data.extend_from_slice(discriminator);
    borsh::to_writer(&mut data, &scheme_id).expect("borsh u16");
    if let Some(p) = payload {
        borsh::to_writer(&mut data, p).expect("borsh MetaAddressPayload");
    }
    data
}

/// Parse the raw bytes of a registry account into a `MetaAddressEntry`.
///
/// Validates the 8-byte Anchor discriminator and borsh-decodes the rest.
/// Returns `Ok(None)` if the bytes do not start with the expected
/// discriminator. Returns `Err` if the discriminator matches but the
/// body fails to deserialize.
pub fn try_parse_meta_address_entry(data: &[u8]) -> Result<Option<MetaAddressEntry>, String> {
    if data.len() < 8 {
        return Ok(None);
    }
    if data[..8] != META_ADDRESS_ENTRY_DISCRIMINATOR {
        return Ok(None);
    }
    let entry = MetaAddressEntry::try_from_slice(&data[8..])
        .map_err(|e| format!("borsh deserialize MetaAddressEntry: {e}"))?;
    Ok(Some(entry))
}

/// Fetch and decode a registered meta-address.
///
/// Returns `Ok(None)` if no entry exists at the derived PDA. Returns
/// `Err` on RPC failure or malformed account data.
#[cfg(feature = "rpc")]
pub async fn fetch_meta_address(
    rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    program_id: &Pubkey,
    registrant: &Pubkey,
    scheme_id: u16,
) -> Result<Option<MetaAddressEntry>, String> {
    let (pda, _) = registry_pda(program_id, registrant, scheme_id);
    let account = match rpc.get_account(&pda).await {
        Ok(a) => a,
        Err(e) => {
            // solana-client returns a structured error for "account not
            // found"; distinguish it from real RPC failures.
            let msg = e.to_string();
            if msg.contains("AccountNotFound") {
                return Ok(None);
            }
            return Err(format!("get_account: {msg}"));
        }
    };
    try_parse_meta_address_entry(&account.data)
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
    fn instruction_discriminators_match_anchor_convention() {
        for (name, disc) in [
            ("global:register", REGISTER_DISCRIMINATOR),
            ("global:update", UPDATE_DISCRIMINATOR),
            ("global:close", CLOSE_DISCRIMINATOR),
        ] {
            let mut h = Sha256::new();
            h.update(name.as_bytes());
            assert_eq!(&h.finalize()[..8], &disc, "{name}");
        }
    }

    #[test]
    fn build_register_has_correct_accounts_and_data() {
        let program = Pubkey::new_unique();
        let registrant = Pubkey::new_unique();
        let (pda, _) = registry_pda(&program, &registrant, 1);
        let payload = MetaAddressPayload {
            version: 1,
            b_spend: [0xaa; 32],
            b_scan: [0xbb; 32],
            flags: 0,
        };
        let ix = build_register_instruction(&program, &registrant, 1, payload.clone());

        assert_eq!(ix.program_id, program);
        // registrant (signer, writable), entry PDA (writable), system program
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.accounts[0].pubkey, registrant);
        assert!(ix.accounts[0].is_signer && ix.accounts[0].is_writable);
        assert_eq!(ix.accounts[1].pubkey, pda);
        assert!(ix.accounts[1].is_writable && !ix.accounts[1].is_signer);
        assert_eq!(ix.accounts[2].pubkey, solana_sdk::system_program::id());

        assert_eq!(&ix.data[..8], &REGISTER_DISCRIMINATOR);
        // body = borsh(scheme_id: u16) || borsh(payload)
        let mut expected_body = Vec::new();
        borsh::to_writer(&mut expected_body, &1u16).unwrap();
        borsh::to_writer(&mut expected_body, &payload).unwrap();
        assert_eq!(&ix.data[8..], &expected_body[..]);
    }

    #[test]
    fn build_update_omits_system_program_and_registrant_is_readonly() {
        let program = Pubkey::new_unique();
        let registrant = Pubkey::new_unique();
        let (pda, _) = registry_pda(&program, &registrant, 1);
        let payload = MetaAddressPayload {
            version: 1,
            b_spend: [1; 32],
            b_scan: [2; 32],
            flags: 0,
        };
        let ix = build_update_instruction(&program, &registrant, 1, payload);
        assert_eq!(ix.accounts.len(), 2);
        assert!(ix.accounts[0].is_signer && !ix.accounts[0].is_writable);
        assert_eq!(ix.accounts[1].pubkey, pda);
        assert!(ix.accounts[1].is_writable);
        assert_eq!(&ix.data[..8], &UPDATE_DISCRIMINATOR);
    }

    #[test]
    fn build_close_has_writable_registrant_and_scheme_id_arg() {
        let program = Pubkey::new_unique();
        let registrant = Pubkey::new_unique();
        let ix = build_close_instruction(&program, &registrant, 7);
        assert_eq!(ix.accounts.len(), 2);
        assert!(ix.accounts[0].is_signer && ix.accounts[0].is_writable);
        assert_eq!(&ix.data[..8], &CLOSE_DISCRIMINATOR);
        let mut body = Vec::new();
        borsh::to_writer(&mut body, &7u16).unwrap();
        assert_eq!(&ix.data[8..], &body[..]);
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
