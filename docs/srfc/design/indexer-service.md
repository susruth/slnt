# SLNT Reference Indexer Service (`slnt-indexer`) — Service-Level Design

| | |
|---|---|
| **Component** | `slnt-indexer` — reference announcement indexer (`crates/slnt-indexer`) |
| **Status** | Reference implementation |
| **Spec** | sRFC-0042 (`docs/srfc/0001-slnt-silent-payments.md`), §5.10 |
| **Binary** | `slnt-indexer` (`src/main.rs`) |
| **Crate version** | `0.1.0`, edition 2021, license MIT |

The indexer is the **OPTIONAL** discovery enhancement of sRFC-0042 §5.10. It
subscribes to pinboard `Note` events, retains them in memory, and serves them
over HTTP by slot range. It holds **no** scan keys: matching is recipient-local,
so polling slot ranges leaks nothing about which announcements are interesting to
the caller.

Source: `crates/slnt-indexer/src/main.rs` (HTTP + background task) and
`crates/slnt-indexer/src/store.rs` (data model + store). It consumes the live
scan feed from the SDK (`rust-sdk.md`, `crates/slnt-sdk/src/scan_stream.rs`).
On-chain `Note` event byte detail lives in `pinboard-program.md`; the sibling
push-based discovery service is `announcer.md`.

