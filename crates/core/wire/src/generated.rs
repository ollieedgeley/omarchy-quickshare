//! Generated protobuf bindings from `upstream/sources.toml`.

#![allow(
    clippy::all,
    clippy::cargo,
    clippy::nursery,
    clippy::pedantic,
    clippy::restriction,
    missing_docs,
    reason = "Generated protobuf bindings preserve upstream machine output"
)]

/// Nearby Connections wire messages.
pub mod location {
    /// Google Nearby package hierarchy.
    pub mod nearby {
        /// Nearby Connections messages.
        #[allow(
            missing_docs,
            reason = "prost-generated bindings retain upstream field names"
        )]
        pub mod connections {
            include!("generated/location.nearby.connections.rs");
        }
        /// Nearby Sharing telemetry enums.
        pub mod proto {
            #[allow(
                missing_docs,
                reason = "prost-generated bindings retain upstream field names"
            )]
            pub mod sharing {
                include!("generated/location.nearby.proto.sharing.rs");
            }
        }
    }
}

/// Nearby Sharing wire messages.
pub mod nearby {
    /// Nearby Sharing package hierarchy.
    pub mod sharing {
        /// Sharing enums.
        #[allow(
            missing_docs,
            reason = "prost-generated bindings retain upstream field names"
        )]
        pub mod proto {
            include!("generated/nearby.sharing.proto.rs");
        }
        /// Sharing service messages.
        pub mod service {
            /// Sharing wire messages.
            #[allow(
                missing_docs,
                reason = "prost-generated bindings retain upstream field names"
            )]
            pub mod proto {
                include!("generated/nearby.sharing.service.proto.rs");
            }
        }
    }
}

/// UKEY2 and Secure Message bindings.
#[allow(
    missing_docs,
    reason = "prost-generated bindings retain upstream field names"
)]
pub mod securegcm {
    include!("generated/securegcm.rs");
}

/// Secure Message bindings.
#[allow(
    missing_docs,
    reason = "prost-generated bindings retain upstream field names"
)]
pub mod securemessage {
    include!("generated/securemessage.rs");
}
