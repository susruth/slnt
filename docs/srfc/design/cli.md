# SLNT CLI (`slnt`) — Design & Reference Implementation

| | |
|---|---|
| **Component** | `slnt-cli` (Rust crate, `crates/slnt-cli`) |
| **Status** | Reference implementation |
| **Spec** | sRFC-0042 §5 (normative) |
| **Binary name** | `slnt` (`src/main.rs`) |
| **Crate version** | `0.1.0`, edition 2021, license MIT |

`slnt` is a thin command-line wrapper over the Rust SDK (`rust-sdk.md`). It exposes
the protocol's **offline, pure** operations — key derivation, meta-address
encode/decode, and sender stealth-address derivation — as composable subcommands
suitable for scripting, debugging, and generating cross-language test vectors. It
performs no cryptography of its own: every command is a parse + delegate to
`slnt_sdk`, so it is byte-for-byte equivalent to the reference math defined in
`rust-sdk.md` and the normative spec.

---

## 1. Design philosophy

**Offline and pure by design.** The CLI never opens a network connection, never
reads or writes a keystore, and holds no key material beyond what is passed in on
the command line for the duration of a single invocation. There is no RPC client,
no wallet, no config file. Inputs are hex strings and bech32m meta-addresses;
outputs are deterministic formatted text. This keeps the trust surface tiny and
makes the tool safe to run in CI and air-gapped environments.

