/// The account-free paired-key outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairingStatus {
    /// No account certificate was available to verify the peer.
    Unable,
}

/// One accepted incoming file offer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingOffer {
    name: String,
    size_bytes: i64,
    payload_id: i64,
}

impl IncomingOffer {
    pub(in crate::protocol) const fn new(
        name: String,
        size_bytes: i64,
        payload_id: i64,
    ) -> Self {
        Self {
            name,
            size_bytes,
            payload_id,
        }
    }

    /// Returns the validated basename offered by the peer.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the declared byte length.
    #[must_use]
    pub const fn size_bytes(&self) -> i64 {
        self.size_bytes
    }

    /// Returns the referenced Connections payload identifier.
    #[must_use]
    pub const fn payload_id(&self) -> i64 {
        self.payload_id
    }
}

/// A completely validated incoming file payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingFile {
    name: String,
    bytes: Vec<u8>,
}

impl IncomingFile {
    pub(in crate::protocol) const fn new(name: String, bytes: Vec<u8>) -> Self {
        Self { name, bytes }
    }

    /// Returns the offered safe basename.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the received file bytes after length and chunk checks.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
