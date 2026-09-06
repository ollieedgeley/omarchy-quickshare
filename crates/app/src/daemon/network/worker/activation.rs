//! Builds candidate publication resources outside the visibility lock.

use super::super::inbound::open_listener;
use crate::daemon::media::{endpoint_name, open_visibility};
use crate::daemon::visibility::Resources;
use quickshare_bluez::Adapter;
use quickshare_network::DnsSd;

#[cfg(test)]
pub(crate) type TestActivation =
    Box<dyn FnMut(u64) -> Result<Resources, String> + Send>;

pub(super) fn activate(
    dns_sd: Option<&DnsSd>,
    bluetooth: &mut Option<Adapter>,
    name: Option<&str>,
    #[cfg(test)] generation: u64,
    #[cfg(test)] injected: &mut Option<TestActivation>,
) -> Result<Resources, String> {
    #[cfg(test)]
    if let Some(injected) = injected {
        return injected(generation);
    }
    let dns_sd = dns_sd.ok_or_else(|| String::from("DNS-SD unavailable"))?;
    let lan = open_listener(dns_sd, endpoint_name(name));
    let bluetooth = open_visibility(super::refresh_adapter(bluetooth));
    match lan {
        Ok(listener) => Ok(Resources::new(Some(listener), bluetooth)),
        Err(error) if bluetooth.media().is_empty() => {
            bluetooth.close();
            Err(error.to_string())
        }
        Err(_) => Ok(Resources::new(None, bluetooth)),
    }
}
