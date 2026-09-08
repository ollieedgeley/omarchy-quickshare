//! Daemon composition for Omarchy Quick Share.

#![forbid(unsafe_code)]
#![expect(
    clippy::absolute_paths,
    clippy::arbitrary_source_item_ordering,
    clippy::arithmetic_side_effects,
    clippy::default_numeric_fallback,
    clippy::doc_markdown,
    clippy::double_must_use,
    clippy::indexing_slicing,
    clippy::integer_division,
    clippy::iter_over_hash_type,
    clippy::little_endian_bytes,
    clippy::integer_division_remainder_used,
    clippy::map_err_ignore,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::missing_inline_in_public_items,
    clippy::multiple_crate_versions,
    clippy::needless_pass_by_value,
    clippy::or_fun_call,
    clippy::pattern_type_mismatch,
    clippy::pub_use,
    clippy::pub_with_shorthand,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pub_crate,
    clippy::shadow_reuse,
    clippy::single_call_fn,
    clippy::single_match_else,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::unused_trait_names,
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
    expect(
        clippy::assertions_on_result_states,
        clippy::inline_modules,
        clippy::expect_used,
        clippy::ip_constant,
        clippy::std_instead_of_core,
        clippy::missing_asserts_for_indexing,
        clippy::unwrap_used,
        reason = "Daemon unit tests use expect for control fixtures"
    )
)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream command use"
    )
)]

extern crate alloc;

/// Temporary ZIP archives for folder shares.
mod archive;
/// Local endpoint lifecycle and queue ownership.
pub mod daemon;

#[cfg(test)]
use prost as _;

use clap as _;
use quickshare_network as _;
use tracing_subscriber as _;
