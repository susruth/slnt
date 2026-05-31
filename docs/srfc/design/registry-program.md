# SLNT Registry Program — Service-Level Design

| | |
|---|---|
| **Component** | `registry` — on-chain meta-address registry (the Solana analog of ERC-6538) |
| **Status** | Implemented; OPTIONAL component of SLNT |
| **Spec** | sRFC-0042 §5.6 (normative) |
| **Program id** | `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2` |
| **Immutability** | No upgrade authority, no admin, no privileged instruction. Vanity prefix `SLNTR…` (**R** = Registry) marks the canonical deployment. |

This document is the byte-level design reference for the `registry` program. It is a sibling of `pinboard-program.md` (the announcement layer) and `rust-sdk.md` (the client SDK). The registry is **OPTIONAL**: senders may always exchange meta-addresses off-chain (QR, profile, DM). It exists purely to close the *discovery gap*.

---

## 1. Motivation

A SLNT sender needs the recipient's **meta-address** — the pair `(B_spend, B_scan)` — before they can derive a stealth address (sRFC-0042 §5.3). Without an on-chain directory, the only way a sender obtains that meta-address is out of band: scanning a QR code, reading a profile page, or receiving a DM. That out-of-band step is the discovery gap.

The registry closes it by mapping a recipient's **main Solana wallet pubkey** to their published meta-address. A sender who knows only a counterparty's ordinary wallet address can look up `(wallet, 0x0001)` and obtain the meta-address with a single account read — no scanning, no off-band channel.

### Relationship to `pinboard`

The registry and `pinboard` (see `pinboard-program.md`) are deliberately **separate, independently deployed programs that share no code and no state**:

- **`pinboard`** emits *ephemeral* announcements — one event per payment, opaque and write-once, never read back on-chain. It is stateless.
- **`registry`** stores *long-lived key material* — one durable PDA per registrant that senders read before constructing a payment. It is stateful.

The two have opposite lifetimes (per-transaction vs. per-identity) and opposite access patterns (emit-only vs. read-mostly), so coupling them would only create shared blast radius. Keeping them separate means a vulnerability or redeploy in one cannot affect the other, and a deployment that uses the registry but not pinboard (or vice versa) is fully supported.

---

## 2. PDA derivation

There is **exactly one PDA per `(registrant, scheme_id)` pair**. The seeds are:

```
seeds = [ b"meta", registrant.key().as_ref(), &scheme_id.to_le_bytes() ]
```

| Seed | Bytes | Value |
|---|---|---|
| literal prefix | 4 | `b"meta"` |
| registrant pubkey | 32 | the main wallet pubkey |
| `scheme_id` | 2 | `u16`, **little-endian** |

