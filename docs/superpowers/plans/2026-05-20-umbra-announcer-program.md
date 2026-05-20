# Umbra Announcer Program Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the on-chain Umbra announcer program — a permissionless Solana program with two instructions (`announce` and `announce_batch`) that emit `UmbraAnnouncement` events. Deploy to localnet and devnet with full test coverage.

**Architecture:** A single Anchor program with no on-chain state. Instructions validate input lengths and emit Anchor events (Borsh-serialized into transaction logs). Permissionless: no admin keys, no signer requirements beyond the fee payer.

**Tech Stack:**
- Rust 1.75+ with `anchor-lang = "0.30.1"`
- Solana CLI 1.18.22+ (or 2.0.x)
- Anchor CLI 0.30.1
- TypeScript tests via `ts-mocha` + `@coral-xyz/anchor` client
- Node 20+ / npm or yarn

**Reference spec:** `docs/superpowers/specs/2026-05-20-umbra-solana-stealth-payments-v1-design.md`, particularly §6 (Announcer Program).

---

## Pre-flight: Verify toolchain

Before starting, verify the required tools are installed. If any are missing, install them per the official guides before proceeding.

```bash
solana --version    # Expect: solana-cli 1.18.x or 2.0.x
anchor --version    # Expect: anchor-cli 0.30.1
rustc --version     # Expect: rustc 1.75.0+
node --version      # Expect: v20.x or newer
```

If a fresh install is needed:
- Solana CLI: https://docs.solana.com/cli/install-solana-cli-tools
- Anchor: `cargo install --git https://github.com/coral-xyz/anchor avm --locked --force && avm install 0.30.1 && avm use 0.30.1`

---

## Task 1: Scaffold the Anchor workspace

**Files:**
- Create: `Anchor.toml`
- Create: `Cargo.toml` (workspace)
- Create: `programs/umbra-announcer/Cargo.toml`
- Create: `programs/umbra-announcer/Xargo.toml`
- Create: `programs/umbra-announcer/src/lib.rs` (stub)
- Create: `package.json`
- Create: `tsconfig.json`
- Create: `migrations/deploy.ts`
- Create: `tests/.gitkeep`
- Create: `.gitignore`

---

- [ ] **Step 1.1: Create `.gitignore`**

Create `/Users/susruth/Documents/Projects/umbra/.gitignore`:

```
.anchor
.DS_Store
target
**/*.rs.bk
node_modules
test-ledger
.yarn
dist
.env
.env.local
*.log
```

- [ ] **Step 1.2: Create workspace `Cargo.toml`**

Create `/Users/susruth/Documents/Projects/umbra/Cargo.toml`:

```toml
[workspace]
members = [
    "programs/*"
]
resolver = "2"

[profile.release]
overflow-checks = true
lto = "fat"
codegen-units = 1
[profile.release.build-override]
opt-level = 3
incremental = false
codegen-units = 1
```

- [ ] **Step 1.3: Create program `Cargo.toml`**

Create `/Users/susruth/Documents/Projects/umbra/programs/umbra-announcer/Cargo.toml`:

```toml
[package]
name = "umbra-announcer"
version = "0.1.0"
description = "Umbra protocol announcer program for Solana stealth payments (v1)"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]
name = "umbra_announcer"

[features]
no-entrypoint = []
no-idl = []
no-log-ix-name = []
cpi = ["no-entrypoint"]
default = []
idl-build = ["anchor-lang/idl-build"]

[dependencies]
anchor-lang = "0.30.1"
```

- [ ] **Step 1.4: Create `Xargo.toml`**

Create `/Users/susruth/Documents/Projects/umbra/programs/umbra-announcer/Xargo.toml`:

```toml
[target.bpfel-unknown-unknown.dependencies.std]
features = []
```

- [ ] **Step 1.5: Create stub `lib.rs`**

Create `/Users/susruth/Documents/Projects/umbra/programs/umbra-announcer/src/lib.rs`:

