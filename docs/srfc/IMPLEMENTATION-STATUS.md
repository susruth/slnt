# SLNT v1 — Implementation Status

Tracks the gap between **sRFC-0042** (`docs/srfc/0001-slnt-silent-payments.md`,
normative) and the reference implementation in this repo.

Legend: ✅ done · 🟡 partial / divergent · ⬜ not started · 🔵 future-reserved (spec marks as future)

Conformance priority: items marked **MUST/SHOULD/default** in the sRFC are required
for a conforming v1 implementation; **OPTIONAL** items are enhancements.

---

## Already canonical (complete)

- ✅ Pinboard program: `post`, `post_batch`, `Note` event (§5.5)
- ✅ Registry program: `register` / `update` / `close` + events (§5.6)
- ✅ Method 2 key derivation — signed canonical message (§5.2.1.2)
- ✅ Spend/scan key mapping (§5.2.1.3)
- ✅ bech32m meta-address encode/decode (§5.2.2)
- ✅ Sender stealth-address derivation (§5.3)
- ✅ Recipient scan + spend-scalar reconstruction — unlabeled (§5.4)
- ✅ Scalar-mode Ed25519 stealth signing (§5.9 spend path)
- ✅ Registry read helpers — PDA derive, parse, fetch (§5.6)
- ✅ Pinboard event parsing — `try_parse_note_log` (§5.5.2)

---

## Gap checklist

### Phase A — Crypto / codec foundations
- [x] **A1** Network-parameterized canonical message: `Network` enum + `canonical_message()` for Mainnet/Devnet/Testnet (§5.2.1.2) ✅
- [x] **A2** Method 1 — wallet-native HD derivation, SLIP-0010 ed25519, `m/0x534C4E54'/501'/account'/{0',1'}` (§5.2.1.1); verified against official SLIP-0010 ed25519 vector ✅
- [x] **A3** Labels end-to-end: `label_tweak_scalar`, `MetaAddress::for_label`, `scan_note_candidates` over known label indices (§5.2.3, §5.4) ✅
- [x] **A4** Meta-address / ECDH hardening: reject non-v1 flags, non-prime-order Ed25519 spend keys, and all-zero X25519 shared secrets (§5.2.2, §5.3, §5.4, §8.4) ✅
- [x] **A5** Public conformance vectors: `test-vectors.json` covers Method 1, Method 2, labels, sender derivation, recipient scanning, pinboard bytes, registry bytes, and invalid hardening cases ✅

### Phase B — SDK instruction builders
- [x] **B1** `build_post_batch_instruction` + `NoteEntry` (§5.5.1) ✅
- [x] **B2** `build_register_instruction` / `build_update_instruction` / `build_close_instruction` + `MetaAddressPayload` (§5.6.2) ✅

### Phase C — Transaction flows (`flows.rs`, `sweep.rs`)
- [x] **C1** `build_sol_payment` (checked amount + rent buffer), decoupled (§5.7) ✅
- [x] **C2** `build_spl_payment`: idempotent ATA create + `transfer_checked` (SPL + Token-2022 via program id) (§5.7) ✅
- [x] **C3** `build_nft_payment`: amount=1/decimals=0 case (§5.7) ✅ — *pNFT token-record/rule-set accounts noted as `mpl-token-metadata` follow-up*
- [x] **C4** `build_sol_sweep` + `build_spl_sweep` (relayer fee-payer; `transfer_checked` + `CloseAccount`) (§5.9) ✅
- [x] **C5** Close-to-relayer enforcement — `ensure_not_main_wallet`, rejects close/dest = main wallet (§5.9, §8.3) **MUST** ✅
- [x] **C6** Stealth-to-stealth sweep — same builder, stealth destination accepted (§5.9) ✅

