use assets::ItemVisualRoute;
use protocol::{ActorKind, ActorMetadataValue};

use super::{ActorSnapshot, ActorStore, FUSE_TIME_METADATA_KEY};

const DISPLAY_BLOCK_METADATA_KEY: u32 = 2;
const OWNER_METADATA_KEY: u32 = 5;
const LEASH_HOLDER_METADATA_KEY: u32 = 37;
const INVALID_LEASH_HOLDER_ID: i64 = -1;

// Provisional presentation constants; each needs independent measurement.
const BLOCK_ENTITY_CENTER_HEIGHT: f32 = 0.49;
const TNT_FLASH_PERIOD_TICKS: i32 = 5;
const TNT_SWELL_TICKS: f32 = 10.0;
const TNT_SWELL_SCALE: f32 = 0.3;
const ROD_TIP_HEIGHT: f32 = 1.0;
const ROD_TIP_FORWARD: f32 = 0.6;
const ROD_TIP_RIGHT: f32 = 0.35;
const LEASH_ATTACH_HEIGHT: f32 = 0.7;

#[derive(Debug, Clone, PartialEq)]
pub enum BlockEntityKind {
    /// A falling block; the network block id comes from the display-block metadata.
    Falling { block_runtime_id: i32 },
    /// Primed TNT; the block visual resolves from the item registry.
    PrimedTnt { visual: ItemVisualRoute },
}

/// A block-model entity ready to draw with its retained world geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntityView {
    pub runtime_id: u64,
    pub kind: BlockEntityKind,
    /// World centre of the block.
    pub center: [f32; 3],
    /// Uniform scale, above 1 while primed TNT swells.
    pub scale: f32,
    /// White flash active this frame.
    pub flash: bool,
}

/// Retained block geometry whose visibility can change with a terrain upload.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntityCandidate {
    pub unique_id: i64,
    pub view: BlockEntityView,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RopeKind {
    FishingLine,
    Lead,
}

/// A rope between two world points; sag and width are the renderer's concern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RopeView {
    pub kind: RopeKind,
    pub from: [f32; 3],
    pub to: [f32; 3],
}

fn interpolated(actor: &ActorSnapshot, alpha: f32) -> [f32; 3] {
    std::array::from_fn(|axis| {
        actor.previous_pose.position[axis]
            + (actor.position[axis] - actor.previous_pose.position[axis]) * alpha
    })
}

fn metadata_i64(actor: &ActorSnapshot, key: u32) -> Option<i64> {
    match actor.metadata.get(&key)? {
        ActorMetadataValue::Long(value) => Some(*value),
        ActorMetadataValue::Int(value) => Some(i64::from(*value)),
        _ => None,
    }
}

/// Swell scale and flash for primed TNT with `fuse` ticks remaining (fractional between ticks).
#[must_use]
pub fn tnt_presentation(fuse: f32) -> (f32, bool) {
    let remaining = fuse.max(0.0);
    let swell = (1.0 - remaining / TNT_SWELL_TICKS).clamp(0.0, 1.0);
    let scale = 1.0 + swell.powi(4) * TNT_SWELL_SCALE;
    let flash = (remaining.floor() as i32 / TNT_FLASH_PERIOD_TICKS) % 2 == 0;
    (scale, flash)
}

impl ActorStore {
    pub(crate) fn block_entity_candidates(&self, partial_tick: f32) -> Vec<BlockEntityCandidate> {
        self.block_entity_candidates_at(partial_tick, std::time::Instant::now())
    }

