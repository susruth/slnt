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
const PINBOARD_PROGRAM_ID: &str = "SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2";

const ONE_SOL: u64 = 1_000_000_000;

fn main() {
    let rpc = RpcClient::new_with_commitment(RPC_URL.to_string(), CommitmentConfig::confirmed());

    println!("== Slnt lifecycle demo ==");
    println!("RPC: {RPC_URL}");
    let pinboard_id = Pubkey::from_str(PINBOARD_PROGRAM_ID).expect("PINBOARD_PROGRAM_ID parse");
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
    let canonical_msg = slnt_sdk::keys::CANONICAL_MESSAGE_LOCALNET.as_bytes();
    let recipient_id_seed: [u8; 32] = [
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
        0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45,
        0x67, 0x89,
    ];
    let recipient_id_sk = ed25519_dalek::SigningKey::from_bytes(&recipient_id_seed);
    let signature: ed25519_dalek::Signature = recipient_id_sk.sign(canonical_msg);
    let sig_bytes: [u8; 64] = signature.to_bytes();

    let (spend, scan) =
        slnt_sdk::keys::derive_stealth_keys(&sig_bytes).expect("derive_stealth_keys");
    let meta = slnt_sdk::keys::MetaAddress::from_keys(&spend, &scan);
    let meta_str = meta.encode_bech32m().expect("encode meta-address");
    println!("  meta-address: {meta_str}");

    // ---- 3. Sender: derive stealth address ----
    println!("\n[3/6] sender: deriving stealth address");
    let decoded_meta =
        slnt_sdk::keys::MetaAddress::decode_bech32m(&meta_str).expect("decode meta-address");
    // Use a strong RNG in production. Seeded here so demo output is
    // reproducible across runs.
    let mut sender_rng = ChaCha20Rng::seed_from_u64(0xdeadbeef);
    let payment =
        slnt_sdk::sender::derive_payment(&decoded_meta, &mut sender_rng).expect("derive_payment");
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
    let post_ix = slnt_sdk::pinboard::build_post_instruction(
        &pinboard_id,
        &sender_wallet.pubkey(),
        slnt_sdk::keys::SCHEME_ID_V1,
        payment.ephemeral_pub,
        payment.view_tag,
        vec![], // metadata: empty for demo
    );
    let latest_blockhash = rpc.get_latest_blockhash().expect("get_latest_blockhash");
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

    // ---- 5. Recipient: scan pinboard logs ----
    println!("\n[5/6] recipient: scanning pinboard logs");
    let matched =
        scan_pinboard_for_match(&rpc, &pinboard_id, &spend, &scan).expect("scan returned a match");
    println!("  found match: stealth address {}", matched.stealth_address);
    assert_eq!(matched.stealth_address, payment.stealth_address);

    // ---- 6. Recipient: sweep stealth address ----
    println!("\n[6/6] recipient: sweeping stealth balance to main wallet");
    let stealth_signing_key =
        slnt_sdk::stealth_signing::StealthSigningKey::new(matched.stealth_scalar);
    // Sanity check before we sign anything: the signing key's public
    // bytes must equal the stealth address bytes.
    assert_eq!(
        stealth_signing_key.public_bytes(),
        payment.stealth_address.to_bytes(),
    );

    let recipient_before = rpc
        .get_balance(&recipient_wallet.pubkey())
        .expect("recipient balance before");
    let stealth_before = rpc
        .get_balance(&payment.stealth_address)
        .expect("stealth balance before");
    const TX_FEE: u64 = 5_000;
    let sweep_amount = stealth_before - TX_FEE;

    let sweep_ix = solana_system_interface::instruction::transfer(
        &payment.stealth_address,
        &recipient_wallet.pubkey(),
        sweep_amount,
    );
    let latest_blockhash = rpc
        .get_latest_blockhash()
        .expect("get_latest_blockhash for sweep");

    // Build the message and sign it manually using our scalar-mode key.
    let message = solana_sdk::message::Message::new_with_blockhash(
        &[sweep_ix],
        Some(&payment.stealth_address),
        &latest_blockhash,
    );
    let message_bytes = message.serialize();
    let ed_sig = stealth_signing_key.sign(&message_bytes);
    let signature = solana_sdk::signature::Signature::from(ed_sig.to_bytes());

    let sweep_tx = solana_sdk::transaction::Transaction {
        signatures: vec![signature],
        message,
    };
    // Solana's local validator double-checks signatures during simulation;
    // verify ours locally too to fail fast if anything is off.
    sweep_tx
        .verify()
        .expect("locally-built sweep tx must verify");

    let sweep_sig = rpc
        .send_and_confirm_transaction(&sweep_tx)
        .expect("send_and_confirm_transaction (sweep)");
    println!("  sweep tx: {sweep_sig}");

    let recipient_after = rpc
        .get_balance(&recipient_wallet.pubkey())
        .expect("recipient balance after");
    let stealth_after = rpc
        .get_balance(&payment.stealth_address)
        .expect("stealth balance after");
    println!("  recipient balance: {recipient_before} → {recipient_after}");
    println!("  stealth balance:   {stealth_before} → {stealth_after}");

    // ---- Verification ----
    assert_eq!(stealth_after, 0, "stealth account should drain to 0");
    let recipient_gain = recipient_after - recipient_before;
    assert_eq!(
        recipient_gain, sweep_amount,
        "recipient should gain exactly the swept lamports"
    );
    println!("\n== SUCCESS: stealth payment delivered and swept ==");
    println!(
        "   {} lamports moved to recipient through a stealth address",
        recipient_gain
    );
}

/// Scan recent pinboard transactions, parse Note events, and try
/// `scan_note` until we find one for the given (spend, scan) pair.
/// Times out after ~10 seconds of polling.
fn scan_pinboard_for_match(
    rpc: &RpcClient,
    pinboard_id: &Pubkey,
    spend: &slnt_sdk::keys::SpendKey,
    scan: &slnt_sdk::keys::ScanKey,
) -> Option<slnt_sdk::recipient::NoteMatch> {
    use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
    use solana_sdk::commitment_config::CommitmentConfig;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let cfg = GetConfirmedSignaturesForAddress2Config {
            before: None,
            until: None,
            limit: Some(100),
            commitment: Some(CommitmentConfig::confirmed()),
        };
        let sigs = rpc
            .get_signatures_for_address_with_config(pinboard_id, cfg)
            .unwrap_or_default();
        for sig_info in &sigs {
            let sig = sig_info
                .signature
                .parse::<solana_sdk::signature::Signature>()
                .ok();
            let Some(sig) = sig else { continue };
            let tx = rpc.get_transaction_with_config(
                &sig,
                solana_client::rpc_config::RpcTransactionConfig {
                    encoding: Some(solana_transaction_status::UiTransactionEncoding::Json),
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(0),
                },
            );
            let Ok(tx) = tx else { continue };
            let logs: Vec<String> = tx
                .transaction
                .meta
                .map(|m| {
                    let opt: Option<Vec<String>> = m.log_messages.into();
                    opt.unwrap_or_default()
                })
                .unwrap_or_default();
            for line in logs {
                if let Ok(Some(note)) = slnt_sdk::pinboard::try_parse_note_log(&line) {
                    if let Ok(Some(m)) = slnt_sdk::recipient::scan_note(
                        spend,
                        scan,
                        &note.ephemeral_pub,
                        note.view_tag,
                    ) {
                        return Some(m);
                    }
                }
            }
        }
        if std::time::Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
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