**On-chain submission is out of scope.** Posting announcements, registering
meta-addresses, sending funds, and sweeping are deliberately *not* CLI
responsibilities — they require signing keys, an RPC endpoint, and fee payment,
which belong to wallets and relayers. See [§5](#5-scope-boundaries).

**The `dispatch` seam.** The entire command surface is implemented as one pure
function:

```rust
fn dispatch(command: Command) -> Result<String, String>
```

`dispatch` takes the parsed clap `Command` enum and returns either the exact
string to print on success or an error message. It touches no global state and
performs no I/O. This single seam is what makes every subcommand unit-testable
without spawning a process or capturing stdout — the tests in `main.rs` call
`dispatch(Command::…)` directly and assert on the returned `String`
([§6](#6-testing)).

**`main()` is a thin shell.** `main` does nothing but parse args, call
`dispatch`, and route the result:

```rust
fn main() {
    match dispatch(Cli::parse().command) {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
```

Success prints to stdout and exits `0`; any error prints `error: <message>` to
stderr and exits `1` ([§4](#4-error-handling-and-exit-codes)).

---

## 2. Subcommand reference

| Subcommand | Purpose | SDK call | Key args |
|---|---|---|---|
| `canonical-message` | Print the Method 2 message to sign | `keys::canonical_message` | `--network` |
| `derive` | Meta-address from a Method 2 signature | `keys::derive_stealth_keys` | `--signature` (128 hex) |
| `derive-hd` | Meta-address from a BIP-39 seed (Method 1) | `keys::derive_stealth_keys_hd` | `--seed`, `--account` |
| `label` | Labeled meta-address (Method 2 + label index) | `keys::derive_stealth_keys` + `MetaAddress::for_label` | `--signature`, `--index` |
| `meta-decode` | Decode a `slnt1…` meta-address into fields | `MetaAddress::decode_bech32m` | `<meta>` (positional) |
| `pay` | Derive a one-time stealth address (sender) | `sender::derive_payment` | `--meta`, `--rng` (64 hex) |

All hex inputs are case-insensitive and trimmed of surrounding whitespace before
decoding. The derivation methods, key math, and byte layouts are documented in
`rust-sdk.md`; the CLI only adapts argument strings to SDK calls and formats the
results.

### 2.1 `canonical-message --network <name>`

**Purpose.** Print the exact canonical message a generic signing wallet must sign
for Method 2 key derivation (sRFC-0042 §5.2.1.2). The output is intended to be
fed verbatim into a wallet's "sign message" flow; the resulting 64-byte signature
is then passed to `derive`.

**Arguments.** `--network <name>` (default `mainnet`). Parsed via
[§3](#3-network-parsing).

**SDK call.** `slnt_sdk::keys::canonical_message(network)`.

**Output.** The multi-line UTF-8 message, with the `Network:` line substituted
for the selected network and **no trailing newline** (the trailing newline in the
terminal comes from `println!`, not the message itself):

```
$ slnt canonical-message --network devnet
Slnt Protocol: Derive Stealth Keys

Version: 1
Network: Devnet
Warning: Only sign this message in the Slnt wallet or a trusted Slnt integration.
Signing this in any other context will reveal your stealth address scanning ability.
```

### 2.2 `derive --signature <hex>`

**Purpose.** Derive a recipient's meta-address from a Method 2 signature over the
canonical message (sRFC-0042 §5.2.1.2 → §5.2.1.3 → §5.2.2).

**Arguments.** `--signature <hex>` — a 64-byte Ed25519 signature encoded as
**128 hex characters**. Parsed by `parse_fixed::<64>`; any other length is
rejected with `signature must be 64 bytes`, and non-hex input with
`signature hex: …`.

**SDK call.** `keys::derive_stealth_keys(&sig)` to obtain `(B_spend, B_scan)`,
then `MetaAddress::from_keys(&spend, &scan).encode_bech32m()`.

**Output.** A single `slnt1…` bech32m meta-address string. See
[§2.7](#27-worked-example-derive--pay) for a real example.

### 2.3 `derive-hd --seed <hex> [--account <n>]`

**Purpose.** Derive a meta-address via Method 1 — wallet-native HD derivation from
a BIP-39 seed (sRFC-0042 §5.2.1.1). This is the recommended path for wallets with
seed access; it has no signing step.

**Arguments.**
- `--seed <hex>` — the BIP-39 seed as **arbitrary-length hex** (variable length;
  decoded with `hex::decode`, no fixed-size check — length validation is the
  SDK's responsibility). Non-hex input is rejected with `seed hex: …`.
- `--account <n>` — the SLNT stealth-identity index (`u32`, default `0`),
  becoming `account'` in the path `m/0x534C4E54'/501'/account'/{0',1'}`.

**SDK call.** `keys::derive_stealth_keys_hd(&seed, account)`, then
`MetaAddress::from_keys(...).encode_bech32m()`.

**Output.** A single `slnt1…` meta-address string.

```
$ slnt derive-hd --seed 000102030405060708090a0b0c0d0e0f --account 0
slnt1…
```

### 2.4 `label --signature <hex> --index <n>`

**Purpose.** Derive a *labeled* meta-address (BIP-352-style labels,
sRFC-0042 §5.2.3) from a Method 2 signature plus a label index. Labels let one
scan key publish multiple distinguishable meta-addresses.

**Arguments.**
- `--signature <hex>` — 64-byte / 128-hex signature, same format and validation
  as `derive`.
- `--index <n>` — the label index (`u32`). Index `0` is the unlabeled default;
  `1+` applies the label tweak `m_i`.

**SDK call.** `keys::derive_stealth_keys(&sig)` for the base keys, then
`MetaAddress::for_label(&spend, &scan, index).encode_bech32m()`.

**Output.** A single `slnt1…` meta-address whose decoded `label_index` equals
`<n>`.

### 2.5 `meta-decode <meta>`

**Purpose.** Decode a bech32m `slnt1…` meta-address back into its fields for
inspection or debugging (sRFC-0042 §5.2.2). Useful for confirming `version`,
`flags`, key bytes, and label index.

**Arguments.** A single positional `<meta>` — the `slnt1…` string (trimmed).

**SDK call.** `MetaAddress::decode_bech32m(meta.trim())`.

**Output.** Five aligned, fixed-format lines:

```
$ slnt meta-decode slnt1qytx5j6qsy4pr4un72tf6rr0f0vpzf7my2swgx3sdy0ug5l057h7uwfysf0e2gavthnjy4r553rucxv09hr8texhdwhycnsmz7msshzhqqqqwug2z6
version:     0x01
b_spend:     166a4b40812a11d793f2969d0c6f4bd81127db22a0e41a30691fc453efa7afee
b_scan:      3924825f9523ac5de7225474a447cc198f2dc675e4d76bae4c4e1b17b7085c57
label_index: 0
flags:       0x00
```

`version` and `flags` are printed as `0x%02x`; `b_spend` and `b_scan` as lowercase
hex; `label_index` as a decimal integer. A meta-address with `version != 0x01` or
`flags != 0x00`, or a bad checksum/HRP, is rejected by the SDK and surfaces as an
error.

### 2.6 `pay --meta <meta> --rng <hex>`

**Purpose.** Perform the **sender** stealth-address derivation (sRFC-0042 §5.3):
given a recipient meta-address, produce a one-time stealth address, the ephemeral
public key `R` to announce, and the `view_tag`.

**Arguments.**
- `--meta <meta>` — the recipient's `slnt1…` meta-address.
- `--rng <hex>` — a 32-byte / **64-hex** seed used to seed a `ChaCha20Rng` so the
  ephemeral scalar `r` is reproducible. Parsed by `parse_fixed::<32>`; wrong
  length is rejected with `rng must be 32 bytes`. See
  [§2.8](#28-determinism-and-the-rng-seed).

**SDK call.** `MetaAddress::decode_bech32m(meta)` → seed `ChaCha20Rng::from_seed`
→ `sender::derive_payment(&meta, &mut rng)`.

**Output.** Three lines: the base58 `stealth_address`, the hex `ephemeral_pub`
(`R`), and the `view_tag` as `0x%02x`:

```
stealth_address: <base58>
ephemeral_pub:   <64-hex>
view_tag:        0x<hh>
```

### 2.7 Worked example: `derive` → `pay`

Derive a meta-address from an (illustrative) all-`0x77` signature, then derive a
payment to it with an all-`0xaa` RNG seed. Both outputs are reproduced byte-for-byte
by running the CLI:

```
$ slnt derive --signature 7777777777777777777777777777777777777777777777777777777777777777\
7777777777777777777777777777777777777777777777777777777777777777
slnt1qytx5j6qsy4pr4un72tf6rr0f0vpzf7my2swgx3sdy0ug5l057h7uwfysf0e2gavthnjy4r553rucxv09hr8texhdwhycnsmz7msshzhqqqqwug2z6

$ slnt pay \
    --meta slnt1qytx5j6qsy4pr4un72tf6rr0f0vpzf7my2swgx3sdy0ug5l057h7uwfysf0e2gavthnjy4r553rucxv09hr8texhdwhycnsmz7msshzhqqqqwug2z6 \
    --rng aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
stealth_address: Ac8MM66HM2tVVPkLZVag7h2XzCeHRCm912yeMmSy5RqV
ephemeral_pub:   0e4af3530b966e62131cf24d898fb8a7b24ef15580c46fd57c3a5115f8e19c6e
view_tag:        0xa5
```

(The `--signature` and `--rng` arguments are each a single unbroken hex string —
`77`×64 and `aa`×32 respectively; line breaks above are for readability only.)

### 2.8 Determinism and the RNG seed

`pay` is the one subcommand whose underlying math (sRFC-0042 §5.3) consumes
randomness: the sender draws a fresh ephemeral scalar `r`. To make the command
**reproducible**, `--rng` supplies a 32-byte seed that initializes a
`ChaCha20Rng` (`ChaCha20Rng::from_seed(seed)`); the SDK then draws `r` from that
deterministic stream. Given the same `--meta` and `--rng`, `pay` always emits the
same `stealth_address`, `ephemeral_pub`, and `view_tag`.

This determinism is **for reproducibility and testing only**. Production senders
**MUST** use OS randomness (a cryptographically secure system RNG), never a fixed
or guessable seed — a predictable `r` would let an observer link the announcement
to the recipient. The CLI exposes a seed precisely because a deterministic `pay`
is what produces the **cross-language known-answer vectors**: the worked example
in [§2.7](#27-worked-example-derive--pay) is exactly the kind of fixed
`(meta, rng) → (stealth_address, R, view_tag)` triple that the TypeScript SDK
(`typescript-sdk.md`) replays in its tests to prove byte-compatibility with the
Rust reference.

---

## 3. Network parsing

`--network` (used by `canonical-message`) is mapped to the SDK `Network` enum by
`parse_network`, case-insensitively:

| Input (case-insensitive) | `Network` |
|---|---|
| `mainnet`, `mainnet-beta` | `Network::Mainnet` |
| `devnet` | `Network::Devnet` |
| `testnet` | `Network::Testnet` |
| `localnet` | `Network::Localnet` |

Any other value yields `unknown network: <other>` and exit code `1`. Because the
network string is part of the signed canonical message, keys derived for different
networks differ — devnet experiments cannot leak a mainnet stealth identity.

---

## 4. Error handling and exit codes

All fallible operations return `Result<_, String>` and propagate through
`dispatch` to `main`. There are two outcomes:

- **Success:** the result string is printed to **stdout**; process exits `0`.
- **Failure:** `error: <message>` is printed to **stderr**; process exits `1`.

Error messages are produced at two layers:

- **CLI parse helpers** — `parse_fixed::<N>` returns `<what> hex: <e>` for non-hex
  input and `<what> must be <N> bytes` for the wrong length (where `<what>` is
  `signature` or `rng`); `parse_network` returns `unknown network: <other>`; the
  `derive-hd` seed decode returns `seed hex: <e>`.
- **SDK** — every `slnt_sdk` error (invalid signature reduction, bech32m
  checksum/HRP failure, unsupported `version`/`flags`, low-order scan key, etc.)
  is mapped through `.map_err(|e| e.to_string())` and surfaced verbatim. The CLI
  adds no recovery logic; it is a faithful conduit for SDK errors.

clap itself handles unknown subcommands, missing required args, and `--help` /
`--version`, exiting with its own conventional codes before `dispatch` is reached.

---

## 5. Scope boundaries

The CLI is intentionally limited to operations that are **pure and offline**. It
deliberately does **not**:

- **Send funds** to a stealth address (the on-chain transfer in sRFC-0042 §5.3).
- **Post announcements** to the `pinboard` program (sRFC-0042 §5.5).
- **Register / update / close** meta-addresses in the `registry` program
  (sRFC-0042 §5.6).
- **Scan** for incoming payments or **sweep** stealth balances (sRFC-0042 §5.4,
  §5.7, §5.9) — both require RPC access, and sweeping requires the recipient's
  scan key plus the close-to-relayer rule.

These all require one or more of: a signing key held in custody, an RPC endpoint,
fee payment, and transaction construction/submission — concerns that belong to a
**wallet or relayer** built on `slnt-sdk` (the SDK already provides the instruction
builders and scan/sweep helpers; see `rust-sdk.md`). Keeping them out of the CLI
preserves its zero-trust, zero-network posture.

They are nonetheless natural **candidate future subcommands** (e.g. `register`,
`announce`, `scan`, `sweep`) should an opinionated, key-bearing CLI surface be
desired later; they would necessarily relax the offline/pure guarantees above and
take an RPC endpoint plus a keypair source.

---

## 6. Testing

`main.rs` ships six unit tests, all of which exercise the pure `dispatch` seam
directly (no process spawn, no stdout capture):

1. **`canonical_message_matches_sdk`** — `canonical-message --network devnet`
   output contains `Network: Devnet`, confirming network substitution.
2. **`derive_then_decode_roundtrips`** — `derive` (sig `0x11`×64) yields a `slnt1…`
   string that `meta-decode` reports with `version: 0x01` and `label_index: 0`.
3. **`label_sets_label_index`** — `label` with `--index 9` (sig `0x22`×64) produces
   a meta-address whose decoded `label_index` is `9`.
4. **`pay_is_deterministic_for_fixed_rng`** — two `pay` calls with the same meta
   and the same `--rng` (`0xab`×32) return identical output, and it contains
   `stealth_address:` (the determinism contract of [§2.8](#28-determinism-and-the-rng-seed)).
5. **`derive_rejects_wrong_length_signature`** — `derive --signature 00` (1 byte)
   returns `Err`, exercising `parse_fixed`'s length check.
6. **`hd_derivation_produces_meta_address`** — `derive-hd` with a 32-byte seed and
   `account 0` produces a `slnt1…` meta-address (Method 1 path).

Because `dispatch` is the same code path `main` uses, these tests fully cover the
command behavior; the cross-language conformance vectors are then anchored by the
deterministic `pay` output ([§2.8](#28-determinism-and-the-rng-seed)).

---

## 7. Related documents

- `rust-sdk.md` — the SDK this CLI wraps; canonical byte-level reference for all
  derivation math, the meta-address codec, and sender derivation.
- `typescript-sdk.md` — the TypeScript SDK whose tests replay the deterministic
  `pay` known-answer vectors emitted by this tool.
- sRFC-0042 — the normative SLNT silent-payments standard (§5.2 key derivation /
  meta-address, §5.3 sender derivation).
