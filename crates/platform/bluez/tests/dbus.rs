//! Production D-Bus discovery against a private fake `org.bluez`.

#![expect(
    clippy::absolute_paths,
    clippy::arbitrary_source_item_ordering,
    clippy::doc_markdown,
    clippy::missing_const_for_fn,
    clippy::pub_with_shorthand,
    clippy::redundant_pub_crate,
    clippy::shadow_reuse,
    clippy::std_instead_of_alloc,
    clippy::tests_outside_test_module,
    clippy::unnecessary_wraps,
    clippy::unused_self,
    clippy::used_underscore_binding,
    reason = "Private D-Bus fakes are integration-test support"
)]

/// Private D-Bus BlueZ fake.
#[path = "dbus/fake.rs"]
mod fake;
/// Shared discovery session contract tests.
#[path = "dbus/tests.rs"]
mod tests;