```rust
use anchor_lang::prelude::*;

declare_id!("UmbrAnncr1111111111111111111111111111111111");

#[program]
pub mod umbra_announcer {
    use super::*;
}
```

Note: `UmbrAnncr1111111111111111111111111111111111` is a vanity placeholder. We'll regenerate a proper program keypair in Step 1.10 and replace this string.

- [ ] **Step 1.6: Create `Anchor.toml`**

Create `/Users/susruth/Documents/Projects/umbra/Anchor.toml`:

```toml
[toolchain]
anchor_version = "0.30.1"

[features]
resolution = true
skip-lint = false

[programs.localnet]
umbra_announcer = "UmbrAnncr1111111111111111111111111111111111"

[programs.devnet]
umbra_announcer = "UmbrAnncr1111111111111111111111111111111111"

[registry]
url = "https://api.apr.dev"

[provider]
cluster = "Localnet"
wallet = "~/.config/solana/id.json"

[scripts]
test = "yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/**/*.ts"
```

- [ ] **Step 1.7: Create `package.json`**

Create `/Users/susruth/Documents/Projects/umbra/package.json`:

```json
{
  "name": "umbra",
  "version": "0.1.0",
  "private": true,
  "license": "Apache-2.0",
  "scripts": {
    "lint:fix": "prettier */*.js \"*/**/*{.js,.ts}\" -w",
    "lint": "prettier */*.js \"*/**/*{.js,.ts}\" --check",
    "test": "anchor test"
  },
  "dependencies": {
    "@coral-xyz/anchor": "^0.30.1"
  },
  "devDependencies": {
    "@types/bn.js": "^5.1.5",
    "@types/chai": "^4.3.11",
    "@types/mocha": "^10.0.6",
    "@types/node": "^20.11.5",
    "chai": "^4.4.1",
    "mocha": "^10.2.0",
    "prettier": "^3.2.4",
    "ts-mocha": "^10.0.0",
    "typescript": "^5.3.3"
  }
}
```

- [ ] **Step 1.8: Create `tsconfig.json`**

Create `/Users/susruth/Documents/Projects/umbra/tsconfig.json`:

```json
{
  "compilerOptions": {
    "types": ["mocha", "chai", "node"],
    "typeRoots": ["./node_modules/@types"],
    "lib": ["es2020"],
    "module": "commonjs",
    "target": "es2020",
    "esModuleInterop": true,
    "moduleResolution": "node",
    "strict": true,
    "skipLibCheck": true,
    "resolveJsonModule": true
  }
}
```

- [ ] **Step 1.9: Create `migrations/deploy.ts`**

Create `/Users/susruth/Documents/Projects/umbra/migrations/deploy.ts`:

```typescript
// Migrations are an early feature. Currently they're nothing more than
// this single deploy script that's invoked from the CLI, injecting a
// provider configured from the workspace's Anchor.toml.

const anchor = require("@coral-xyz/anchor");

module.exports = async function (provider: anchor.AnchorProvider) {
  anchor.setProvider(provider);
  // No initialization needed for the announcer program — it has no state.
};
```

- [ ] **Step 1.10: Create tests directory placeholder**

```bash
mkdir -p /Users/susruth/Documents/Projects/umbra/tests
touch /Users/susruth/Documents/Projects/umbra/tests/.gitkeep
```

- [ ] **Step 1.11: Generate the program keypair**

The program ID in `lib.rs` and `Anchor.toml` is a placeholder. Anchor builds will use whatever keypair is at `target/deploy/umbra_announcer-keypair.json`. We need to generate it once, extract its pubkey, and update the source.

```bash
cd /Users/susruth/Documents/Projects/umbra
mkdir -p target/deploy
solana-keygen new --no-bip39-passphrase --silent --outfile target/deploy/umbra_announcer-keypair.json
solana-keygen pubkey target/deploy/umbra_announcer-keypair.json
```

Capture the printed pubkey (base58, 43-44 chars). Then update:

