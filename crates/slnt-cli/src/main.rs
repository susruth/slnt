//! `slnt` — command-line tools for the Slnt stealth-payment protocol
//! (sRFC-0042). The subcommands here are offline/pure: key derivation,
//! meta-address encode/decode, and sender stealth-address derivation.
//! On-chain submission is left to a wallet/relayer using `slnt-sdk`.

use clap::{Parser, Subcommand};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use slnt_sdk::keys::{
    canonical_message, derive_stealth_keys, derive_stealth_keys_hd, MetaAddress, Network,
};
use slnt_sdk::sender::derive_payment;

#[derive(Parser)]
#[command(name = "slnt", version, about = "Slnt stealth-payment CLI (sRFC-0042)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the canonical message to sign for Method 2 key derivation.
    CanonicalMessage {
        #[arg(long, default_value = "mainnet")]
        network: String,
    },
    /// Derive a meta-address from a Method 2 signature (64-byte hex).
    Derive {
        #[arg(long)]
        signature: String,
    },
    /// Derive a meta-address from a BIP-39 seed (Method 1, HD).
    DeriveHd {
        #[arg(long)]
        seed: String,
        #[arg(long, default_value_t = 0)]
        account: u32,
    },
    /// Derive a labeled meta-address (Method 2 signature + label index).
    Label {
        #[arg(long)]
        signature: String,
        #[arg(long)]
        index: u32,
    },
    /// Decode a `slnt1…` meta-address into its fields.
    MetaDecode {
        meta: String,
    },
    /// Derive a one-time stealth address (sender side) for a meta-address.
    /// `--rng` is a 32-byte hex seed for reproducible ephemeral randomness.
    Pay {
        #[arg(long)]
        meta: String,
        #[arg(long)]
        rng: String,
    },
}

fn main() {
    match dispatch(Cli::parse().command) {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn dispatch(command: Command) -> Result<String, String> {
    match command {
        Command::CanonicalMessage { network } => Ok(canonical_message(parse_network(&network)?)),
        Command::Derive { signature } => {
            let sig = parse_fixed::<64>(&signature, "signature")?;
            let (spend, scan) = derive_stealth_keys(&sig).map_err(|e| e.to_string())?;
            MetaAddress::from_keys(&spend, &scan)
                .encode_bech32m()
                .map_err(|e| e.to_string())
        }
        Command::DeriveHd { seed, account } => {
            let seed = hex::decode(seed.trim()).map_err(|e| format!("seed hex: {e}"))?;
            let (spend, scan) = derive_stealth_keys_hd(&seed, account).map_err(|e| e.to_string())?;
            MetaAddress::from_keys(&spend, &scan)
                .encode_bech32m()
                .map_err(|e| e.to_string())
        }
        Command::Label { signature, index } => {
            let sig = parse_fixed::<64>(&signature, "signature")?;
            let (spend, scan) = derive_stealth_keys(&sig).map_err(|e| e.to_string())?;
            MetaAddress::for_label(&spend, &scan, index)
                .encode_bech32m()
                .map_err(|e| e.to_string())
        }
        Command::MetaDecode { meta } => {
            let m = MetaAddress::decode_bech32m(meta.trim()).map_err(|e| e.to_string())?;
            Ok(format!(
                "version:     0x{:02x}\nb_spend:     {}\nb_scan:      {}\nlabel_index: {}\nflags:       0x{:02x}",
                m.version,
                hex::encode(m.b_spend),
                hex::encode(m.b_scan),
                m.label_index,
                m.flags,
            ))
        }
        Command::Pay { meta, rng } => {
            let m = MetaAddress::decode_bech32m(meta.trim()).map_err(|e| e.to_string())?;
            let seed = parse_fixed::<32>(&rng, "rng")?;
            let mut rng = ChaCha20Rng::from_seed(seed);
            let payment = derive_payment(&m, &mut rng).map_err(|e| e.to_string())?;
            Ok(format!(
                "stealth_address: {}\nephemeral_pub:   {}\nview_tag:        0x{:02x}",
                payment.stealth_address,
                hex::encode(payment.ephemeral_pub),
                payment.view_tag,
            ))
        }
    }
}

fn parse_network(s: &str) -> Result<Network, String> {
    match s.to_ascii_lowercase().as_str() {
        "mainnet" | "mainnet-beta" => Ok(Network::Mainnet),
        "devnet" => Ok(Network::Devnet),
        "testnet" => Ok(Network::Testnet),
        "localnet" => Ok(Network::Localnet),
        other => Err(format!("unknown network: {other}")),
    }
}

fn parse_fixed<const N: usize>(s: &str, what: &str) -> Result<[u8; N], String> {
    let bytes = hex::decode(s.trim()).map_err(|e| format!("{what} hex: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| format!("{what} must be {N} bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_message_matches_sdk() {
        let out = dispatch(Command::CanonicalMessage { network: "devnet".into() }).unwrap();
        assert!(out.contains("Network: Devnet"));
    }

    #[test]
    fn derive_then_decode_roundtrips() {
        let sig = "11".repeat(64);
        let meta = dispatch(Command::Derive { signature: sig }).unwrap();
        assert!(meta.starts_with("slnt1"));
        let decoded = dispatch(Command::MetaDecode { meta }).unwrap();
        assert!(decoded.contains("version:     0x01"));
        assert!(decoded.contains("label_index: 0"));
    }

    #[test]
    fn label_sets_label_index() {
        let sig = "22".repeat(64);
        let meta = dispatch(Command::Label { signature: sig, index: 9 }).unwrap();
        let decoded = dispatch(Command::MetaDecode { meta }).unwrap();
        assert!(decoded.contains("label_index: 9"));
    }

    #[test]
    fn pay_is_deterministic_for_fixed_rng() {
        let sig = "33".repeat(64);
        let meta = dispatch(Command::Derive { signature: sig }).unwrap();
        let rng = "ab".repeat(32);
        let a = dispatch(Command::Pay { meta: meta.clone(), rng: rng.clone() }).unwrap();
        let b = dispatch(Command::Pay { meta, rng }).unwrap();
        assert_eq!(a, b);
        assert!(a.contains("stealth_address:"));
    }

    #[test]
    fn derive_rejects_wrong_length_signature() {
        let err = dispatch(Command::Derive { signature: "00".into() });
        assert!(err.is_err());
    }

    #[test]
    fn hd_derivation_produces_meta_address() {
        let seed = "00".repeat(64);
        let meta = dispatch(Command::DeriveHd { seed, account: 0 }).unwrap();
        assert!(meta.starts_with("slnt1"));
    }
}
