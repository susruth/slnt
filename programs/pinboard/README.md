# pinboard

A minimal, permissionless on-chain primitive for publishing tagged notes on
Solana. Anyone can post; anyone can read; the program holds no state. The
[Slnt](../../docs/superpowers/specs/2026-05-20-umbra-solana-stealth-payments-v1-design.md)
stealth-payment protocol is the first consumer, but pinboard is intended as
a generic substrate that any protocol can adopt or fork.

## Why pinboard

Think of a public corkboard at the entrance of a building. Anyone can tack
up a note; anyone walking past can read every note. Only the people the
note is *for* recognize their own — because the writer left a tiny private
signal (a `view_tag`) that means something to them and nothing to anyone
else. The privacy doesn't come from hiding the notes; it comes from the
cryptography of recognition.

That is exactly the on-chain shape pinboard provides.

## Instruction interface

Two instructions, both permissionless. No accounts created, no rent paid,
no admin keys.

### `post`

| Arg | Type | Notes |
|---|---|---|
| `scheme_id` | `u16` | Cryptographic-suite identifier. The program does not validate this; consuming protocols define their own scheme registries. |
| `ephemeral_pub` | `[u8; 32]` | The sender's ephemeral public key for the protocol (e.g., X25519 R for Slnt v1). |
| `view_tag` | `u8` | Short recognition hint, recipient-derived. |
| `metadata` | `Vec<u8>` | Opaque bytes, max 64. Consuming protocols define semantics; the program treats it as a blob. |

Validation:
- `metadata.len() <= 64` — exceeds returns `MetadataTooLong`.

### `post_batch`

| Arg | Type | Notes |
|---|---|---|
| `entries` | `Vec<NoteEntry>` | One or more notes posted in a single transaction. |

Validation:
- `entries.len() >= 1` — empty returns `EmptyBatch`.
- Each `entries[i].metadata.len() <= 64` — exceeds returns `MetadataTooLong`.

Practical batch size: ~50 entries per transaction, bounded by the Solana
compute-unit budget (200k CU default, ~3k CU per entry). Tested up to 20
in CI; production batchers should benchmark on target validators.

### Event

```rust
#[event]
pub struct Note {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}
```

Borsh-serialized into the transaction log. Wire layout:

```
8 bytes  discriminator (Anchor-generated)
2 bytes  scheme_id (LE)
32 bytes ephemeral_pub
1 byte   view_tag
4 bytes  metadata length (u32 LE; borsh Vec<u8> length prefix)
N bytes  metadata (N = metadata length, 0..=64)
```

Total event size: 47–111 bytes.

Anchor 0.31's IDL spec may camelCase the event name for high-level decoders.
Low-level clients parsing logs should match the 8-byte `event:Note`
discriminator, not an event-name string.

## Adopting pinboard for your protocol

Pick a `scheme_id` (the program does not validate this; pick whatever your
protocol publishes). Define what `ephemeral_pub`, `view_tag`, and
`metadata` mean in your scheme. Document your scheme so other clients can
recognize and ignore it. The pinboard contract itself is intentionally
generic and takes no opinion on cryptography.

A registry of known scheme IDs (community-maintained) is the eventual
coordination point; until then, treat the high half of the u16 range
(`0xFF00`–`0xFFFF`) as experimental.

## Build

Use the workspace build script (from the repo root):

```bash
./scripts/build.sh
```

It runs three steps:

1. `anchor build --no-idl -- --tools-version v1.54` — compile the workspace programs
2. `anchor idl build -p <program> -o target/idl/<program>.json`
3. `anchor idl type target/idl/<program>.json -o target/types/<program>.ts`

The IDL/type steps run for both `pinboard` and `registry`.

The `--tools-version v1.54` override is required because Solana CLI 2.3.0
ships with platform-tools v1.48 (cargo 1.84.0), which predates Rust
edition2024. Transitive dependencies of `solana-program 2.3.0` and
`anchor-lang 0.31.1` use edition2024 and fail to parse on the older cargo.
Platform-tools v1.54 ships cargo 1.89, which supports edition2024.

Outputs:
- `target/deploy/pinboard.so` — the program binary
- `target/deploy/registry.so` — the sibling registry program binary
- `target/idl/{pinboard,registry}.json` — IDLs consumed by clients
- `target/types/{pinboard,registry}.ts` — TypeScript types for the IDLs

## Test

```bash
./scripts/build.sh
anchor test --skip-build
```

Anchor will spin up a local validator, deploy the program, and run the
TypeScript test suite. Twelve tests cover the spec's requirements for §6
(pinboard program).

## Deploy

Devnet and testnet deployments are currently **upgradeable** while SLNT v1 is
draft and unaudited. As soon as v1 is finalized and independently audited, the
canonical v1 deployment is intended to have **no upgrade authority**, so no
party can alter the on-chain primitive after that point. See the repo-level
[`docs/DEPLOYMENT.md`](../../docs/DEPLOYMENT.md) for the current deployments,
authorities, and signatures.

```bash
# Deploy to devnet (preserves upgrade authority — for testing only)
anchor deploy --provider.cluster devnet

# Deploy canonical v1 and disable upgrades after v1 finalization + audit
anchor deploy --provider.cluster mainnet
solana program set-upgrade-authority <PROGRAM_ID> --final --url mainnet-beta
```

## Program ID

| Network | Program ID | Upgrade authority |
|---|---|
| Localnet (dev) | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` (keypair at `target/deploy/pinboard-keypair.json`) | local deployer |
| Devnet | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` | `78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR` |
| Testnet | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` | `78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR` |
| Mainnet | TBD — final canonical deployment | none, after v1 finalization + audit |
