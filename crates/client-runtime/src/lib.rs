//! Bevy runtime ownership for transport, world work and session boundaries.

pub mod blob_cache;
pub mod endpoint;
pub mod ingress;
pub mod inventory;
pub mod item_diagnostics;
pub mod network;
pub mod pack_admission;
pub mod pack_geometry;
pub mod publication;
pub mod session;
pub mod shutdown;
pub mod world_service;

use bevy::prelude::{App, IntoScheduleConfigs, Last, Plugin, Update};
use client_state::RuntimeFrameSet;
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
        app.add_systems(
            Update,
            publication::begin_publication_frame
                .in_set(RuntimeFrameSet::PublicationBudget)
                .before(render_scene::ChunkRenderApplySet),
        )
        .add_systems(Last, shutdown::arm_shutdown_watchdog);
    }
}
