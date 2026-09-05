use super::{Medium, Role};

pub(super) fn event(
    operation: &'static str,
    outcome: &'static str,
    reason: &'static str,
    medium: Option<Medium>,
    role: Option<Role>,
) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "network_upgrade",
        operation,
        outcome,
        reason,
        medium = medium.map_or("none", medium_name),
        role = role.map_or("none", role_name)
    );
}

const fn medium_name(medium: Medium) -> &'static str {
    match medium {
        Medium::Lan => "lan",
        Medium::Hotspot => "hotspot",
        Medium::WifiDirect => "wifi_direct",
    }
}

const fn role_name(role: Role) -> &'static str {
    match role {
        Role::Client => "client",
        Role::Owner => "owner",
    }
}