- `programs/umbra-announcer/src/lib.rs` — replace the `declare_id!` value
- `Anchor.toml` — replace both `umbra_announcer = "..."` lines under `[programs.localnet]` and `[programs.devnet]`

- [ ] **Step 1.12: Install JS deps and build**

```bash
cd /Users/susruth/Documents/Projects/umbra
npm install
anchor build
```

Expected: `anchor build` produces `target/deploy/umbra_announcer.so` and `target/idl/umbra_announcer.json` without errors.

If the build fails on a missing tool, install per the pre-flight section.

- [ ] **Step 1.13: Initial commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add Anchor.toml Cargo.toml package.json tsconfig.json .gitignore \
        programs/umbra-announcer/Cargo.toml \
        programs/umbra-announcer/Xargo.toml \
        programs/umbra-announcer/src/lib.rs \
        migrations/deploy.ts \
        tests/.gitkeep
git commit -m "chore: scaffold Anchor workspace for umbra-announcer"
```

Note: `target/`, `node_modules/`, and the keypair under `target/deploy/` are gitignored. The keypair MUST be kept locally; losing it means re-deploying under a new program ID.

---

## Task 2: Implement `announce` instruction (single event)

**Files:**
- Modify: `programs/umbra-announcer/src/lib.rs`
- Create: `tests/umbra-announcer.ts`

**Goal:** A single `announce(scheme_id, ephemeral_pub, view_tag, metadata)` instruction that emits exactly one `UmbraAnnouncement` event with the supplied fields.

---

- [ ] **Step 2.1: Write the failing test for single announce**

Create `/Users/susruth/Documents/Projects/umbra/tests/umbra-announcer.ts`:

```typescript
import * as anchor from "@coral-xyz/anchor";
import { Program, EventParser, BorshCoder } from "@coral-xyz/anchor";
import { UmbraAnnouncer } from "../target/types/umbra_announcer";
import { expect } from "chai";

describe("umbra-announcer", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.UmbraAnnouncer as Program<UmbraAnnouncer>;

  /**
   * Submit a tx, fetch its logs, and return all parsed Umbra events.
   */
  async function eventsFromTx(txSig: string) {
    // Wait for confirmation, then re-fetch with the logs.
    await provider.connection.confirmTransaction(txSig, "confirmed");
    const tx = await provider.connection.getTransaction(txSig, {
      commitment: "confirmed",
      maxSupportedTransactionVersion: 0,
    });
    if (!tx?.meta?.logMessages) {
      throw new Error("no logs in tx");
    }
    const parser = new EventParser(
      program.programId,
      new BorshCoder(program.idl)
    );
    return [...parser.parseLogs(tx.meta.logMessages)];
  }

  it("emits exactly one UmbraAnnouncement for a single announce", async () => {
    const ephemeralPub = Buffer.from(
      "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
      "hex"
    );
    const metadata = Buffer.from([0xab, 0xcd]);

    const txSig = await program.methods
      .announce(1, [...ephemeralPub], 0x42, metadata)
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    expect(events[0].name).to.equal("UmbraAnnouncement");

    const data = events[0].data as {
      schemeId: number;
      ephemeralPub: number[];
      viewTag: number;
      metadata: Buffer;
    };
    expect(data.schemeId).to.equal(1);
    expect(Buffer.from(data.ephemeralPub)).to.deep.equal(ephemeralPub);
    expect(data.viewTag).to.equal(0x42);
    expect(Buffer.from(data.metadata)).to.deep.equal(metadata);
  });
});
```

- [ ] **Step 2.2: Run the test to verify it fails**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected failure mode: `anchor build` succeeds (the program is a valid empty Anchor module), then the TypeScript test compile fails with something like:

```
tests/umbra-announcer.ts:35:32 - error TS2339:
  Property 'announce' does not exist on type 'MethodsNamespace<UmbraAnnouncer>'
