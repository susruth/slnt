# Slnt Test UI — Design Spec

**Date:** 2026-05-21
**Status:** Draft for review
**Scope:** A small local web UI to exercise `slnt-sdk` end-to-end against `solana-test-validator`.

## 1. Goal & Non-goals

### Goal

Let a developer try the Slnt stealth-payment protocol in a browser by driving two simulated users (sender and recipient) through the full lifecycle — generate keys, share a meta-address, send a payment, scan the pinboard, sweep the stealth address — and see results update live.

The UI is a tool for testing and demoing the SDK. It is **not** a production wallet.

### Non-goals

- No real browser wallet integration (no Phantom, no wallet-adapter). Server-managed keypairs only.
- No persistence: state is in-memory and lost on server restart.
- No multi-user isolation: there is exactly one Sender slot and one Recipient slot per server.
- No devnet/mainnet support: localhost validator only.
- No SPL tokens, no labels, no relayer-paid sweeps. SOL transfers only, matching `examples/lifecycle.rs` scope.
- No auth, no rate limiting, no TLS. Bind to `127.0.0.1` only.

## 2. Architecture

### 2.1 Process model

A single Rust binary (`slnt-ui`) runs an `axum` HTTP server on `127.0.0.1:3000`. The server:

- Serves a static HTML/CSS/JS frontend from an embedded `assets/` directory.
- Exposes a JSON REST API at `/api/*` for sender and recipient operations.
- Owns a single `solana_client::rpc_client::RpcClient` pointed at `http://127.0.0.1:8899`.
- Holds two in-memory singleton sessions (`Sender`, `Recipient`) behind a mutex.

The validator and the pinboard program are assumed to be running. A helper script (`scripts/demo-ui.sh`) starts both alongside the UI; the binary itself never spawns or kills the validator.

### 2.2 State model

```rust
struct AppState {
    rpc: RpcClient,
    pinboard_id: Pubkey,
    sender: Mutex<Option<Sender>>,
    recipient: Mutex<Option<Recipient>>,
}

struct Sender {
    wallet: Keypair,
    history: Vec<SenderPayment>,  // tx_sig, stealth_address, view_tag, ephemeral_pub, amount, timestamp
}

struct Recipient {
    wallet: Keypair,                   // sweep destination
    spend: SpendKey,
    scan: ScanKey,
    meta_address: String,              // bech32m-encoded
    known_stealth: HashMap<Pubkey, Scalar>,  // stealth_address -> p_stealth (cache to skip re-derivation)
    swept: HashSet<Pubkey>,            // stealth addresses already swept (don't show in incoming list)
    sweep_history: Vec<SweepRecord>,
}
```

All mutation goes through the mutex. Operations are short (microseconds for crypto, ~100 ms for RPC) so a single mutex is fine.

### 2.3 Frontend

Vanilla HTML + CSS + JS, no framework, no bundler. Three pages:

- `/` → 302 redirect to `/sender`
- `/sender` → `sender.html`
- `/recipient` → `recipient.html`

Each page is roughly:

- Wallet bar (address, balance, "Airdrop" button)
- Page-specific panels (see §3.2 and §3.3)
- Inline JS that calls `/api/*` and re-renders

Shared dark theme via `assets/styles.css`. Addresses rendered in monospace. Common helpers (`fetchJson`, `truncateAddr`, `copyToClipboard`) live in `assets/common.js`.

The recipient page sets a `setInterval` of 2 s that calls `GET /api/recipient/scan` and updates the incoming-payments list and balances.

## 3. UI Layout

### 3.1 Shared elements

- **Wallet bar** (top of each page): address (monospace, truncated `8eDqJX…KayD`), full address visible on hover, current balance in SOL (4 decimal places), and an `Airdrop` button that requests 10 SOL.
- **Empty state**: if no wallet exists yet for that role, the bar reads "no wallet" and shows a `Create wallet` button instead. Creating it auto-airdrops 10 SOL.

### 3.2 Sender page (`/sender`)

Stacked panels, top to bottom:

1. Wallet bar.
2. **Send payment**: textarea for `meta_address` (slnt1…), number input for SOL amount, `Send` button. Disabled while in-flight. On success, the bottom of this panel renders a small receipt with `tx_sig`, derived `stealth_address`, and `view_tag`.
3. **History**: list of past payments (newest first), each row: `↗ <amount> SOL → <stealth_address_short> · <tx_sig_short>`.

### 3.3 Recipient page (`/recipient`)

Stacked panels:

1. Wallet bar.
2. **Your meta-address**: a card showing the slnt1… string in a monospace block. Buttons: `Copy`, `Regenerate`. If no keys derived yet, panel shows a `Generate stealth keys` button instead.
3. **Incoming payments**: header has `Scan now` button and a "scanning · last refreshed Ns ago" status line. Body lists matched, unswept stealth addresses. Each row: stealth address (mono), balance in SOL, view_tag (hex), and a `Sweep →` button. On sweep success the row disappears (added to `swept`).
4. **Sweep history**: list of past sweeps (newest first), each row: `✓ <amount> SOL from <stealth_short> · <tx_sig_short>`.

## 4. REST API

All responses are JSON with `Content-Type: application/json`. All errors return `{ "error": "<message>" }` with an appropriate HTTP status. Field names are `snake_case`.

### 4.1 Sender

| Method | Path | Body | Response |
|---|---|---|---|
| `POST` | `/api/sender/wallet` | `{}` | `{ address, balance_lamports }` — creates a new keypair (overwrites any existing) and airdrops 10 SOL synchronously |
| `GET`  | `/api/sender/wallet` | — | `{ address, balance_lamports }` or 404 |
| `POST` | `/api/sender/airdrop` | `{}` | `{ balance_lamports }` — adds 10 SOL |
| `POST` | `/api/sender/payment` | `{ meta_address: string, amount_sol: number }` | `{ tx_sig, stealth_address, view_tag, ephemeral_pub_hex }` |
| `GET`  | `/api/sender/history` | — | `{ payments: [...] }` newest first |

