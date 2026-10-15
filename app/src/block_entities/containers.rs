//! Lid openness for chests and shulker boxes, eased toward the server's open cue.

use bevy::platform::collections::HashMap;

const TICKS_PER_SECOND: f32 = 20.0;
/// Openness change per tick while a lid moves; needs native measurement per container.
const CHEST_RATE_PER_TICK: f32 = 0.1;
const SHULKER_RATE_PER_TICK: f32 = 0.1;

/// The `BlockEventPacket` type carrying a container's viewer state.
const CONTAINER_EVENT_TYPE: i32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ContainerKind {
    Chest,
    Shulker,
}

impl ContainerKind {
    fn rate(self) -> f32 {
        match self {
            Self::Chest => CHEST_RATE_PER_TICK,
            Self::Shulker => SHULKER_RATE_PER_TICK,
        }
    }
}

/// Whether a container cue means a viewer has it open.
pub(super) fn cue_is_open(event_type: i32, event_value: i32) -> bool {
    event_type == CONTAINER_EVENT_TYPE && event_value > 0
}

#[derive(Debug, Default)]
pub(super) struct ContainerLids {
    /// Openness and the frame that last advanced it.
    openness: HashMap<[i32; 3], (f32, u64)>,
    frame: u64,
}

impl ContainerLids {
    /// Starts a frame; positions not touched before [`Self::finish`] are forgotten.
    pub(super) fn begin(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Advances the lid at `position` toward its target and returns the raw openness.
    pub(super) fn advance(
        &mut self,
        position: [i32; 3],
        kind: ContainerKind,
        open: bool,
        delta_seconds: f32,
    ) -> f32 {
        let step = kind.rate() * TICKS_PER_SECOND * delta_seconds.max(0.0);
        let (value, seen) = self.openness.entry(position).or_insert((0.0, self.frame));
        *seen = self.frame;
        *value = if open {
            (*value + step).min(1.0)
        } else {
            (*value - step).max(0.0)
        };
        *value
    }

    pub(super) fn finish(&mut self) {
        let frame = self.frame;
        self.openness.retain(|_, (_, seen)| *seen == frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lids_ease_toward_the_cue_at_a_fixed_tick_rate() {
        let mut lids = ContainerLids::default();
        lids.begin();
        // Half a second is ten ticks: fully open at 0.1 per tick.
        let open = lids.advance([0; 3], ContainerKind::Chest, true, 0.5);
        assert!((open - 1.0).abs() < 1.0e-6);
        lids.finish();
        lids.begin();
        let closing = lids.advance([0; 3], ContainerKind::Chest, false, 0.25);
        assert!((closing - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn untouched_positions_are_forgotten() {
        let mut lids = ContainerLids::default();
        lids.begin();
        lids.advance([1; 3], ContainerKind::Shulker, true, 0.05);
        lids.finish();
        lids.begin();
        lids.finish();
        assert!(lids.openness.is_empty());
    }

    #[test]
    fn only_viewer_count_cues_open_a_lid() {
        assert!(cue_is_open(1, 2));
        assert!(!cue_is_open(1, 0));
        assert!(!cue_is_open(0, 1));
    }
}