### Phase D — Announcement modes & discovery (`announce.rs`)
- [x] **D1** Decoupled announce: `Announcement`, `AnnounceMode::Decoupled`, `AnnounceRequest::from_announcement` (§5.8.1) ✅ *(pure construction; networked submission = D4)*
- [x] **D2** Self-announce fallback decision: `should_self_announce` (T=60s), `dedup_by_ephemeral_pub` (§5.8.2) ✅ *(async watch loop driven by caller)*
- [x] **D3** Coupled escape hatch: `AnnounceMode::Coupled` + `Announcement::to_post_instruction` (§5.8.3) ✅
- [x] **D4** Announcement-service HTTP client `AnnounceClient` (submit/status) + wire types, behind `net` feature (§5.8.4) ✅
- [x] **D5** `logsSubscribe` self-scan: `subscribe_pinboard_notes` + `notes_from_log_lines`, behind `net` feature (§5.10) ✅
- [x] **D6** Determinism guard: `derive_stealth_keys_checked` rejects randomized signers (§8.5) ✅

### Phase E — Larger deliverables (separate artifacts)
- [x] **E1** Reference indexer (`crates/slnt-indexer`): axum server, `GET /announcements?since_slot&limit`, `logsSubscribe`-fed in-memory store (§5.10) ✅
- [x] **E2** View-key delegated scanning primitive: `ScanKey::from_raw` + `view_tag_matches` (§5.10, OPTIONAL) ✅
- [x] **E3** Reference announcement service (`crates/slnt-announcer`): axum `POST /announce` + `GET /announce/status/{id}`, queue, `post_batch` publisher (§5.8.4) ✅
- [x] **E4** TypeScript wallet SDK (`clients/typescript`, `@slnt/sdk`): **feature-equivalent with the Rust SDK** — Method 1 HD + Method 2 + determinism guard, codec, labels, hardened sender/recipient, scalar-mode signing, pinboard/registry builders, SOL/SPL/NFT flows, sweep + close-to-relayer, announce + HTTP client, `onLogs` scan; cross-impl KATs (meta-address, stealth recovery, HD) vs Rust CLI pass; 54 TS tests (§9) ✅
- [x] **E5** `slnt` CLI offline commands: canonical message, derive, derive-hd, label, meta-decode, pay (§9) ✅ *(on-chain send/register/sweep commands remain future CLI scope)*
- [ ] 🔵 **E6** `register_on_behalf` gasless registration (§7) — spec-reserved, future

### Cross-cutting
- [x] **X1** SDK + pinboard program doc-comments now cite sRFC-0042 §5.x numbering ✅

---

## Progress log

- **2026-05-31** — Phases A, B, C complete; D core complete, including HTTP client wire-up and `logsSubscribe` helpers behind the `net` feature. SDK test count 27 → 69, all green.
- **2026-05-31 (cont.)** — All of Phase E delivered (except spec-reserved E6): `slnt` CLI, `slnt-indexer` (axum + logsSubscribe), `slnt-announcer` (axum + post_batch publisher), and the `@slnt/sdk` TypeScript wallet SDK with a cross-impl known-answer test proving byte-compatibility with the Rust reference. Full workspace green: **73** SDK (net) + 6 CLI + 5 service + 4 indexer + program tests, plus **6** TS tests. Only E6 (`register_on_behalf`, spec-reserved future) remains.
- **2026-05-31 (hardening + TS parity)** — Repo-wide hardening: HD seed-length bound (`InvalidSeedLength`), pinboard `MAX_BATCH_ENTRIES` cap (`BatchTooLarge`), announcer bounded pending-queue + status history (`QueueFull`) and poison-resilient locks + graceful keypair load, indexer bounded retention store (drop-oldest + `dropped` counter on `/health`) + poison-resilient locks; clippy clean. **TypeScript SDK brought to full feature parity** with the Rust SDK (HD derivation, determinism guard, hardened sender/recipient, scalar-mode signing, pinboard/registry builders, flows, sweep, announce + client, scan stream) on `@solana/web3.js` + `@solana/spl-token`. Counts: Rust **74** SDK + 7 announcer + 5 indexer + 6 CLI + program tests; **54** TS tests; cross-impl HD KAT added.
