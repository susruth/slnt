// Typed error taxonomy mirroring the Rust `SlntError` enum, so callers
// can branch on `err.code` the way Rust code matches variants.

export type SlntErrorCode =
  | "Derivation"
  | "InvalidPoint"
  | "InvalidSharedSecret"
  | "MetaAddressDecode"
  | "UnsupportedVersion"
  | "UnsupportedFlags"
  | "MetadataTooLong"
  | "CloseToMainWallet"
  | "RelayerTakeTooLarge"
  | "NonDeterministicSignature"
  | "InvalidSeedLength"
  | "LamportOverflow"
  | "Rpc";

export class SlntError extends Error {
  readonly code: SlntErrorCode;
  constructor(code: SlntErrorCode, message: string) {
    super(`${code}: ${message}`);
    this.name = "SlntError";
    this.code = code;
  }
}
