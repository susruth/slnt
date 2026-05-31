use thiserror::Error;

#[derive(Debug, Error)]
pub enum SlntError {
    #[error("key derivation failed (signature produced anomalous scalar)")]
    Derivation,

    #[error("invalid or non-spendable Ed25519 point")]
    InvalidPoint,

    #[error("invalid X25519 shared secret (all zero)")]
    InvalidSharedSecret,

    #[error("meta-address encoding failed")]
    MetaAddressEncode,

    #[error("meta-address decoding failed: {0}")]
    MetaAddressDecode(String),

    #[error("unsupported meta-address version: {0:#x}")]
    UnsupportedVersion(u8),

    #[error("unsupported meta-address flags: {0:#x}")]
    UnsupportedFlags(u8),

    #[error("note metadata exceeds 64 bytes (got {0})")]
    MetadataTooLong(usize),

    #[error("base58 decode failed")]
    Base58,

    #[error("rpc error: {0}")]
    Rpc(String),

    #[error("close/rent destination is the recipient's main wallet — this would create a stealth→main link (sRFC §5.9)")]
    CloseToMainWallet,

    #[error("relayer take ({take}) exceeds the swept balance ({balance})")]
    RelayerTakeTooLarge { take: u64, balance: u64 },

    #[error(
        "lamport amount overflow: amount ({amount}) + rent buffer ({rent_buffer}) exceeds u64"
    )]
    LamportOverflow { amount: u64, rent_buffer: u64 },

    #[error("non-deterministic signature: two signings of the canonical message differ — this wallet cannot be used with Method 2 (sRFC §5.2.1.2/§8.5)")]
    NonDeterministicSignature,

    #[error("invalid HD seed length {0}: SLIP-0010 requires 16–64 bytes")]
    InvalidSeedLength(usize),
}
