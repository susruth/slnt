# SLNT — Deployment

How the on-chain programs (`pinboard`, `registry`) are built and deployed, and
the addresses/authorities of the current deployments. The programs are intended
to be **immutable** on a canonical mainnet deploy (sRFC-0042 §5.5/§5.6,
[`SECURITY.md`](../SECURITY.md)); the current devnet/testnet deployments are kept
**upgradeable** until the programs are audited and the sRFC is accepted.

## Canonical program IDs (vanity)

| Program | Address | Vanity |
|---|---|---|
| `pinboard` | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` | `SLNTP…` = **P**inboard |
| `registry` | `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2` | `SLNTR…` = **R**egistry |

These are pinned in `declare_id!` (`programs/*/src/lib.rs`) and in every
`[programs.*]` block of `Anchor.toml`. Deploying *at* these addresses requires
the matching **program keypairs** (the files whose pubkey equals the address).
They are **not** committed — place them at `target/deploy/pinboard-keypair.json`
and `target/deploy/registry-keypair.json` before deploying, and back them up
securely. Losing them means the canonical address can never be re-deployed.

> Do **not** run `anchor keys sync` with the canonical keypairs in place — it
> would overwrite `declare_id!`/`Anchor.toml` with freshly generated IDs.

## Current deployments

Both clusters host the same vanity addresses. Upgrade authority is the deployer
wallet (**not** `--final` yet).

| Cluster | Program | Address | Authority | Deploy signature |
|---|---|---|---|---|
| devnet | pinboard | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` | `78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR` | `53gyNA8DDo1oH9JnJL9sRw7hr31hy244FxFEvGVWp7eANRdFymmLe4M9u7XLYvQmea1BKoSUQBfBw6FeawBniZgs` |
| devnet | registry | `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2` | `78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR` | `4HhTcDtq2JLyVUMu7tWM3StffSpJ2AEegrgKd4fu3GE6g4zvBvfKf42KShTXuXjhxrzzs8Kcp7M8UbfF65Tmc6L8` |
| testnet | pinboard | `SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2` | `78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR` | `5LcCBoRhubgjQbQkWowSDcJiSXVgeE3kAaN16b8Gfy5kjYtNYBJGrYydMrniFGPrsp8o9kmSSNgrEq7tW6gMLM4n` |
| testnet | registry | `SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2` | `78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR` | `tCScYyTpjNCVzDCAPtoHe3vUGrP5EcbHFQXV1gm8nVNEAVFvcx1Hs2NVRnEhbeHxHxf8vUqXwMAFhZs5VgA49RA` |

Explorer: `https://explorer.solana.com/address/<ADDRESS>?cluster=devnet`
(swap `cluster=testnet`).

Verify any program:

```bash
solana program show SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2 --url devnet
# Authority: 78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR  → upgradeable (not final)
```

## The deployer wallet

The wallet that pays deploy fees + rent and becomes the upgrade authority is the
Anchor provider wallet, `~/.config/solana/id.json`:

```
78ZkB1rxMk46Nddff3WJCXbML7fGXhX2tkXUgPhfZ7mR
```

Fund **this** address (never the program IDs). Each cluster cost ~2.7 SOL for
both programs. Fund via `solana airdrop` (rate-limited) or
<https://faucet.solana.com> for larger amounts.

## Build

The programs need a newer platform-tools than Anchor's default: the default
(`v1.48`, Cargo 1.84) cannot parse a transitive `edition2024` dependency, and
the IDL build step does not accept the tools-version passthrough. So build the
deployable binaries with:

```bash
anchor build --no-idl -- --tools-version v1.54
# → target/deploy/pinboard.so, target/deploy/registry.so
```

(The host workspace — SDK, CLI, services — builds with the normal host toolchain;
this flag only matters for the on-chain SBF build.)

## Deploy

Deploy each program with its keypair (sets the address) and the deployer wallet
(pays + becomes upgrade authority; upgradeable unless `--final`):

```bash
solana program deploy target/deploy/pinboard.so \
  --program-id target/deploy/pinboard-keypair.json \
  --keypair ~/.config/solana/id.json \
  --url devnet            # or testnet

solana program deploy target/deploy/registry.so \
  --program-id target/deploy/registry-keypair.json \
  --keypair ~/.config/solana/id.json \
  --url devnet            # or testnet
```

`anchor deploy --provider.cluster <devnet|testnet>` also works once the IDL build
is unblocked; the direct `solana program deploy` path above sidesteps it.

## Upgrades (pre-audit)

The deployments were sized to the exact program length (no 2× headroom). A
post-audit rebuild that is the **same size or smaller** redeploys directly. A
**larger** binary needs a one-time extend first:

```bash
solana program extend <PROGRAM_ID> <ADDITIONAL_BYTES> --url <cluster>
solana program deploy  <...> --url <cluster>
```

## Making it immutable (post-audit, post-sRFC-acceptance)

Once the programs are audited and the sRFC is accepted, renounce the upgrade
authority. **Irreversible** — do it only on a deploy you are keeping, and note
the program-data account rent becomes unrecoverable:

```bash
solana program set-upgrade-authority SLNTPDxgFKwSZ31CbbdSKKHyRpBpKjEMYVj2gpGxkN2 --final --url <cluster>
solana program set-upgrade-authority SLNTRCsjJXUQM3UbHjgJ48xe4GjKFSiLmrF1mXA8Vn2 --final --url <cluster>
```

## Deployment log

| Date | Cluster | Action |
|---|---|---|
| 2026-05-31 | devnet | Initial deploy of `pinboard` + `registry` (upgradeable). |
| 2026-05-31 | testnet | Initial deploy of `pinboard` + `registry` (upgradeable). |
