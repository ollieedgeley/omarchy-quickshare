use std::path::Path;

use super::{Envelope, Request};
use crate::PROTOCOL_VERSION;

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Submission constructors stay in their owned request submodule"
)]
impl Envelope {
    /// Creates a request to submit one file for sharing.
    #[must_use]
    #[inline]
    pub fn submit_file(path: &Path, request_id: &str) -> Self {
        Self::submit_file_for(path, None, request_id)
    }

    /// Builds one file submission with an optional selected peer.
    fn submit_file_for(
        path: &Path,
        peer_id: Option<&str>,
        request_id: &str,
    ) -> Self {
        Self {
            request: Request::SubmitFile {
                path: path.to_path_buf(),
                peer_id: peer_id.map(String::from),
            },
            request_id: Some(String::from(request_id)),
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit one file to a selected peer.
    #[must_use]
    #[inline]
    pub fn submit_file_to_peer(
        path: &Path,
        peer_id: &str,
        request_id: &str,
    ) -> Self {
        Self::submit_file_for(path, Some(peer_id), request_id)
    }

    /// Creates a request to submit plain text for sharing.
    #[must_use]
    #[inline]
    pub fn submit_text(text: &str, request_id: &str) -> Self {
        Self::submit_text_for(text, None, request_id)
    }

    /// Builds one text submission with an optional selected peer.
    fn submit_text_for(
        text: &str,
        peer_id: Option<&str>,
        request_id: &str,
    ) -> Self {
        Self {
            request: Request::SubmitText {
                peer_id: peer_id.map(String::from),
                text: String::from(text),
            },
            request_id: Some(String::from(request_id)),
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit plain text to a selected peer.
    #[must_use]
    #[inline]
    pub fn submit_text_to_peer(
        text: &str,
        peer_id: &str,
        request_id: &str,
    ) -> Self {
        Self::submit_text_for(text, Some(peer_id), request_id)
    }

    /// Creates a request to submit a URL for sharing.
    #[must_use]
    #[inline]
    pub fn submit_url(url: &str, request_id: &str) -> Self {
        Self::submit_url_for(url, None, request_id)
    }

    /// Builds one URL submission with an optional selected peer.
    fn submit_url_for(
        url: &str,
        peer_id: Option<&str>,
        request_id: &str,
    ) -> Self {
        Self {
            request: Request::SubmitUrl {
                peer_id: peer_id.map(String::from),
                url: String::from(url),
            },
            request_id: Some(String::from(request_id)),
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit a URL to a selected peer.
    #[must_use]
    #[inline]
    pub fn submit_url_to_peer(
        url: &str,
        peer_id: &str,
        request_id: &str,
    ) -> Self {
        Self::submit_url_for(url, Some(peer_id), request_id)
    }
}
