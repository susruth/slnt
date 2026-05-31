// Live pinboard scanning via `logsSubscribe` (sRFC-0042 §5.10),
// mirroring the Rust `slnt-sdk` `scan_stream` module.
//
// The REQUIRED baseline: subscribe to pinboard program logs over a
// websocket and parse `Note` events as they stream in. Backfill for
// offline gaps is done separately via `getSignaturesForAddress` +
// `getTransaction` (see the lifecycle example).

import { Connection, PublicKey } from "@solana/web3.js";
import { tryParseNoteLog, NoteEvent } from "./pinboard";

/**
 * Parse all `Note` events out of one transaction's log lines. Non-Note
 * lines are ignored; malformed Note lines are skipped.
 */
export function notesFromLogLines(lines: string[]): NoteEvent[] {
  const out: NoteEvent[] = [];
  for (const line of lines) {
    const note = tryParseNoteLog(line);
    if (note !== null) {
      out.push(note);
    }
  }
  return out;
}

/**
 * Subscribe to pinboard program logs and invoke `onNote` for every
 * `Note` event observed. Returns the `onLogs` subscription id (pass to
 * `connection.removeOnLogsListener` to unsubscribe).
 *
 * `onNote` runs the recipient-local scan (e.g. `scanNoteCandidates`);
 * this function performs no key operations and learns nothing about
 * which notes matched.
 */
export async function subscribePinboardNotes(
  connection: Connection,
  pinboardProgramId: PublicKey,
  onNote: (note: NoteEvent) => void,
): Promise<number> {
  return connection.onLogs(
    pinboardProgramId,
    (logs) => {
      for (const note of notesFromLogLines(logs.logs)) {
        onNote(note);
      }
    },
    "confirmed",
  );
}

/**
 * Like {@link subscribePinboardNotes} but also passes the confirmation
 * `slot` of each note, for indexers that serve announcements by slot
 * range (§5.10).
 */
export async function subscribePinboardNotesWithSlot(
  connection: Connection,
  pinboardProgramId: PublicKey,
  onNote: (slot: number, note: NoteEvent) => void,
): Promise<number> {
  return connection.onLogs(
    pinboardProgramId,
    (logs, ctx) => {
      for (const note of notesFromLogLines(logs.logs)) {
        onNote(ctx.slot, note);
      }
    },
    "confirmed",
  );
}
