//! End-to-end stealth-payment lifecycle demo.
//!
//! Run against a fresh `solana-test-validator` with the pinboard program
//! preloaded. See `scripts/demo-lifecycle.sh` for the orchestration.
//!
//! Stages:
//!   1. Setup        — create keypairs, airdrop SOL
//!   2. Recipient    — derive stealth keys, emit meta-address
//!   3. Sender       — derive stealth address, transfer SOL, post note
//!   4. Recipient    — scan pinboard logs, recover scalar
//!   5. Recipient    — sweep stealth address to main wallet
//!   6. Verification — assert balances

use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{str::FromStr, time::Duration};

const RPC_URL: &str = "http://127.0.0.1:8899";

/// Pinboard program ID (current dev keypair; baked in for demo
/// reproducibility — the shell wrapper deploys it).
const PINBOARD_PROGRAM_ID: &str = "G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P";

const ONE_SOL: u64 = 1_000_000_000;

fn main() {
    let rpc = RpcClient::new_with_commitment(
        RPC_URL.to_string(),
        CommitmentConfig::confirmed(),
    );

    println!("== Umbra lifecycle demo ==");
    println!("RPC: {}", RPC_URL);
    let pinboard_id = Pubkey::from_str(PINBOARD_PROGRAM_ID)
        .expect("PINBOARD_PROGRAM_ID parse");
    println!("pinboard program: {pinboard_id}");

    // 1. Setup: keypairs + airdrops.
    println!("\n[1/6] setup: creating keypairs");
    let sender_wallet = Keypair::new();
    let recipient_wallet = Keypair::new();
    println!("  sender:    {}", sender_wallet.pubkey());
    println!("  recipient: {}", recipient_wallet.pubkey());

    airdrop_blocking(&rpc, &sender_wallet.pubkey(), 10 * ONE_SOL);
    airdrop_blocking(&rpc, &recipient_wallet.pubkey(), 10 * ONE_SOL);
    println!("  airdropped 10 SOL to each");

    // Sanity check: balances are visible.
    println!(
        "  sender balance after airdrop:    {} lamports",
        rpc.get_balance(&sender_wallet.pubkey()).expect("get_balance")
    );
    println!(
        "  recipient balance after airdrop: {} lamports",
        rpc.get_balance(&recipient_wallet.pubkey()).expect("get_balance")
    );

    // Suppress unused warnings until later tasks wire these in.
    let _ = ChaCha20Rng::seed_from_u64(0);
    let _ = pinboard_id;
}

/// Request an airdrop and poll until the balance is at least
/// `min_lamports`. Panics on RPC error or 30s timeout.
fn airdrop_blocking(rpc: &RpcClient, recipient: &Pubkey, lamports: u64) {
    let sig = rpc
        .request_airdrop(recipient, lamports)
        .expect("request_airdrop");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if rpc.confirm_transaction(&sig).unwrap_or(false) {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("airdrop {sig} did not confirm within 30s");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    // Confirmation does not always mean the balance updated; poll.
    let bal_deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let bal = rpc.get_balance(recipient).unwrap_or(0);
        if bal >= lamports {
            return;
        }
        if std::time::Instant::now() > bal_deadline {
            panic!("airdrop balance did not appear within 10s");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
