//! Networked announcement-service client (sRFC-0042 §5.8.4).
//!
//! Thin async HTTP client over the §5.8.4 protocol: submit an
//! announcement for decoupled publishing, then poll its batch status.
//! URL construction is split out so it can be unit-tested without a
//! server; the actual `send()` requires a running service.
//!
//! Enabled by the `net` feature.

use crate::announce::{AnnounceRequest, AnnounceResponse, AnnounceStatus};
use crate::error::SlntError;

/// Client for a single announcement service base URL.
pub struct AnnounceClient {
    base_url: String,
    http: reqwest::Client,
}

impl AnnounceClient {
    /// Create a client for `base_url` (trailing slashes trimmed).
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_client(base_url, reqwest::Client::new())
    }

    /// Create a client reusing a caller-provided `reqwest::Client`.
    pub fn with_client(base_url: impl Into<String>, http: reqwest::Client) -> Self {
        Self {
            base_url: trim_trailing_slashes(base_url.into()),
            http,
        }
    }

    /// `POST {base}/announce` endpoint URL.
    pub fn announce_url(&self) -> String {
        format!("{}/announce", self.base_url)
    }

    /// `GET {base}/announce/status/{batch_id}` endpoint URL.
    pub fn status_url(&self, batch_id: &str) -> String {
        format!("{}/announce/status/{}", self.base_url, batch_id)
    }

    /// Submit an announcement for decoupled publishing (§5.8.1).
    pub async fn submit(&self, req: &AnnounceRequest) -> Result<AnnounceResponse, SlntError> {
        let resp = self
            .http
            .post(self.announce_url())
            .json(req)
            .send()
            .await
            .map_err(|e| SlntError::Rpc(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(SlntError::Rpc(format!(
                "POST /announce: HTTP {}",
                resp.status()
            )));
        }
        resp.json().await.map_err(|e| SlntError::Rpc(e.to_string()))
    }

    /// Poll the status of a previously-submitted batch.
    pub async fn status(&self, batch_id: &str) -> Result<AnnounceStatus, SlntError> {
        let resp = self
            .http
            .get(self.status_url(batch_id))
            .send()
            .await
            .map_err(|e| SlntError::Rpc(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(SlntError::Rpc(format!(
                "GET /announce/status: HTTP {}",
                resp.status()
            )));
        }
        resp.json().await.map_err(|e| SlntError::Rpc(e.to_string()))
    }
}

fn trim_trailing_slashes(mut s: String) -> String {
    while s.ends_with('/') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_joined_without_double_slashes() {
        let c = AnnounceClient::new("https://svc.example.com/");
        assert_eq!(c.announce_url(), "https://svc.example.com/announce");
        assert_eq!(
            c.status_url("batch-7"),
            "https://svc.example.com/announce/status/batch-7"
        );
    }

    #[test]
    fn base_url_without_trailing_slash_works() {
        let c = AnnounceClient::new("http://localhost:8080");
        assert_eq!(c.announce_url(), "http://localhost:8080/announce");
    }
}
