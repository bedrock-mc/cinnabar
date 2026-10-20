//! Copied transport measurements without shared transport lifecycle permission.

use std::time::Duration;

/// Read-only full RTT from the original native transport.
///
/// Reading never drives I/O, borrows a lock or retains a connection. A retired
/// transport permanently withdraws its estimate, including on abrupt owner drop.
#[derive(Clone, Debug)]
pub struct RoundTripTime {
    #[cfg(any(
        feature = "test-support",
        all(feature = "native-networking", not(target_arch = "wasm32"))
    ))]
    source: Source,
}

#[cfg(any(
    feature = "test-support",
    all(feature = "native-networking", not(target_arch = "wasm32"))
))]
#[derive(Clone, Debug)]
enum Source {
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    RakNet(bedrock_net::raknet::RttObservation),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    NetherNet(nethernet_tokio::RttObservation),
    #[cfg(feature = "test-support")]
    Controlled(std::sync::Arc<std::sync::atomic::AtomicU64>),
}

impl RoundTripTime {
    /// Copies the latest positive full estimate, or `None` if unmeasured or retired.
    pub fn get(&self) -> Option<Duration> {
        #[cfg(any(
            feature = "test-support",
            all(feature = "native-networking", not(target_arch = "wasm32"))
        ))]
        {
            match &self.source {
                #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                Source::RakNet(source) => source.get(),
                #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                Source::NetherNet(source) => source.get(),
                #[cfg(feature = "test-support")]
                Source::Controlled(source) => {
                    match source.load(std::sync::atomic::Ordering::Relaxed) {
                        0 | u64::MAX => None,
                        nanos => Some(Duration::from_nanos(nanos)),
                    }
                }
            }
        }
        #[cfg(not(any(
            feature = "test-support",
            all(feature = "native-networking", not(target_arch = "wasm32"))
        )))]
        {
            None
        }
    }

    /// Wraps only the actual selected RakNet connection's observation.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(crate) fn raknet(source: bedrock_net::raknet::RttObservation) -> Self {
        Self {
            source: Source::RakNet(source),
        }
    }

    /// Wraps only the actual established NetherNet actor's observation.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(crate) fn nethernet(source: nethernet_tokio::RttObservation) -> Self {
        Self {
            source: Source::NetherNet(source),
        }
    }
}

#[cfg(feature = "test-support")]
pub(crate) mod test_support {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Controlled measurement source for session identity and retirement projection tests.
    /// Dropping it permanently withdraws the paired observation.
    pub struct RttPublisher(Arc<AtomicU64>);

    /// Creates an unmeasured source without any session or authority capability.
    pub fn rtt_observation() -> (RttPublisher, RoundTripTime) {
        let value = Arc::new(AtomicU64::new(0));
        (
            RttPublisher(value.clone()),
            RoundTripTime {
                source: Source::Controlled(value),
            },
        )
    }

    impl RttPublisher {
        /// Replaces only this fixture source's copied estimate.
        pub fn publish(&self, sample: Option<Duration>) {
            let nanos = sample.map_or(0, |sample| {
                u64::try_from(sample.as_nanos())
                    .unwrap_or(u64::MAX - 1)
                    .min(u64::MAX - 1)
            });
            self.0.store(nanos, Ordering::Relaxed);
        }
    }

    impl Drop for RttPublisher {
        fn drop(&mut self) {
            self.0.store(u64::MAX, Ordering::Relaxed);
        }
    }
}
