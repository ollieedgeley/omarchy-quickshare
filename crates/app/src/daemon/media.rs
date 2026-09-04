//! Discovery, visibility, connection, and bandwidth-upgrade composition.

mod connection;
mod upgrade;

#[cfg(test)]
mod tests;

pub(super) use self::connection::{
    DiscoveryLeases, ENDPOINT_ID_BYTES, ENDPOINT_NAME, PeerRoute,
    VisibilityLeases, accept_connection, attempt_order, connect_route,
    medium_name, open_visibility, sharing_session, start_discovery,
};
pub(super) use self::upgrade::{
    accept_negotiated_upgrade, initiate_bandwidth_upgrade,
};
