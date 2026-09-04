/// The account-free paired-key outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairingStatus {
    /// No account certificate was available to verify the peer.
    Unable,
}

/// The attachment kind declared by an inbound introduction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfferKind {
    /// One regular file.
    File,
    /// One plain-text value.
    Text,
    /// One web address.
    Url,
    /// One Android application package received as a file.
    AndroidApp,
}

impl OfferKind {
    /// Returns whether the offer is persisted as a staged file payload.
    #[must_use]
    pub const fn persists_as_file(self) -> bool {
        matches!(self, Self::File | Self::AndroidApp)
    }
}

/// One inbound attachment offer waiting for consent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingOffer {
    files: Vec<IncomingFile>,
    kind: OfferKind,
    package_name: Option<String>,
    size_bytes: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::protocol) struct IncomingFile {
    name: String,
    payload_id: i64,
    size_bytes: i64,
}

impl IncomingOffer {
    pub(in crate::protocol) fn new(
        kind: OfferKind,
        name: String,
        size_bytes: i64,
        payload_id: i64,
    ) -> Self {
        Self {
            files: vec![IncomingFile {
                name,
                payload_id,
                size_bytes,
            }],
            kind,
            package_name: None,
            size_bytes,
        }
    }

    pub(in crate::protocol) fn with_files(
        kind: OfferKind,
        files: Vec<IncomingFile>,
        size_bytes: i64,
    ) -> Self {
        Self {
            files,
            kind,
            package_name: None,
            size_bytes,
        }
    }

    pub(in crate::protocol) fn with_package_name(
        mut self,
        package_name: String,
    ) -> Self {
        self.package_name = Some(package_name);
        self
    }

    /// Returns one payload as its own offer for staging and transfer.
    #[must_use]
    pub fn file(&self, index: usize) -> Option<Self> {
        let file = self.files.get(index)?.clone();
        let size_bytes = file.size_bytes;
        Some(Self {
            files: vec![file],
            kind: self.kind,
            package_name: self.package_name.clone(),
            size_bytes,
        })
    }

    /// Returns the number of file payloads declared by the introduction.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Returns the attachment kind declared by the introduction.
    #[must_use]
    pub const fn kind(&self) -> OfferKind {
        self.kind
    }

    /// Returns the validated basename or text title offered by the peer.
    #[must_use]
    pub fn name(&self) -> &str {
        self.files.first().map_or("", |file| file.name.as_str())
    }

    /// Returns the declared byte length.
    #[must_use]
    pub const fn size_bytes(&self) -> i64 {
        self.size_bytes
    }

    /// Returns the referenced Connections payload identifier.
    #[must_use]
    pub fn payload_id(&self) -> i64 {
        self.files.first().map_or(0, |file| file.payload_id)
    }

    /// Returns the Android package name when the offer is an application.
    #[must_use]
    pub fn package_name(&self) -> Option<&str> {
        self.package_name.as_deref()
    }
}

impl IncomingFile {
    pub(in crate::protocol) fn new(
        name: String,
        size_bytes: i64,
        payload_id: i64,
    ) -> Self {
        Self {
            name,
            payload_id,
            size_bytes,
        }
    }
}
