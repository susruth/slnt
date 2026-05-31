// Announcement modes and the announcement-service protocol
// (sRFC-0042 §5.8), mirroring the Rust `slnt-sdk` `announce` and
// `announce_client` modules.
//
// In v1 the sender MUST default to decoupled mode: the asset transfer
// carries no SLNT instruction, and the announcement tuple
// `(schemeId, R, viewTag, metadata)` is published separately — ideally
// by an announcement service so the transfer stays silent on-chain.
// This module models the announcement itself, the self-announce
// fallback decision (§5.8.2), the coupled escape hatch (§5.8.3), the
// service HTTP wire types (§5.8.4) and a thin async HTTP client.

import { base58 } from "@scure/base";
import { SCHEME_ID_V1 } from "./keys";
import { SlntError } from "./errors";
import { StealthPayment } from "./sender";

/** Max announcement metadata size (§5.5.1). */
export const MAX_METADATA_LEN = 64;

/** RECOMMENDED self-announce timeout `T` (§5.8.2), in milliseconds. */
export const SELF_ANNOUNCE_TIMEOUT_MS = 60_000;

/** The announcement tuple published on pinboard (§5.4 / §5.5). */
export interface Announcement {
  schemeId: number;
  /** The sender's ephemeral X25519 public key `R` (32 bytes). */
  ephemeralPub: Uint8Array;
  viewTag: number;
  metadata: Uint8Array;
}

/**
 * Build the announcement for a derived {@link StealthPayment}, with an
 * optional opaque `metadata` blob (≤ 64 bytes, §5.5.1). Throws
 * `SlntError("MetadataTooLong")` if `metadata` exceeds the limit.
 */
export function announcementFromPayment(
  payment: StealthPayment,
  metadata: Uint8Array = new Uint8Array(0),
): Announcement {
  if (metadata.length > MAX_METADATA_LEN) {
    throw new SlntError("MetadataTooLong", `${metadata.length} bytes (max ${MAX_METADATA_LEN})`);
  }
  return {
    schemeId: SCHEME_ID_V1,
    ephemeralPub: payment.ephemeralPub,
    viewTag: payment.viewTag,
    metadata,
  };
}

/** How the announcement reaches pinboard (§5.8). */
export enum AnnounceMode {
  /** Default: a service publishes the announcement in a tx it pays for. */
  Decoupled,
  /** Escape hatch: announcement rides in the same tx as the transfer. */
  Coupled,
}

/**
 * Self-announce decision (§5.8.2). After submitting to a service the
 * wallet watches pinboard for a note with matching `R`; if none appears
 * within `T`, it MUST publish the announcement itself.
 *
 * Returns `true` when the wallet should now self-announce.
 */
export function shouldSelfAnnounce(
  matchingNoteSeen: boolean,
  elapsedMs: number,
  timeoutMs: number,
): boolean {
  return !matchingNoteSeen && elapsedMs >= timeoutMs;
}

/**
 * Deduplicate observed announcements by `R` (§5.8.2): a service+sender
 * race may publish two notes with the same ephemeral key. Preserves
 * first-seen order.
 */
export function dedupByEphemeralPub(announcements: Announcement[]): Announcement[] {
  const seen = new Set<string>();
  const out: Announcement[] = [];
  for (const a of announcements) {
    const key = base58.encode(a.ephemeralPub);
    if (!seen.has(key)) {
      seen.add(key);
      out.push(a);
    }
  }
  return out;
}

// ---- Announcement-service HTTP protocol (§5.8.4) ----

/** `POST /announce` request body. Binary fields are base58 strings. */
export interface AnnounceRequest {
  scheme_id: number;
  /** `R`, base58-encoded. */
  ephemeral_pub: string;
  view_tag: number;
  /** `metadata`, base58-encoded (empty string if none). */
  metadata: string;
  payment_proof?: string;
}

/** `POST /announce` response body. */
export interface AnnounceResponse {
  queued: boolean;
  batch_id: string;
  expected_slot: number;
}

/** `GET /announce/status/{batchId}` response body. */
export interface AnnounceStatus {
  /** `pending` | `confirmed` | `failed`. */
  status: string;
  tx_signature?: string;
}

/** Build the wire request for an announcement (base58-encodes binary fields). */
export function announceRequestFromAnnouncement(
  a: Announcement,
  paymentProof?: string,
): AnnounceRequest {
  const req: AnnounceRequest = {
    scheme_id: a.schemeId,
    ephemeral_pub: base58.encode(a.ephemeralPub),
    view_tag: a.viewTag,
    metadata: base58.encode(a.metadata),
  };
  // Mirror serde `skip_serializing_if = "Option::is_none"`: omit when absent.
  if (paymentProof !== undefined) {
    req.payment_proof = paymentProof;
  }
  return req;
}

function trimTrailingSlashes(s: string): string {
  return s.replace(/\/+$/, "");
}

/**
 * Client for a single announcement-service base URL (§5.8.4). URL
 * construction is split out so it can be unit-tested without a server;
 * `submit`/`status` require a running service.
 */
export class AnnounceClient {
  private readonly baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = trimTrailingSlashes(baseUrl);
  }

  /** `POST {base}/announce` endpoint URL. */
  announceUrl(): string {
    return `${this.baseUrl}/announce`;
  }

  /** `GET {base}/announce/status/{batchId}` endpoint URL. */
  statusUrl(batchId: string): string {
    return `${this.baseUrl}/announce/status/${batchId}`;
  }

  /** Submit an announcement for decoupled publishing (§5.8.1). */
  async submit(req: AnnounceRequest): Promise<AnnounceResponse> {
    const resp = await fetch(this.announceUrl(), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(req),
    });
    if (!resp.ok) {
      throw new SlntError("Rpc", `POST /announce: HTTP ${resp.status}`);
    }
    return (await resp.json()) as AnnounceResponse;
  }

  /** Poll the status of a previously-submitted batch. */
  async status(batchId: string): Promise<AnnounceStatus> {
    const resp = await fetch(this.statusUrl(batchId), { method: "GET" });
    if (!resp.ok) {
      throw new SlntError("Rpc", `GET /announce/status: HTTP ${resp.status}`);
    }
    return (await resp.json()) as AnnounceStatus;
  }
}