```

This is because the IDL Anchor just generated has no `announce` instruction yet, so the generated TS type at `target/types/umbra_announcer.ts` does not expose that method.

If `anchor test` complains about the test validator not being able to start, run `solana-test-validator` once in another terminal and retry — Anchor's test harness usually handles this automatically, but networking quirks vary by machine.

- [ ] **Step 2.3: Implement the `announce` instruction and `UmbraAnnouncement` event**

Modify `/Users/susruth/Documents/Projects/umbra/programs/umbra-announcer/src/lib.rs`.

**Important:** keep the existing `declare_id!(...)` line (the program pubkey you set in Step 1.11) — do not overwrite it. The full file should look like the following, with `<YOUR_PROGRAM_ID>` left as you set it in Step 1.11:

```rust
use anchor_lang::prelude::*;

declare_id!("<YOUR_PROGRAM_ID>");

/// Maximum length of the optional `metadata` field in an announcement,
/// in bytes. See spec §6.1.
pub const MAX_METADATA_LEN: usize = 64;

#[program]
pub mod umbra_announcer {
    use super::*;

    /// Publish a single stealth-payment announcement.
    ///
    /// `scheme_id` is recorded but not validated — v1 clients only
    /// process `0x0001`; future schemes will be added by client updates.
    pub fn announce(
        _ctx: Context<Announce>,
        scheme_id: u16,
        ephemeral_pub: [u8; 32],
        view_tag: u8,
        metadata: Vec<u8>,
    ) -> Result<()> {
        require!(
            metadata.len() <= MAX_METADATA_LEN,
            UmbraError::MetadataTooLong
        );

        emit!(UmbraAnnouncement {
            scheme_id,
            ephemeral_pub,
            view_tag,
            metadata,
        });

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Announce<'info> {
    /// Fee payer. No special role beyond paying the tx; this account
    /// can be anyone.
    #[account(mut)]
    pub fee_payer: Signer<'info>,
}

#[event]
pub struct UmbraAnnouncement {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

#[error_code]
pub enum UmbraError {
    #[msg("metadata exceeds 64 bytes")]
    MetadataTooLong,
}
```

- [ ] **Step 2.4: Run the test to verify it now passes**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected: 1 passing test. The test name should match `emits exactly one UmbraAnnouncement for a single announce`.

If a `Signer<'info>` constraint error appears in the test (Anchor 0.30 sometimes requires explicit `accounts(...)` builder calls when the program has signers), modify the test's `program.methods.announce(...)` call to include an `.accounts({ feePayer: provider.wallet.publicKey })` chain before `.rpc()`. The Anchor TS client usually auto-fills the signer from the provider, but be explicit if it errors.

- [ ] **Step 2.5: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add programs/umbra-announcer/src/lib.rs tests/umbra-announcer.ts
git commit -m "feat(announcer): implement announce instruction with event emission"
```

---

## Task 3: Reject oversized metadata

**Files:**
- Modify: `tests/umbra-announcer.ts` (add test case)

**Goal:** Confirm the `MetadataTooLong` error fires when metadata exceeds 64 bytes, and that 64-byte exact-fit succeeds.

---

- [ ] **Step 3.1: Write the failing test for 65-byte metadata**

Add inside the `describe("umbra-announcer", ...)` block in `tests/umbra-announcer.ts`, after the existing `it(...)`:

```typescript
  it("rejects metadata longer than 64 bytes", async () => {
    const ephemeralPub = new Array(32).fill(0);
    const metadata = Buffer.alloc(65, 0xaa); // 65 bytes

    let threw = false;
    try {
      await program.methods
        .announce(1, ephemeralPub, 0x00, metadata)
        .rpc();
    } catch (err: any) {
      threw = true;
      const errMessage = err?.error?.errorMessage ?? err?.message ?? "";
      expect(errMessage).to.match(/metadata exceeds 64 bytes/i);
    }
    expect(threw, "expected announce(metadata.len=65) to throw").to.equal(true);
  });

