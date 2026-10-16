//! Installed executable identity queried before an offline helper replaces it.

use serde::{Deserialize, Serialize};

/// Prints build identity before client setup, asset loading or window creation.
pub const QUERY_FLAG: &str = "--update-build-info";

/// App version and publication number reported by the executable being inspected.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Build {
    pub version: String,
    pub number: u64,
}

impl Build {
    /// Reports the workspace version and publication number compiled into this executable.
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            number: crate::build_number(),
        }
    }
}

/// The channel this payload was published on, stamped by release packaging; none in development.
pub fn release_channel() -> Option<crate::channel::Channel> {
    crate::channel::Channel::parse(option_env!("CINNABAR_RELEASE_CHANNEL")?)
}
