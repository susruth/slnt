//! `slnt-announcer` — reference announcer / announcement service
//! (sRFC-0042 §5.8.4).
//!
//! Accepts announcement tuples over HTTP, batches them, and publishes
//! them to pinboard in transactions the service pays for — enabling the
//! sender's transfer to stay decoupled/silent (§5.8.1). The service
//! learns only the announcement tuple; without `b_scan` it cannot
//! determine the recipient.
//!
//! ```text
//! POST /announce                    -> { queued, batch_id, expected_slot }
//! GET  /announce/status/{batch_id}  -> { status, tx_signature? }
//! GET  /health
//! ```

mod service;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use slnt_sdk::announce::{AnnounceRequest, AnnounceResponse, AnnounceStatus};
use slnt_sdk::pinboard::build_post_batch_instruction;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::str::FromStr;

use service::{request_to_note_entry, AnnounceQueue, BatchStatus};

const PUBLISH_INTERVAL: Duration = Duration::from_secs(2);
const MAX_BATCH: usize = 40;

type Shared = Arc<Mutex<AnnounceQueue>>;

#[derive(Parser)]
#[command(name = "slnt-announcer", about = "Slnt announcer / announcement service (sRFC-0042 §5.8.4)")]
struct Args {
    #[arg(long, default_value = "SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2")]
    pinboard: String,
    /// Solana JSON-RPC URL. If unset, the service collects but does not publish.
    #[arg(long)]
    rpc_url: Option<String>,
    /// Path to the fee-payer keypair JSON (Solana CLI format).
    #[arg(long)]
    keypair: Option<String>,
    #[arg(long, default_value = "127.0.0.1:8082")]
    bind: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let pinboard = Pubkey::from_str(&args.pinboard).expect("invalid pinboard pubkey");
    let queue: Shared = Arc::new(Mutex::new(AnnounceQueue::new()));

    if let (Some(rpc_url), Some(keypair_path)) = (args.rpc_url.clone(), args.keypair.clone()) {
        let payer = match read_keypair(&keypair_path) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("error: failed to load keypair `{keypair_path}`: {e}");
                std::process::exit(2);
            }
        };
        let bg = queue.clone();
        tokio::spawn(async move {
            publisher_loop(bg, rpc_url, pinboard, payer).await;
        });
    } else {
        eprintln!("collect-only mode: set --rpc-url and --keypair to publish");
    }

    let app = Router::new()
        .route("/announce", post(post_announce))
        .route("/announce/status/{batch_id}", get(get_status))
        .route("/health", get(|| async { "ok" }))
        .with_state(queue);

    let listener = tokio::net::TcpListener::bind(&args.bind).await.expect("bind");
    println!("slnt-announcer listening on http://{}", args.bind);
    axum::serve(listener, app).await.expect("serve");
}

async fn post_announce(
    State(queue): State<Shared>,
    Json(req): Json<AnnounceRequest>,
) -> Result<Json<AnnounceResponse>, (StatusCode, String)> {
    let entry = request_to_note_entry(&req).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let id = lock_queue(&queue).enqueue(entry).map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "announcement queue is full".to_string(),
        )
    })?;
    Ok(Json(AnnounceResponse {
        queued: true,
        batch_id: id.to_string(),
        expected_slot: 0,
    }))
}

async fn get_status(
    State(queue): State<Shared>,
    Path(batch_id): Path<String>,
) -> Result<Json<AnnounceStatus>, (StatusCode, String)> {
    let id: u64 = batch_id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid batch_id".to_string()))?;
    let q = lock_queue(&queue);
    let status = q
        .status(id)
        .ok_or((StatusCode::NOT_FOUND, "unknown batch_id".to_string()))?;
    Ok(Json(to_wire_status(status)))
}

/// Acquire the queue lock, recovering the guard if a previous holder
/// panicked (a poisoned `Mutex` must not wedge the whole service).
fn lock_queue(queue: &Shared) -> std::sync::MutexGuard<'_, AnnounceQueue> {
    queue.lock().unwrap_or_else(|e| e.into_inner())
}

fn to_wire_status(s: &BatchStatus) -> AnnounceStatus {
    match s {
        BatchStatus::Pending => AnnounceStatus { status: "pending".into(), tx_signature: None },
        BatchStatus::Confirmed(sig) => {
            AnnounceStatus { status: "confirmed".into(), tx_signature: Some(sig.clone()) }
        }
        BatchStatus::Failed(_) => AnnounceStatus { status: "failed".into(), tx_signature: None },
    }
}

async fn publisher_loop(queue: Shared, rpc_url: String, pinboard: Pubkey, payer: Keypair) {
    let rpc = RpcClient::new(rpc_url);
    loop {
        tokio::time::sleep(PUBLISH_INTERVAL).await;
        let drained = { lock_queue(&queue).take_pending(MAX_BATCH) };
        if drained.is_empty() {
            continue;
        }
        let (ids, entries): (Vec<u64>, Vec<_>) = drained.into_iter().unzip();
        let ix = build_post_batch_instruction(&pinboard, &payer.pubkey(), entries);
        let result = (|| {
            let bh = rpc.get_latest_blockhash()?;
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&payer.pubkey()),
                &[&payer],
                bh,
            );
            rpc.send_and_confirm_transaction(&tx)
        })();
        let mut q = lock_queue(&queue);
        match result {
            Ok(sig) => {
                for id in ids {
                    q.set_status(id, BatchStatus::Confirmed(sig.to_string()));
                }
            }
            Err(e) => {
                for id in ids {
                    q.set_status(id, BatchStatus::Failed(e.to_string()));
                }
            }
        }
    }
}

/// Load a Solana CLI keypair (JSON array of 64 bytes). Returns a clean
/// error string instead of panicking, so a bad path/format exits the
/// process with a useful message rather than an unwinding panic.
fn read_keypair(path: &str) -> Result<Keypair, String> {
    let bytes = std::fs::read_to_string(path).map_err(|e| format!("read file: {e}"))?;
    let nums: Vec<u8> =
        serde_json::from_str(&bytes).map_err(|e| format!("parse JSON byte array: {e}"))?;
    if nums.len() != 64 {
        return Err(format!("expected 64 bytes, got {}", nums.len()));
    }
    Keypair::try_from(&nums[..]).map_err(|e| format!("invalid keypair bytes: {e}"))
}