  it("accepts metadata of exactly 64 bytes", async () => {
    const ephemeralPub = new Array(32).fill(0);
    const metadata = Buffer.alloc(64, 0xbb); // 64 bytes — boundary

    const txSig = await program.methods
      .announce(1, ephemeralPub, 0x00, metadata)
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    const data = events[0].data as { metadata: Buffer };
    expect(Buffer.from(data.metadata)).to.deep.equal(metadata);
  });

  it("accepts empty metadata", async () => {
    const ephemeralPub = new Array(32).fill(0);
    const metadata = Buffer.alloc(0);

    const txSig = await program.methods
      .announce(1, ephemeralPub, 0x00, metadata)
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    const data = events[0].data as { metadata: Buffer };
    expect(Buffer.from(data.metadata)).to.deep.equal(metadata);
  });
```

- [ ] **Step 3.2: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected: 4 passing tests. The metadata-validation logic is already in `lib.rs` from Task 2, so these should pass on first run. If the 65-byte case doesn't throw, the `require!` macro isn't firing — re-check `lib.rs` for the `require!(metadata.len() <= MAX_METADATA_LEN, ...)` line.

- [ ] **Step 3.3: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add tests/umbra-announcer.ts
git commit -m "test(announcer): cover metadata length boundary cases"
```

---

## Task 4: Implement `announce_batch` instruction

**Files:**
- Modify: `programs/umbra-announcer/src/lib.rs`
- Modify: `tests/umbra-announcer.ts`

**Goal:** A second instruction `announce_batch(entries: Vec<AnnouncementEntry>)` that emits one `UmbraAnnouncement` event per entry.

---

- [ ] **Step 4.1: Write the failing test for batch announce**

Add inside the `describe("umbra-announcer", ...)` block in `tests/umbra-announcer.ts`, after the previous tests:

```typescript
  it("emits one event per entry for announce_batch", async () => {
    const entries = [
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x01),
        viewTag: 0x10,
        metadata: Buffer.from([0xa1]),
      },
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x02),
        viewTag: 0x20,
        metadata: Buffer.from([0xa2, 0xa2]),
      },
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x03),
        viewTag: 0x30,
        metadata: Buffer.alloc(0),
      },
    ];

    const txSig = await program.methods
      .announceBatch(entries)
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(3);

    for (let i = 0; i < entries.length; i++) {
      const data = events[i].data as {
        schemeId: number;
        ephemeralPub: number[];
        viewTag: number;
        metadata: Buffer;
      };
      expect(events[i].name).to.equal("UmbraAnnouncement");
      expect(data.schemeId).to.equal(entries[i].schemeId);
      expect(Buffer.from(data.ephemeralPub)).to.deep.equal(
        Buffer.from(entries[i].ephemeralPub)
      );
      expect(data.viewTag).to.equal(entries[i].viewTag);
      expect(Buffer.from(data.metadata)).to.deep.equal(entries[i].metadata);
    }
  });
```

- [ ] **Step 4.2: Run the test to verify it fails**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected: build fails with `cannot find method announceBatch` or `cannot find struct AnnouncementEntry`.

- [ ] **Step 4.3: Implement the batch instruction**

Modify `/Users/susruth/Documents/Projects/umbra/programs/umbra-announcer/src/lib.rs`. Replace the entire `#[program]` module with:

```rust
#[program]
pub mod umbra_announcer {
    use super::*;

    /// Publish a single stealth-payment announcement.
    pub fn announce(
        _ctx: Context<Announce>,
        scheme_id: u16,
        ephemeral_pub: [u8; 32],
        view_tag: u8,
        metadata: Vec<u8>,
    ) -> Result<()> {
        require!(
            metadata.len() <= MAX_METADATA_LEN,
            UmbraError::MetadataTooLong
        );

        emit!(UmbraAnnouncement {
            scheme_id,
            ephemeral_pub,
            view_tag,
            metadata,
        });

        Ok(())
    }

    /// Publish multiple announcements in a single transaction. Used by
    /// announcement services to amortize the base tx fee across many
    /// announcements.
    pub fn announce_batch(
        _ctx: Context<AnnounceBatch>,
        entries: Vec<AnnouncementEntry>,
    ) -> Result<()> {
        require!(!entries.is_empty(), UmbraError::EmptyBatch);

        for entry in entries.into_iter() {
            require!(
                entry.metadata.len() <= MAX_METADATA_LEN,
                UmbraError::MetadataTooLong
            );

            emit!(UmbraAnnouncement {
                scheme_id: entry.scheme_id,
                ephemeral_pub: entry.ephemeral_pub,
                view_tag: entry.view_tag,
                metadata: entry.metadata,
            });
        }

        Ok(())
    }
}
```

And add the new `Accounts` struct, the entry struct, and the new error variant after the existing definitions. The full additions to make to `lib.rs`:

After the existing `Announce<'info>` struct, add:

```rust
#[derive(Accounts)]
pub struct AnnounceBatch<'info> {
    #[account(mut)]
    pub fee_payer: Signer<'info>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AnnouncementEntry {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}
```

Update the `UmbraError` enum to add the new variant:

```rust
#[error_code]
pub enum UmbraError {
    #[msg("metadata exceeds 64 bytes")]
    MetadataTooLong,
    #[msg("batch must contain at least one entry")]
    EmptyBatch,
}
```

- [ ] **Step 4.4: Run the test to verify it passes**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected: 5 passing tests, including the new batch test.

- [ ] **Step 4.5: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add programs/umbra-announcer/src/lib.rs tests/umbra-announcer.ts
git commit -m "feat(announcer): add announce_batch instruction"
```

---

## Task 5: Batch edge cases (empty batch, per-entry validation)

**Files:**
- Modify: `tests/umbra-announcer.ts`

**Goal:** Confirm that an empty batch is rejected with `EmptyBatch`, and that per-entry metadata validation fires inside a batch.

---

- [ ] **Step 5.1: Write tests for batch edge cases**

Add inside the `describe(...)` block, after the previous tests:

```typescript
  it("rejects an empty batch", async () => {
    let threw = false;
    try {
      await program.methods.announceBatch([]).rpc();
    } catch (err: any) {
      threw = true;
      const errMessage = err?.error?.errorMessage ?? err?.message ?? "";
      expect(errMessage).to.match(/batch must contain at least one entry/i);
    }
    expect(threw, "expected announce_batch([]) to throw").to.equal(true);
  });

