//! Nearby Sharing frame and payload protocol.
#![expect(
    clippy::absolute_paths,
    clippy::field_scoped_visibility_modifiers,
    clippy::missing_const_for_fn,
    clippy::multiple_inherent_impl,
    clippy::needless_range_loop,
    clippy::shadow_reuse,
    clippy::too_many_lines,
    reason = "Sharing protocol helpers stay next to the frames they encode"
)]

mod advertisement;
mod error;
mod frames;
mod offer;
mod session;
mod text;
mod types;

pub use advertisement::{EndpointInfo, MdnsInstance};
pub use error::{PairingError, PairingStep, ProtocolError};
pub use session::SharingSession;
pub use types::{IncomingOffer, OfferKind, PairingStatus};
