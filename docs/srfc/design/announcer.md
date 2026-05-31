# Announcer (`slnt-announcer`) — Service-Level Design

| | |
|---|---|
| **Component** | `slnt-announcer` — SLNT reference announcer / announcement relayer (HTTP → pinboard) |
| **Status** | Reference implementation |
| **Spec** | sRFC-0042 (`docs/srfc/0001-slnt-silent-payments.md`), §5.8 |
| **Binary** | `slnt-announcer` (`crates/slnt-announcer`) |

Source: `crates/slnt-announcer/src/main.rs` (HTTP router, publisher loop,
keypair loading) and `crates/slnt-announcer/src/service.rs` (queue +
request conversion, isolated from HTTP/RPC for testing). Wire types and the
on-chain instruction builder come from `crates/slnt-sdk` — see `rust-sdk.md`.

---

## 1. Purpose and role in the decoupled-announce flow (§5.8.1)

In v1 the sender **MUST** default to **decoupled mode**. The asset-transfer
transaction carries only the asset movement and any required account creation —
**no SLNT instruction, no marker** — so on-chain it is indistinguishable from a
normal transfer to a fresh address. That is what keeps the payment *silent*.

The announcement tuple `(scheme_id, R, view_tag, metadata)` must still reach the
pinboard program so the recipient can detect the payment, but publishing it from
the sender's own wallet would re-link the announcement to the sender. The
announcement service breaks that link: it accepts the tuple over HTTP, batches
it with other senders' tuples, and publishes it to pinboard in a `post_batch`
transaction **the service pays for**. The transfer and the announcement are now
decoupled — different transactions, different fee payers, no on-chain
correlation between the sender's transfer and the published note.

Critically, the service learns **only** the announcement tuple. Without the
recipient's scan key `b_scan` it cannot compute `P_stealth`, cannot identify the
recipient, and cannot link the note to a transfer. See §5 for the full trust
analysis.

### 1.1 Self-announce fallback (§5.8.2)

The service is a convenience, not a trusted dependency. Because a service can
fail or censor, the sender's wallet runs a **self-announce fallback** as its
safety net (this lives in the SDK / wallet, not in this service — see
`rust-sdk.md`):

1. After submitting to the service, the wallet starts a timer
   (RECOMMENDED `T` = 60 s) and watches pinboard logs for a note with matching
   `R`.
2. If no matching note appears before `T`, the wallet **MUST** publish
   `post(scheme_id, R, view_tag, metadata)` itself, paid from the sender's
   wallet — trading a little unlinkability for a guarantee the funds are not
   stranded.
3. Recipients **MUST** deduplicate announcements by `R`, since a service+sender
   race may publish two notes for the same payment.

This service therefore never needs to be highly available or trusted for
correctness; a dropped or delayed announcement degrades gracefully to a
self-announce.

---

## 2. HTTP API

The router (`main.rs`) exposes three routes. Default bind address is
`127.0.0.1:8082` (`--bind`).

```
POST /announce                    -> { queued, batch_id, expected_slot }
GET  /announce/status/{batch_id}  -> { status, tx_signature? }
GET  /health                      -> "ok"
```

Binary fields (`ephemeral_pub`, `metadata`) travel as **base58 strings**, matching
the SDK wire types in `crates/slnt-sdk/src/announce.rs` (`AnnounceRequest`,
`AnnounceResponse`, `AnnounceStatus`).

### 2.1 `POST /announce`

Request body — `AnnounceRequest`:

| Field | Type | Notes |
|---|---|---|
| `scheme_id` | `u16` | Cryptographic suite id; v1 = `1`. MUST be non-zero. |
| `ephemeral_pub` | `string` | The sender's ephemeral X25519 public key `R`, base58. Decodes to exactly 32 bytes. |
| `view_tag` | `u8` | First byte of the shared-secret hash; the scanner's cheap pre-filter. |
| `metadata` | `string` | Opaque blob, base58 (empty string if none). Decodes to ≤ 64 bytes. |
| `payment_proof` | `string?` | OPTIONAL, opaque, service-defined. Omitted when absent. |

