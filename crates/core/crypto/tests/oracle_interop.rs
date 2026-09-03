//! Live UKEY2 and D2D interoperability with the pinned Google oracle.

use aes as _;
use cbc as _;
use hkdf as _;
use hmac as _;
use p256 as _;
use prost as _;
use quickshare_crypto as _;
use quickshare_wire as _;
use rand_core as _;
use sha2 as _;

#[cfg(quickshare_oracle_reference)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    clippy::big_endian_bytes,
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::missing_trait_methods,
    clippy::panic_in_result_fn,
    clippy::shadow_unrelated,
    clippy::single_call_fn,
    clippy::tests_outside_test_module,
    clippy::unwrap_in_result,
    reason = "The oracle harness keeps byte-level assertions legible"
)]
#[path = "oracle_interop/reference.rs"]
mod reference;