    pub(super) fn block_entity_candidates_at(
        &self,
        partial_tick: f32,
        now: std::time::Instant,
    ) -> Vec<BlockEntityCandidate> {
        let alpha = partial_tick.clamp(0.0, 1.0);
        let mut candidates = self
            .actors
            .values()
            .filter_map(|actor| {
                Some(BlockEntityCandidate {
                    unique_id: actor.unique_id,
                    view: self.block_entity_view(actor, alpha)?,
                    visible: actor.status.terrain_interlock.visible_at(now),
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable_by_key(|candidate| candidate.view.runtime_id);
        candidates
    }

    /// Falling blocks and primed TNT with interpolated centres.
    pub(crate) fn block_entities(&self, partial_tick: f32) -> Vec<BlockEntityView> {
        self.block_entities_at(partial_tick, std::time::Instant::now())
    }

    pub(super) fn block_entities_at(
        &self,
        partial_tick: f32,
        now: std::time::Instant,
    ) -> Vec<BlockEntityView> {
        let alpha = partial_tick.clamp(0.0, 1.0);
        let mut views = self
            .actors
            .values()
            .filter(|actor| actor.status.terrain_interlock.visible_at(now))
            .filter_map(|actor| self.block_entity_view(actor, alpha))
            .collect::<Vec<_>>();
        views.sort_unstable_by_key(|view| view.runtime_id);
        views
    }

    fn block_entity_view(&self, actor: &ActorSnapshot, alpha: f32) -> Option<BlockEntityView> {
        let ActorKind::Entity { identifier } = &actor.kind else {
            return None;
        };
        let feet = interpolated(actor, alpha);
        let center = [feet[0], feet[1] + BLOCK_ENTITY_CENTER_HEIGHT, feet[2]];
        match identifier.as_ref() {
            "minecraft:falling_block" => {
                let ActorMetadataValue::Int(block) =
                    actor.metadata.get(&DISPLAY_BLOCK_METADATA_KEY)?
                else {
                    return None;
                };
                Some(BlockEntityView {
                    runtime_id: actor.runtime_id,
                    kind: BlockEntityKind::Falling {
                        block_runtime_id: *block,
                    },
                    center: [
                        feet[0],
                        feet[1] + super::FALLING_BLOCK_NETWORK_OFFSET,
                        feet[2],
                    ],
                    scale: 1.0,
                    flash: false,
                })
            }
            "minecraft:tnt" => {
                let fuse = match actor.metadata.get(&FUSE_TIME_METADATA_KEY) {
                    Some(ActorMetadataValue::Int(value)) => {
                        let elapsed = actor
                            .status
                            .age_ticks
                            .saturating_sub(actor.status.fuse_age_ticks);
                        Some(*value as f32 - (elapsed as f32 + alpha))
                    }
                    _ => None,
                };
                let (scale, flash) = fuse.map_or((1.0, false), tnt_presentation);
                Some(BlockEntityView {
                    runtime_id: actor.runtime_id,
                    kind: BlockEntityKind::PrimedTnt {
                        visual: self.items.visual_for_identifier("minecraft:tnt"),
                    },
                    center,
                    scale,
                    flash,
                })
            }
            _ => None,
        }
    }

    /// Fishing lines (owner rod tip to bobber) and leads (leashed mob to holder).
    pub(crate) fn ropes(&self, partial_tick: f32) -> Vec<RopeView> {
        let alpha = partial_tick.clamp(0.0, 1.0);
        let mut ropes = Vec::new();
        let mut actors = self.actors.values().collect::<Vec<_>>();
        actors.sort_unstable_by_key(|actor| actor.runtime_id);
        for actor in actors {
            if let Some(holder) = metadata_i64(actor, LEASH_HOLDER_METADATA_KEY)
                .filter(|holder| *holder != INVALID_LEASH_HOLDER_ID)
                .and_then(|holder| self.actor_by_unique(holder))
            {
                let attach = |actor: &ActorSnapshot| {
                    let position = interpolated(actor, alpha);
                    [position[0], position[1] + LEASH_ATTACH_HEIGHT, position[2]]
                };
                ropes.push(RopeView {
                    kind: RopeKind::Lead,
                    from: attach(actor),
                    to: attach(holder),
                });
            }
            let is_hook = matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:fishing_hook");
            if !is_hook {
                continue;
            }
            let Some(owner) =
                metadata_i64(actor, OWNER_METADATA_KEY).and_then(|id| self.actor_by_unique(id))
            else {
                continue;
            };
            let base = interpolated(owner, alpha);
            let yaw = owner.body_yaw.to_radians();
            let (sine, cosine) = yaw.sin_cos();
            ropes.push(RopeView {
                kind: RopeKind::FishingLine,
                from: [
                    base[0] - sine * ROD_TIP_FORWARD - cosine * ROD_TIP_RIGHT,
                    base[1] + ROD_TIP_HEIGHT,
                    base[2] + cosine * ROD_TIP_FORWARD - sine * ROD_TIP_RIGHT,
                ],
                to: interpolated(actor, alpha),
            });
        }
        ropes
    }

    fn actor_by_unique(&self, unique_id: i64) -> Option<&ActorSnapshot> {
        self.actors.get(self.unique_to_runtime.get(&unique_id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tnt_flashes_in_alternating_bands_and_swells_only_at_the_end() {
        assert_eq!(tnt_presentation(80.0).0, 1.0);
        assert!(tnt_presentation(80.0).1 != tnt_presentation(75.0).1);
        assert!(tnt_presentation(2.0).0 > 1.0);
        assert!(tnt_presentation(2.0).0 <= 1.0 + TNT_SWELL_SCALE);
        assert_eq!(tnt_presentation(-3.0), tnt_presentation(0.0));
    }
}
