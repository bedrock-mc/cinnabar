//! Frame boundaries shared by native pacing and browser event-loop composition.

use bevy::{
    app::MainScheduleOrder,
    ecs::schedule::ScheduleLabel,
    prelude::{App, First, Last},
};

/// Starts frame timing before reclaiming background work and running `First`.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameStart;

impl FrameStart {
    /// Installs the timing boundary once, preserving schedules already placed after it.
    pub fn install(app: &mut App) {
        app.init_schedule(Self);
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        if !order.labels.contains(&Self.intern()) {
            order.insert_before(First, Self);
        }
    }
}

/// Waits for the next frame only after the current frame's work has finished.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameEnd;

impl FrameEnd {
    /// Installs the wait boundary once so other owners can finish work before it.
    pub fn install(app: &mut App) {
        app.init_schedule(Self);
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        if !order.labels.contains(&Self.intern()) {
            order.insert_after(Last, Self);
        }
    }
}
