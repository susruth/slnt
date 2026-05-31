# Pinboard Program — Service-Level Design

| | |
|---|---|
| **Component** | `pinboard` — SLNT on-chain announcement program (Solana / Anchor) |
| **Status** | Reference implementation |
| **Spec** | sRFC-0042 (`docs/srfc/0001-slnt-silent-payments.md`), §5.5 |
| **Program id** | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` |
| **Deployment status** | Live on devnet and testnet at the vanity address above, upgradeable under `78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR` while v1 is draft and unaudited. Canonical v1 deployment is intended to become immutable as soon as v1 is finalized and independently audited. |

Source: `programs/pinboard/src/lib.rs`. Client-side instruction builder and
event parser: `crates/slnt-sdk/src/pinboard.rs` (see `rust-sdk.md`).

---

## 1. Purpose

Pinboard is SLNT's **announcement layer**: a permissionless, stateless program
that emits opaque tagged notes as Anchor events. It is the Solana analog of the
ERC-5564 `Announcer`. A sender who has built a stealth payment publishes the
tuple `(scheme_id, R, view_tag, metadata)` to pinboard; recipients scan the
program's event stream to discover payments addressed to them (sRFC-0042 §5.5,
§5.10).

The program does exactly one thing: it validates a length bound and re-emits its
arguments as a `Note` event. It **holds no on-chain state** — there are no PDAs,
no accounts created, no rent. The recipient's wallet (or an indexer) reconstructs
meaning entirely from the event log.

### 1.1 Deliberate genericity

Pinboard is **not SLNT-specific**. It is a generic tagged-note bulletin board;
SLNT is merely its first consumer. Two design choices enforce this:

- **`scheme_id` is recorded, not whitelisted.** The program accepts any `u16`
  (`0x0000`–`0xFFFF`) and writes it verbatim into the event. It performs no
  validation of the value, no registry lookup, no feature gate. The meaning of a
  scheme id lives entirely in clients (sRFC-0042 §5.11): SLNT v1 clients MUST
  process only `scheme_id = 0x0001` and MUST ignore every other value. Future
  SLNT schemes — and entirely unrelated protocols — can share the same
  immutable deployment by picking their own scheme ids, without a program
  upgrade.

- **`metadata` is opaque.** The protocol treats it as raw bytes (≤ 64). It MAY
  carry an implementation-defined encrypted memo, but pinboard never parses it.

This is why the canonical program can become immutable after v1 finalization
and audit: it encodes no policy that could need revision. Policy lives in
clients.

---

## 2. Instruction set

The program exposes two instructions: `post` (one note) and `post_batch` (many
notes in one transaction). Both take a single signer — the fee payer — and emit
`Note` events.

On the wire, every Anchor instruction is `8-byte discriminator || borsh(args)`,
where the discriminator is `SHA-256("global:<instruction_snake_name>")[..8]`.

### 2.1 `post`

Emits exactly one `Note`.

```rust
pub fn post(
    _ctx: Context<Post>,
    scheme_id: u16,
    ephemeral_pub: [u8; 32],
    view_tag: u8,
    metadata: Vec<u8>,   // ≤ 64 bytes
) -> Result<()>;
```

**Discriminator:** `SHA-256("global:post")[..8]` =
`[223, 96, 234, 236, 158, 106, 145, 94]`
(`POST_DISCRIMINATOR` in `crates/slnt-sdk/src/pinboard.rs`).

**Instruction-data layout** (offsets are within `instruction.data`):

| Offset | Size (bytes) | Field | Encoding |
|--------|--------------|-------|----------|
| 0 | 8 | discriminator | fixed `[223,96,234,236,158,106,145,94]` |
| 8 | 2 | `scheme_id` | `u16`, little-endian |
| 10 | 32 | `ephemeral_pub` (`R`) | raw 32-byte array (no length prefix) |
| 42 | 1 | `view_tag` | `u8` |
| 43 | 4 | `metadata_len` | `u32`, little-endian (borsh `Vec<u8>` length prefix) |
| 47 | `metadata_len` (0–64) | `metadata` | raw bytes |

**Total instruction-data size:** 47 bytes (empty metadata) to 111 bytes
(64-byte metadata).

**Accounts / signer model** — `Post<'info>`:

| Index | Account | Signer | Writable | Role |
|-------|---------|--------|----------|------|
| 0 | `fee_payer` | yes | yes (`mut`) | Pays the transaction fee. No other role — it can be *anyone*; it is not bound to the sender, recipient, or `ephemeral_pub`. Marked `mut` only because its lamport balance changes when it pays the fee. |

No program accounts, no PDAs, no system program, no rent. The single account is
the fee payer.

