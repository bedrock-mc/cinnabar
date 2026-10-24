use assets::ItemVisualRoute;
use assets::facts::entity::NativeEntityClass;
use protocol::ActorMetadataValue;
use protocol_ids::ActorDataKey;

use super::{ActorSnapshot, ActorStore};

pub(super) const INVALID_LEASH_HOLDER_ID: i64 = -1;

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
    /// Runtime id of the actor holding a fishing rod; leads do not have an owner.
    pub owner_runtime_id: Option<u64>,
    pub from: [f32; 3],
    pub to: [f32; 3],
}

fn interpolated(actor: &ActorSnapshot, alpha: f32) -> [f32; 3] {
    std::array::from_fn(|axis| {
        actor.previous_pose.position[axis]
            + (actor.position[axis] - actor.previous_pose.position[axis]) * alpha
    })
}

pub(super) fn metadata_i64(actor: &ActorSnapshot, key: u32) -> Option<i64> {
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
    /// Reports whether an owned fishing hook is currently tracked for a player.
    pub(crate) fn has_fishing_hook(&self, owner_unique_id: i64) -> bool {
        self.effect_members
            .ropes
            .iter()
            .filter_map(|runtime| self.actors.get(runtime))
            .any(|actor| {
                actor.entity_fact.native_class == NativeEntityClass::FishingHook
                    && metadata_i64(actor, ActorDataKey::Owner.wire_value())
                        == Some(owner_unique_id)
            })
    }

    /// Replaces `candidates` with every block actor's view and terrain-interlock visibility.
    pub(crate) fn block_entity_candidates_into(
        &self,
        partial_tick: f32,
        candidates: &mut Vec<BlockEntityCandidate>,
    ) {
        self.block_entity_candidates_at_into(partial_tick, web_time::Instant::now(), candidates);
    }

    fn block_entity_candidates_at_into(
        &self,
        partial_tick: f32,
        now: web_time::Instant,
        candidates: &mut Vec<BlockEntityCandidate>,
    ) {
        let alpha = partial_tick.clamp(0.0, 1.0);
        candidates.clear();
        candidates.extend(
            self.effect_members
                .blocks
                .iter()
                .filter_map(|runtime| self.actors.get(runtime))
                .filter_map(|actor| {
                    Some(BlockEntityCandidate {
                        unique_id: actor.unique_id,
                        view: self.block_entity_view(actor, alpha)?,
                        visible: actor.status.terrain_interlock.visible_at(now),
                    })
                }),
        );
    }

    /// [`Self::block_entity_candidates_into`] at `now`, as a new list.
    #[cfg(test)]
    pub(super) fn block_entity_candidates_at(
        &self,
        partial_tick: f32,
        now: web_time::Instant,
    ) -> Vec<BlockEntityCandidate> {
        let mut candidates = Vec::new();
        self.block_entity_candidates_at_into(partial_tick, now, &mut candidates);
        candidates
    }

    /// Falling blocks and primed TNT with interpolated centres.
    pub(crate) fn block_entities(&self, partial_tick: f32) -> Vec<BlockEntityView> {
        self.block_entities_at(partial_tick, web_time::Instant::now())
    }

    pub(super) fn block_entities_at(
        &self,
        partial_tick: f32,
        now: web_time::Instant,
    ) -> Vec<BlockEntityView> {
        let alpha = partial_tick.clamp(0.0, 1.0);
        self.effect_members
            .blocks
            .iter()
            .filter_map(|runtime| self.actors.get(runtime))
            .filter(|actor| actor.status.terrain_interlock.visible_at(now))
            .filter_map(|actor| self.block_entity_view(actor, alpha))
            .collect::<Vec<_>>()
    }

    fn block_entity_view(&self, actor: &ActorSnapshot, alpha: f32) -> Option<BlockEntityView> {
        let feet = interpolated(actor, alpha);
        let center = [feet[0], feet[1] + BLOCK_ENTITY_CENTER_HEIGHT, feet[2]];
        match actor.entity_fact.native_class {
            NativeEntityClass::FallingBlock => {
                let ActorMetadataValue::Int(block) =
                    actor.metadata.get(&ActorDataKey::Variant.wire_value())?
                else {
                    return None;
                };
                Some(BlockEntityView {
                    runtime_id: actor.runtime_id,
                    kind: BlockEntityKind::Falling {
                        block_runtime_id: *block,
                    },
                    // The block's centre is its network position, half its scaled size up.
                    center: [feet[0], feet[1] + actor.network_position_offset(), feet[2]],
                    scale: actor.render_scale(),
                    flash: false,
                })
            }
            NativeEntityClass::Tnt => {
                let fuse = match actor.metadata.get(&ActorDataKey::FuseTime.wire_value()) {
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
                        visual: self.items.visual_for_identifier(
                            NativeEntityClass::Tnt
                                .identifier()
                                .expect("pinned TNT identity"),
                        ),
                    },
                    center,
                    scale,
                    flash,
                })
            }
            _ => None,
        }
    }

    /// Replaces `ropes` with fishing lines (owner rod tip to bobber) and leads (leashed mob to
    /// holder).
    pub(crate) fn ropes_into(&self, partial_tick: f32, ropes: &mut Vec<RopeView>) {
        let alpha = partial_tick.clamp(0.0, 1.0);
        ropes.clear();
        for runtime in &self.effect_members.ropes {
            let actor = &self.actors[runtime];
            if let Some(holder) = metadata_i64(actor, ActorDataKey::LeashHolder.wire_value())
                .filter(|holder| *holder != INVALID_LEASH_HOLDER_ID)
                .and_then(|holder| self.actor_by_unique(holder))
            {
                let attach = |actor: &ActorSnapshot| {
                    let position = interpolated(actor, alpha);
                    [position[0], position[1] + LEASH_ATTACH_HEIGHT, position[2]]
                };
                ropes.push(RopeView {
                    kind: RopeKind::Lead,
                    owner_runtime_id: None,
                    from: attach(actor),
                    to: attach(holder),
                });
            }
            let is_hook = actor.entity_fact.native_class == NativeEntityClass::FishingHook;
            if !is_hook {
                continue;
            }
            let Some(owner) = metadata_i64(actor, ActorDataKey::Owner.wire_value())
                .and_then(|id| self.actor_by_unique(id))
            else {
                continue;
            };
            let base = interpolated(owner, alpha);
            let yaw = owner.body_yaw.to_radians();
            let (sine, cosine) = yaw.sin_cos();
            ropes.push(RopeView {
                kind: RopeKind::FishingLine,
                owner_runtime_id: Some(owner.runtime_id),
                from: [
                    base[0] - sine * ROD_TIP_FORWARD + cosine * ROD_TIP_RIGHT,
                    base[1] + ROD_TIP_HEIGHT,
                    base[2] + cosine * ROD_TIP_FORWARD + sine * ROD_TIP_RIGHT,
                ],
                to: interpolated(actor, alpha),
            });
        }
    }

    /// [`Self::ropes_into`] as a new list.
    #[cfg(test)]
    pub(crate) fn ropes(&self, partial_tick: f32) -> Vec<RopeView> {
        let mut ropes = Vec::new();
        self.ropes_into(partial_tick, &mut ropes);
        ropes
    }

    fn actor_by_unique(&self, unique_id: i64) -> Option<&ActorSnapshot> {
        self.actors.get(self.unique_to_runtime.get(&unique_id)?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue};
    use protocol_ids::ActorDataKey;

    use super::*;

    #[test]
    fn tnt_flashes_in_alternating_bands_and_swells_only_at_the_end() {
        assert_eq!(tnt_presentation(80.0).0, 1.0);
        assert!(tnt_presentation(80.0).1 != tnt_presentation(75.0).1);
        assert!(tnt_presentation(2.0).0 > 1.0);
        assert!(tnt_presentation(2.0).0 <= 1.0 + TNT_SWELL_SCALE);
        assert_eq!(tnt_presentation(-3.0), tnt_presentation(0.0));
    }

    #[test]
    fn fishing_line_starts_from_the_players_right_hand() {
        let mut store = ActorStore::new(1, 0);
        let ActorEvent::Spawn(mut owner) = super::super::tests::spawn(1, 10) else {
            unreachable!()
        };
        owner.kind = ActorKind::Player {
            uuid: [1; 16],
            username: "owner".into(),
        };
        owner.position = [0.0; 3];
        owner.body_yaw = 0.0;
        store.apply(1, 1, ActorEvent::Spawn(owner));

        let ActorEvent::Spawn(mut hook) = super::super::tests::spawn(2, 20) else {
            unreachable!()
        };
        hook.kind = ActorKind::Entity {
            identifier: "minecraft:fishing_hook".into(),
        };
        hook.position = [0.0; 3];
        hook.metadata = Arc::from([ActorMetadata {
            key: ActorDataKey::Owner.wire_value(),
            value: ActorMetadataValue::Long(10),
        }]);
        store.apply(1, 2, ActorEvent::Spawn(hook));

        let ropes = store.ropes(0.0);
        assert_eq!(ropes.len(), 1);
        assert_eq!(ropes[0].kind, RopeKind::FishingLine);
        assert_eq!(ropes[0].owner_runtime_id, Some(1));
        assert_eq!(
            ropes[0].from,
            [ROD_TIP_RIGHT, ROD_TIP_HEIGHT, ROD_TIP_FORWARD]
        );
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::*;

    #[test]
    fn rope_views_allocate_nothing_without_matching_actors() {
        let mut store = ActorStore::new(1, 0);
        for runtime in 1..=512 {
            store.apply(
                1,
                runtime,
                super::super::tests::spawn(runtime, runtime as i64),
            );
        }
        let (ropes, allocations) = test_alloc::count(|| store.ropes(0.5));
        assert!(ropes.is_empty());
        assert_eq!(allocations, 0);
        println!("512 unrelated actors: rope-view allocations={allocations}");
    }
}