This document describes the **reference** implementation. It is in-memory and is
**not** production-hardened — see [§8](#8-costoperational-analysis).

---

## 1. Architecture

```text
                       pinboard program logs (on-chain)
                                  │
                                  │  logsSubscribe (Mentions: pinboard pid,
                                  │  commitment = confirmed)
                                  ▼
          ┌─────────────────────────────────────────────────┐
          │  slnt-sdk::scan_stream                           │
          │  subscribe_pinboard_notes_with_slot(ws, pid, cb) │
          │    → notes_from_log_lines() parses Note events   │
          │    → cb(slot, NoteEvent) per event               │
          └─────────────────────────────────────────────────┘
                                  │  (slot, NoteEvent)
                                  ▼
          ┌─────────────────────────────────────────────────┐
          │  background tokio task (reconnect loop, 2s)      │
          │    store.write().insert(slot, &note)             │
          └─────────────────────────────────────────────────┘
                                  │
                                  ▼
          ┌─────────────────────────────────────────────────┐
          │  Arc<RwLock<AnnouncementStore>>  (in-memory)     │
          │    append-only Vec<StoredAnnouncement>          │
          └─────────────────────────────────────────────────┘
                                  │  store.read().query(...)
                                  ▼
          ┌─────────────────────────────────────────────────┐
          │  axum HTTP server                                │
          │    GET /announcements?since_slot&limit           │
          │    GET /health                                   │
          └─────────────────────────────────────────────────┘
                                  │
                                  ▼
              recipient wallets (poll, then scan locally)
```

Three moving parts share one `Arc<RwLock<AnnouncementStore>>`:

1. **The ingest feed.** `slnt_sdk::scan_stream::subscribe_pinboard_notes_with_slot`
   opens a websocket `logsSubscribe` against the pinboard program id with
   `commitment = confirmed`, parses every `Note` event out of each transaction's
   log lines via `notes_from_log_lines`, and invokes a callback with the
   confirmation **slot** and the `NoteEvent`. The by-slot variant is used (rather
   than the plain `subscribe_pinboard_notes`) precisely because the indexer serves
   by slot range. The SDK function performs no key operations and learns nothing
   about which notes matched.

2. **The background task.** A `tokio::spawn`ed task owns the subscription. The
   callback takes the store write lock and appends each note. The whole
   subscription call is wrapped in a `loop` that reconnects after any termination
   (see [§6](#6-reconnection--liveness)).

3. **The HTTP server.** An `axum` router with two routes, sharing the store via
   `with_state`. The `/announcements` handler takes the read lock and serves a
   slot-range query; `/health` returns `"ok"`.

The read/write split via `RwLock` lets HTTP queries run concurrently with each
other while ingest appends are serialized.

---

## 2. Data model

### 2.1 `StoredAnnouncement`

Each observed `Note` is flattened into a `StoredAnnouncement`
(`store.rs`), the JSON record served by the API:

| Field | Type | JSON | Source |
|---|---|---|---|
| `slot` | `u64` | number | log context confirmation slot |
| `scheme_id` | `u16` | number | `NoteEvent.scheme_id` |
| `ephemeral_pub` | `String` | base58 | `bs58(NoteEvent.ephemeral_pub)` — the sender ephemeral `R` |
| `view_tag` | `u8` | number | `NoteEvent.view_tag` |
| `metadata` | `String` | base58 | `bs58(NoteEvent.metadata)`, empty string if none |

```json
{
  "slot": 123456789,
  "scheme_id": 1,
  "ephemeral_pub": "8Z9p2Yx...base58 32-byte R...",
  "view_tag": 85,
  "metadata": ""
}
```

### 2.2 Construction from a `NoteEvent`

`StoredAnnouncement::from_note(slot, note)` is the single conversion point:

- `slot` is carried verbatim from the `logsSubscribe` context.
- `ephemeral_pub` is the 32-byte `R` (`note.ephemeral_pub`), base58-encoded.
- `metadata` is `note.metadata` (a byte vector, empty when the sender posted
  no metadata), base58-encoded — an empty input yields the empty string.
- `scheme_id` and `view_tag` are copied as integers.

Base58 (not hex/base64) is used so the encoded `R` reads like a Solana address,
matching how the SDK and explorers render 32-byte keys.

### 2.3 The store

`AnnouncementStore` wraps a single `Vec<StoredAnnouncement>`. It is
**append-only**: `insert` pushes each new announcement onto the tail. Because
notes stream from `logsSubscribe` in confirmation order, **insertion order tracks
slot order** — the vector is non-decreasing in `slot`. No sorting, indexing, or
deduplication is performed; the reference store trusts the feed's ordering.

---

## 3. HTTP API

### 3.1 `GET /announcements`

Query parameters (`AnnouncementsQuery`):

| Param | Type | Default | Semantics |
|---|---|---|---|
| `since_slot` | `u64` (optional) | none → all | return announcements with `slot >= since_slot` |
| `limit` | `usize` (optional) | `DEFAULT_LIMIT = 1000` | cap result count; clamped to `MAX_LIMIT = 10000` |

The handler resolves the limit as `q.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT)`,
then `store.query(since_slot, limit)` filters by `slot >= since_slot` (or keeps
all when `since_slot` is absent) and `take(limit)`s from the front of the
append-only vector. Results come back in insertion (slot) order. A caller paging
forward passes the highest `slot` it has already seen as the next `since_slot`.

Because the store is append-only and time-ordered, `take(limit)` returns the
**earliest** matching announcements at or after `since_slot`; a client draining a
backlog repeats with an advancing `since_slot` until fewer than `limit` rows come
back.

**Example request**

```
GET /announcements?since_slot=123456000&limit=2
```

**Example response** (`200 OK`, `application/json`)

```json
[
  {
    "slot": 123456001,
    "scheme_id": 1,
    "ephemeral_pub": "8Z9p2Yx...",
    "view_tag": 85,
    "metadata": ""
  },
  {
    "slot": 123456042,
    "scheme_id": 1,
    "ephemeral_pub": "3KkRm1Q...",
    "view_tag": 17,
    "metadata": "2NEpo7TZ"
  }
]
```

The response is always a JSON array (empty `[]` when nothing matches). If the
read lock is poisoned the handler degrades to an empty array rather than erroring.

### 3.2 `GET /health`

Returns `200 OK` with the body `ok`. A liveness probe only — it does not assert
that the upstream subscription is currently connected.

---

## 4. Client usage and the scan loop

A recipient wallet uses the indexer as a **pull** alternative to running its own
`logsSubscribe`:

1. Poll `GET /announcements?since_slot=<last_seen>&limit=<n>`.
2. For each `StoredAnnouncement`, base58-decode `ephemeral_pub` back to the
   32-byte `R`, then run the **recipient-local** scan (`scan_note` /
   `scan_note_candidates`, see `rust-sdk.md`): the view-tag pre-filter rejects
   ~255/256 candidates after one ECDH + one SHA-256, and surviving candidates are
   confirmed with a full derivation.
3. Advance `last_seen` to the highest slot returned and repeat.

The view-tag filter and all key math run on the client. The indexer never sees
which `R` matched, never sees a scan key, and cannot tell which rows the caller
cared about.

---

## 5. Relationship to the §5.10 baseline

sRFC-0042 §5.10 makes **self-scan via logs the REQUIRED baseline**: every
conforming wallet **MUST** be able to scan by subscribing to pinboard logs
(`logsSubscribe`) and backfilling gaps via `getSignaturesForAddress` +
`getTransaction`. That baseline is implemented directly by the SDK's
`scan_stream` module (`rust-sdk.md`) — the same feed this indexer consumes.

The indexer is the §5.10 **OPTIONAL** enhancement. RPC retention is
provider-dependent and many public endpoints do not provide full historical logs,
so a recipient offline beyond the provider's retention window cannot reconstruct
the missed announcements from `logsSubscribe` alone. An indexer that has been
continuously subscribed retains those announcements and serves them by slot
range, closing the offline gap without the recipient running a full backfill. It
is strictly additive: a wallet
that only ever self-scans is fully conforming; the indexer just spares recipients
the log-retention cliff.

This is the **pull** side of discovery. The push counterpart — an announcement
service that delivers `R` to a recipient endpoint at payment time — is documented
in `announcer.md`. Both are optional and neither holds scan keys.

---

## 6. Reconnection & liveness

The background task is a perpetual reconnect loop:

```text
loop {
    res = subscribe_pinboard_notes_with_slot(ws_url, pinboard, insert_cb).await;
    eprintln!("subscription ended ({res:?}); reconnecting in 2s");
    sleep(2s).await;
}
```

- `subscribe_pinboard_notes_with_slot` returns only when the stream ends — on a
  clean close, a websocket drop, or a connect/subscribe error. Any of these
  returns control to the loop.
- The loop logs the outcome and sleeps **2 seconds** (fixed backoff) before
  reconnecting, so a flapping or unreachable RPC produces at most one reconnect
  attempt every ~2 s rather than a hot spin.
- On reconnect a fresh `PubsubClient` and a fresh `logs_subscribe` are
  established; the existing in-memory store is untouched and continues to serve.

### 6.1 Failure modes

- **Gap on reconnect.** Any announcements confirmed during the disconnect-plus-2s
  window are **not** in the store: `logsSubscribe` is live-only and the reference
  indexer does **not** backfill via `getSignaturesForAddress`. A recipient relying
  solely on one indexer can miss notes across an outage. Production deployments
  should add backfill and/or run multiple independent indexers.
- **No connection at all.** If the RPC is unreachable from boot, the loop retries
  every 2 s indefinitely; `/health` still returns `ok` and `/announcements`
  returns `[]`. Liveness of the HTTP server does not imply a live feed.
- **Process restart.** The store is in-memory only; a restart loses all retained
  announcements and the indexer is only as complete as the time since it last
  came up.
- **Malformed log lines.** Non-`Note` log lines are ignored and malformed `Note`
  lines are skipped by `notes_from_log_lines`; the feed does not abort on bad
  input.

---

## 7. Limits & configuration

CLI flags (`clap`, `src/main.rs`):

| Flag | Default | Meaning |
|---|---|---|
| `--pinboard` | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` | Pinboard program id to index. Parsed as a `Pubkey`; an invalid value aborts at startup. |
| `--ws-url` | `ws://127.0.0.1:8900` | Solana websocket RPC URL for `logsSubscribe`. |
| `--bind` | `127.0.0.1:8081` | Address the HTTP server binds to. |

Built-in constants:

| Constant | Value | Role |
|---|---|---|
| `DEFAULT_LIMIT` | `1000` | result cap when `limit` is omitted |
| `MAX_LIMIT` | `10000` | hard ceiling; larger `limit` values are clamped |
| reconnect backoff | `2s` | fixed sleep between subscription retries |
| commitment | `confirmed` | subscription commitment level (from the SDK) |

There is no configured retention bound, no auth, and no TLS — all deliberately
out of scope for the reference build.

---

## 8. Cost / operational analysis

**The reference implementation is in-memory and not production-hardened.** Its
purpose is to demonstrate the §5.10 indexer contract, not to run a service.

- **Unbounded memory growth.** The store is an append-only `Vec` that never
  evicts. Memory grows linearly with the total number of announcements observed
  since process start. Each `StoredAnnouncement` is small (a slot, two integers,
  a ~44-char base58 `R`, and base58 metadata), but there is no cap — long-running
  instances will eventually exhaust memory. Restarting frees memory but loses
  history.
- **Query cost.** `query` is a linear scan over the vector
  (`filter` + `take`). Because the vector is slot-ordered, `since_slot` could be a
  binary search, but the reference keeps it a simple linear `filter` for clarity.
  At reference scale this is fine; at production scale it is not.
- **No persistence.** A restart starts from empty and only retains announcements
  observed thereafter.

A production deployment would add, at minimum:

- **Durable storage** (e.g. SQLite or another embedded/append store) so the index
  survives restarts and can be backfilled, with a slot-indexed `since_slot` lookup
  instead of a linear scan.
- **Retention commitments** — a published, bounded retention window (or full
  history) that recipients can rely on when budgeting how long they may stay
  offline, replacing the reference build's unbounded-but-volatile behavior.
- **Backfill on reconnect** via `getSignaturesForAddress` + `getTransaction` to
  close the disconnect gap described in [§6.1](#61-failure-modes).
- **Multiple competing indexers.** Because an indexer is an availability
  dependency (not a privacy one — it holds no keys), recipients should be able to
  query several independently operated indexers. A diverse set prevents any single
  operator from capturing discovery or censoring announcements, and lets a
  recipient cross-check coverage across the offline window.

Operationally the indexer is privacy-cheap: it stores only public on-chain data
and answers public slot-range queries, so it carries none of the key-custody risk
of a delegated scanner ([§5.10](#5-relationship-to-the-510-baseline)).

---

## 9. Testing

Unit tests live alongside the store (`store.rs`) and cover the query/encoding
contract:

| Test | Asserts |
|---|---|
| `query_filters_by_since_slot` | `query(Some(20), …)` over slots {10,20,30} returns exactly the slot-≥20 rows, in order (20 then 30). |
| `query_respects_limit` | `query(None, 3)` over 10 inserts returns 3 rows. |
| `query_none_since_returns_all` | `query(None, 100)` with one insert returns that one row (absent `since_slot` ⇒ all). |
| `stored_announcement_encodes_r_as_base58` | `from_note` base58-encodes `ephemeral_pub` such that decoding round-trips the original 32-byte `R`, and `slot` is carried verbatim. |

The SDK feed the indexer depends on is tested separately in
`scan_stream.rs` (`extracts_only_note_events_from_mixed_lines`,
`no_notes_when_no_program_data`) — see `rust-sdk.md`.

Run with:

```sh
cargo test -p slnt-indexer
```