**Validation:** `require!(metadata.len() <= MAX_METADATA_LEN, MetadataTooLong)`,
where `MAX_METADATA_LEN = 64`. There is no validation of `scheme_id`,
`ephemeral_pub`, or `view_tag` — any 32-byte `R` and any `u8` view tag are
accepted (curve-point validity of `R` is the sender's responsibility per
sRFC-0042 §5.3). On success it emits a single `Note`.

### 2.2 `post_batch`

Emits one `Note` per entry. Used by relayers and batching services (see
`announcer.md`) to amortize the per-transaction base fee across many
notes.

```rust
pub fn post_batch(_ctx: Context<PostBatch>, entries: Vec<NoteEntry>) -> Result<()>;

pub struct NoteEntry {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,   // ≤ 64 bytes
}
```

**Discriminator:** `SHA-256("global:post_batch")[..8]` =
`[172, 123, 234, 102, 14, 213, 76, 36]` (`POST_BATCH_DISCRIMINATOR`).

**Instruction-data layout:**

| Offset | Size | Field | Encoding |
|--------|------|-------|----------|
| 0 | 8 | discriminator | fixed `[172,123,234,102,14,213,76,36]` |
| 8 | 4 | `entries_len` | `u32`, little-endian (borsh `Vec<NoteEntry>` length prefix) |
| 12 | variable | `entries[0..N]` | each entry borsh-serialized, back to back |

Each `NoteEntry` serializes identically to the `post` argument tail:

| Within-entry offset | Size | Field | Encoding |
|---------------------|------|-------|----------|
| 0 | 2 | `scheme_id` | `u16` LE |
| 2 | 32 | `ephemeral_pub` | raw 32 bytes |
| 34 | 1 | `view_tag` | `u8` |
| 35 | 4 | `metadata_len` | `u32` LE |
| 39 | `metadata_len` (0–64) | `metadata` | raw bytes |

So one entry is **39 bytes** (empty metadata) to **103 bytes** (full metadata).
A batch's data size is `12 + Σ entry_size`. (The SDK's
`build_post_batch_instruction` pre-sizes its buffer at `8 + 4 + entries.len()*40`
as a fast estimate for the small-metadata common case.)

**Accounts / signer model** — `PostBatch<'info>`: identical to `post` — a single
`fee_payer` signer, `mut`, no other accounts.

**Validation, in order:**

1. `require!(!entries.is_empty(), EmptyBatch)` — an empty batch is rejected.
2. For each entry, `require!(entry.metadata.len() <= 64, MetadataTooLong)`.

Validation is **all-or-nothing**: the loop validates *and* emits per entry, but
because Anchor's `require!` aborts the whole transaction, a failure on entry `i`
rolls back every event — including those already emitted for entries `0..i`. A
batch with one oversized entry therefore produces **zero** on-chain events
(verified in `tests/pinboard.ts`).

---

## 3. `Note` event

Anchor events are written to transaction logs as a `Program data: <base64>` line.
The decoded payload is `SHA-256("event:<EventName>")[..8] || borsh(event)`.

```rust
#[event]
pub struct Note {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}
```

**Event discriminator:** `SHA-256("event:Note")[..8]` =
`[40, 182, 5, 151, 115, 43, 27, 97]` (`NOTE_EVENT_DISCRIMINATOR`).

**Decoded-payload byte layout** (this is the byte string scanners parse, and the
canonical layout from sRFC-0042 §5.5.2):

| Offset | Size | Field | Encoding |
|--------|------|-------|----------|
| 0 | 8 | event discriminator | fixed `[40,182,5,151,115,43,27,97]` |
| 8 | 2 | `scheme_id` | `u16` LE |
| 10 | 32 | `ephemeral_pub` (`R`) | raw 32 bytes |
| 42 | 1 | `view_tag` | `u8` |
| 43 | 4 | `metadata_len` | `u32` LE (borsh `Vec<u8>` length prefix) |
| 47 | `metadata_len` (0–64) | `metadata` | raw bytes |

**Total decoded size:** **47 bytes** (`metadata_len = 0`) to **111 bytes**
(`metadata_len = 64`). The first 47 bytes are a **fixed header**; only the
trailing `metadata` is variable.

**How it appears in logs.** For a successful `post`, the transaction's
`logMessages` contains a line of the form:

```
Program data: KLYFl3Mr...base64...
```

where the base64, once decoded, is exactly the 47–111 byte payload above. A
`post_batch` of N entries produces N such lines, one per emitted `Note`, in entry
order.

