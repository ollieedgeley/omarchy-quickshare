//! BlueZ D-Bus adapters for Quick Share BLE and Classic media.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    expect(
        dead_code_pub_in_binary,
        reason = "Integration tests and app composition use this public API."
    )
)]
#![expect(
    clippy::absolute_paths,
    clippy::arbitrary_source_item_ordering,
    clippy::big_endian_bytes,
    clippy::let_underscore_untyped,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::use_self,
    clippy::doc_markdown,
    clippy::else_if_without_else,
    clippy::error_impl_error,
    clippy::field_scoped_visibility_modifiers,
    clippy::if_same_then_else,
    clippy::wildcard_enum_match_arm,
    clippy::indexing_slicing,
    clippy::inline_modules,
    clippy::iter_over_hash_type,
    clippy::map_err_ignore,
    clippy::missing_asserts_for_indexing,
    clippy::missing_const_for_fn,
    clippy::missing_trait_methods,
    clippy::multiple_crate_versions,
    clippy::multiple_inherent_impl,
    clippy::needless_pass_by_ref_mut,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::pattern_type_mismatch,
    clippy::pub_use,
    clippy::pub_with_shorthand,
    clippy::redundant_closure,
    clippy::redundant_pub_crate,
    clippy::renamed_function_params,
    clippy::same_name_method,
    clippy::shadow_reuse,
    clippy::significant_drop_tightening,
    clippy::single_call_fn,
    clippy::std_instead_of_alloc,
    clippy::std_instead_of_core,
    clippy::trivially_copy_pass_by_ref,
    clippy::type_complexity,
    clippy::unnecessary_literal_bound,
    clippy::unnecessary_wraps,
    clippy::unused_self,
    clippy::unused_trait_names,
    clippy::used_underscore_binding,
    reason = "BlueZ D-Bus adapters keep rustfmt visibility and FakeRadio tests"
)]
#![cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "Private BlueZ session fields follow public adapter methods"
    )
)]
#![cfg_attr(
    test,
    expect(
        clippy::expect_used,
        clippy::panic,
        clippy::shadow_unrelated,
        reason = "FakeRadio and weave tests use expect for fixtures"
    )
)]

extern crate alloc;

/// Public adapter methods.
mod adapter;
/// BLE receiver advertisements.
mod advertisement;
/// Production D-Bus session against `org.bluez`.
mod bus;
/// Bluetooth Classic discovery and sockets.
mod classic;
/// GATT and Nearby weave sockets.
mod gatt;
/// OS-medium connection-stream adapters.
mod io;
/// L2CAP listeners and channels.
mod l2cap;
/// BLE scan leases.
mod monitor;
/// In-process radio used by tests.
mod radio;
/// Weave connect-token codec.
mod token;
/// Nearby weave layer-A/B framing.
mod weave;

pub use advertisement::{BleAdvertisement, ReceiverAdvertisement};
pub use classic::{
    ClassicCandidate, ClassicDiscovery, ClassicListener, ClassicSocket,
};
pub use gatt::{GattWeaveServer, WeaveSocket};
pub use io::BluetoothIo;
pub use l2cap::{L2capChannel, L2capListener};
pub use monitor::BleScan;
pub use radio::{Adapter, Address, BleCandidate, Error, ErrorKind, testing};

/// Nearby/Quick Share BLE service UUID (`0xFEF3`).
pub const QUICK_SHARE_BLE_UUID: &str = "0000fef3-0000-1000-8000-00805f9b34fb";
