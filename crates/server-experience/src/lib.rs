//! Opt-in Cinnabar extensions. Nothing here changes vanilla login or packet IDs.

#[cfg(feature = "host")]
pub mod bundle;
#[cfg(feature = "host")]
pub mod cache;
#[cfg(feature = "host")]
pub mod crypto;
#[cfg(feature = "host")]
pub mod download;
#[cfg(feature = "host")]
pub mod fetch;
#[cfg(feature = "host")]
pub mod manifest;
#[cfg(feature = "host")]
pub mod media;
#[cfg(feature = "host")]
pub mod negotiation;
pub mod policy;
pub mod rate_limit;
#[cfg(feature = "host")]
pub mod runtime;
#[cfg(feature = "data")]
pub mod screen;
#[cfg(feature = "host")]
pub mod session;
#[cfg(feature = "data")]
pub mod session_data;
#[cfg(feature = "host")]
pub mod trust;
#[cfg(feature = "host")]
pub mod wire;

#[cfg(all(test, feature = "host"))]
mod tests;

/// Lets tests exercise the media helper's heap ceiling in a real process.
#[cfg(all(test, feature = "host"))]
#[global_allocator]
static TEST_ALLOCATOR: media::ceiling::BoundedAllocator = media::ceiling::BoundedAllocator::new();
