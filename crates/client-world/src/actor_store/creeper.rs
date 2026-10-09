use super::{ActorKind, ActorSnapshot};

const IGNITED_FLAG: u32 = 10;
const MAX_SWELL_TICKS: u8 = 30;
pub(crate) const SWELL_FULL_TICKS: f32 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CreeperSwell {
    previous: u8,
    current: u8,
    direction: i8,
}

impl Default for CreeperSwell {
    fn default() -> Self {
        Self {
            previous: 0,
            current: 0,
            direction: -1,
        }
    }
}

impl CreeperSwell {
    pub(super) fn clear(&mut self) {
        self.previous = 0;
        self.current = 0;
    }
}

impl ActorSnapshot {
    pub(crate) fn is_creeper(&self) -> bool {
        matches!(&self.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:creeper")
    }

    pub(super) fn advance_creeper_swell(&mut self) {
        if !self.is_creeper() || self.status.dead {
            return;
        }
        let ignited = self.flag(IGNITED_FLAG);
        let swell = &mut self.status.creeper_swell;
        swell.previous = swell.current;
        swell.direction = if ignited { 1 } else { -1 };
        swell.current = if ignited {
            swell.current.saturating_add(1).min(MAX_SWELL_TICKS)
        } else {
            swell.current.saturating_sub(1)
        };
    }

    pub(crate) fn creeper_swell_amount(&self, partial_tick: f32) -> f32 {
        let swell = self.status.creeper_swell;
        let previous = f32::from(swell.previous);
        (previous + (f32::from(swell.current) - previous) * partial_tick) / SWELL_FULL_TICKS
    }

    pub(crate) fn creeper_swell_changes(&self) -> bool {
        self.status.creeper_swell.previous != self.status.creeper_swell.current
    }

    pub(crate) fn creeper_swelling_direction(&self) -> f32 {
        f32::from(self.status.creeper_swell.direction)
    }
}
