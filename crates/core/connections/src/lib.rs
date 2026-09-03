//! Encrypted Nearby Connections sessions over TCP.

#![forbid(unsafe_code)]
#![expect(
    clippy::absolute_paths,
    reason = "Protocol wire paths remain explicit at boundary conversions"
)]
#![expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Connection methods follow protocol chronology"
)]
#![expect(
    clippy::as_conversions,
    reason = "prost protocol enum fields use i32 wire values"
)]
#![expect(
    clippy::big_endian_bytes,
    reason = "Nearby framing specifies network-byte-order length prefixes"
)]
#![expect(
    clippy::error_impl_error,
    reason = "The public connection failure type implements Error"
)]
#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "Connection options are shared only with the protocol module"
)]
#![expect(
    clippy::map_err_ignore,
    reason = "Connection failures do not disclose cryptographic library details"
)]
#![expect(
    clippy::let_underscore_untyped,
    reason = "Payload bookkeeping discards replaced internal state"
)]
#![cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "Private framing details follow public connection operations"
    )
)]
#![expect(
    clippy::missing_inline_in_public_items,
    reason = "Connection operations avoid mandatory cross-crate inlining"
)]
#![expect(
    clippy::missing_trait_methods,
    reason = "Error uses the standard default trait methods"
)]
#![expect(
    clippy::pub_with_shorthand,
    reason = "rustfmt scoped visibility conflicts with the restriction lint"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed protocol-state matches retain readable enum patterns"
)]
#![expect(
    clippy::renamed_function_params,
    reason = "Debug formatter names remain descriptive"
)]
#![expect(
    clippy::single_call_fn,
    reason = "Named protocol frame stages remain auditable"
)]
#![expect(
    clippy::std_instead_of_alloc,
    reason = "The connection crate requires std TCP I/O"
)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream crate-root use"
    )
)]
#![expect(
    clippy::pub_use,
    reason = "The architecture requires a stable crate-root interface"
)]

/// TCP session establishment and encrypted frame transport.
mod session;

pub use session::{Connection, ConnectionOptions, Error, Event};
