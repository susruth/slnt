//! Core queue + conversion logic for the announcement service
//! (sRFC-0042 §5.8.4), isolated from HTTP/RPC for testing.

use slnt_sdk::announce::{AnnounceRequest, MAX_METADATA_LEN};
use slnt_sdk::pinboard::NoteEntry;
use std::collections::{HashMap, VecDeque};

/// Default cap on un-published (pending) announcements. Bounds memory if
/// the publisher stalls or the service runs collect-only.
pub const MAX_PENDING: usize = 10_000;

/// Default cap on retained batch-status records (oldest evicted first),
/// so status history does not grow without bound.
pub const MAX_STATUSES: usize = 50_000;

/// Returned by [`AnnounceQueue::enqueue`] when the pending queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFull;

/// Decode a wire `AnnounceRequest` (base58 fields) into an on-chain
/// `NoteEntry`, validating field sizes.
pub fn request_to_note_entry(req: &AnnounceRequest) -> Result<NoteEntry, String> {
    if req.scheme_id == 0 {
        return Err("scheme_id must be non-zero".into());
    }
    let r = bs58::decode(&req.ephemeral_pub)
        .into_vec()
        .map_err(|e| format!("ephemeral_pub base58: {e}"))?;
    let ephemeral_pub: [u8; 32] = r
        .try_into()
        .map_err(|_| "ephemeral_pub must be 32 bytes".to_string())?;
    let metadata = if req.metadata.is_empty() {
        Vec::new()
    } else {
        bs58::decode(&req.metadata)
            .into_vec()
            .map_err(|e| format!("metadata base58: {e}"))?
    };
    if metadata.len() > MAX_METADATA_LEN {
        return Err(format!("metadata exceeds {MAX_METADATA_LEN} bytes"));
    }
    Ok(NoteEntry {
        scheme_id: req.scheme_id,
        ephemeral_pub,
        view_tag: req.view_tag,
        metadata,
    })
}

/// Status of a submitted batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchStatus {
    Pending,
    Confirmed(String),
    Failed(String),
}

/// FIFO queue of pending announcements, each assigned a monotonically
/// increasing batch id. The publisher drains pending entries into one
/// `post_batch` transaction. The pending queue and the retained status
/// history are both bounded.
pub struct AnnounceQueue {
    pending: Vec<(u64, NoteEntry)>,
    next_id: u64,
    statuses: HashMap<u64, BatchStatus>,
    status_order: VecDeque<u64>,
    max_pending: usize,
    max_statuses: usize,
}

impl Default for AnnounceQueue {
    fn default() -> Self {
        Self::with_caps(MAX_PENDING, MAX_STATUSES)
    }
}

impl AnnounceQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with explicit caps (primarily for tests).
    pub fn with_caps(max_pending: usize, max_statuses: usize) -> Self {
        Self {
            pending: Vec::new(),
            next_id: 0,
            statuses: HashMap::new(),
            status_order: VecDeque::new(),
            max_pending: max_pending.max(1),
            max_statuses: max_statuses.max(1),
        }
    }

    /// Enqueue a note and return its batch id, or [`QueueFull`] if the
    /// pending queue is at capacity.
    pub fn enqueue(&mut self, entry: NoteEntry) -> Result<u64, QueueFull> {
        if self.pending.len() >= self.max_pending {
            return Err(QueueFull);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.pending.push((id, entry));
        self.set_status(id, BatchStatus::Pending);
        Ok(id)
    }

    /// Remove up to `max` pending entries for publishing.
    pub fn take_pending(&mut self, max: usize) -> Vec<(u64, NoteEntry)> {
        let n = max.min(self.pending.len());
        self.pending.drain(..n).collect()
    }

    pub fn set_status(&mut self, id: u64, status: BatchStatus) {
        if !self.statuses.contains_key(&id) {
            self.status_order.push_back(id);
            // Evict the oldest status records beyond the cap.
            while self.status_order.len() > self.max_statuses {
                if let Some(old) = self.status_order.pop_front() {
                    self.statuses.remove(&old);
                }
            }
        }
        self.statuses.insert(id, status);
    }

    pub fn status(&self, id: u64) -> Option<&BatchStatus> {
        self.statuses.get(&id)
    }

    #[allow(dead_code)] // used in tests and by operators inspecting queue depth
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(meta_b58: &str) -> AnnounceRequest {
        AnnounceRequest {
            scheme_id: 1,
            ephemeral_pub: bs58::encode([4u8; 32]).into_string(),
            view_tag: 0x10,
            metadata: meta_b58.to_string(),
            payment_proof: None,
        }
    }

    #[test]
    fn converts_valid_request() {
        let entry = request_to_note_entry(&req("")).unwrap();
        assert_eq!(entry.ephemeral_pub, [4u8; 32]);
        assert_eq!(entry.scheme_id, 1);
        assert!(entry.metadata.is_empty());
    }

    #[test]
    fn rejects_oversized_metadata() {
        let big = bs58::encode(vec![0u8; 65]).into_string();
        assert!(request_to_note_entry(&req(&big)).is_err());
    }

    #[test]
    fn rejects_bad_ephemeral_length() {
        let mut r = req("");
        r.ephemeral_pub = bs58::encode([1u8; 8]).into_string();
        assert!(request_to_note_entry(&r).is_err());
    }

    #[test]
    fn queue_assigns_incrementing_ids_and_tracks_status() {
        let mut q = AnnounceQueue::new();
        let e = request_to_note_entry(&req("")).unwrap();
        let id0 = q.enqueue(e.clone()).unwrap();
        let id1 = q.enqueue(e).unwrap();
        assert_eq!((id0, id1), (0, 1));
        assert_eq!(q.status(id0), Some(&BatchStatus::Pending));

        let drained = q.take_pending(10);
        assert_eq!(drained.len(), 2);
        assert_eq!(q.pending_len(), 0);

        q.set_status(id0, BatchStatus::Confirmed("sig123".into()));
        assert_eq!(q.status(id0), Some(&BatchStatus::Confirmed("sig123".into())));
    }

    #[test]
    fn enqueue_rejects_when_pending_full() {
        let mut q = AnnounceQueue::with_caps(2, 100);
        let e = request_to_note_entry(&req("")).unwrap();
        assert!(q.enqueue(e.clone()).is_ok());
        assert!(q.enqueue(e.clone()).is_ok());
        assert_eq!(q.enqueue(e.clone()), Err(QueueFull));
        // Draining frees capacity again.
        q.take_pending(1);
        assert!(q.enqueue(e).is_ok());
    }

    #[test]
    fn status_history_is_bounded() {
        let mut q = AnnounceQueue::with_caps(1_000, 3);
        let e = request_to_note_entry(&req("")).unwrap();
        let mut ids = Vec::new();
        for _ in 0..6 {
            let id = q.enqueue(e.clone()).unwrap();
            q.take_pending(1); // resolve out of pending so we can keep enqueuing
            ids.push(id);
        }
        // Only the most recent 3 statuses are retained.
        assert!(q.status(ids[0]).is_none());
        assert!(q.status(ids[2]).is_none());
        assert!(q.status(ids[3]).is_some());
        assert!(q.status(ids[5]).is_some());
    }

    #[test]
    fn take_pending_respects_max() {
        let mut q = AnnounceQueue::new();
        let e = request_to_note_entry(&req("")).unwrap();
        for _ in 0..5 {
            q.enqueue(e.clone()).unwrap();
        }
        assert_eq!(q.take_pending(2).len(), 2);
        assert_eq!(q.pending_len(), 3);
    }
}
