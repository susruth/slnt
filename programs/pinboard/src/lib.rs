use anchor_lang::prelude::*;

declare_id!("G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P");

/// Maximum length of the optional `metadata` field in a note,
/// in bytes. See spec §6.1.
pub const MAX_METADATA_LEN: usize = 64;

#[program]
pub mod pinboard {
    use super::*;

    /// Post a single note to the pinboard.
    ///
    /// `scheme_id` is recorded but not validated — v1 clients only
    /// process `0x0001`; future schemes will be added by client updates.
    pub fn post(
        _ctx: Context<Post>,
        scheme_id: u16,
        ephemeral_pub: [u8; 32],
        view_tag: u8,
        metadata: Vec<u8>,
    ) -> Result<()> {
        require!(
            metadata.len() <= MAX_METADATA_LEN,
            PinboardError::MetadataTooLong
        );

        emit!(Note {
            scheme_id,
            ephemeral_pub,
            view_tag,
            metadata,
        });

        Ok(())
    }

    /// Post multiple notes in a single transaction. Used by relayers
    /// and batching services to amortize the base tx fee across many
    /// notes.
    pub fn post_batch(
        _ctx: Context<PostBatch>,
        entries: Vec<NoteEntry>,
    ) -> Result<()> {
        require!(!entries.is_empty(), PinboardError::EmptyBatch);

        for entry in entries.into_iter() {
            require!(
                entry.metadata.len() <= MAX_METADATA_LEN,
                PinboardError::MetadataTooLong
            );

            emit!(Note {
                scheme_id: entry.scheme_id,
                ephemeral_pub: entry.ephemeral_pub,
                view_tag: entry.view_tag,
                metadata: entry.metadata,
            });
        }

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Post<'info> {
    /// Fee payer. No special role beyond paying the tx; this account
    /// can be anyone.
    #[account(mut)]
    pub fee_payer: Signer<'info>,
}

#[derive(Accounts)]
pub struct PostBatch<'info> {
    #[account(mut)]
    pub fee_payer: Signer<'info>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct NoteEntry {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

#[event]
pub struct Note {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

#[error_code]
pub enum PinboardError {
    #[msg("metadata exceeds 64 bytes")]
    MetadataTooLong,
    #[msg("batch must contain at least one entry")]
    EmptyBatch,
}
