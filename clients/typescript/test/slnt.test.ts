import { expect } from "chai";
import { hexToBytes } from "@noble/hashes/utils";
import {
  canonicalMessage,
  decodeMetaAddress,
  deriveStealthKeysFromSignature,
  encodeMetaAddress,
  metaForLabel,
  metaFromKeys,
} from "../src/keys";
import { derivePayment } from "../src/sender";
import { scanNoteCandidates, viewTagMatches } from "../src/recipient";

// Known-answer vectors produced by the Rust reference (`slnt` CLI):
//   slnt derive --signature 77..77 (64 bytes of 0x77)
//   slnt pay    --meta <above>     --rng aa..aa
const SIG_77 = hexToBytes("77".repeat(64));
const RUST_META =
  "slnt1qytx5j6qsy4pr4un72tf6rr0f0vpzf7my2swgx3sdy0ug5l057h7uwfysf0e2gavthnjy4r553rucxv09hr8texhdwhycnsmz7msshzhqqqqwug2z6";
const RUST_R = hexToBytes("0e4af3530b966e62131cf24d898fb8a7b24ef15580c46fd57c3a5115f8e19c6e");
const RUST_VIEW_TAG = 0xa5;
const RUST_STEALTH = "Ac8MM66HM2tVVPkLZVag7h2XzCeHRCm912yeMmSy5RqV";

describe("slnt TS SDK", () => {
  it("derives the same meta-address as the Rust reference", () => {
    const keys = deriveStealthKeysFromSignature(SIG_77);
    expect(encodeMetaAddress(metaFromKeys(keys))).to.equal(RUST_META);
  });

  it("recovers the Rust stealth address from its R + view tag (cross-impl)", () => {
    const keys = deriveStealthKeysFromSignature(SIG_77);
    const matches = scanNoteCandidates(keys, RUST_R, RUST_VIEW_TAG);
    expect(matches.length).to.be.greaterThan(0);
    expect(matches[0].stealthAddress).to.equal(RUST_STEALTH);
    expect(matches[0].labelIndex).to.equal(0);
  });

  it("canonical message embeds the network verbatim", () => {
    expect(canonicalMessage("Devnet")).to.contain("Network: Devnet");
    expect(canonicalMessage("Mainnet").endsWith("ability.")).to.equal(true);
  });

  it("meta-address bech32m round-trips", () => {
    const keys = deriveStealthKeysFromSignature(SIG_77);
    const meta = metaFromKeys(keys);
    const decoded = decodeMetaAddress(encodeMetaAddress(meta));
    expect(decoded.version).to.equal(meta.version);
    expect([...decoded.bSpend]).to.deep.equal([...meta.bSpend]);
    expect([...decoded.bScan]).to.deep.equal([...meta.bScan]);
    expect(decoded.labelIndex).to.equal(0);
  });

  it("sender↔recipient round-trips locally", () => {
    const keys = deriveStealthKeysFromSignature(hexToBytes("0a".repeat(64)));
    const meta = metaFromKeys(keys);
    const r = hexToBytes("0b".repeat(32));
    const payment = derivePayment(meta, r);

    const matches = scanNoteCandidates(keys, payment.ephemeralPub, payment.viewTag);
    expect(matches[0].stealthAddress).to.equal(payment.stealthAddress);
    expect(viewTagMatches(keys.bScanRaw, payment.ephemeralPub, payment.viewTag)).to.equal(true);
  });

  it("labeled payment is recovered only with the known label index", () => {
    const keys = deriveStealthKeysFromSignature(hexToBytes("0c".repeat(64)));
    const labelIndex = 7;
    const meta = metaForLabel(keys, labelIndex);
    const r = hexToBytes("0d".repeat(32));
    const payment = derivePayment(meta, r);

    const withLabel = scanNoteCandidates(keys, payment.ephemeralPub, payment.viewTag, [labelIndex]);
    const hit = withLabel.find((m) => m.stealthAddress === payment.stealthAddress);
    expect(hit, "labeled candidate should match").to.not.equal(undefined);
    expect(hit!.labelIndex).to.equal(labelIndex);
  });
});
