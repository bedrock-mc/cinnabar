//! Reference paired project; its client keeps the runtime test terminal behavior.
#[cfg(all(feature = "server", feature = "client"))]
compile_error!("build exactly one mod half");
#[cfg(feature = "client")]
mod client;
#[cfg(feature = "server")]
mod server;
pub mod shared {
    experience_sdk::include_shared!();
}
