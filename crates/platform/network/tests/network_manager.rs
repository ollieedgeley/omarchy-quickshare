//! `NetworkManager` adapter behavior through a private D-Bus fake.

#![expect(
    clippy::absolute_paths,
    clippy::arbitrary_source_item_ordering,
    clippy::doc_markdown,
    clippy::pub_with_shorthand,
    clippy::redundant_pub_crate,
    clippy::same_name_method,
    clippy::shadow_reuse,
    clippy::std_instead_of_alloc,
    clippy::too_many_lines,
    clippy::unnecessary_wraps,
    clippy::unused_self,
    clippy::used_underscore_binding,
    reason = "Private D-Bus fakes are integration-test support"
)]

/// Private D-Bus NetworkManager fake.
#[cfg(test)]
#[path = "network_manager/fake.rs"]
mod fake;
/// Public adapter contract tests.
#[cfg(test)]
#[path = "network_manager/tests.rs"]
mod tests;