Because the wallet pubkey and `scheme_id` are both seeds, the sender can derive the PDA address deterministically from public inputs and fetch it with a **single `getAccountInfo`** — there are no `getProgramAccounts` scans and no secondary index. `scheme_id` is part of the seed, so distinct schemes for the same registrant land at distinct, independent PDAs (verified by `pda_differs_by_scheme_id` / `pda_differs_by_registrant` in `rust-sdk.md`'s test suite).

SDK derivation (`crates/slnt-sdk/src/registry.rs`):

```rust
pub fn registry_pda(program_id: &Pubkey, registrant: &Pubkey, scheme_id: u16) -> (Pubkey, u8)
```

---

## 3. Account layout — `MetaAddressEntry`

The account is **fixed-size**: a 101-byte payload plus the 8-byte Anchor discriminator = **109 bytes total**. There is no `realloc` and no variable-length field; the on-chain `space` is `8 + MetaAddressEntry::SIZE`.

| Offset | Field | Size | Type | Notes |
|---:|---|---:|---|---|
| 0 | discriminator | 8 | `[u8; 8]` | `[165, 7, 241, 154, 7, 172, 74, 178]` |
| 8 | `registrant` | 32 | `Pubkey` | main wallet pubkey (also a PDA seed) |
| 40 | `scheme_id` | 2 | `u16` (LE) | meta-address scheme; `0x0001` = SLNT v1 |
| 42 | `bump` | 1 | `u8` | canonical PDA bump |
| 43 | `version` | 1 | `u8` | meta-address encoding version; pinned `0x01` |
| 44 | `b_spend` | 32 | `[u8; 32]` | Ed25519 spend pubkey |
| 76 | `b_scan` | 32 | `[u8; 32]` | X25519 scan pubkey |
| 108 | `flags` | 1 | `u8` | reserved, MUST be `0x00` in v1 |
| | **total** | **109** | | 101-byte payload + 8-byte discriminator |

### Account discriminator

The 8-byte leading discriminator is the standard Anchor account tag:

```
discriminator = SHA-256("account:MetaAddressEntry")[..8]
              = [165, 7, 241, 154, 7, 172, 74, 178]
```

The SDK re-derives and asserts this in a unit test (`account_discriminator_matches_anchor_convention`), so the constant cannot silently drift from the program. `try_parse_meta_address_entry` rejects any account whose first 8 bytes differ.

---

## 4. Instructions

All three instructions are **registrant-signed**; there is no admin or upgrade path. Each instruction's on-the-wire data is:

```
data = discriminator(8) || borsh(scheme_id: u16) || borsh(MetaAddressPayload)?
```

`MetaAddressPayload` is **66 bytes**, borsh-serialized in field order:

| Offset | Field | Size | Type |
|---:|---|---:|---|
| 0 | `version` | 1 | `u8` |
| 1 | `b_spend` | 32 | `[u8; 32]` |
| 33 | `b_scan` | 32 | `[u8; 32]` |
| 65 | `flags` | 1 | `u8` |
| | **total** | **66** | |

Note that `bump`, `scheme_id`, and `registrant` are **not** in the payload: `bump` is set on-chain from `ctx.bumps`, `scheme_id` is a separate borsh arg (and a PDA seed), and `registrant` is taken from the signer account. The wire payload carries only the user-supplied key material.

Instruction discriminators (`SHA-256("global:<name>")[..8]`, asserted by `instruction_discriminators_match_anchor_convention`):

| Instruction | Discriminator |
|---|---|
| `register` | `[211, 124, 67, 15, 211, 194, 178, 240]` |
| `update` | `[219, 200, 88, 176, 158, 63, 253, 127]` |
| `close` | `[98, 165, 201, 177, 108, 65, 206, 96]` |

### 4.1 `register(scheme_id, payload)`

Creates the PDA. The registrant pays rent and signs; the instruction fails on-chain if the `(registrant, scheme_id)` entry already exists (Anchor `init`).

Accounts context (`Register<'info>`):

| Account | Attributes | Role |
|---|---|---|
| `registrant` | `Signer`, `mut` | pays rent, must sign; writable because lamports are debited |
| `entry` | `init`, `payer = registrant`, `space = 8 + SIZE`, PDA seeds, `bump` | the new `MetaAddressEntry` |
| `system_program` | `Program<System>` | required by `init` to allocate the account |

Wire: `register_disc || borsh(scheme_id) || borsh(payload)` (8 + 2 + 66 = 76 bytes).

SDK: `build_register_instruction(program_id, registrant, scheme_id, payload)` produces accounts `[registrant (signer, writable), entry (writable), system_program (readonly)]`.

### 4.2 `update(scheme_id, payload)`

Overwrites `version`, `b_spend`, `b_scan`, `flags` in place. The size never changes (verified by the test that asserts `data.length` is unchanged after update). `has_one = registrant` ensures only the owning registrant can update; the instruction fails if the PDA does not exist.

Accounts context (`Update<'info>`):

| Account | Attributes | Role |
|---|---|---|
| `registrant` | `Signer` (**readonly**) | must sign; **not** marked `mut` — no lamports move |
| `entry` | `mut`, PDA seeds, `bump = entry.bump`, `has_one = registrant` | the entry being overwritten |

There is **no `system_program`** account: nothing is allocated. The key per-instruction difference from `register` is that the registrant is read-only here. The SDK encodes this faithfully (`build_update_omits_system_program_and_registrant_is_readonly`).

Wire: `update_disc || borsh(scheme_id) || borsh(payload)` (76 bytes).

SDK: `build_update_instruction(...)` → accounts `[registrant (signer, readonly), entry (writable)]`.

### 4.3 `close(scheme_id)`

Closes the PDA and returns the rent lamports to the registrant. The pair may be re-registered afterward (the `close then register` test confirms re-registration succeeds against a closed PDA).

Accounts context (`Close<'info>`):

| Account | Attributes | Role |
|---|---|---|
| `registrant` | `Signer`, `mut` | rent destination; writable because it receives the reclaimed lamports |
| `entry` | `mut`, PDA seeds, `bump = entry.bump`, `has_one = registrant`, `close = registrant` | the entry being closed; lamports flow to `registrant` |

Wire: `close_disc || borsh(scheme_id)` — **no payload** (8 + 2 = 10 bytes).

SDK: `build_close_instruction(...)` → accounts `[registrant (signer, writable), entry (writable)]`.

### Account-writability summary

| Instruction | `registrant` | `entry` | `system_program` |
|---|---|---|---|
| `register` | signer + **mut** | **init** (writable) | present (readonly) |
| `update` | signer, **readonly** | **mut** | absent |
| `close` | signer + **mut** | **mut** + **close** | absent |

---

## 5. Validation

`require!` checks run at the top of `register` and `update`, before any account mutation:

| Rule | Error |
|---|---|
| `scheme_id != 0` | `InvalidSchemeId` ("scheme_id must be non-zero") |
| `payload.version == 0x01` | `InvalidVersion` ("only meta-address version 0x01 is supported by this program") |
| `payload.flags == 0x00` | `InvalidFlags` ("flags must be 0x00 in v1") |

### Why `version` is pinned to `0x01`

This program is **immutable** — there is no upgrade authority that could later teach it to interpret a `version` byte it does not understand. Accepting any version it cannot validate would let a registrant store an entry that this code can never round-trip correctly. Pinning `version == 0x01` keeps the on-chain encoding and this binary in lock-step for the life of the deployment. A future meta-address encoding does not patch this program; it ships as a **new immutable deployment** (see §8).

### Why curve-point validation is intentionally omitted

The program does **not** check that `b_spend` / `b_scan` are valid Ed25519 / X25519 curve points. That validation is the **sender's** responsibility (sRFC-0042 §5.3): the sender validates the meta-address before building any transfer, so garbage keys fail *before* funds move. Pushing curve checks on-chain would burn compute on every write for a guarantee the sender must re-establish anyway, and would not protect anyone — an attacker can only ever corrupt their *own* entry. The registry therefore stores the 64 bytes opaquely.

### Why `label_index` is not on the wire

SLNT supports *labeled* meta-addresses off-chain, but the registry **MUST accept only unlabeled meta-addresses**. `MetaAddressPayload` has no `label_index` field — labeled entries are impossible *by construction*. This is a privacy decision: a label distinguishes which counterparty relationship a meta-address belongs to, so publishing a labeled meta-address in a public on-chain directory would leak relationship metadata. By omitting the field entirely, the registry can only ever hold the single, unlabeled, public meta-address.

---

## 6. Events

Every state-changing instruction emits an Anchor event:

| Event | Fields |
|---|---|
| `MetaAddressRegistered` | `registrant: Pubkey`, `scheme_id: u16`, `version: u8`, `b_spend: [u8;32]`, `b_scan: [u8;32]`, `flags: u8` |
| `MetaAddressUpdated` | same fields as `MetaAddressRegistered` |
| `MetaAddressClosed` | `registrant: Pubkey`, `scheme_id: u16` |

`Registered` and `Updated` carry the **full new entry**, so an indexer never needs to read the account back. `Closed` carries only the key `(registrant, scheme_id)` to tombstone the entry.

**Indexer model:** an indexer tails program logs and folds these events into a `(registrant, scheme_id) → MetaAddressEntry` map — register/update set, close deletes — **without ever calling `getProgramAccounts`**. This keeps indexers cheap and avoids the heavy full-program scan that `getProgramAccounts` requires. The on-chain test suite parses these events from transaction logs with Anchor's `EventParser` and asserts the emitted `name` and fields for each instruction.

---

## 7. SDK surface

From `crates/slnt-sdk/src/registry.rs` (see `rust-sdk.md` for the crate overview):

| Item | Purpose |
|---|---|
| `registry_pda(program_id, registrant, scheme_id) -> (Pubkey, u8)` | Derive the PDA; matches the on-chain seeds exactly. |
| `try_parse_meta_address_entry(data) -> Result<Option<MetaAddressEntry>, String>` | Validate the 8-byte discriminator and borsh-decode the body. `Ok(None)` on length < 8 or discriminator mismatch; `Err` on a matching discriminator with a malformed body. |
| `fetch_meta_address(rpc, program_id, registrant, scheme_id)` *(behind the `rpc` feature)* | Async: derive PDA, `get_account`, parse. `Ok(None)` when the account does not exist (`AccountNotFound`); `Err` on real RPC failure or malformed data. |
| `build_register_instruction(...)` | Build the `register` instruction (3 accounts, 76-byte data). |
| `build_update_instruction(...)` | Build the `update` instruction (2 accounts, registrant read-only). |
| `build_close_instruction(...)` | Build the `close` instruction (2 accounts, no payload). |

Exported constants mirror the on-chain program and are unit-asserted against the Anchor conventions: `META_SEED`, the three instruction discriminators, and `META_ADDRESS_ENTRY_DISCRIMINATOR`. `MetaAddressPayload` and `MetaAddressEntry` are re-declared with matching borsh layouts.

### Optional sender prelude

The registry is **an enhancement, never a hard dependency** (sRFC-0042 §5.6.2). The OPTIONAL sender prelude is: given a recipient wallet `A`, call `fetch_meta_address(rpc, registry_id, A, 0x0001)`; on a hit, decode the meta-address and proceed with stealth-address derivation (§5.3); on a miss (`Ok(None)`), fall back to off-chain meta-address entry. A wallet that never touches the registry remains fully conformant.

---

## 8. Cost / rent analysis

- **Account size:** 109 bytes (8 discriminator + 101 payload).
- **Rent-exempt reserve:** ≈ **1.53M lamports**, deposited at `register` time.
- **Who pays:** the **registrant** (`payer = registrant`). The registrant signs and funds their own entry; no relayer or third party is required.
- **`update`:** moves **no lamports** — the account size is fixed, so no top-up or refund occurs (registrant is read-only).
- **`close`:** returns the full rent reserve to the registrant (`close = registrant`). The on-chain test confirms the registrant's balance rises by ~rent minus the transaction fee, and that the account then reads back as `null`.

Because rent is fully reclaimable on close, the steady-state cost of maintaining a published meta-address is just the locked ≈1.53M-lamport reserve, recoverable at will.

---

## 9. Forward compatibility

- **`register_on_behalf` reserved.** A delegated-registration path (a third party paying rent / submitting on behalf of a registrant) is reserved for a future version. It is intentionally **not** in this immutable deployment; the current model is strictly registrant-self-service.
- **New `scheme_id` coexists.** Because `scheme_id` is a PDA seed, a new scheme simply occupies a new, independent PDA. Existing `0x0001` entries are untouched, and a single registrant can hold multiple schemes simultaneously (the `different scheme_ids … coexist` test exercises this).
- **New encoding ⇒ new deployment.** Because `version` is pinned to `0x01` and there is no upgrade authority, a new meta-address encoding cannot be retrofitted into this program. It ships as a **new immutable deployment** with its own program id, run side-by-side. This is the deliberate cost of immutability: no admin can change the rules out from under registrants.

---

## 10. Testing summary

From the on-chain suite (`tests/registry.ts`) and the SDK unit tests (`crates/slnt-sdk/src/registry.rs`):

**`register`**
- Initializes the PDA, persists all payload fields, and emits `MetaAddressRegistered`.
- Re-registering the same `(registrant, scheme_id)` fails (Anchor `init` collision).
- Different `scheme_id`s for the same registrant coexist at independent PDAs.
- `scheme_id == 0` → `InvalidSchemeId`; `version != 1` (e.g. 2 or 0) → `InvalidVersion`; `flags != 0` → `InvalidFlags`.

**`update`**
- Changes fields in place, leaves the account size unchanged, and emits `MetaAddressUpdated`.
- Updating a non-existent PDA fails.
- A non-registrant signer fails — exercised via `accountsPartial` to force the attacker's signer against the victim's PDA, isolating the `has_one` boundary from the "PDA missing" case.
- Validation (`version = 2`) fires on `update` as well as `register`.

**`close`**
- Closes the PDA, returns rent to the registrant (balance up by ~rent − fee), and emits `MetaAddressClosed`.
- After close, the PDA can be re-registered.
- A non-registrant signer cannot close (same `accountsPartial` boundary test).

**SDK unit tests**
- The account and instruction discriminators re-derive from the Anchor `SHA-256(...)` conventions.
- PDA derivation is deterministic and varies by both `scheme_id` and `registrant`.
- Instruction builders produce the correct account metas (signer/writable flags) and exact wire bytes.
- `try_parse_meta_address_entry` round-trips a full entry and returns `None` for short or wrong-discriminator input.