```json
{
  "scheme_id": 1,
  "ephemeral_pub": "6vhKzLT2P6...8sQ2",
  "view_tag": 16,
  "metadata": "",
  "payment_proof": null
}
```

Response — `AnnounceResponse`:

```json
{
  "queued": true,
  "batch_id": "42",
  "expected_slot": 0
}
```

`batch_id` is the queue id assigned to this submission (the handle for status
polling). `expected_slot` is `0` in this reference implementation (it does not
predict a landing slot). On a validation error the service returns
`400 Bad Request` with the error string as the body (see §3).

### 2.2 `GET /announce/status/{batch_id}`

`batch_id` is parsed as a `u64`; a non-numeric value returns `400`, an unknown id
returns `404`. The body is `AnnounceStatus`:

| Field | Type | Notes |
|---|---|---|
| `status` | `string` | `pending` \| `confirmed` \| `failed`. |
| `tx_signature` | `string?` | Present only when `confirmed`; the pinboard tx signature. |

```json
{ "status": "pending" }
```

```json
{ "status": "confirmed", "tx_signature": "5Jh...Qp" }
```

```json
{ "status": "failed" }
```

`tx_signature` is omitted entirely when absent (`pending`/`failed`).

### 2.3 `GET /health`

Returns `200 OK` with the literal body `ok`. A liveness probe only; it does not
check RPC connectivity or queue depth.

---

## 3. Request conversion (`request_to_note_entry`)

`service::request_to_note_entry` decodes the base58 wire request into an on-chain
`NoteEntry` (`crates/slnt-sdk/src/pinboard.rs`), validating every field. This is
the service's only input-validation boundary; it runs synchronously inside the
`POST /announce` handler and any error becomes a `400`.

| Check | Rule | Error |
|---|---|---|
| `scheme_id` | MUST be non-zero | `scheme_id must be non-zero` |
| `ephemeral_pub` | base58-decodes, **exactly 32 bytes** | `ephemeral_pub base58: …` / `ephemeral_pub must be 32 bytes` |
| `metadata` | empty → empty vec; else base58-decodes | `metadata base58: …` |
| `metadata` | decoded length ≤ `MAX_METADATA_LEN` (64) | `metadata exceeds 64 bytes` |

The resulting `NoteEntry { scheme_id, ephemeral_pub, view_tag, metadata }` is the
exact unit `post_batch` serializes. Note that `view_tag` is passed through
unvalidated (any `u8` is legal) and `payment_proof` is **not consulted** by this
reference service — it is accepted and discarded.

---

## 4. Queue model (`AnnounceQueue`)

`AnnounceQueue` is an in-memory FIFO with monotonic ids, isolated from HTTP/RPC
so it can be unit-tested directly. It is shared across the HTTP handlers and the
publisher loop behind an `Arc<Mutex<…>>`.

- `enqueue(entry) -> u64` — assigns the next monotonically increasing id (`0, 1,
  2, …`), pushes `(id, entry)` onto the pending vector, and records status
  `Pending`. The id is returned to the caller as `batch_id`.
- `take_pending(max) -> Vec<(u64, NoteEntry)>` — drains up to `max` pending
  entries in FIFO order for publishing.
- `set_status(id, status)` / `status(id) -> Option<&BatchStatus>` — the status
  map drives `GET /announce/status`.

### 4.1 `BatchStatus` state machine

Each enqueued id moves through:

```
                 take_pending + post_batch
   [Pending] ───────────────────────────────► one of:
       │                                        ├─ send_and_confirm Ok(sig)
       │                                        │     └─► [Confirmed(sig)]   (terminal)
       │                                        └─ send_and_confirm Err(e)
       │                                              └─► [Failed(err)]      (terminal)
       └─ (still queued / not yet drained) stays [Pending]
```

