//! Recipient sweep flows (sRFC-0042 §5.9).
//!
//! A stealth address holds value but only the rent-exempt minimum in SOL,
//! so it cannot pay its own fees. A **relayer** signs as fee payer and is
//! compensated from the swept value. The transactions these builders
//! produce are signed by both the relayer (fee payer) and the stealth
//! key (authority over the swept funds).
//!
//! **Close-to-relayer rule (§5.9, §8.3):** rent reclaimed by closing the
//! stealth account/ATA MUST go to the relayer or another stealth address
//! the recipient controls — never the recipient's main wallet, which
//! would create a direct `stealth → main` link. These builders enforce
//! that by rejecting a destination equal to `main_wallet`.

use crate::error::SlntError;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

/// Reject `candidate` if it equals the recipient's `main_wallet` (§8.3).
///
/// Pass `None` for `main_wallet` only when the caller has independently
/// guaranteed unlinkability (e.g. a known-stealth destination).
pub fn ensure_not_main_wallet(
    candidate: &Pubkey,
    main_wallet: Option<&Pubkey>,
) -> Result<(), SlntError> {
    if main_wallet == Some(candidate) {
        return Err(SlntError::CloseToMainWallet);
    }
    Ok(())
}

/// Build a SOL sweep from a stealth account (§5.9).
///
/// Two `SystemProgram::transfer`s from the stealth account: one paying
/// the relayer `relayer_take` lamports, one paying `destination` the
/// remainder (`balance - relayer_take`). The account reaches zero and is
/// reclaimed by the runtime. `destination` MUST NOT be `main_wallet`.
///
/// The returned instructions must be assembled into a transaction whose
/// **fee payer is the relayer** and which is signed by the stealth key.
pub fn build_sol_sweep(
    stealth_address: &Pubkey,
    destination: &Pubkey,
    relayer: &Pubkey,
    balance: u64,
    relayer_take: u64,
    main_wallet: Option<&Pubkey>,
) -> Result<Vec<Instruction>, SlntError> {
    ensure_not_main_wallet(destination, main_wallet)?;
    if relayer_take >= balance {
        return Err(SlntError::RelayerTakeTooLarge {
            take: relayer_take,
            balance,
        });
    }
    let to_recipient = balance - relayer_take;
    Ok(vec![
        solana_system_interface::instruction::transfer(stealth_address, relayer, relayer_take),
        solana_system_interface::instruction::transfer(stealth_address, destination, to_recipient),
    ])
}

