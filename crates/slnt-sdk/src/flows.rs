//! Sender transaction flows (sRFC-0042 §5.7).
//!
//! These build the **decoupled-mode** asset transfer: only the value
//! movement and any required account creation — no SLNT instruction — so
//! the transaction is indistinguishable from an ordinary transfer to a
//! fresh address. The announcement is published separately (§5.8).

use crate::error::SlntError;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

/// Rent-exempt minimum for a bare system account (lamports). Added to a
/// SOL payment so the fresh stealth account is valid (§5.7).
pub const RENT_EXEMPT_MIN: u64 = 890_880;

/// Build the SOL transfer to a stealth address (§5.7).
///
/// Transfers `amount + RENT_EXEMPT_MIN`: the extra rent buffer makes the
/// fresh system account valid and is reclaimed by the recipient on sweep.
/// Errors if `amount + RENT_EXEMPT_MIN` overflows `u64`.
pub fn build_sol_payment(
    sender: &Pubkey,
    stealth_address: &Pubkey,
    amount: u64,
) -> Result<Instruction, SlntError> {
    let lamports = amount
        .checked_add(RENT_EXEMPT_MIN)
        .ok_or(SlntError::LamportOverflow {
            amount,
            rent_buffer: RENT_EXEMPT_MIN,
        })?;
    Ok(solana_system_interface::instruction::transfer(
        sender,
        stealth_address,
        lamports,
    ))
}

/// Build the SPL-token transfer to a stealth address (§5.7).
///
/// Returns two instructions: idempotently create the stealth owner's ATA
/// (sender pays ATA rent), then `transfer_checked` into it. Works for
/// SPL Token and Token-2022 by passing the matching `token_program_id`;
/// NFTs are the `amount = 1, decimals = 0` case (see [`build_nft_payment`]).
///
/// `sender_token_account` is the sender's existing token account for
/// `mint`. The mint is necessarily visible on-chain; only the recipient
/// identity is hidden.
pub fn build_spl_payment(
    sender: &Pubkey,
    stealth_address: &Pubkey,
    mint: &Pubkey,
    sender_token_account: &Pubkey,
    token_program_id: &Pubkey,
    amount: u64,
    decimals: u8,
) -> Vec<Instruction> {
    use spl_associated_token_account::{
        get_associated_token_address_with_program_id,
        instruction::create_associated_token_account_idempotent,
    };

    let stealth_ata =
        get_associated_token_address_with_program_id(stealth_address, mint, token_program_id);

    let create_ata =
        create_associated_token_account_idempotent(sender, stealth_address, mint, token_program_id);

    let transfer = spl_token::instruction::transfer_checked(
        token_program_id,
        sender_token_account,
        mint,
        &stealth_ata,
        sender,
        &[],
        amount,
        decimals,
    )
    .expect("transfer_checked args are valid");

    vec![create_ata, transfer]
}

/// Build an NFT transfer to a stealth address (§5.7) — the
/// `amount = 1, decimals = 0` SPL case. For standard and Token-2022
/// NFTs. (Programmable NFTs additionally require Metaplex token-record /
/// rule-set accounts — construct those via `mpl-token-metadata`.)
pub fn build_nft_payment(
    sender: &Pubkey,
    stealth_address: &Pubkey,
    mint: &Pubkey,
    sender_token_account: &Pubkey,
    token_program_id: &Pubkey,
) -> Vec<Instruction> {
    build_spl_payment(
        sender,
        stealth_address,
        mint,
        sender_token_account,
        token_program_id,
        1,
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamports_of(ix: &Instruction) -> u64 {
        // System Transfer: bincode tag 2 (u32 LE) then lamports (u64 LE).
        assert_eq!(&ix.data[0..4], &[2, 0, 0, 0]);
        u64::from_le_bytes(ix.data[4..12].try_into().unwrap())
    }

    #[test]
    fn sol_payment_adds_rent_buffer() {
        let sender = Pubkey::new_unique();
        let stealth = Pubkey::new_unique();
        let ix = build_sol_payment(&sender, &stealth, 1_000_000).unwrap();

        assert_eq!(ix.program_id, solana_sdk::system_program::id());
        assert_eq!(ix.accounts[0].pubkey, sender);
        assert!(ix.accounts[0].is_signer && ix.accounts[0].is_writable);
        assert_eq!(ix.accounts[1].pubkey, stealth);
        assert!(ix.accounts[1].is_writable);
        assert_eq!(lamports_of(&ix), 1_000_000 + RENT_EXEMPT_MIN);
    }

    #[test]
    fn sol_payment_rejects_lamport_overflow() {
        let sender = Pubkey::new_unique();
        let stealth = Pubkey::new_unique();
        let result = build_sol_payment(&sender, &stealth, u64::MAX);

        assert!(matches!(
            result,
            Err(crate::SlntError::LamportOverflow { .. })
        ));
    }

    #[test]
    fn spl_payment_creates_ata_then_transfers() {
        let sender = Pubkey::new_unique();
        let stealth = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let sender_ata = Pubkey::new_unique();
        let token_program = spl_token::id();

        let ixs = build_spl_payment(&sender, &stealth, &mint, &sender_ata, &token_program, 42, 6);
        assert_eq!(ixs.len(), 2);
        // 1st: create ATA (owned by the ATA program).
        assert_eq!(ixs[0].program_id, spl_associated_token_account::id());
        // 2nd: transfer_checked on the token program.
        assert_eq!(ixs[1].program_id, token_program);
        // transfer_checked instruction tag is 12.
        assert_eq!(ixs[1].data[0], 12);
    }

    #[test]
    fn nft_payment_is_amount_one_decimals_zero() {
        let sender = Pubkey::new_unique();
        let stealth = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let sender_ata = Pubkey::new_unique();
        let token_program = spl_token::id();

        let nft = build_nft_payment(&sender, &stealth, &mint, &sender_ata, &token_program);
        let spl = build_spl_payment(&sender, &stealth, &mint, &sender_ata, &token_program, 1, 0);
        assert_eq!(nft.len(), spl.len());
        assert_eq!(nft[1].data, spl[1].data);
    }
}