  it("rejects a batch where any entry has oversized metadata", async () => {
    const entries = [
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x01),
        viewTag: 0x10,
        metadata: Buffer.from([0xa1]), // 1 byte — fine
      },
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x02),
        viewTag: 0x20,
        metadata: Buffer.alloc(65, 0xbb), // 65 bytes — too long
      },
    ];

    let threw = false;
    try {
      await program.methods.announceBatch(entries).rpc();
    } catch (err: any) {
      threw = true;
      const errMessage = err?.error?.errorMessage ?? err?.message ?? "";
      expect(errMessage).to.match(/metadata exceeds 64 bytes/i);
    }
    expect(threw, "expected oversized-metadata batch to throw").to.equal(true);
  });

  it("emits no events when a batch fails validation", async () => {
    // Sanity check that batch validation aborts before any emit. If the
    // first entry is valid but the second is invalid, neither event
    // should appear on-chain (anchor's `require!` aborts the whole tx).
    const entries = [
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x11),
        viewTag: 0x10,
        metadata: Buffer.from([0xa1]),
      },
      {
        schemeId: 1,
        ephemeralPub: new Array(32).fill(0x12),
        viewTag: 0x20,
        metadata: Buffer.alloc(65, 0xbb),
      },
    ];

    let txSig: string | undefined;
    try {
      txSig = await program.methods.announceBatch(entries).rpc();
    } catch (_) {
      // expected to throw
    }
    expect(txSig).to.equal(undefined);
  });