**Anchor 0.31 camelCase note.** Under Anchor 0.31 the generated IDL camelCases
the event name, so high-level decoders (e.g. the TypeScript `EventParser` /
`BorshCoder`) may surface the event under the name **`note`**, not `Note`.
Low-level log parsers MUST match the 8-byte event discriminator, not the event
name string or IDL casing. The **on-the-wire discriminator is unaffected** — it
is still computed from `"event:Note"` and remains
`[40,182,5,151,115,43,27,97]`. Likewise the event's field names are camelCased
in decoded objects (`schemeId`, `ephemeralPub`, `viewTag`, `metadata`), as seen
in `tests/pinboard.ts`.

---

## 4. How scanners parse events

The reference parser is `try_parse_note_log` in
`crates/slnt-sdk/src/pinboard.rs` (consumed by `indexer-service.md` and by
self-scanning wallets). Given one log line, it:

1. Strips the `"Program data: "` prefix; if absent, returns `Ok(None)` (this is
   a `Program log:` line or similar — not an event).
2. Base64-decodes the remainder (STANDARD alphabet); decode failure → `Err`.
3. If the decoded buffer is `< 8` bytes, returns `Ok(None)`.
4. Compares bytes `[..8]` against `NOTE_EVENT_DISCRIMINATOR`; mismatch →
   `Ok(None)` (some other program's or event's data — silently skipped).
5. Borsh-deserializes bytes `[8..]` into `NoteEvent`; failure → `Err` (the line
   *looked* like a `Note` but the body was malformed).

This three-valued contract (`None` = not for us / skip, `Err` = corrupt Note,
`Ok(Some)` = a parsed note) lets a scanner stream every log line of a pinboard
transaction and cheaply filter to the notes it cares about. The returned
`NoteEvent` mirrors the on-chain struct field-for-field.

A scanner's full pipeline per sRFC-0042 §5.10: subscribe to pinboard program
logs (`logsSubscribe`) and/or backfill via `getSignaturesForAddress` +
`getTransaction`, run each `Program data:` line through `try_parse_note_log`,
then apply the §5.4 view-tag filter (see §6 below) to the parsed `(R, view_tag)`.

---

## 5. Cost analysis

Because pinboard is stateless, the cost model is just transaction fees — there is
**no rent**, since no account is ever created.

| Quantity | Value |
|----------|-------|
| Base transaction fee | ~5,000 lamports (5,000 × number of signatures; pinboard txs have one signature) |
| Rent | 0 — stateless; no account allocation |
| Single `post` total | ~5,000 lamports (~$0.001 at typical SOL prices) |

**`post_batch` amortization.** A batch is still **one transaction with one
signature**, so it pays the base ~5,000 lamports once regardless of entry count
(plus a modest compute-unit surcharge that grows with N). The marginal on-chain
cost of an additional note is therefore dominated by the few hundred extra
compute units to serialize and emit one more event, not by a fresh base fee. At a
full batch the per-note cost amortizes toward roughly **~100–200 lamports**.

**Batch-size bound.** `post_batch` does not cap its entry count in code (only
`EmptyBatch` is enforced). The practical ceiling is the **transaction
compute-unit budget**: each emitted event consumes CU for borsh serialization and
the log write, and the whole transaction must also fit Solana's ~1232-byte packet
limit for the instruction data. In practice this lands at **~50 entries per
transaction** (sRFC-0042 §5.5.1). Callers that need more split across multiple
transactions. `tests/pinboard.ts` exercises a 20-entry batch end-to-end.

---

## 6. Spam / DoS implications

Pinboard is permissionless: anyone can post anything. An attacker can flood it
with garbage notes to inflate the work each recipient does while scanning. The
**1-byte view tag** is the structural defense (sRFC-0042 §5.4, §8.4), inherited
from ERC-5564.

**Why the view tag defuses scan-cost spam.** During scanning, each candidate note
costs the recipient one X25519 ECDH (~30 µs) plus one SHA-256 to derive the
expected tag; **~255/256 notes are rejected by the 1-byte tag comparison before
any expensive scalar multiplication.** Only the surviving ~1/256 incur the
per-label scalar-mult cost. The scan loop is thus cheap enough to run on commodity
hardware even under heavy spam.

**Attacker cost vs. scanner cost:**

| Attacker volume | Attacker cost (single posts) | Per-recipient scan work |
|-----------------|------------------------------|-------------------------|
| 1 M spam notes | ~5,000 lamports each ≈ **$1,000** | ~30 CPU-seconds |
| 100 M spam notes | ≈ **$100,000** | ~50 CPU-minutes (~3,000 CPU-seconds) |

