use anchor_lang::prelude::*;

declare_id!("G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P");

/// Maximum length of the optional `metadata` field in an announcement,
/// in bytes. See spec §6.1.
pub const MAX_METADATA_LEN: usize = 64;

#[program]
pub mod umbra_announcer {
    use super::*;

    /// Publish a single stealth-payment announcement.
    ///
    /// `scheme_id` is recorded but not validated — v1 clients only
    /// process `0x0001`; future schemes will be added by client updates.
    pub fn announce(
        _ctx: Context<Announce>,
        scheme_id: u16,
        ephemeral_pub: [u8; 32],
        view_tag: u8,
        metadata: Vec<u8>,
    ) -> Result<()> {
        require!(
            metadata.len() <= MAX_METADATA_LEN,
            UmbraError::MetadataTooLong
        );

        emit!(UmbraAnnouncement {
            scheme_id,
            ephemeral_pub,
            view_tag,
            metadata,
        });

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Announce<'info> {
    /// Fee payer. No special role beyond paying the tx; this account
    /// can be anyone.
    #[account(mut)]
    pub fee_payer: Signer<'info>,
}

#[event]
pub struct UmbraAnnouncement {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

#[error_code]
pub enum UmbraError {
    #[msg("metadata exceeds 64 bytes")]
    MetadataTooLong,
}
