//! Suspend-inclusive elapsed time, shared by visibility and admitted consent.

use core::time::Duration;

#[derive(Clone, Debug, Default)]
pub(crate) enum Clock {
    #[default]
    Boot,
    #[cfg(test)]
    Fake(alloc::sync::Arc<core::sync::atomic::AtomicU64>),
}

impl Clock {
    #[expect(
        clippy::expect_used,
        reason = "CLOCK_BOOTTIME guarantees nonnegative normalized elapsed time"
    )]
    pub(crate) fn now(&self) -> Duration {
        match self {
            Self::Boot => Duration::try_from(rustix::time::clock_gettime(
                rustix::time::ClockId::Boottime,
            ))
            .expect("Linux boot elapsed time is nonnegative"),
            #[cfg(test)]
            Self::Fake(seconds) => Duration::from_secs(
                seconds.load(core::sync::atomic::Ordering::Acquire),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn fake(seconds: u64) -> Self {
        Self::Fake(alloc::sync::Arc::new(core::sync::atomic::AtomicU64::new(
            seconds,
        )))
    }

    #[cfg(test)]
    pub(crate) fn advance(&self, seconds: u64) {
        if let Self::Fake(value) = self {
            let _previous =
                value.fetch_add(seconds, core::sync::atomic::Ordering::AcqRel);
        }
    }
}
