//! Live pinboard scanning via `logsSubscribe` (sRFC-0042 §5.10).
//!
//! The REQUIRED baseline: subscribe to pinboard program logs over a
//! websocket and parse `Note` events as they stream in. Backfill for
//! offline gaps is done separately via `getSignaturesForAddress` +
//! `getTransaction` (see the lifecycle example).
//!
//! Enabled by the `net` feature.

use crate::error::SlntError;
use crate::pinboard::{try_parse_note_log, NoteEvent};
use futures_util::StreamExt;
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_client::rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};

/// Parse all `Note` events out of one transaction's log lines.
/// Non-Note lines are ignored; malformed Note lines are skipped.
pub fn notes_from_log_lines(lines: &[String]) -> Vec<NoteEvent> {
    lines
        .iter()
        .filter_map(|l| try_parse_note_log(l).ok().flatten())
        .collect()
}

/// Subscribe to pinboard program logs at `ws_url` and invoke `on_note`
/// for every `Note` event observed, until the stream ends.
///
/// `on_note` runs the recipient-local scan (e.g. `scan_note` /
/// `scan_note_candidates`); this function performs no key operations and
/// learns nothing about which notes matched.
pub async fn subscribe_pinboard_notes<F>(
    ws_url: &str,
    pinboard_program_id: &Pubkey,
    mut on_note: F,
) -> Result<(), SlntError>
where
    F: FnMut(NoteEvent),
{
    subscribe_pinboard_notes_with_slot(ws_url, pinboard_program_id, move |_slot, note| {
        on_note(note)
    })
    .await
}

/// Like [`subscribe_pinboard_notes`] but also passes the confirmation
/// `slot` of each note, for indexers that serve announcements by slot
/// range (§5.10).
pub async fn subscribe_pinboard_notes_with_slot<F>(
    ws_url: &str,
    pinboard_program_id: &Pubkey,
    mut on_note: F,
) -> Result<(), SlntError>
where
    F: FnMut(u64, NoteEvent),
{
    let client = PubsubClient::new(ws_url)
        .await
        .map_err(|e| SlntError::Rpc(format!("pubsub connect: {e}")))?;

    let filter = RpcTransactionLogsFilter::Mentions(vec![pinboard_program_id.to_string()]);
    let config = RpcTransactionLogsConfig {
        commitment: Some(CommitmentConfig::confirmed()),
    };

    let (mut stream, _unsubscribe) = client
        .logs_subscribe(filter, config)
        .await
        .map_err(|e| SlntError::Rpc(format!("logs_subscribe: {e}")))?;

    while let Some(log) = stream.next().await {
        let slot = log.context.slot;
        for note in notes_from_log_lines(&log.value.logs) {
            on_note(slot, note);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pinboard::NOTE_EVENT_DISCRIMINATOR;
    use base64::Engine;

    fn note_log_line(note: &NoteEvent) -> String {
        let mut payload = Vec::new();
        payload.extend_from_slice(&NOTE_EVENT_DISCRIMINATOR);
        borsh::to_writer(&mut payload, note).unwrap();
        format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(&payload)
        )
    }

    #[test]
    fn extracts_only_note_events_from_mixed_lines() {
        let note = NoteEvent {
            scheme_id: 1,
            ephemeral_pub: [3u8; 32],
            view_tag: 0x55,
            metadata: vec![1, 2],
        };
        let lines = vec![
            "Program log: instruction post".to_string(),
            note_log_line(&note),
            "Program consumed 1234 of 200000 compute units".to_string(),
        ];
        let notes = notes_from_log_lines(&lines);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0], note);
    }

    #[test]
    fn no_notes_when_no_program_data() {
        let lines = vec!["Program log: hello".to_string()];
        assert!(notes_from_log_lines(&lines).is_empty());
    }
}
