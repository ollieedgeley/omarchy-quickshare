//! Inbound publication leases owned by shared visibility state.

use crate::daemon::media::VisibilityLeases;
use quickshare_network::lan::PublishedLanListener;

#[derive(Debug, Default)]
pub(crate) struct Resources {
    lan: Option<PublishedLanListener>,
    bluetooth: VisibilityLeases,
    #[cfg(test)]
    fake: Option<FakeLease>,
}

impl Resources {
    pub(crate) const fn new(
        lan: Option<PublishedLanListener>,
        bluetooth: VisibilityLeases,
    ) -> Self {
        Self {
            lan,
            bluetooth,
            #[cfg(test)]
            fake: None,
        }
    }

    pub(crate) const fn lan_mut(
        &mut self,
    ) -> Option<&mut PublishedLanListener> {
        self.lan.as_mut()
    }

    pub(crate) const fn bluetooth_mut(&mut self) -> &mut VisibilityLeases {
        &mut self.bluetooth
    }

    #[cfg(test)]
    pub(crate) fn fake(
        released: alloc::sync::Arc<core::sync::atomic::AtomicUsize>,
        listener: Option<std::net::TcpListener>,
    ) -> Self {
        Self {
            fake: Some(FakeLease { released, listener }),
            ..Self::default()
        }
    }

    pub(crate) fn accept_lan(&self) -> Option<std::net::TcpStream> {
        if let Some(listener) = &self.lan {
            return listener.accept().ok().flatten();
        }
        #[cfg(test)]
        if let Some(listener) =
            self.fake.as_ref().and_then(|fake| fake.listener.as_ref())
        {
            return listener.accept().ok().map(|(stream, _)| stream);
        }
        None
    }

    pub(crate) fn media(&self) -> Vec<String> {
        let mut media = self.bluetooth.media();
        #[cfg(test)]
        if self.fake.is_some() {
            media.push(String::from("wifi_lan"));
        }
        if self.lan.is_some() {
            media.insert(0, String::from("wifi_lan"));
        }
        media
    }

    pub(crate) fn close(self) -> Result<(), String> {
        let result = self.lan.map_or(Ok(()), |listener| {
            listener.stop().map_err(|error| error.to_string())
        });
        self.bluetooth.close();
        result
    }
}

#[cfg(test)]
#[derive(Debug)]
struct FakeLease {
    released: alloc::sync::Arc<core::sync::atomic::AtomicUsize>,
    listener: Option<std::net::TcpListener>,
}

#[cfg(test)]
#[expect(
    clippy::missing_trait_methods,
    reason = "Lease cleanup uses only Drop's destructor hook"
)]
impl Drop for FakeLease {
    fn drop(&mut self) {
        let _previous = self
            .released
            .fetch_add(1, core::sync::atomic::Ordering::AcqRel);
    }
}
