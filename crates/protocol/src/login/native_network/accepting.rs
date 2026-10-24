//! Configured host pack metadata and original Go world packet projection.

mod packs;
mod world;
pub use packs::native_host_packs;
use std::io;
pub use world::NativeHostWorld;

/// Reports unusable host metadata without exposing signed identities or provider diagnostics.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid native accepting packet",
    )
}
