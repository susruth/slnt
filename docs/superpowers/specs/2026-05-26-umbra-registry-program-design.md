# Umbra Registry Program — Design Spec

**Date:** 2026-05-26
**Status:** Draft for review
**Scope:** A second on-chain program (`umbra_registry`) that maps Solana wallet pubkeys to Umbra meta-addresses, closing the discovery gap identified against ERC-6538.

---

## 1. Motivation

The v1 stealth-payments spec (`2026-05-20-umbra-solana-stealth-payments-v1-design.md`) intentionally keeps meta-addresses off-chain: users share them via QR codes, profiles, or DMs. This works but lacks an ERC-6538 analog — a sender who only knows the recipient's main Solana wallet pubkey has no on-chain path to discover the recipient's meta-address.

`umbra_registry` adds that path as a separate, optional primitive. It is not required by the protocol: senders without registry knowledge continue using off-chain meta-address sharing exactly as before. With the registry, the flow becomes:

1. Sender knows recipient's main Solana pubkey `A`.
2. Sender derives the registry PDA from `A` and `scheme_id = 0x0001`.
3. Sender reads the account; on hit, decodes the meta-address and proceeds with the normal sender flow (stealth-payments spec §4).

The registry is deployed independently from `pinboard` and holds different state. Pinboard publishes ephemeral announcements; the registry publishes long-lived key material. They share no code.

---

## 2. Privacy and trade-offs

Registering publicly reveals that a given main wallet has Umbra stealth capability. This is a deliberate trade-off — discovery UX in exchange for one bit of metadata. The protocol does not force registration; users who want to stay invisible continue using off-chain sharing.

The registry only accepts the *unlabeled* meta-address (`label_index = 0`). Labelled meta-addresses (spec §3.3) are meant for per-counterparty sharing and would leak relationship information if published publicly. The label policy is enforced on-chain (§5).

The registry stores `B_spend` and `B_scan` in the clear — exactly the same data the user already shares via QR code or profile. No new key material is exposed.

---

## 3. Program shape

- **Crate path:** `programs/registry`
- **Anchor program name:** `umbra_registry`
- **Deployment:** permissionless, immutable (no upgrade authority), no global program state, no admin.
- **State:** one PDA per `(registrant, scheme_id)` pair.

The program holds no singleton accounts. There is no initialization step.

### 3.1 PDA derivation

```
seeds = [
  b"meta",
  registrant.key().as_ref(),   // 32 bytes
  &scheme_id.to_le_bytes(),    // 2 bytes
]
```

Senders derive the PDA deterministically from the recipient's main wallet pubkey and the scheme id they want to look up. A single RPC `getAccountInfo` retrieves the entry without scanning.

### 3.2 Account layout

```rust
#[account]
pub struct MetaAddressEntry {
    pub registrant: Pubkey,     // 32
    pub scheme_id: u16,         // 2
    pub bump: u8,               // 1
    pub version: u8,            // 1 — meta-address encoding version (spec §3.2)
    pub b_spend: [u8; 32],      // 32 — Ed25519 spend pubkey
    pub b_scan: [u8; 32],       // 32 — X25519 scan pubkey
    pub flags: u8,              // 1 — reserved, 0x00 in v1
}
```

Payload: 101 bytes. With the 8-byte Anchor discriminator: 109 bytes. Fixed size — no realloc on update. Rent-exempt minimum: approximately 1,530,000 lamports.

**Fields intentionally omitted:**

- `label_index` — enforced to `0`, no need to store.
- `created_slot` / `updated_slot` — derivable from transaction history if needed.
- bech32m string — derivable client-side from `version`, `b_spend`, `b_scan`, `label_index = 0`, `flags`.
- Display name, memo, profile data — out of scope; this is a key-discovery primitive, not a profile system.

---

## 4. Instructions

Four instructions, all registrant-signed. No admin instructions.

### 4.1 `register`

```rust
pub fn register(
    ctx: Context<Register>,
    scheme_id: u16,
    payload: MetaAddressPayload,
) -> Result<()>;
```

Creates the PDA at `(registrant, scheme_id)`. The registrant signs and funds rent. Fails if the PDA already exists — use `update` instead.

```rust
#[derive(Accounts)]
#[instruction(scheme_id: u16)]
pub struct Register<'info> {
    #[account(mut)]
    pub registrant: Signer<'info>,

    #[account(
        init,
        payer = registrant,
        space = 8 + MetaAddressEntry::SIZE,
        seeds = [b"meta", registrant.key().as_ref(), &scheme_id.to_le_bytes()],
        bump,
    )]
    pub entry: Account<'info, MetaAddressEntry>,

    pub system_program: Program<'info, System>,
}
```

### 4.2 `update`

```rust
pub fn update(
    ctx: Context<Update>,
    scheme_id: u16,
    payload: MetaAddressPayload,
) -> Result<()>;
```

Overwrites the existing entry in-place. Fails if the PDA does not exist. The constraint `has_one = registrant` ensures only the original registrant can update.

### 4.3 `close`

```rust
pub fn close(
    ctx: Context<Close>,
    scheme_id: u16,
) -> Result<()>;
```

Closes the PDA via Anchor's `close = registrant` attribute. Rent returns to the registrant. After close, the same `(registrant, scheme_id)` pair can be re-registered later.

### 4.4 Instruction argument type

```rust
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct MetaAddressPayload {
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub flags: u8,
    // label_index implicitly 0; not transmitted on the wire.
}
```

---

## 5. Validation rules

Applied identically to `register` and `update`:

