//! Nearby Sharing frame and payload protocol.

mod advertisement;
mod error;
mod frames;
mod offer;
mod session;
mod types;

pub use advertisement::{EndpointInfo, MdnsInstance};
pub use error::ProtocolError;
pub use session::SharingSession;
pub use types::{IncomingFile, IncomingOffer, PairingStatus};