- **`Pending`** — enqueued, not yet included in a published transaction.
- **`Confirmed(sig)`** — included in a `post_batch` tx that `send_and_confirm`
  confirmed; `sig` surfaces as `tx_signature`. Terminal.
- **`Failed(err)`** — the publishing transaction errored; `err` is retained
  internally for diagnostics but is **not** returned over the wire (the status
  response only reports `"failed"` with no signature). Terminal.

There is no retry: a `Failed` id stays failed. The sender's self-announce
fallback (§1.1) is the recovery path, not a service-side retry.

---

## 5. Publisher loop

When started with both `--rpc-url` and `--keypair`, `main` spawns a background
`publisher_loop`. Every `PUBLISH_INTERVAL` (**2 s**) it:

1. Drains up to `MAX_BATCH` (**40**) pending entries with `take_pending(40)`. If
   the queue is empty, it sleeps again.
2. Splits the drained `(id, entry)` pairs and builds a single `post_batch`
   instruction via `build_post_batch_instruction(&pinboard, &payer.pubkey(),
   entries)` (`crates/slnt-sdk/src/pinboard.rs`). The instruction is the 8-byte
   Anchor discriminator `SHA-256("global:post_batch")[..8]` followed by
   borsh-serialized `Vec<NoteEntry>` — see the byte layout in
   `pinboard-program.md` §2.2.
3. Fetches a recent blockhash, signs the transaction with the **service fee-payer
   keypair** (`Transaction::new_signed_with_payer`, payer = the service), and
   calls `send_and_confirm_transaction`.
4. Marks **every** id in the drained batch `Confirmed(sig)` on success or
   `Failed(err)` on error — the whole batch shares one transaction outcome.

Because the fee payer is the service's keypair, the published note carries no
link to any sender — this is the mechanism that realizes decoupled announce
(§1).

### 5.1 Fee-payer keypair file format

`read_keypair` reads the path given by `--keypair` and parses it as a JSON array
of bytes — the standard **Solana CLI keypair format** (e.g. the output of
`solana-keygen new -o payer.json`), a 64-element `u8` array `[12, 34, …]`. It is
loaded with `Keypair::try_from(&bytes[..])`; a malformed or non-64-byte file
panics at startup.

### 5.2 Collect-only mode

If **either** `--rpc-url` or `--keypair` is missing, the publisher loop is not
spawned. The service still accepts `POST /announce` and assigns batch ids, but
nothing is ever published — all ids stay `Pending` forever. It logs
`collect-only mode: set --rpc-url and --keypair to publish`. This is useful for
testing the HTTP surface and queueing behaviour without an RPC endpoint or
funded keypair.

---

## 6. Privacy and trust analysis (§5.8.1 / §5.8.4)

**What the service learns.** Exactly the announcement tuple
`(scheme_id, R, view_tag, metadata)` — the same bytes that are already public on
pinboard once published. `R` is fresh per-payment ephemeral randomness;
`view_tag` is one hash byte; `metadata` is opaque.

**What the service cannot learn.** Without the recipient's scan key `b_scan` it
**cannot**:

- compute the stealth address `P_stealth`,
- recover the recipient's spend key `B_spend` or scan key `B_scan`,
- identify the recipient, or
- link the announcement to the on-chain transfer.

Detection (the ECDH + view-tag match against `b_scan`) is entirely
recipient-local; the service performs none of it.

**What a malicious service *can* do.** Only liveness/censorship attacks: it can
**drop** or **delay** an announcement. Both are mitigated by the sender's
self-announce fallback (§1.1) — after `T` = 60 s with no matching `R` on
pinboard, the wallet republishes itself. A malicious service therefore cannot
strand funds; at worst it forces a slightly-less-private self-announce.

**What it *cannot* do.** It cannot deanonymize the recipient, forge a different
recipient, or alter the tuple meaningfully (a tampered `R`/`view_tag`/`metadata`
simply produces a note the intended recipient won't match, equivalent to
dropping it — again covered by self-announce).

