//! Bevy runtime ownership for transport, world work and session boundaries.

pub mod blob_cache;
#[cfg(any(target_arch = "wasm32", test))]
pub mod browser_jobs;
#[cfg(not(target_arch = "wasm32"))]
pub mod endpoint;
pub mod ingress;
pub mod inventory;
pub mod item_diagnostics;
pub mod network;
pub mod pack_admission;
pub mod pack_geometry;
pub mod publication;
pub mod session;
#[cfg(not(target_arch = "wasm32"))]
pub mod shutdown;
pub mod world_service;

use bevy::prelude::{App, Plugin};
use std::marker::PhantomData;

/// Registers runtime schedules while keeping the prepared session payload caller-owned.
pub struct ClientRuntimePlugin<P: Send + Sync + 'static>(PhantomData<fn() -> P>);

impl<P: Send + Sync + 'static> Default for ClientRuntimePlugin<P> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<P: Send + Sync + 'static> Plugin for ClientRuntimePlugin<P> {
    /// Preserves the frame flush, between-frame service and bounded shutdown boundaries.
    fn build(&self, app: &mut App) {
        network::configure_network_frame_flush::<P>(app);
        world_service::configure_world_service(app, network::NetworkFrameFlush);
        publication::configure_publication_frame(app);
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(bevy::prelude::Last, shutdown::arm_shutdown_watchdog);
    }

    fn finish(&self, app: &mut App) {
        network::trace_presented_frames(app);
    }
}
