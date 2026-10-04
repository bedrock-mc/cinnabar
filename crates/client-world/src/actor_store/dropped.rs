use protocol::{ActorHandedness, ActorKind, ITEM_ACTOR_NETWORK_OFFSET};
use std::{
    hash::{BuildHasher, Hash},
    sync::OnceLock,
};

use super::{ActorSnapshot, ActorStore};
use crate::item::CanonicalItemStack;

/// Most stacked copies drawn for one dropped stack.
pub const MAX_DROPPED_ITEM_COPIES: usize = 4;

// ItemRenderer::render, current 1.26.50.26. The random phase belongs to
// vertical bob only, never spin; the half-tick subtraction is performed after the rate.
const SPIN_RATE_PER_TICK: f32 = 0.05;
const SPIN_HALF_TICK: f32 = SPIN_RATE_PER_TICK * 0.5;
const COPY_SPREAD: f32 = 0.2;
const DEFAULT_COLLECTOR_HEIGHT: f32 = 1.8;

/// One dropped-item stack ready to draw: interpolated origin, spin, and stacked copy offsets.
#[derive(Debug, Clone, PartialEq)]
pub struct DroppedItemView {
    pub runtime_id: u64,
    pub spawn_revision: u64,
    /// Native interpolated ItemActor origin (not collision feet), including pickup
    /// flight, before renderer bob/lift.
    pub position: [f32; 3],
    pub age_ticks: f32,
    pub bob_phase: f32,
    /// Bob/lift contribution remaining while the item flies to its collector.
    pub bob_multiplier: f32,
    pub render_scale: f32,
    pub yaw_radians: f32,
    pub item: CanonicalItemStack,
    /// Item-local offsets, before the spin rotation; only `copy_count` entries are live.
    pub copy_offsets: [[f32; 3]; MAX_DROPPED_ITEM_COPIES],
    pub copy_count: u8,
}

/// Visible copies for a stack of `count` items.
#[must_use]
pub const fn dropped_item_copy_count(count: u16) -> u8 {
    match count {
        0 | 1 => 1,
        2..=5 => 2,
        6..=20 => 3,
        _ => MAX_DROPPED_ITEM_COPIES as u8,
    }
}

/// Session-independent local entropy: vanilla's visual randomness is not transmitted. This
/// retains one uniform sample per lifetime without allocating per-frame state. Its random
/// generator is ours; the native phase range and renderer-wide sample ownership are retained.
fn random_unit(key: impl Hash) -> f32 {
    static RANDOM: OnceLock<std::collections::hash_map::RandomState> = OnceLock::new();
    let hash = RANDOM.get_or_init(Default::default).hash_one(key);
    (hash >> 40) as f32 / (1_u32 << 24) as f32
}

fn phase(lifetime: impl Hash) -> f32 {
    let value = random_unit((lifetime, "item bob"));
    value * std::f32::consts::PI + value * std::f32::consts::PI
}

fn copy_offsets(count: u8) -> [[f32; 3]; MAX_DROPPED_ITEM_COPIES] {
    let mut offsets = [[0.0; 3]; MAX_DROPPED_ITEM_COPIES];
    // The first copy stays on the entity origin.
    for (index, offset) in offsets
        .iter_mut()
        .enumerate()
        .take(usize::from(count))
        .skip(1)
    {
        // The renderer constructor owns this table, shared by all actors.
        *offset = std::array::from_fn(|axis| {
            let unit = random_unit((index, axis, "item copies"));
            (unit + unit - 1.0) * COPY_SPREAD
        });
    }
    offsets
}

impl ActorStore {
    /// Views for every dropped-item actor whose stack resolved; picked-up items fly to their
    /// collector and vanish once they arrive.
    pub(crate) fn dropped_items(&self, partial_tick: f32) -> Vec<DroppedItemView> {
        let alpha = partial_tick.clamp(0.0, 1.0);
        let mut views = self
            .actors
            .values()
            .filter_map(|actor| self.dropped_item_view(actor, alpha))
            .collect::<Vec<_>>();
        views.sort_unstable_by_key(|view| view.runtime_id);
        views
    }

    fn dropped_item_view(&self, actor: &ActorSnapshot, alpha: f32) -> Option<DroppedItemView> {
        let ActorKind::Entity { identifier } = &actor.kind else {
            return None;
        };
        if identifier.as_ref() != "minecraft:item" {
            return None;
        }
        let item = self
            .equipment_in_hand(actor.runtime_id, ActorHandedness::Right)?
            .item
            .clone();
        if item.identity.is_empty() {
            return None;
        }
        let ticks = actor.status.age_ticks as f32 + alpha;
        let mut position = actor.interpolated_position(alpha)?;
        // Native ActorRenderDispatcher uses StateVector origin,
        // whereas our actor store retains collision feet for boxes and brightness.
        position[1] += ITEM_ACTOR_NETWORK_OFFSET;
        let mut bob_multiplier = 1.0;
        if let Some(pickup) = actor.status.pickup {
            let progress =
                (f32::from(pickup.ticks) + alpha) / f32::from(super::PICKUP_DURATION_TICKS);
            if progress >= 1.0 {
                return None;
            }
            if let Some(collector) = self.actors.get(&pickup.collector_runtime_id) {
                let height = collector
                    .bounding_box()
                    .map_or(DEFAULT_COLLECTOR_HEIGHT, |(min, max)| max[1] - min[1]);
                let anchor = collector.interpolated_position(alpha)?;
                let target = [anchor[0], anchor[1] + height * 0.5, anchor[2]];
                position = std::array::from_fn(|axis| {
                    position[axis] + (target[axis] - position[axis]) * progress
                });
                bob_multiplier = 1.0 - progress;
            }
        }
        let copy_count = dropped_item_copy_count(item.identity.count);
        Some(DroppedItemView {
            runtime_id: actor.runtime_id,
            spawn_revision: actor.spawn_revision,
            position,
            age_ticks: ticks,
            bob_phase: phase((
                self.session_id,
                self.dimension,
                actor.runtime_id,
                actor.spawn_revision,
            )),
            bob_multiplier,
            render_scale: actor.render_scale(),
            yaw_radians: (ticks * SPIN_RATE_PER_TICK - SPIN_HALF_TICK).max(0.0),
            item,
            copy_offsets: copy_offsets(copy_count),
            copy_count,
        })
    }
}

#[cfg(test)]
mod tests;
