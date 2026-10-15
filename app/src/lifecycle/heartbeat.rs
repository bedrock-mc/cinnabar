//! Main-loop progress for the crash monitor's hang watchdog: one tick per frame, and a loading
//! phase while a world join blocks frames.

use bevy::prelude::*;
use launcher_host::lifecycle::crash::heartbeat::{self, Loading};

use crate::{runtime::world::ClientWorld, session::SessionController};

/// In-world frames a join stays loading for: with pipelined rendering the main thread waits on
/// the first in-world frame's rendering during the next frame.
const JOIN_SETTLE_FRAMES: u8 = 2;

/// Ticks the heartbeat at the start of every frame and marks joins as loading.
pub(crate) fn configure(app: &mut App) {
    app.add_systems(First, tick)
        .add_systems(Last, hold_join_loading);
}

fn tick() {
    heartbeat::tick();
}

/// A join in progress and how many in-world frames have finished since it ended.
#[derive(Default)]
struct JoinLoading {
    phase: Option<Loading>,
    settled_frames: u8,
}

impl JoinLoading {
    /// Opens the loading phase while `joining`, and closes it after [`JOIN_SETTLE_FRAMES`]
    /// frames without a join; frames already ticking never open one.
    fn observe(&mut self, joining: bool) {
        if joining {
            self.phase.get_or_insert_with(Loading::begin);
            self.settled_frames = 0;
        } else if self.phase.is_some() {
            self.settled_frames += 1;
            if self.settled_frames >= JOIN_SETTLE_FRAMES {
                self.phase = None;
            }
        }
    }

    #[cfg(test)]
    fn loading(&self) -> bool {
        self.phase.is_some()
    }
}

/// Holds a loading phase from the start of a join, including the frame that bootstraps the
/// world, until the first in-world frames are done.
fn hold_join_loading(
    world: Res<ClientWorld>,
    session: Res<SessionController>,
    mut join: Local<JoinLoading>,
) {
    join.observe(world.stream.is_none() && session.status().connecting);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_join_loads_until_the_first_in_world_frames_are_done() {
        let mut join = JoinLoading::default();
        join.observe(false);
        assert!(!join.loading(), "menus are not loading");
        join.observe(true);
        join.observe(true);
        assert!(join.loading());
        for _ in 1..JOIN_SETTLE_FRAMES {
            join.observe(false);
            assert!(join.loading(), "the first in-world frames still load");
        }
        join.observe(false);
        assert!(!join.loading());
    }

    #[test]
    fn a_new_join_restarts_the_settling_frames() {
        let mut join = JoinLoading::default();
        join.observe(true);
        join.observe(false);
        join.observe(true);
        for _ in 1..JOIN_SETTLE_FRAMES {
            join.observe(false);
        }
        assert!(join.loading());
    }
}
