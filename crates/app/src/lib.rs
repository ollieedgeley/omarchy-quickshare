//! Command-line and daemon composition for Omarchy Quick Share.

#![forbid(unsafe_code)]
#![expect(
    clippy::absolute_paths,
    clippy::arbitrary_source_item_ordering,
    clippy::arithmetic_side_effects,
    clippy::default_numeric_fallback,
    clippy::doc_markdown,
    clippy::double_must_use,
    clippy::exhaustive_structs,
    clippy::format_push_string,
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
    clippy::module_name_repetitions,
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
    clippy::single_match,
    clippy::single_match_else,
    clippy::string_slice,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::unnecessary_wraps,
    clippy::unused_trait_names,
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
/// Command-line dispatch for the local endpoint.
mod cli;
/// Strict user settings for the local endpoint.
pub mod config;
/// Local endpoint lifecycle and queue ownership.
pub mod daemon;

use quickshare_network as _;

pub use cli::{run, run_from_environment};
