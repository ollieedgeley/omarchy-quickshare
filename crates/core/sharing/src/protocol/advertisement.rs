use super::ProtocolError;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

const MINIMUM_LENGTH: usize = 17;

/// A decoded Sharing endpoint advertisement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointInfo {
    version: u8,
    device_type: u8,
    salt: [u8; 2],
    metadata_key: [u8; 14],
    device_name: Option<String>,
    device_name_length: u8,
    vendor_id: Option<u8>,
    capabilities: Vec<u8>,
    capabilities_length: u8,
}

impl EndpointInfo {
    /// Creates a bounded endpoint advertisement.
    ///
    /// # Errors
    ///
    /// Returns an error when a field cannot fit the Google wire layout.
    pub fn new(
        version: u8,
        device_type: u8,
        salt: [u8; 2],
        metadata_key: [u8; 14],
        device_name: Option<&str>,
        vendor_id: Option<u8>,
        capabilities: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        let capabilities_length = u8::try_from(capabilities.len())
            .map_err(|_| ProtocolError::InvalidAdvertisement)?;
        let device_name_length = device_name
            .map_or(Ok(0), |name| u8::try_from(name.len()))
            .map_err(|_| ProtocolError::InvalidAdvertisement)?;
        if version > 1 || device_type > 6 || vendor_id.is_some_and(|id| id != 1)
        {
            return Err(ProtocolError::InvalidAdvertisement);
        }
        Ok(Self {
            version,
            device_type,
            salt,
            metadata_key,
            device_name: device_name.map(String::from),
            device_name_length,
            vendor_id,
            capabilities,
            capabilities_length,
        })
    }
    /// Encodes the Google Sharing endpoint-info layout.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![
            (self.version << 5)
                | (u8::from(self.device_name.is_none()) << 4)
                | (self.device_type << 1),
        ];
        bytes.extend_from_slice(&self.salt);
        bytes.extend_from_slice(&self.metadata_key);
        if let Some(name) = &self.device_name {
            bytes.push(self.device_name_length);
            bytes.extend_from_slice(name.as_bytes());
        }
        if let Some(vendor_id) = self.vendor_id {
            bytes.extend_from_slice(&[2, 1, vendor_id]);
        }
        if !self.capabilities.is_empty() {
            bytes.extend_from_slice(&[3, self.capabilities_length]);
            bytes.extend_from_slice(&self.capabilities);
        }
        bytes
    }

    /// Encodes the endpoint-info value used by the DNS-SD `n` property.
    #[must_use]
    pub fn property(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.encode())
    }

    /// Decodes the endpoint-info value from a DNS-SD `n` property.
    ///
    /// # Errors
    ///
    /// Returns an error when Base64 or endpoint-info bytes are invalid.
    pub fn decode_property(value: &str) -> Result<Self, ProtocolError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| ProtocolError::InvalidAdvertisement)?;
        Self::decode(&bytes)
    }

    /// Returns the advertised peer name, when the endpoint includes one.
    #[must_use]
    #[inline]
    pub fn device_name(&self) -> Option<&str> {
        self.device_name.as_deref()
    }

    /// Decodes a bounded Google Sharing endpoint advertisement.
    ///
    /// # Errors
    ///
    /// Returns an error when required fields are absent or invalid.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < MINIMUM_LENGTH {
            return Err(ProtocolError::InvalidAdvertisement);
        }
        let first = bytes[0];
        let (version, device_type) = (first >> 5, (first >> 1) & 7);
        if version > 1 || device_type > 6 {
            return Err(ProtocolError::InvalidAdvertisement);
        }
        let salt = bytes[1..3]
            .try_into()
            .map_err(|_| ProtocolError::InvalidAdvertisement)?;
        let metadata_key = bytes[3..17]
            .try_into()
            .map_err(|_| ProtocolError::InvalidAdvertisement)?;
        let mut offset = MINIMUM_LENGTH;
        let device_name = if first & 0x10 == 0 {
            let length = *bytes
                .get(offset)
                .ok_or(ProtocolError::InvalidAdvertisement)?
                as usize;
            offset += 1;
            let name = bytes
                .get(offset..offset + length)
                .filter(|name| !name.is_empty())
                .ok_or(ProtocolError::InvalidAdvertisement)?;
            offset += length;
            Some(
                String::from_utf8(name.to_vec())
                    .map_err(|_| ProtocolError::InvalidAdvertisement)?,
            )
        } else {
            None
        };
        let (vendor_id, capabilities) = decode_extensions(bytes, offset)?;
        Self::new(
            version,
            device_type,
            salt,
            metadata_key,
            device_name.as_deref(),
            vendor_id,
            capabilities,
        )
    }
}

fn decode_extensions(
    bytes: &[u8],
    mut offset: usize,
) -> Result<(Option<u8>, Vec<u8>), ProtocolError> {
    let mut vendor_id = None;
    let mut capabilities = Vec::new();
    while offset < bytes.len() {
        let kind = *bytes
            .get(offset)
            .ok_or(ProtocolError::InvalidAdvertisement)?;
        let length = *bytes
            .get(offset + 1)
            .ok_or(ProtocolError::InvalidAdvertisement)?
            as usize;
        offset += 2;
        let value = bytes
            .get(offset..offset + length)
            .ok_or(ProtocolError::InvalidAdvertisement)?;
        offset += length;
        match kind {
            2 if value.len() == 1 => {
                vendor_id = (value[0] == 1).then_some(1);
            }
            2 => return Err(ProtocolError::InvalidAdvertisement),
            3 => capabilities = value.to_vec(),
            _ => {}
        }
    }
    Ok((vendor_id, capabilities))
}

/// The binary mDNS instance metadata for a Nearby Sharing LAN peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MdnsInstance {
    endpoint_id: [u8; 4],
}
impl MdnsInstance {
    /// Creates Nearby point-to-point LAN metadata for an endpoint identifier.
    #[must_use]
    pub const fn new(endpoint_id: [u8; 4]) -> Self {
        Self { endpoint_id }
    }
    /// Returns the Nearby Sharing DNS-SD service type.
    #[must_use]
    pub const fn service_type() -> &'static str {
        "_FC9F5ED42C8A._tcp.local."
    }
    /// Encodes the version, strategy, endpoint, and service hash.
    #[must_use]
    pub const fn encode(&self) -> [u8; 10] {
        [
            0x23,
            self.endpoint_id[0],
            self.endpoint_id[1],
            self.endpoint_id[2],
            self.endpoint_id[3],
            0xFC,
            0x9F,
            0x5E,
            0,
            0,
        ]
    }

    /// Encodes the DNS-SD service instance label.
    #[must_use]
    pub fn label(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.encode())
    }

    /// Decodes a DNS-SD service instance label.
    ///
    /// # Errors
    ///
    /// Returns an error unless Base64 and the LAN instance bytes are valid.
    pub fn decode_label(value: &str) -> Result<Self, ProtocolError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| ProtocolError::InvalidMdnsInstance)?;
        Self::decode(&bytes)
    }

    /// Decodes Nearby point-to-point LAN metadata.
    ///
    /// # Errors
    ///
    /// Returns an error unless the bytes match the LAN instance layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let [0x23, a, b, c, d, 0xFC, 0x9F, 0x5E, 0, 0] =
            *<&[u8; 10]>::try_from(bytes)
                .map_err(|_| ProtocolError::InvalidMdnsInstance)?
        else {
            return Err(ProtocolError::InvalidMdnsInstance);
        };
        Ok(Self::new([a, b, c, d]))
    }
}
