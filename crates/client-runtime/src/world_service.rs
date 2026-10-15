//! Owns the world worker and its between-frame handoff schedules.

use bevy::{
    app::{App, MainScheduleOrder},
    ecs::schedule::ScheduleLabel,
    prelude::Resource,
};
use chunk_pipeline::WorldStreamService;
use render::FrameStart;

/// Hands the stream to its worker after the frame's stream readers have finished.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldServiceLaunch;

/// Runs before `First`; every frame system finds the stream where it left it.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldServiceReclaim;

/// The production client's service. Without it the stream never leaves the frame thread.
#[derive(Resource)]
pub struct WorldServiceSlot {
    pub service: WorldStreamService,
}

impl WorldServiceSlot {
    /// Starts the service unless `CINNABAR_WORLD_SERVICE=0` keeps polling on the frame thread.
    pub fn from_environment() -> std::io::Result<Option<Self>> {
        if env_compat::var_os(diagnostics::markers::WORLD_SERVICE).is_some_and(|value| value == "0")
        {
            return Ok(None);
        }
        Ok(Some(Self {
            service: WorldStreamService::spawn()?,
        }))
    }
}

/// Launches after `after`, which must follow every stream reader, and reclaims after frame
/// timing starts but before `First`. The final pacing wait may run while the worker owns it.
pub fn configure_world_service(app: &mut App, after: impl ScheduleLabel) {
    FrameStart::install(app);
    app.init_schedule(WorldServiceLaunch)
        .init_schedule(WorldServiceReclaim);
    let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
    order.insert_after(after, WorldServiceLaunch);
    order.insert_after(FrameStart, WorldServiceReclaim);
}
