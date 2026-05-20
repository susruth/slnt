# umbra-announcer

The on-chain announcer program for the Umbra stealth-payments protocol on
Solana. See the v1 design spec at
`docs/superpowers/specs/2026-05-20-umbra-solana-stealth-payments-v1-design.md`
for full protocol details.

## Purpose

The announcer program is permissionless and stateless. It exposes two
instructions:

- `announce(scheme_id, ephemeral_pub, view_tag, metadata)` — publishes a
  single stealth-payment announcement as an Anchor event.
- `announce_batch(entries)` — publishes N announcements in one transaction,
  used by announcement services to amortize the base tx fee.

Both instructions emit `UmbraAnnouncement` events. The program holds no
state; the same announcement can be published multiple times without
conflict (recipients deduplicate by R themselves).

## Instruction interface

### `announce`

| Arg | Type | Notes |
|---|---|---|
| `scheme_id` | `u16` | Cryptographic suite identifier. `0x0001` for v1. Not validated by the program. |
| `ephemeral_pub` | `[u8; 32]` | Sender's ephemeral X25519 public key (R in the spec). |
| `view_tag` | `u8` | First byte of `H(S)` where S is the ECDH shared secret. |
| `metadata` | `Vec<u8>` | Opaque bytes, max 64. Recipient-only; encryption (if any) is out of scope for v1. |

Validation:
- `metadata.len() <= 64` — exceeds returns `MetadataTooLong`.

### `announce_batch`

| Arg | Type | Notes |
|---|---|---|
| `entries` | `Vec<AnnouncementEntry>` | One or more announcement tuples. |

Validation:
- `entries.len() >= 1` — empty returns `EmptyBatch`.
- Each `entries[i].metadata.len() <= 64` — exceeds returns `MetadataTooLong`.

Practical batch size limit: ~50 entries per transaction, bounded by the
Solana compute-unit budget (200k CU default, ~3k CU per entry). Tested up
to 20 in CI; production services should benchmark on target validators.

### Event

```rust
#[event]
pub struct UmbraAnnouncement {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}
```

Borsh-serialized into the transaction log. The on-the-wire layout is:

```
8 bytes  discriminator (Anchor-generated)
2 bytes  scheme_id (LE)
32 bytes ephemeral_pub
1 byte   view_tag
4 bytes  metadata length (u32 LE; borsh Vec<u8> length prefix)
N bytes  metadata (N = metadata length, 0..=64)
```

Total event size: 47-111 bytes.

Note: the IDL emitted by Anchor 0.31 (new IDL spec) names the event
`umbraAnnouncement` (camelCase). Clients parsing events should match on
that name string.

## Build

Use the workspace build script (from the repo root):

```bash
./scripts/build.sh
```

It runs three steps:

1. `anchor build --no-idl -- --tools-version v1.54` — compile the .so
2. `anchor idl build -p umbra_announcer -o target/idl/umbra_announcer.json`
3. `anchor idl type target/idl/umbra_announcer.json -o target/types/umbra_announcer.ts`

The `--tools-version v1.54` override is required because Solana CLI 2.3.0
ships with platform-tools v1.48 (cargo 1.84.0), which predates Rust
edition2024. Transitive dependencies of `solana-program 2.3.0` and
`anchor-lang 0.31.1` use edition2024 and fail to parse on the older
cargo. Platform-tools v1.54 ships cargo 1.89, which supports edition2024.

Outputs:
- `target/deploy/umbra_announcer.so` — the program binary
- `target/idl/umbra_announcer.json` — the IDL consumed by clients
- `target/types/umbra_announcer.ts` — TypeScript types for the IDL

## Test

```bash
./scripts/build.sh
anchor test --skip-build
```

Anchor will spin up a local validator, deploy the program, and run the
TypeScript test suite. Twelve tests cover the spec's requirements for §6
(announcer program).

## Deploy

The program is intended to be deployed with **no upgrade authority** so
that no party can alter the on-chain protocol. Once deployed:

```bash
# Deploy to devnet (preserves upgrade authority — for testing only)
anchor deploy --provider.cluster devnet

# Deploy to mainnet and immediately disable upgrades
anchor deploy --provider.cluster mainnet
solana program set-upgrade-authority \
  <PROGRAM_ID> \
  --new-upgrade-authority none \
  --skip-new-upgrade-authority-signer-check
```

The canonical mainnet program ID will be published in this README once a
final deployment is made.

## Program ID

| Network | Program ID |
|---|---|
| Localnet (dev) | `G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P` (keypair at `target/deploy/umbra_announcer-keypair.json`) |
| Devnet | TBD — populated on first devnet deployment |
| Mainnet | TBD — populated on first mainnet deployment |
