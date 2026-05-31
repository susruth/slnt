import { expect } from "chai";
import { hexToBytes } from "@noble/hashes/utils";
import { ed25519 } from "@noble/curves/ed25519";
import {
  deriveStealthKeysFromSignature,
  deriveStealthKeysChecked,
  deriveStealthKeysHd,
  encodeMetaAddress,
  metaFromKeys,
  metaForLabel,
  SlntError,
} from "../src/index";
import { derivePayment } from "../src/sender";
import { StealthSigningKey } from "../src/signing";
import { scanNoteCandidates } from "../src/recipient";

describe("slnt TS SDK — crypto core parity", () => {
  describe("Method 1 HD derivation (SLIP-0010)", () => {
    // Known-answer vectors from the Rust `slnt` CLI:
    //   slnt derive-hd --seed 00..00 --account {0,1}
    const SEED = hexToBytes("00".repeat(64));

    it("matches the Rust reference for account 0", () => {
      const keys = deriveStealthKeysHd(SEED, 0);
      expect(encodeMetaAddress(metaFromKeys(keys))).to.equal(
        "slnt1qxz5pwgpxgdpqdsnxu6plfanrw3acgxg544ygryehm5ckqp28uc0qrjcchl8sw2mpdzk5mq0m3p3dnrf6fg342myvcksxzstpncx8dfzqqqqm493l5",
      );
    });

    it("matches the Rust reference for account 1", () => {
      const keys = deriveStealthKeysHd(SEED, 1);
      expect(encodeMetaAddress(metaFromKeys(keys))).to.equal(
        "slnt1qx285wxpy6z5hesuywh8g3cqer0rydldfr8hhdwswjpmkkacmhm836m3d652qcdarqjld4u5m2dk7dzvjlcdknydskhk789cufchn5zzqqqqs75yg9",
      );
    });

    it("rejects out-of-range seed lengths", () => {
      expect(() => deriveStealthKeysHd(new Uint8Array(15), 0)).to.throw(SlntError);
      expect(() => deriveStealthKeysHd(new Uint8Array(65), 0)).to.throw(SlntError);
      expect(() => deriveStealthKeysHd(new Uint8Array(16), 0)).to.not.throw();
    });

    it("HD and signed-message methods yield distinct identities", () => {
      const hd = deriveStealthKeysHd(SEED, 0);
      const sig = deriveStealthKeysFromSignature(hexToBytes("00".repeat(64)));
      expect(encodeMetaAddress(metaFromKeys(hd))).to.not.equal(
        encodeMetaAddress(metaFromKeys(sig)),
      );
    });
  });

  describe("determinism guard", () => {
    it("rejects non-deterministic signatures", () => {
      const a = hexToBytes("11".repeat(64));
      const b = hexToBytes("11".repeat(63) + "12");
      expect(() => deriveStealthKeysChecked(a, b))
        .to.throw(SlntError)
        .with.property("code", "NonDeterministicSignature");
    });

    it("accepts matching signatures", () => {
      const a = hexToBytes("11".repeat(64));
      const checked = deriveStealthKeysChecked(a, a);
      const plain = deriveStealthKeysFromSignature(a);
      expect([...checked.BSpend]).to.deep.equal([...plain.BSpend]);
    });
  });

  describe("sender hardening", () => {
    const keys = deriveStealthKeysFromSignature(hexToBytes("22".repeat(64)));
    const meta = metaFromKeys(keys);
    const r = hexToBytes("33".repeat(32));

    it("rejects unsupported version", () => {
      expect(() => derivePayment({ ...meta, version: 2 }, r))
        .to.throw(SlntError)
        .with.property("code", "UnsupportedVersion");
    });

    it("rejects nonzero flags", () => {
      expect(() => derivePayment({ ...meta, flags: 1 }, r))
        .to.throw(SlntError)
        .with.property("code", "UnsupportedFlags");
    });

    it("rejects a small-order spend point", () => {
      // The Ed25519 identity point (compressed) is small-order.
      const identity = ed25519.ExtendedPoint.ZERO.toRawBytes();
      expect(() => derivePayment({ ...meta, bSpend: identity }, r))
        .to.throw(SlntError)
        .with.property("code", "InvalidPoint");
    });

    it("rejects an all-zero shared secret (low-order scan key)", () => {
      expect(() => derivePayment({ ...meta, bScan: new Uint8Array(32) }, r))
        .to.throw(SlntError)
        .with.property("code", "InvalidSharedSecret");
    });
  });

  describe("scalar-mode stealth signing", () => {
    it("produces signatures that verify against a standard Ed25519 verifier", () => {
      // Recover a real stealth scalar via the sender→recipient flow.
      const keys = deriveStealthKeysFromSignature(hexToBytes("44".repeat(64)));
      const payment = derivePayment(metaFromKeys(keys), hexToBytes("55".repeat(32)));
      const match = scanNoteCandidates(keys, payment.ephemeralPub, payment.viewTag)[0];

      const sk = new StealthSigningKey(match.stealthScalar);
      // The signing key's public bytes equal the stealth address bytes.
      expect([...sk.publicKey]).to.deep.equal([...payment.stealthBytes]);

      const msg = new TextEncoder().encode("a stealth sweep tx");
      const sig = sk.sign(msg);
      expect(ed25519.verify(sig, msg, sk.publicKey)).to.equal(true);
    });

    it("is deterministic for a fixed scalar", () => {
      const sk1 = new StealthSigningKey(123456789n);
      const sk2 = new StealthSigningKey(123456789n);
      const msg = new TextEncoder().encode("twice");
      expect([...sk1.sign(msg)]).to.deep.equal([...sk2.sign(msg)]);
    });
  });

  describe("labels", () => {
    it("recovers a labeled payment only with the known label index", () => {
      const keys = deriveStealthKeysFromSignature(hexToBytes("66".repeat(64)));
      const label = 12;
      const meta = metaForLabel(keys, label);
      const payment = derivePayment(meta, hexToBytes("77".repeat(32)));
      const hit = scanNoteCandidates(keys, payment.ephemeralPub, payment.viewTag, [label]).find(
        (m) => m.stealthAddress === payment.stealthAddress,
      );
      expect(hit?.labelIndex).to.equal(label);
    });
  });
});