### 4.2 Recipient

| Method | Path | Body | Response |
|---|---|---|---|
| `POST` | `/api/recipient/wallet` | `{}` | `{ address, balance_lamports }` — creates sweep-destination keypair, airdrops 10 SOL |
| `GET`  | `/api/recipient/wallet` | — | `{ address, balance_lamports }` or 404 |
| `POST` | `/api/recipient/airdrop` | `{}` | `{ balance_lamports }` |
| `POST` | `/api/recipient/derive_keys` | `{}` | `{ meta_address }` — generates a fresh Ed25519 identity seed, signs the canonical message, derives (SpendKey, ScanKey), stores them, returns the meta-address. Overwrites any existing. |
| `GET`  | `/api/recipient/scan` | — | `{ incoming: [{ stealth_address, balance_lamports, view_tag, ephemeral_pub_hex }], last_scanned_at }` — fetches up to 100 recent pinboard txs, parses Note events, runs `scan_note` per note, returns matches whose stealth address has nonzero balance and isn't in `swept` |
| `POST` | `/api/recipient/sweep` | `{ stealth_address: string }` | `{ tx_sig, swept_lamports }` — looks up the cached scalar, signs and submits sweep tx, adds to `swept` + sweep history |
| `GET`  | `/api/recipient/sweeps` | — | `{ sweeps: [...] }` newest first |

### 4.3 Error handling

All handlers return `Result<Json<T>, ApiError>`. `ApiError` is one variant per failure class:

- `BadRequest(String)` → 400 — malformed body, invalid meta-address, etc.
- `NotInitialized(&'static str)` → 409 — e.g. `POST /payment` when no sender wallet exists
- `Rpc(String)` → 502 — Solana RPC failure
- `Internal(String)` → 500 — unexpected

The frontend renders `error` as a toast or inline error string under the relevant button.

## 5. Validator orchestration

A new script `scripts/demo-ui.sh` mirrors `scripts/demo-lifecycle.sh`:

1. Ensure `target/deploy/pinboard.so` exists (build if missing).
2. Kill any stray `solana-test-validator`.
3. Start a fresh validator with the pinboard preloaded.
4. Wait until `solana cluster-version` succeeds.
5. `cargo run --release -p slnt-ui`.
6. Trap EXIT to tear down the validator.

The binary itself never touches the validator. It assumes RPC at `127.0.0.1:8899` and the pinboard at the well-known program id.

## 6. Crate layout

```
crates/slnt-ui/
├── Cargo.toml              # deps: slnt-sdk (path), axum, tokio (rt-multi-thread, macros),
│                           #       tower-http (fs, trace), serde (derive), serde_json,
│                           #       solana-sdk = "2.3", solana-client = "2.3",
│                           #       solana-system-interface = "1", anyhow, hex,
│                           #       tracing, tracing-subscriber, rand_chacha, rand_core
├── src/
│   ├── main.rs             # tokio main, build AppState, mount router, axum::serve
│   ├── state.rs            # AppState, Sender, Recipient structs
│   ├── sender.rs           # 5 handler fns
│   ├── recipient.rs        # 6 handler fns; scan helper that reuses scan_pinboard_for_match logic
│   └── error.rs            # ApiError + IntoResponse impl
└── assets/                 # served via tower_http::services::ServeDir
    ├── index.html
    ├── sender.html
    ├── recipient.html
    ├── styles.css
    ├── common.js
    ├── sender.js
    └── recipient.js
```

The crate is added to the workspace `members` list (already includes `"crates/*"`, so it picks up automatically).

## 7. Concurrency & correctness notes

- The single mutex around `Sender` and `Recipient` makes the server effectively serial within each role. That's acceptable: only one human is driving the UI.
- `scan` is read-heavy and writes only to `swept` filtering. To avoid blocking sweeps during a poll, `scan`'s mutex hold time is minimized (snapshot the cache, drop the lock, then call the SDK + RPC, then re-acquire briefly to update `known_stealth`). This is the only handler that needs care.
- Sweeps use the same scalar-mode signing path as `examples/lifecycle.rs`. We tx-locally `.verify()` before submit, same as the example.

## 8. Testing

- Per-handler unit tests are skipped — handlers are thin wrappers over SDK calls already covered in `slnt-sdk` tests.
- Manual end-to-end via `scripts/demo-ui.sh` is the acceptance test. The implementation plan will spell out the exact click-through script.
- A smoke `cargo build -p slnt-ui --release` runs in the plan's verification steps.

## 9. Out of scope / future

- TypeScript SDK + production wallet flow (documented in `2026-05-20-umbra-sdk-rust-design.md`)
- Multi-tab session isolation (would require session cookies + per-session state)
- Devnet/mainnet support (would require a network selector + wallet adapter)
- Encrypted metadata, labels, SPL tokens — all out of v1 SDK scope too

## 10. Open questions resolved during brainstorming

| Question | Decision |
|---|---|
| UI surface | Local web app (axum + browser) |
| User model | Two separate views, simulated as singleton sessions |
| Wallet handling | Server-managed keypairs, no browser-wallet adapter |
| Sender layout | Stacked panels |
| Recipient layout | Wallet bar → meta-address card → live incoming list → sweep history |
| Auto-scan | Frontend polls `GET /api/recipient/scan` every 2 s |
| Frontend stack | Vanilla HTML + CSS + JS, no framework |
| Persistence | None — in-memory, server restart wipes state |