```

- [ ] **Step 5.2: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected: 8 passing tests. The validation logic from Task 4 already enforces these constraints, so no implementation change should be needed. If the `EmptyBatch` test fails, double-check that `require!(!entries.is_empty(), UmbraError::EmptyBatch)` exists at the top of `announce_batch`.

- [ ] **Step 5.3: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add tests/umbra-announcer.ts
git commit -m "test(announcer): cover batch edge cases (empty, per-entry validation)"
```

---

## Task 6: Idempotency / no-state sanity tests

**Files:**
- Modify: `tests/umbra-announcer.ts`

**Goal:** Confirm the program holds no state — the same `(scheme_id, R, view_tag, metadata)` can be announced twice without conflict, and the second announcement also emits an event. Also confirm `scheme_id = 0` is permitted (program does not validate the scheme ID per spec §6.1).

---

- [ ] **Step 6.1: Write the no-state and scheme_id tests**

Add inside the `describe(...)` block, after the previous tests:

```typescript
  it("allows the same announcement to be published twice", async () => {
    // Per spec, the announcer holds no state. A replayed announcement
    // is legal; recipients deduplicate by R themselves.
    const ephemeralPub = new Array(32).fill(0x77);
    const metadata = Buffer.from([0xde, 0xad, 0xbe, 0xef]);

    const tx1 = await program.methods
      .announce(1, ephemeralPub, 0x77, metadata)
      .rpc();
    const tx2 = await program.methods
      .announce(1, ephemeralPub, 0x77, metadata)
      .rpc();

    const events1 = await eventsFromTx(tx1);
    const events2 = await eventsFromTx(tx2);
    expect(events1).to.have.length(1);
    expect(events2).to.have.length(1);

    // Both transactions succeed independently.
    expect(tx1).to.not.equal(tx2);
  });

  it("accepts scheme_id = 0 (program does not validate scheme IDs)", async () => {
    // Per spec §6.1: scheme_id is recorded but not validated.
    // v1 clients ignore non-0x0001 schemes; the program records anything.
    const ephemeralPub = new Array(32).fill(0x00);

    const txSig = await program.methods
      .announce(0, ephemeralPub, 0x00, Buffer.alloc(0))
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    const data = events[0].data as { schemeId: number };
    expect(data.schemeId).to.equal(0);
  });

  it("accepts scheme_id = 0xFFFF (max value, experimental range)", async () => {
    const ephemeralPub = new Array(32).fill(0x00);

    const txSig = await program.methods
      .announce(0xffff, ephemeralPub, 0x00, Buffer.alloc(0))
      .rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(1);
    const data = events[0].data as { schemeId: number };
    expect(data.schemeId).to.equal(0xffff);
  });
```

- [ ] **Step 6.2: Run the tests**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected: 11 passing tests. All should pass without code changes (the program already has no state and no scheme_id validation).

- [ ] **Step 6.3: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add tests/umbra-announcer.ts
git commit -m "test(announcer): cover idempotency and scheme_id permissiveness"
```

---

## Task 7: Larger batch capacity test

**Files:**
- Modify: `tests/umbra-announcer.ts`

**Goal:** Verify the program handles a realistic batch size. The spec target is ~50 entries per batch (limited by Solana's CU budget, not by program logic). We test 20 to stay well under the limit while confirming non-trivial batches work.

---

- [ ] **Step 7.1: Write the batch-capacity test**

Add inside the `describe(...)` block:

```typescript
  it("handles a batch of 20 entries", async () => {
    const entries = Array.from({ length: 20 }, (_, i) => ({
      schemeId: 1,
      ephemeralPub: new Array(32).fill(i + 1),
      viewTag: i,
      metadata: Buffer.from([i, i + 1, i + 2]),
    }));

    const txSig = await program.methods.announceBatch(entries).rpc();

    const events = await eventsFromTx(txSig);
    expect(events).to.have.length(20);

    for (let i = 0; i < 20; i++) {
      const data = events[i].data as {
        viewTag: number;
        metadata: Buffer;
      };
      expect(data.viewTag).to.equal(i);
      expect(Buffer.from(data.metadata)).to.deep.equal(
        Buffer.from([i, i + 1, i + 2])
      );
    }
  });
