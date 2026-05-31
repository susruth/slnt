//! `slnt-indexer` — reference announcement indexer (sRFC-0042 §5.10).
//!
//! Subscribes to pinboard `Note` events via `logsSubscribe`, retains them
//! in memory, and serves them over HTTP:
//!
//! ```text
//! GET /announcements?since_slot=<u64>&limit=<usize>
//! GET /health
//! ```
//!
//! The indexer receives **no** scan keys; matching is recipient-local.

mod store;

use std::sync::{Arc, RwLock};

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use clap::Parser;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use store::{AnnouncementStore, StoredAnnouncement};

const DEFAULT_LIMIT: usize = 1000;
const MAX_LIMIT: usize = 10_000;

type Shared = Arc<RwLock<AnnouncementStore>>;

#[derive(Parser)]
#[command(name = "slnt-indexer", about = "Slnt announcement indexer (sRFC-0042 §5.10)")]
struct Args {
    /// Pinboard program id to index.
    #[arg(long, default_value = "SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2")]
    pinboard: String,
    /// Solana websocket RPC URL (e.g. ws://127.0.0.1:8900).
    #[arg(long, default_value = "ws://127.0.0.1:8900")]
    ws_url: String,
    /// Address to bind the HTTP server to.
    #[arg(long, default_value = "127.0.0.1:8081")]
    bind: String,
}

#[derive(Debug, Deserialize)]
struct AnnouncementsQuery {
    since_slot: Option<u64>,
    limit: Option<usize>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let pinboard = Pubkey::from_str(&args.pinboard).expect("invalid pinboard pubkey");
    let store: Shared = Arc::new(RwLock::new(AnnouncementStore::new()));

    // Background: stream pinboard notes into the store.
    let ws_url = args.ws_url.clone();
    let bg_store = store.clone();
    tokio::spawn(async move {
        loop {
            let store_for_cb = bg_store.clone();
            let res = slnt_sdk::scan_stream::subscribe_pinboard_notes_with_slot(
                &ws_url,
                &pinboard,
                move |slot, note| {
                    // Recover the guard if a previous holder panicked, so a
                    // poisoned lock never silently stops ingestion.
                    store_for_cb
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(slot, &note);
                },
            )
            .await;
            eprintln!("subscription ended ({res:?}); reconnecting in 2s");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });

    let app = Router::new()
        .route("/announcements", get(get_announcements))
        .route("/health", get(health))
        .with_state(store);

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .expect("bind");
    println!("slnt-indexer listening on http://{}", args.bind);
    axum::serve(listener, app).await.expect("serve");
}

async fn get_announcements(
    State(store): State<Shared>,
    Query(q): Query<AnnouncementsQuery>,
) -> Json<Vec<StoredAnnouncement>> {
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
    let guard = store.read().unwrap_or_else(|e| e.into_inner());
    Json(guard.query(q.since_slot, limit))
}

#[derive(serde::Serialize)]
struct Health {
    /// Announcements currently retained.
    retained: usize,
    /// Announcements evicted by the retention cap.
    dropped: u64,
}

async fn health(State(store): State<Shared>) -> Json<Health> {
    let guard = store.read().unwrap_or_else(|e| e.into_inner());
    Json(Health {
        retained: guard.len(),
        dropped: guard.dropped(),
    })
}
