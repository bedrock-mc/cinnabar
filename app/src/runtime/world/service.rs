//! Lends the world stream to its between-frames service after the frame's last schedule and
//! takes it back before the next frame's first, so commits and light and mesh scheduling
//! overlap render extraction and frame pacing instead of the next frame's critical path.

use std::time::{Duration, Instant};

use bevy::{
    app::App,
    prelude::{Res, ResMut},
};
use render::{ChunkUploadBudget, RuntimeStage, RuntimeStageProfiler};

use super::{ClientWorld, WorldStreamFramePoll};
use client_presentation::local_player::LocalViewPose;
use client_runtime::world_service::{WorldServiceLaunch, WorldServiceReclaim, WorldServiceSlot};

/// Registers the adapters that lend app-owned world and camera state to the service.
pub(crate) fn configure_world_service(app: &mut App) {
    app.add_systems(WorldServiceLaunch, launch_world_service)
        .add_systems(WorldServiceReclaim, reclaim_world_service);
}

fn launch_world_service(
    slot: Option<ResMut<WorldServiceSlot>>,
    client_world: Option<ResMut<ClientWorld>>,
    view: Option<Res<LocalViewPose>>,
    upload_budget: Option<Res<ChunkUploadBudget>>,
) {
    let (Some(mut slot), Some(mut client_world), Some(view), Some(upload_budget)) =
        (slot, client_world, view, upload_budget)
    else {
        return;
    };
    let Some(stream) = client_world.stream.take() else {
        return;
    };
    slot.service.launch(
        stream,
        view.eye_translation().to_array(),
        upload_budget.max_per_frame,
    );
}

fn reclaim_world_service(
    slot: Option<ResMut<WorldServiceSlot>>,
    client_world: Option<ResMut<ClientWorld>>,
    frame_poll: Option<ResMut<WorldStreamFramePoll>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let (Some(mut slot), Some(mut client_world), Some(mut frame_poll)) =
        (slot, client_world, frame_poll)
    else {
        return;
    };
    let started = Instant::now();
    let Some(serviced) = slot.service.reclaim() else {
        return;
    };
    let reclaimed = started.elapsed();
    assert!(
        client_world.stream.is_none(),
        "a world stream appeared while the service held the previous one"
    );
    client_world.stream = Some(serviced.stream);
    // The frame's publication diagnostics cover the service's work as well as its own poll.
    frame_poll.report = serviced.report;
    if let Some(profiler) = profiler {
        profiler.record(RuntimeStage::WorldServiceReclaim, started, reclaimed);
        if serviced.busy > Duration::ZERO {
            profiler.record_background_sample(RuntimeStage::WorldService, serviced.busy);
        }
    }
}

#[cfg(test)]
mod tests;
