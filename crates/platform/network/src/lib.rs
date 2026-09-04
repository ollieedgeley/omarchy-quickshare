//! Linux DNS-SD, TCP, hotspot, and Wi-Fi Direct media.

#![forbid(unsafe_code)]
#![expect(
    clippy::absolute_paths,
    clippy::arbitrary_source_item_ordering,
    clippy::doc_markdown,
    clippy::field_scoped_visibility_modifiers,
    clippy::inline_trait_bounds,
    clippy::missing_const_for_fn,
    clippy::multiple_crate_versions,
    clippy::multiple_inherent_impl,
    clippy::needless_pass_by_value,
    clippy::pub_use,
    clippy::pub_with_shorthand,
    clippy::shadow_reuse,
    clippy::single_call_fn,
    clippy::single_char_lifetime_names,
    clippy::std_instead_of_core,
    clippy::too_many_arguments,
    reason = "Linux D-Bus adapters keep rustfmt visibility grouping"
)]
#![cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "Private NetworkManager fields follow public adapter methods"
    )
)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream crate-root use"
    )
)]

extern crate alloc;

/// In-process DNS-SD advertising and browsing.
pub mod dns_sd;
/// TCP transport adapters for the local network.
pub mod lan;
/// NetworkManager D-Bus adapter for hotspot and Wi-Fi Direct roles.
pub mod network_manager;

pub use dns_sd::{
    Advertisement, Browser, DnsSd, Registration, ResolvedService, host_label,
    local_ipv4_addresses,
};
pub use network_manager::{
    Candidate, Credentials, Discovery, Medium, NetworkManager, Peer, Role,
    Session,
};

use core::{error, fmt};

/// A network adapter failure.
#[derive(Clone, Debug, Eq, PartialEq)]
#[expect(
    clippy::error_impl_error,
    reason = "The public adapter error keeps the established name"
)]
pub struct Error(pub(crate) String);

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "The adapter error stores display text and has no source error"
)]
impl error::Error for Error {}
