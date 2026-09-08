//! Standalone command-line client for Omarchy Quick Share.

#![forbid(unsafe_code)]
#![expect(
    clippy::absolute_paths,
    clippy::arbitrary_source_item_ordering,
    clippy::arithmetic_side_effects,
    clippy::default_numeric_fallback,
    clippy::missing_const_for_fn,
    clippy::multiple_crate_versions,
    clippy::pattern_type_mismatch,
    clippy::pub_use,
    clippy::pub_with_shorthand,
    clippy::shadow_reuse,
    clippy::single_call_fn,
    clippy::string_slice,
    clippy::too_many_lines,
    clippy::use_debug,
    clippy::wildcard_enum_match_arm,
    reason = "Composition keeps rustfmt visibility and protocol grouping"
)]
#![cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "Private daemon fields follow public control operations"
    )
)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream command use"
    )
)]

mod cli;

pub use cli::{run, run_from_environment};
