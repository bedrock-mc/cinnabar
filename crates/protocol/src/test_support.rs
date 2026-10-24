//! Fixture builders and semantic observations for other crates' tests. Wire layouts stay
//! private to this crate; callers build and observe packets through owned values only.

pub mod ddui;
pub mod items;
pub mod negotiation;
pub mod packets;
pub mod recipes;
pub mod remote_equipment;
pub mod startup;
pub mod transactions;

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub mod friend_peer;

#[cfg(all(
    feature = "native-networking",
    feature = "test-support",
    not(target_arch = "wasm32")
))]
pub use crate::login::native_network::fixtures::{hosted_client, hosted_provider};

#[cfg(all(
    feature = "native-networking",
    feature = "test-support",
    not(target_arch = "wasm32")
))]
pub use crate::login::native_network::retain_hosted_received_pair;

#[cfg(feature = "test-support")]
pub use crate::round_trip_time::test_support::{RttPublisher, rtt_observation};

#[cfg(all(
    feature = "native-networking",
    feature = "test-support",
    not(target_arch = "wasm32")
))]
pub mod hosted_packless;
