//! Build instructions and parse events for the pinboard program.
//!
//! We don't depend on `anchor-client` (heavy, version-coupled). We
//! hand-build the instruction using Anchor's 8-byte discriminator
//! (`SHA-256("global:<instruction_snake>")[..8]`) followed by borsh-
//! serialized args. For events, the on-chain logs contain
//! `Program data: <base64>` where the payload is
//! `SHA-256("event:<EventName>")[..8] || borsh(event)`.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

/// `SHA-256("global:post")[..8]`. Verified against `target/idl/pinboard.json`.
pub const POST_DISCRIMINATOR: [u8; 8] = [223, 96, 234, 236, 158, 106, 145, 94];

/// `SHA-256("global:post_batch")[..8]`.
pub const POST_BATCH_DISCRIMINATOR: [u8; 8] = [172, 123, 234, 102, 14, 213, 76, 36];

/// `SHA-256("event:Note")[..8]`.
pub const NOTE_EVENT_DISCRIMINATOR: [u8; 8] = [40, 182, 5, 151, 115, 43, 27, 97];

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PostArgs {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

/// One entry of a `post_batch` call. Mirrors `NoteEntry` in
/// `programs/pinboard/src/lib.rs`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NoteEntry {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

/// On-chain `Note` event, matching `programs/pinboard/src/lib.rs`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NoteEvent {
    pub scheme_id: u16,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u8,
    pub metadata: Vec<u8>,
}

/// Build a `pinboard.post(...)` instruction.
pub fn build_post_instruction(
    pinboard_program_id: &Pubkey,
    fee_payer: &Pubkey,
    scheme_id: u16,
    ephemeral_pub: [u8; 32],
    view_tag: u8,
    metadata: Vec<u8>,
) -> Instruction {
    let args = PostArgs {
        scheme_id,
        ephemeral_pub,
        view_tag,
        metadata,
    };
    let mut data = Vec::with_capacity(8 + 2 + 32 + 1 + 4 + args.metadata.len());
    data.extend_from_slice(&POST_DISCRIMINATOR);
    borsh::to_writer(&mut data, &args).expect("borsh serialize PostArgs");
    Instruction {
        program_id: *pinboard_program_id,
        accounts: vec![AccountMeta::new(*fee_payer, true)],
        data,
    }
}

/// Build a `pinboard.post_batch(...)` instruction (sRFC-0042 §5.5.1).
///
/// `entries` must be non-empty (the program rejects an empty batch);
/// practical size is bounded by the transaction compute budget.
pub fn build_post_batch_instruction(
    pinboard_program_id: &Pubkey,
    fee_payer: &Pubkey,
    entries: Vec<NoteEntry>,
) -> Instruction {
    let mut data = Vec::with_capacity(8 + 4 + entries.len() * 40);
    data.extend_from_slice(&POST_BATCH_DISCRIMINATOR);
    borsh::to_writer(&mut data, &entries).expect("borsh serialize Vec<NoteEntry>");
    Instruction {
        program_id: *pinboard_program_id,
        accounts: vec![AccountMeta::new(*fee_payer, true)],
        data,
    }
}

/// Parse a `Program data: <base64>` log line into a `NoteEvent`.
///
/// Returns `Ok(None)` if the line is not a `Program data:` line or if
/// the discriminator doesn't match `Note`. Returns `Err` if the line
/// looks like a Note event but fails to deserialize.
pub fn try_parse_note_log(line: &str) -> Result<Option<NoteEvent>, String> {
    const PREFIX: &str = "Program data: ";
    let Some(b64) = line.strip_prefix(PREFIX) else {
        return Ok(None);
    };
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("base64 decode: {e}"))?;
    if raw.len() < 8 {
        return Ok(None);
    }
    if raw[..8] != NOTE_EVENT_DISCRIMINATOR {
        return Ok(None);
    }
    let event = NoteEvent::try_from_slice(&raw[8..])
        .map_err(|e| format!("borsh deserialize NoteEvent: {e}"))?;
    Ok(Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn post_discriminator_matches_anchor_convention() {
        let mut h = Sha256::new();
        h.update(b"global:post");
        let computed = h.finalize();
        assert_eq!(&computed[..8], &POST_DISCRIMINATOR);
    }

    #[test]
    fn note_event_discriminator_matches_anchor_convention() {
        let mut h = Sha256::new();
        h.update(b"event:Note");
        let computed = h.finalize();
        assert_eq!(&computed[..8], &NOTE_EVENT_DISCRIMINATOR);
    }

    #[test]
    fn post_batch_discriminator_matches_anchor_convention() {
        let mut h = Sha256::new();
        h.update(b"global:post_batch");
        let computed = h.finalize();
        assert_eq!(&computed[..8], &POST_BATCH_DISCRIMINATOR);
    }

    #[test]
    fn build_post_batch_roundtrips_entries() {
        let program = Pubkey::new_unique();
        let fee_payer = Pubkey::new_unique();
        let entries = vec![
            NoteEntry {
                scheme_id: 1,
                ephemeral_pub: [1u8; 32],
                view_tag: 0x11,
                metadata: vec![],
            },
            NoteEntry {
                scheme_id: 1,
                ephemeral_pub: [2u8; 32],
                view_tag: 0x22,
                metadata: vec![9, 9],
            },
        ];
        let ix = build_post_batch_instruction(&program, &fee_payer, entries.clone());

        assert_eq!(ix.program_id, program);
        assert_eq!(ix.accounts.len(), 1);
        assert!(ix.accounts[0].is_signer);
        assert_eq!(&ix.data[..8], &POST_BATCH_DISCRIMINATOR);

        // The borsh body must decode back to the same Vec<NoteEntry>.
        let decoded: Vec<NoteEntry> = BorshDeserialize::try_from_slice(&ix.data[8..]).unwrap();
        assert_eq!(decoded, entries);
    }

    #[test]
    fn note_event_roundtrip_through_log() {
        let original = NoteEvent {
            scheme_id: 1,
            ephemeral_pub: [7u8; 32],
            view_tag: 0x42,
            metadata: vec![0xab, 0xcd],
        };
        // Build a synthetic Program data line.
        let mut payload = Vec::new();
        payload.extend_from_slice(&NOTE_EVENT_DISCRIMINATOR);
        borsh::to_writer(&mut payload, &original).unwrap();
        use base64::Engine;
        let line = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(&payload),
        );
        let parsed = try_parse_note_log(&line).unwrap().unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn non_program_data_line_is_none() {
        let result = try_parse_note_log("Program log: hello").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn other_event_discriminator_is_none() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8; 8]); // wrong discriminator
        payload.extend_from_slice(&[1, 2, 3, 4]);
        use base64::Engine;
        let line = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(&payload),
        );
        assert!(try_parse_note_log(&line).unwrap().is_none());
    }
}
