use thiserror::Error;

#[derive(Debug, Error)]
pub enum UmbraError {
    #[error("key derivation failed (signature produced anomalous scalar)")]
    Derivation,

    #[error("invalid Ed25519 point encoding")]
    InvalidPoint,

    #[error("meta-address encoding failed")]
    MetaAddressEncode,

    #[error("meta-address decoding failed: {0}")]
    MetaAddressDecode(String),

    #[error("unsupported meta-address version: {0:#x}")]
    UnsupportedVersion(u8),

    #[error("note metadata exceeds 64 bytes (got {0})")]
    MetadataTooLong(usize),

    #[error("base58 decode failed")]
    Base58,

    #[error("rpc error: {0}")]
    Rpc(String),
}