**Sender-side correlation risk.** The privacy guarantee is about *on-chain*
unlinkability. At the transport layer, a sender who submits over an
authenticated or otherwise identifying channel (a logged-in API session, a
stable IP, a `payment_proof` tied to an account) lets the service correlate
*which sender submitted which `R`* — even though it still cannot tell who the
*recipient* is. Senders that care about this **SHOULD** submit anonymously
(e.g. over Tor, unauthenticated) and treat `payment_proof` as a deanonymization
vector. This reference service applies no such protections.

---

## 7. Cost analysis

The service pays the pinboard transaction fee — roughly **5,000 lamports** for a
single-signature transaction (one signer: the fee payer). `post_batch` lets that
fixed fee be **amortized across up to `MAX_BATCH` = 40 notes**, so the marginal
per-announcement cost approaches `5,000 / N` lamports as the batch fills (≈ 125
lamports/note at N = 40), plus per-entry compute. See `pinboard-program.md` §5
for the on-chain side of this amortization.

**Payment economics are out of scope for v1.** The spec does **not** standardize
how a service is compensated. `payment_proof` is an opaque, service-defined field
(§5.8.4: "pricing/auth/SLA out of scope"); this reference implementation accepts
it and ignores it, publishing every well-formed request for free. A production
service would define and enforce its own payment/rate-limiting policy here.

---

## 8. Configuration and operational notes

CLI arguments (`clap`, in `main.rs`):

| Flag | Default | Meaning |
|---|---|---|
| `--pinboard` | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` | Pinboard program id to publish to. |
| `--rpc-url` | *(unset)* | Solana JSON-RPC URL. Unset → collect-only mode. |
| `--keypair` | *(unset)* | Path to the fee-payer keypair JSON (Solana CLI format). Unset → collect-only mode. |
| `--bind` | `127.0.0.1:8082` | HTTP listen address. |

**This is a reference implementation, not production-hardened.** Known
limitations, by design:

- **No durable queue.** The queue and status map are in-memory; a restart loses
  all `Pending` entries and all status history. Senders rely on self-announce,
  not on this service's durability.
- **No real payment enforcement** (`payment_proof` ignored) and **no
  rate-limiting / spam protection** — anyone who can reach the port can spend the
  fee payer's SOL.
- **No retry / backoff** on a failed `post_batch`; failed ids are terminal.
- **No TLS / auth** built in; deploy behind a reverse proxy if exposed.
- Batch outcome is all-or-nothing: 40 ids share one tx result.

---

## 9. Testing summary

Unit tests in `service.rs` cover the conversion and queue logic (the pure core),
independent of HTTP and RPC:

- **`converts_valid_request`** — a valid request maps to a `NoteEntry` with `R`,
  `scheme_id`, and empty metadata preserved.
- **`rejects_oversized_metadata`** — metadata decoding to 65 bytes is rejected
  (> `MAX_METADATA_LEN`).
- **`rejects_bad_ephemeral_length`** — an `ephemeral_pub` that decodes to a
  non-32-byte length is rejected.
- **`queue_assigns_incrementing_ids_and_tracks_status`** — ids start at `0` and
  increment; new ids are `Pending`; `take_pending` drains them; `set_status`
  transitions to `Confirmed(sig)`.
- **`take_pending_respects_max`** — draining with a `max` smaller than the queue
  returns exactly `max` and leaves the rest pending (FIFO).

The wire-type round-trips and the `build_post_batch_instruction` byte layout are
covered in the SDK tests (`crates/slnt-sdk/src/announce.rs`,
`crates/slnt-sdk/src/pinboard.rs`) — see `rust-sdk.md`.

---

## See also

- `pinboard-program.md` — the on-chain program this service publishes to, and the
  `post_batch` byte layout.
- `rust-sdk.md` — `AnnounceRequest`/`Response`/`Status` wire types,
  `build_post_batch_instruction`, and the sender-side self-announce logic.
- `indexer-service.md` — the read side: an indexer that retains and serves
  published announcements for recipient scanning (§5.10).