| Rule | Error |
|---|---|
| `scheme_id != 0` | `InvalidSchemeId` |
| `payload.version == 0x01` | `InvalidVersion` |
| `payload.flags == 0` | `InvalidFlags` |

The `version` check accepts only `0x01` in this program. A future v2 encoding would have a different account layout and must be deployed as a separate program (§9). Allowing other canonical versions through this program would store them in a v1-shaped account, silently breaking consumers.

The registry does **not** validate that `b_spend` is a valid Ed25519 point or that `b_scan` is a valid X25519 point. Curve-point validation is expensive on-chain and pinboard does not perform analogous validation. A garbage payload causes senders to fail at derivation time; it cannot cause loss of funds at the registry layer.

`label_index` is not on the wire — the payload type has no field for it, so there is nothing to validate.

```rust
#[error_code]
pub enum RegistryError {
    InvalidSchemeId,        // scheme_id == 0
    InvalidVersion,         // version != 0x01
    InvalidFlags,           // flags != 0 in v1
}
```

---

## 6. Events

Three Anchor events. Indexers tail these to maintain a `(registrant, scheme_id) → current_entry` map without `getProgramAccounts` scans.

```rust
#[event]
pub struct MetaAddressRegistered {
    pub registrant: Pubkey,
    pub scheme_id: u16,
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub flags: u8,
}

#[event]
pub struct MetaAddressUpdated {
    pub registrant: Pubkey,
    pub scheme_id: u16,
    pub version: u8,
    pub b_spend: [u8; 32],
    pub b_scan: [u8; 32],
    pub flags: u8,
}

#[event]
pub struct MetaAddressClosed {
    pub registrant: Pubkey,
    pub scheme_id: u16,
}
```

`MetaAddressClosed` carries no payload — consumers delete the entry from their map.

---

## 7. SDK additions

Two helpers in `crates/umbra-sdk`:

```rust
/// Canonical registry PDA for a (registrant, scheme_id) pair.
pub fn registry_pda(
    program_id: &Pubkey,
    registrant: &Pubkey,
    scheme_id: u16,
) -> (Pubkey, u8);

/// Fetch and decode a registered meta-address. Returns Ok(None) if the PDA
/// does not exist. Returns an error on RPC failure or malformed account data.
pub async fn fetch_meta_address(
    rpc: &RpcClient,
    program_id: &Pubkey,
    registrant: &Pubkey,
    scheme_id: u16,
) -> Result<Option<MetaAddress>>;
```

The sender flow gains an optional prelude: given Alice's main wallet pubkey `A`, call `fetch_meta_address(A, 1)`. On `Some(meta)`, proceed with the existing sender flow (stealth-payments spec §4). On `None`, fall back to asking the user for a bech32m meta-address directly. The registry remains an enhancement, never a hard dependency.

Register/update/close transaction builders are out of scope for v1 SDK work and can be added when wallet integrations need them; CLI usage is sufficient at this stage.

---

## 8. Testing

Anchor mocha tests in `tests/registry.ts`:

1. `register` happy path: PDA exists, all fields match, `MetaAddressRegistered` event emitted.
2. `register` twice on the same `(registrant, scheme_id)`: fails with Anchor `init` collision.
3. `register` with `scheme_id = 0`: fails with `InvalidSchemeId`.
4. `register` with `version = 0` or `version = 0x02`: fails with `InvalidVersion`.
5. `register` with `flags != 0`: fails with `InvalidFlags`.
6. `update` happy path: fields change, account size unchanged, `MetaAddressUpdated` event emitted.
7. `update` by a non-registrant signer: fails on Anchor constraint.
8. `update` on a non-existent PDA: fails (account not initialized).
9. `close` happy path: account closed, rent returned to registrant, `MetaAddressClosed` event emitted.
10. `close` followed by `register` with the same key: succeeds.
11. Two different `scheme_id` values for the same registrant: both PDAs coexist independently.

No fuzzing needed — inputs are tightly typed and validations are equality checks.

---

## 9. Forward compatibility

**Adding a new cryptographic scheme.** A v2 scheme uses a new `scheme_id` (e.g., `0x0002` for a post-quantum suite). It gets its own PDA per registrant and coexists with the v1 entry. Senders try the highest `scheme_id` they support first and fall back. No registry program change required.

**Changing the meta-address encoding.** A v2 encoding (different payload shape — e.g., multi-curve keys for cross-chain meta-addresses) would not fit the fixed `MetaAddressEntry` layout. Because the registry program is deployed immutable, this requires a new program ID with a new layout. Clients try v2 first, fall back to v1. This is acceptable because the registry is meant to be long-lived and immutable; coordinated migrations to a new deployment are the intended mechanism for breaking layout changes.

**Adding `register_on_behalf`.** Future versions may add a relayer-signed register path (analog to ERC-6538's `registerKeysOnBehalf`) for gasless onboarding. This adds nonce state per registrant and Ed25519-sysvar signature verification. Not blocking v1. The current instruction set is forward-compatible: a new instruction can be added without touching existing ones.

---

## 10. Out of scope (v1)

- `register_on_behalf` / gasless registration.
- Payment-for-registration economics.
- Cross-chain meta-addresses (would need a new encoding version, see §9).
- Profile data (display name, avatar URI, social links).
- Discovery indexes by anything other than `(registrant, scheme_id)` (e.g., reverse lookups, SNS integration).
- On-chain curve-point validation.

---

## 11. Open questions

1. **Program deploy address.** Same as pinboard — probably immutable, vanity prefix for visibility. Coordinated with the pinboard deploy.
2. **Whether SDKs should warn before registration.** The privacy trade-off (publishing that a wallet has Umbra capability) is mild but worth surfacing once on first use.
