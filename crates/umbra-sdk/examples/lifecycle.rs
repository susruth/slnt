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

use ed25519_dalek::Signer as _;
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
    println!("RPC: {RPC_URL}");
    let pinboard_id = Pubkey::from_str(PINBOARD_PROGRAM_ID)
        .expect("PINBOARD_PROGRAM_ID parse");
    println!("pinboard program: {pinboard_id}");

    // ---- 1. Setup ----
    println!("\n[1/6] setup: creating keypairs");
    let sender_wallet = Keypair::new();
    let recipient_wallet = Keypair::new();
    println!("  sender:    {}", sender_wallet.pubkey());
    println!("  recipient: {}", recipient_wallet.pubkey());

    airdrop_blocking(&rpc, &sender_wallet.pubkey(), 10 * ONE_SOL);
    airdrop_blocking(&rpc, &recipient_wallet.pubkey(), 10 * ONE_SOL);

    // ---- 2. Recipient: derive stealth keys + meta-address ----
    println!("\n[2/6] recipient: deriving stealth keys");
    // For the demo, "sign" the canonical message with a fresh Ed25519
    // keypair derived from a fixed seed. In production this would be
    // a user wallet signature.
    let canonical_msg = umbra_sdk::keys::CANONICAL_MESSAGE_LOCALNET.as_bytes();
    let recipient_id_seed: [u8; 32] = [
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
    ];
    let recipient_id_sk =
        ed25519_dalek::SigningKey::from_bytes(&recipient_id_seed);
    let signature: ed25519_dalek::Signature = recipient_id_sk.sign(canonical_msg);
    let sig_bytes: [u8; 64] = signature.to_bytes();

    let (spend, scan) = umbra_sdk::keys::derive_stealth_keys(&sig_bytes)
        .expect("derive_stealth_keys");
    let meta = umbra_sdk::keys::MetaAddress::from_keys(&spend, &scan);
    let meta_str = meta.encode_bech32m().expect("encode meta-address");
    println!("  meta-address: {meta_str}");

    // ---- 3. Sender: derive stealth address ----
    println!("\n[3/6] sender: deriving stealth address");
    let decoded_meta =
        umbra_sdk::keys::MetaAddress::decode_bech32m(&meta_str)
            .expect("decode meta-address");
    // Use a strong RNG in production. Seeded here so demo output is
    // reproducible across runs.
    let mut sender_rng = ChaCha20Rng::seed_from_u64(0xdeadbeef);
    let payment = umbra_sdk::sender::derive_payment(&decoded_meta, &mut sender_rng)
        .expect("derive_payment");
    println!("  stealth address: {}", payment.stealth_address);
    println!("  ephemeral_pub:   {}", hex::encode(payment.ephemeral_pub));
    println!("  view_tag:        0x{:02x}", payment.view_tag);

    // ---- 4. Sender: transfer SOL + post pinboard Note in one tx ----
    println!("\n[4/6] sender: sending 1 SOL + posting note");
    let transfer_ix = solana_system_interface::instruction::transfer(
        &sender_wallet.pubkey(),
        &payment.stealth_address,
        ONE_SOL,
    );
    let post_ix = umbra_sdk::pinboard::build_post_instruction(
        &pinboard_id,
        &sender_wallet.pubkey(),
        umbra_sdk::keys::SCHEME_ID_V1,
        payment.ephemeral_pub,
        payment.view_tag,
        vec![], // metadata: empty for demo
    );
    let latest_blockhash =
        rpc.get_latest_blockhash().expect("get_latest_blockhash");
    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[transfer_ix, post_ix],
        Some(&sender_wallet.pubkey()),
        &[&sender_wallet],
        latest_blockhash,
    );
    let sig = rpc
        .send_and_confirm_transaction(&tx)
        .expect("send_and_confirm_transaction (payment + post)");
    println!("  payment tx: {sig}");
    let stealth_balance = rpc
        .get_balance(&payment.stealth_address)
        .expect("get_balance stealth");
    println!("  stealth balance: {stealth_balance} lamports");
    assert_eq!(stealth_balance, ONE_SOL);

    // Defer task 10 stages — placeholders.
    println!("\n[5/6] recipient: scanning … (next task)");
    println!("[6/6] recipient: sweeping  … (next task)");
    let _ = (recipient_wallet, spend, scan); // suppress unused warnings
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
