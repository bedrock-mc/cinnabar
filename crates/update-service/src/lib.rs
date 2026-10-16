//! The launcher's update service: signed channel checks, explicit installs and the channel
//! choice. Signatures and selection stay in `update-manifest`; staging, the no-live-game gate
//! and installation stay in `update-installer`.

mod download;
mod handoff;
pub mod preferences;
mod releases;
mod service;
mod state;
#[cfg(test)]
mod tests;

pub use handoff::ManualPackage;
pub use service::{Config, Handoff, Service};
pub use state::{UpdateStage, UpdateState};