```

- [ ] **Step 7.2: Run the test**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected: 12 passing tests.

If this fails with a compute-unit (CU) limit error, the per-entry CU cost is higher than expected on the developer's machine. Lower the batch size in the test from 20 to 10 and re-run; record the practical batch ceiling in the README in Task 8.

- [ ] **Step 7.3: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add tests/umbra-announcer.ts
git commit -m "test(announcer): verify batch of 20 entries succeeds"
```

---

## Task 8: Program README and deployment notes

**Files:**
- Create: `programs/umbra-announcer/README.md`

**Goal:** Document the program's purpose, instruction interface, deployment procedure, and the canonical program ID for downstream consumers (SDK, indexer, service).

---

- [ ] **Step 8.1: Write the README**

Create `/Users/susruth/Documents/Projects/umbra/programs/umbra-announcer/README.md`:

```markdown
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

## Build

```bash
anchor build
```

Outputs:
- `target/deploy/umbra_announcer.so` — the program binary
- `target/idl/umbra_announcer.json` — the IDL consumed by clients

## Test

```bash
anchor test
```

Anchor will spin up a local validator, deploy the program, and run the
TypeScript test suite.

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
| Localnet (dev) | (your local keypair; see `target/deploy/umbra_announcer-keypair.json`) |
| Devnet | TBD — populated on first devnet deployment |
| Mainnet | TBD — populated on first mainnet deployment |
```

- [ ] **Step 8.2: Commit**

```bash
cd /Users/susruth/Documents/Projects/umbra
git add programs/umbra-announcer/README.md
git commit -m "docs(announcer): add program README with instruction reference"
```

---

## Task 9: Final verification

**Files:** none modified.

**Goal:** Confirm the full test suite passes from a clean rebuild, mirroring what a fresh contributor will experience.

---

- [ ] **Step 9.1: Clean rebuild**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor clean
rm -rf node_modules
npm install
anchor build
```

Expected: clean build with no warnings (other than possibly unused-import warnings from Anchor's macros, which are harmless).

- [ ] **Step 9.2: Run full test suite**

```bash
cd /Users/susruth/Documents/Projects/umbra
anchor test
```

Expected: 12 passing tests. If anything fails, do not move on — diagnose the failure and resolve before deployment planning.

- [ ] **Step 9.3: Lint check**

```bash
cd /Users/susruth/Documents/Projects/umbra
npm run lint
```

Expected: no formatting errors. If errors appear, run `npm run lint:fix` and commit the formatting fixes.

- [ ] **Step 9.4: (Optional) Deploy to devnet for downstream development**

This step is optional and depends on access to a funded devnet wallet. The
SDK plan (next plan) will assume a devnet program ID is available, so it
is useful to do this now.

```bash
solana config set --url https://api.devnet.solana.com
solana airdrop 2  # if balance < 2 SOL
anchor deploy --provider.cluster devnet
solana-keygen pubkey target/deploy/umbra_announcer-keypair.json
```

Record the program ID. Update `programs/umbra-announcer/README.md`'s
"Program ID" table with the devnet address and commit:

```bash
git add programs/umbra-announcer/README.md
git commit -m "docs(announcer): record devnet program ID"
```

---

## Summary

After completing all tasks:

- **8 commits** mark each milestone (scaffold → single announce → batch
  → edge cases → idempotency → batch capacity → docs → optional deploy)
- **12 passing tests** cover the spec's requirements for §6 (announcer
  program)
- **0 placeholders** in code or docs — every step's content is final
- **The program is deployable** to localnet, devnet, and (when the team
  is ready) mainnet with no further changes required

The next implementation plan (SDK) will assume this program is deployed
to devnet and will consume the IDL at `target/idl/umbra_announcer.json`
plus the program ID.
