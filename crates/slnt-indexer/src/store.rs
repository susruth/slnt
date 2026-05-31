//! In-memory announcement store (sRFC-0042 §5.10).
//!
//! The indexer retains observed announcements and serves them by slot
//! range. It holds **no** scan keys — matching is recipient-local — so
//! polling slot ranges leaks nothing about which announcements matched.

use serde::Serialize;
use slnt_sdk::pinboard::NoteEvent;
use std::collections::VecDeque;

/// One stored announcement, serialized to JSON for `GET /announcements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoredAnnouncement {
    pub slot: u64,
    pub scheme_id: u16,
    /// `R`, base58.
    pub ephemeral_pub: String,
    pub view_tag: u8,
    /// `metadata`, base58 (empty string if none).
    pub metadata: String,
}

impl StoredAnnouncement {
    pub fn from_note(slot: u64, note: &NoteEvent) -> Self {
        Self {
            slot,
            scheme_id: note.scheme_id,
            ephemeral_pub: bs58::encode(note.ephemeral_pub).into_string(),
            view_tag: note.view_tag,
            metadata: bs58::encode(&note.metadata).into_string(),
        }
    }
}

/// Append-only store, ordered by insertion (which tracks slot order as
/// notes stream in).
///
/// The store is bounded: once it holds `cap` announcements, inserting a
/// new one evicts the oldest (a sliding retention window). `dropped`
/// counts evictions so operators can detect when the window is too
/// small for their recipients' offline tolerance.
pub struct AnnouncementStore {
    items: VecDeque<StoredAnnouncement>,
    cap: usize,
    dropped: u64,
}

/// Default retention capacity (announcements). At ~120 bytes each this is
/// ~120 MB — a sane single-process default; production deployments back
/// this with durable storage.
pub const MAX_STORED: usize = 1_000_000;

impl Default for AnnouncementStore {
    fn default() -> Self {
        Self::with_cap(MAX_STORED)
    }
}

impl AnnouncementStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with an explicit retention cap (primarily for tests).
    pub fn with_cap(cap: usize) -> Self {
        Self {
            items: VecDeque::new(),
            cap: cap.max(1),
            dropped: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)] // companion to `len`; part of the store's public surface
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of announcements evicted by the retention cap so far.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn insert(&mut self, slot: u64, note: &NoteEvent) {
        self.items.push_back(StoredAnnouncement::from_note(slot, note));
        while self.items.len() > self.cap {
            self.items.pop_front();
            self.dropped += 1;
        }
    }

    /// Return announcements with `slot >= since_slot` (or all, if `None`),
    /// capped at `limit`.
    pub fn query(&self, since_slot: Option<u64>, limit: usize) -> Vec<StoredAnnouncement> {
        self.items
            .iter()
            .filter(|a| since_slot.is_none_or(|s| a.slot >= s))
            .take(limit)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(tag: u8) -> NoteEvent {
        NoteEvent {
            scheme_id: 1,
            ephemeral_pub: [tag; 32],
            view_tag: tag,
            metadata: vec![],
        }
    }

    #[test]
    fn store_evicts_oldest_beyond_cap() {
        let mut s = AnnouncementStore::with_cap(3);
        for slot in 0..5 {
            s.insert(slot, &note(slot as u8));
        }
        assert_eq!(s.len(), 3);
        assert_eq!(s.dropped(), 2);
        // Oldest two (slots 0,1) evicted; 2,3,4 retained.
        let all = s.query(None, 100);
        assert_eq!(all.first().unwrap().slot, 2);
        assert_eq!(all.last().unwrap().slot, 4);
    }

    #[test]
    fn query_filters_by_since_slot() {
        let mut s = AnnouncementStore::new();
        s.insert(10, &note(1));
        s.insert(20, &note(2));
        s.insert(30, &note(3));

        let from20 = s.query(Some(20), 100);
        assert_eq!(from20.len(), 2);
        assert_eq!(from20[0].slot, 20);
        assert_eq!(from20[1].slot, 30);
    }

    #[test]
    fn query_respects_limit() {
        let mut s = AnnouncementStore::new();
        for i in 0..10 {
            s.insert(i, &note(i as u8));
        }
        assert_eq!(s.query(None, 3).len(), 3);
    }

    #[test]
    fn query_none_since_returns_all() {
        let mut s = AnnouncementStore::new();
        s.insert(5, &note(1));
        assert_eq!(s.query(None, 100).len(), 1);
    }

    #[test]
    fn stored_announcement_encodes_r_as_base58() {
        let a = StoredAnnouncement::from_note(7, &note(9));
        let decoded = bs58::decode(&a.ephemeral_pub).into_vec().unwrap();
        assert_eq!(decoded, [9u8; 32]);
        assert_eq!(a.slot, 7);
    }
}
