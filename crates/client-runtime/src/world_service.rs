//! Owns the world worker and its between-frame handoff schedules.

use bevy::{
    app::{App, MainScheduleOrder},
    ecs::schedule::ScheduleLabel,
    prelude::Resource,
};
use chunk_pipeline::WorldStreamService;
use render::FrameStart;

/// Runs after every other main schedule; nothing later in the frame reads the stream.
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
    /// Starts the service unless `RUST_MCBE_WORLD_SERVICE=0` keeps polling on the frame thread.
    pub fn from_environment() -> std::io::Result<Option<Self>> {
        if std::env::var_os(diagnostics::markers::WORLD_SERVICE).is_some_and(|value| value == "0") {
            return Ok(None);
        }
        Ok(Some(Self {
            service: WorldStreamService::spawn()?,
        }))
    }
}

/// Orders the hand-off schedules: launch after `after`, which must be the frame's final
/// schedule, and reclaim after frame timing starts but before `First`.
pub fn configure_world_service(app: &mut App, after: impl ScheduleLabel) {
    FrameStart::install(app);
    app.init_schedule(WorldServiceLaunch)
        .init_schedule(WorldServiceReclaim);
    let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
    order.insert_after(after, WorldServiceLaunch);
    order.insert_after(FrameStart, WorldServiceReclaim);
}