/// Build an SPL-token sweep from a stealth ATA (§5.9).
///
/// Three instructions, all with the stealth key as authority:
/// 1. `transfer_checked` the token to `destination_ata` (`amount - relayer_take`);
/// 2. `transfer_checked` `relayer_take` to `relayer_token_account` (in-kind fee);
/// 3. `CloseAccount` the stealth ATA, sending reclaimed rent to `close_destination`.
///
/// `close_destination` MUST NOT be `main_wallet` (§8.3); the relayer
/// fronts the SOL fee. Works for SPL Token and Token-2022 via
/// `token_program_id`.
#[allow(clippy::too_many_arguments)]
pub fn build_spl_sweep(
    stealth_authority: &Pubkey,
    stealth_ata: &Pubkey,
    destination_ata: &Pubkey,
    relayer_token_account: &Pubkey,
    mint: &Pubkey,
    token_program_id: &Pubkey,
    amount: u64,
    relayer_take: u64,
    decimals: u8,
    close_destination: &Pubkey,
    main_wallet: Option<&Pubkey>,
) -> Result<Vec<Instruction>, SlntError> {
    ensure_not_main_wallet(close_destination, main_wallet)?;
    if relayer_take > amount {
        return Err(SlntError::RelayerTakeTooLarge {
            take: relayer_take,
            balance: amount,
        });
    }
    let to_recipient = amount - relayer_take;

    let transfer_to_dest = spl_token::instruction::transfer_checked(
        token_program_id,
        stealth_ata,
        mint,
        destination_ata,
        stealth_authority,
        &[],
        to_recipient,
        decimals,
    )
    .map_err(|e| SlntError::Rpc(e.to_string()))?;

    let pay_relayer = spl_token::instruction::transfer_checked(
        token_program_id,
        stealth_ata,
        mint,
        relayer_token_account,
        stealth_authority,
        &[],
        relayer_take,
        decimals,
    )
    .map_err(|e| SlntError::Rpc(e.to_string()))?;

    let close = spl_token::instruction::close_account(
        token_program_id,
        stealth_ata,
        close_destination,
        stealth_authority,
        &[],
    )
    .map_err(|e| SlntError::Rpc(e.to_string()))?;

    Ok(vec![transfer_to_dest, pay_relayer, close])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamports_of(ix: &Instruction) -> u64 {
        assert_eq!(&ix.data[0..4], &[2, 0, 0, 0]);
        u64::from_le_bytes(ix.data[4..12].try_into().unwrap())
    }

    #[test]
    fn sol_sweep_splits_balance_between_relayer_and_destination() {
        let stealth = Pubkey::new_unique();
        let dest = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let ixs = build_sol_sweep(&stealth, &dest, &relayer, 1_000_000, 5_000, None).unwrap();
        assert_eq!(ixs.len(), 2);
        assert_eq!(ixs[0].accounts[1].pubkey, relayer);
        assert_eq!(lamports_of(&ixs[0]), 5_000);
        assert_eq!(ixs[1].accounts[1].pubkey, dest);
        assert_eq!(lamports_of(&ixs[1]), 1_000_000 - 5_000);
    }

    #[test]
    fn sol_sweep_rejects_destination_equal_to_main_wallet() {
        let stealth = Pubkey::new_unique();
        let main = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let err = build_sol_sweep(&stealth, &main, &relayer, 1_000_000, 5_000, Some(&main));
        assert!(matches!(err, Err(SlntError::CloseToMainWallet)));
    }

    #[test]
    fn sol_sweep_rejects_oversized_relayer_take() {
        let stealth = Pubkey::new_unique();
        let dest = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let err = build_sol_sweep(&stealth, &dest, &relayer, 1_000, 1_000, None);
        assert!(matches!(err, Err(SlntError::RelayerTakeTooLarge { .. })));
    }

    #[test]
    fn stealth_to_stealth_destination_is_allowed() {
        // Destination is another stealth address (not the main wallet),
        // preserving unlinkability across hops (§5.9).
        let stealth = Pubkey::new_unique();
        let next_stealth = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let main = Pubkey::new_unique();
        let ixs = build_sol_sweep(
            &stealth,
            &next_stealth,
            &relayer,
            1_000_000,
            5_000,
            Some(&main),
        )
        .unwrap();
        assert_eq!(ixs.len(), 2);
    }

    #[test]
    fn spl_sweep_transfers_pays_relayer_and_closes() {
        let auth = Pubkey::new_unique();
        let stealth_ata = Pubkey::new_unique();
        let dest_ata = Pubkey::new_unique();
        let relayer_ata = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let close_dest = Pubkey::new_unique();
        let tp = spl_token::id();

        let ixs = build_spl_sweep(
            &auth,
            &stealth_ata,
            &dest_ata,
            &relayer_ata,
            &mint,
            &tp,
            100,
            3,
            0,
            &close_dest,
            None,
        )
        .unwrap();
        assert_eq!(ixs.len(), 3);
        assert_eq!(ixs[0].data[0], 12); // transfer_checked
        assert_eq!(ixs[1].data[0], 12); // transfer_checked
        assert_eq!(ixs[2].data[0], 9); // close_account
                                       // close destination receives reclaimed rent.
        assert!(ixs[2].accounts.iter().any(|a| a.pubkey == close_dest));
    }

    #[test]
    fn spl_sweep_rejects_close_to_main_wallet() {
        let auth = Pubkey::new_unique();
        let stealth_ata = Pubkey::new_unique();
        let dest_ata = Pubkey::new_unique();
        let relayer_ata = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let main = Pubkey::new_unique();
        let tp = spl_token::id();
        let err = build_spl_sweep(
            &auth,
            &stealth_ata,
            &dest_ata,
            &relayer_ata,
            &mint,
            &tp,
            100,
            3,
            0,
            &main,
            Some(&main),
        );
        assert!(matches!(err, Err(SlntError::CloseToMainWallet)));
    }
}