Batching lowers the attacker's *per-note* cost (toward ~100 lamports/note at full
batches) but the same ECDH-then-tag filter bounds the scanner's per-note work
either way. The asymmetry — real dollars to post vs. tens of microseconds to
reject — makes mass spam **annoying, not crippling**.

**Mandatory hardening (clients, not the program).** Scanners MUST discard all-zero
X25519 shared secrets before the view-tag comparison; otherwise a single
low-order `R` could be crafted to pass the tag filter for *every* recipient,
forcing everyone down the expensive post-filter path (sRFC-0042 §5.4). This is a
client invariant — the program cannot and does not enforce it, consistent with
its policy-free design.

**v1 scope.** v1 adds no on-chain anti-spam mitigation; the per-tx fee plus the
view-tag filter are deemed sufficient at projected attack scale. A future
`scheme_id` MAY attach an anti-spam stamp (e.g. a small fixed SOL burn per
announcement) without changing pinboard, since the program records scheme ids
without interpreting them.

---

## 7. Forward compatibility

The event's **fixed-header + variable-tail** layout is intentional. The first 47
bytes (`discriminator + scheme_id + ephemeral_pub + view_tag + metadata_len`) are
fixed-width and fixed-position; only `metadata` varies. This makes a note a clean
fixed-size leaf-plus-tail, which is **Merkle-tree-friendly**: a future migration
to **Light Protocol compressed accounts** (sRFC-0042 §11) can commit notes into a
compressed-account Merkle tree for cheaper long-term availability **without
changing the bytes scanners parse**. A scanner that reads `(scheme_id, R,
view_tag, metadata)` from the layout in §3 continues to work against the migrated
representation, because the canonical note encoding is preserved. The two
versioning axes (`version` for meta-address encoding, `scheme_id` for crypto
suite; sRFC-0042 §5.11) provide the complementary forward path for new schemes
that coexist on the same immutable deployment.

---

## 8. Error table

`PinboardError` (`programs/pinboard/src/lib.rs`):

| Variant | Message | Trigger |
|---------|---------|---------|
| `MetadataTooLong` | `metadata exceeds 64 bytes` | `post`: `metadata.len() > 64`. `post_batch`: any entry's `metadata.len() > 64`. |
| `EmptyBatch` | `batch must contain at least one entry` | `post_batch` called with an empty `entries` vector. |

Both abort the entire transaction (Anchor `require!`), so on either error **no
`Note` event is emitted** — including for any valid entries that preceded the
offending one in a batch.

---

## 9. Testing summary

On-chain tests live in `tests/pinboard.ts` and drive the deployed program through
the Anchor TypeScript client, decoding emitted events with `EventParser` /
`BorshCoder` and asserting on the camelCased `note` event. Coverage:

- **Single `post` emits exactly one `Note`**, with `schemeId`, `ephemeralPub`,
  `viewTag`, and `metadata` round-tripping byte-for-byte.
- **Metadata length boundary:** rejects 65-byte metadata (`metadata exceeds 64
  bytes`), accepts exactly 64 bytes, accepts empty metadata.
- **`post_batch` emits one event per entry**, in entry order, fields preserved.
- **Empty batch rejected** (`batch must contain at least one entry`).
- **Oversized entry in a batch rejected** (`metadata exceeds 64 bytes`).
- **All-or-nothing batch:** a batch whose second entry is oversized produces a
  failed tx and **zero** events — confirming validation aborts before/around the
  emits rather than partially committing.
- **Statelessness / replay:** the same note posted twice succeeds both times
  (distinct tx signatures, one event each) — pinboard holds no state; recipients
  deduplicate by `R` themselves.
- **`scheme_id` not validated:** `scheme_id = 0x0000` and `scheme_id = 0xFFFF`
  (experimental range) are both accepted and recorded verbatim.
- **Larger batch:** a 20-entry batch emits 20 events with correct per-entry
  `viewTag` and `metadata`.

The SDK-side unit tests in `crates/slnt-sdk/src/pinboard.rs` independently verify
that all three discriminators match the Anchor `SHA-256("global:…")` /
`SHA-256("event:…")` convention, that `build_post_batch_instruction` round-trips
its `Vec<NoteEntry>`, and that `try_parse_note_log` correctly parses a synthetic
`Program data:` line, ignores non-event lines, and ignores foreign
discriminators.

---

## See also

- `rust-sdk.md` — the `slnt-sdk` instruction builders and event parser.
- `announcer.md` — the relayer that batches and submits `post_batch`.
- `indexer-service.md` — log scanning and note indexing built on `try_parse_note_log`.
- sRFC-0042 (`docs/srfc/0001-slnt-silent-payments.md`) §5.5 — normative spec.
