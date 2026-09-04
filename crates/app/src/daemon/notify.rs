//! Freedesktop desktop notifications for terminal shares.

use std::collections::HashMap;
use std::env;
use std::io;

use zbus::zvariant::Value;

const DISABLE_NOTIFICATIONS: &str = "OMARCHY_QUICKSHARE_DISABLE_NOTIFICATIONS";

/// Terminal notification classes that never include share content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NotifyKind {
    /// An outbound share completed.
    Sent,
    /// An inbound share completed.
    Received,
    /// A share failed after an error.
    Error,
}

/// Returns the public notification summary for `kind`.
#[must_use]
pub(super) const fn summary(kind: NotifyKind) -> &'static str {
    match kind {
        NotifyKind::Sent => "Share sent",
        NotifyKind::Received => "Share received",
        NotifyKind::Error => "Share failed",
    }
}

/// Best-effort session-bus notification with no peer, file, or content body.
pub(super) fn notify(kind: NotifyKind) {
    if cfg!(test) || env::var_os(DISABLE_NOTIFICATIONS).is_some() {
        return;
    }
    let _result = send(summary(kind));
}

fn send(summary: &str) -> io::Result<()> {
    let connection =
        zbus::blocking::Connection::session().map_err(io::Error::other)?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .map_err(io::Error::other)?;
    let hints: HashMap<&str, Value<'_>> = HashMap::new();
    let actions: Vec<&str> = Vec::new();
    let _: u32 = proxy
        .call(
            "Notify",
            &(
                "omarchy-quickshare",
                0_u32,
                "",
                summary,
                "",
                actions,
                hints,
                5_000_i32,
            ),
        )
        .map_err(io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{NotifyKind, summary};

    #[test]
    fn summaries_do_not_leak_share_details() {
        for kind in [NotifyKind::Sent, NotifyKind::Received, NotifyKind::Error]
        {
            let text = summary(kind);
            assert!(!text.contains("peer"));
            assert!(!text.contains("file"));
            assert!(!text.contains("http"));
        }
    }
}
