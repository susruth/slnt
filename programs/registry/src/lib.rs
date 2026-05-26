use anchor_lang::prelude::*;

declare_id!("CFSsGrZaZz9ZsPayKWSRkLp6xd28HCWwXhpdQFMyupXC");

#[program]
pub mod umbra_registry {
    use super::*;
}

#[account]
pub struct MetaAddressEntry {
    pub registrant: Pubkey,     // 32
    pub scheme_id: u16,         // 2
    pub bump: u8,               // 1
    pub version: u8,            // 1
    pub b_spend: [u8; 32],      // 32
    pub b_scan: [u8; 32],       // 32
    pub flags: u8,              // 1
}

impl MetaAddressEntry {
    pub const SIZE: usize = 32 + 2 + 1 + 1 + 32 + 32 + 1; // 101 bytes
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct MetaAddressPayload {
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub flags: u8,
}

#[event]
pub struct MetaAddressRegistered {
    pub registrant: Pubkey,
    pub scheme_id: u16,
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub flags: u8,
}

#[event]
pub struct MetaAddressUpdated {
    pub registrant: Pubkey,
    pub scheme_id: u16,
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub flags: u8,
}

#[event]
pub struct MetaAddressClosed {
    pub registrant: Pubkey,
    pub scheme_id: u16,
}

#[error_code]
pub enum RegistryError {
    #[msg("scheme_id must be non-zero")]
    InvalidSchemeId,
    #[msg("only meta-address version 0x01 is supported by this program")]
    InvalidVersion,
    #[msg("flags must be 0x00 in v1")]
    InvalidFlags,
}
