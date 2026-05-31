import { expect } from "chai";
import { base58, base64 } from "@scure/base";
import {
  MAX_METADATA_LEN,
  SELF_ANNOUNCE_TIMEOUT_MS,
  Announcement,
  announcementFromPayment,
  shouldSelfAnnounce,
  dedupByEphemeralPub,
  announceRequestFromAnnouncement,
  AnnounceClient,
} from "../src/announce";
import { notesFromLogLines } from "../src/scan";
import { NoteEvent, NOTE_EVENT_DISCRIMINATOR } from "../src/pinboard";
import { ByteWriter } from "../src/anchor";
import { SCHEME_ID_V1 } from "../src/keys";
import { StealthPayment } from "../src/sender";

function samplePayment(rByte: number, tag: number): StealthPayment {
  const ephemeralPub = new Uint8Array(32).fill(rByte);
  return {
    stealthAddress: "unused",
    stealthBytes: new Uint8Array(32),
    ephemeralPub,
    viewTag: tag,
  };
}

/** Build a synthetic `Program data:` log line carrying a NoteEvent. */
function noteLogLine(note: NoteEvent): string {
  const w = new ByteWriter();
  w.bytes(NOTE_EVENT_DISCRIMINATOR);
  w.u16LE(note.schemeId).bytes(note.ephemeralPub).u8(note.viewTag).vecU8(note.metadata);
  return `Program data: ${base64.encode(w.toBytes())}`;
}

describe("announce", () => {
  describe("announcementFromPayment", () => {
    it("carries ephemeralPub + viewTag and uses SCHEME_ID_V1", () => {
      const p = samplePayment(3, 0xa5);
      const a = announcementFromPayment(p);
      expect([...a.ephemeralPub]).to.deep.equal([...p.ephemeralPub]);
      expect(a.viewTag).to.equal(0xa5);
      expect(a.schemeId).to.equal(SCHEME_ID_V1);
      expect(a.metadata.length).to.equal(0);
    });

    it("accepts metadata at the limit and rejects 65 bytes (MetadataTooLong)", () => {
      const p = samplePayment(3, 0x11);
      expect(() => announcementFromPayment(p, new Uint8Array(MAX_METADATA_LEN))).to.not.throw();
      let caught: any;
      try {
        announcementFromPayment(p, new Uint8Array(65));
      } catch (e) {
        caught = e;
      }
      expect(caught).to.not.equal(undefined);
      expect(caught.code).to.equal("MetadataTooLong");
    });
  });

  describe("shouldSelfAnnounce", () => {
    const T = SELF_ANNOUNCE_TIMEOUT_MS;
    const table: Array<[boolean, number, boolean]> = [
      // matchingNoteSeen, elapsedMs, expected
      [true, T, false], // seen on pinboard → never self-announce
      [false, 30_000, false], // not seen, timer not elapsed → wait
      [false, T, true], // not seen, timer elapsed → self-announce
    ];
    for (const [seen, elapsed, expected] of table) {
      it(`seen=${seen} elapsed=${elapsed} → ${expected}`, () => {
        expect(shouldSelfAnnounce(seen, elapsed, T)).to.equal(expected);
      });
    }
  });

  describe("dedupByEphemeralPub", () => {
    it("collapses duplicates by ephemeralPub, preserving first-seen order", () => {
      const a = announcementFromPayment(samplePayment(3, 0x11));
      const b = announcementFromPayment(samplePayment(7, 0x22));
      const deduped = dedupByEphemeralPub([a, a, b, a]);
      expect(deduped.length).to.equal(2);
      expect([...deduped[0].ephemeralPub]).to.deep.equal([...a.ephemeralPub]);
      expect([...deduped[1].ephemeralPub]).to.deep.equal([...b.ephemeralPub]);
    });
  });

  describe("announceRequestFromAnnouncement", () => {
    it("base58-encodes ephemeral_pub so it decodes back to R", () => {
      const a = announcementFromPayment(samplePayment(3, 0xa5), new Uint8Array([9, 9]));
      const req = announceRequestFromAnnouncement(a, "proof");
      expect([...base58.decode(req.ephemeral_pub)]).to.deep.equal([...a.ephemeralPub]);
      expect([...base58.decode(req.metadata)]).to.deep.equal([...a.metadata]);
      expect(req.scheme_id).to.equal(a.schemeId);
      expect(req.view_tag).to.equal(a.viewTag);
      expect(req.payment_proof).to.equal("proof");
    });

    it("JSON round-trips", () => {
      const a = announcementFromPayment(samplePayment(3, 0xa5), new Uint8Array([1, 2, 3]));
      const req = announceRequestFromAnnouncement(a, "proof");
      const back = JSON.parse(JSON.stringify(req));
      expect(back).to.deep.equal(req);
    });

    it("omits payment_proof when undefined", () => {
      const a = announcementFromPayment(samplePayment(3, 0xa5));
      const req = announceRequestFromAnnouncement(a);
      expect("payment_proof" in req).to.equal(false);
      expect(JSON.stringify(req).includes("payment_proof")).to.equal(false);
    });
  });

  describe("AnnounceClient URL construction", () => {
    it("joins endpoints with a trailing slash on the base URL", () => {
      const c = new AnnounceClient("https://svc.example.com/");
      expect(c.announceUrl()).to.equal("https://svc.example.com/announce");
      expect(c.statusUrl("batch-7")).to.equal(
        "https://svc.example.com/announce/status/batch-7",
      );
    });

    it("joins endpoints without a trailing slash on the base URL", () => {
      const c = new AnnounceClient("http://localhost:8080");
      expect(c.announceUrl()).to.equal("http://localhost:8080/announce");
      expect(c.statusUrl("b1")).to.equal("http://localhost:8080/announce/status/b1");
    });
  });
});

describe("scan", () => {
  describe("notesFromLogLines", () => {
    it("extracts exactly the Note events from mixed log lines", () => {
      const note: NoteEvent = {
        schemeId: 1,
        ephemeralPub: new Uint8Array(32).fill(3),
        viewTag: 0x55,
        metadata: new Uint8Array([1, 2]),
      };
      const lines = [
        "Program log: instruction post",
        noteLogLine(note),
        "Program consumed 1234 of 200000 compute units",
      ];
      const notes = notesFromLogLines(lines);
      expect(notes.length).to.equal(1);
      expect(notes[0].schemeId).to.equal(note.schemeId);
      expect(notes[0].viewTag).to.equal(note.viewTag);
      expect([...notes[0].ephemeralPub]).to.deep.equal([...note.ephemeralPub]);
      expect([...notes[0].metadata]).to.deep.equal([...note.metadata]);
    });

    it("returns no notes when there is no Program data line", () => {
      expect(notesFromLogLines(["Program log: hello"]).length).to.equal(0);
    });
  });
});
