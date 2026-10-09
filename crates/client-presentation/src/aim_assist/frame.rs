use std::sync::Arc;

use bevy::prelude::{Resource, Vec3};
use client_world::{ActorSnapshot, WorldAuthority};
use protocol::{ActorMetadataValue, CameraAimAssistTargetMode};
use sim::PaletteWorld;

use super::{
    AIM_ASSIST_ACTOR_METADATA_KEY, AIM_ASSIST_CATEGORY_METADATA_KEY,
    AIM_ASSIST_PRESET_METADATA_KEY, AimAssistCandidate, AimAssistFrustum, AimAssistTarget,
    ServerAimAssist, TargetKind,
};

/// Vanilla bounds actor admission before visibility and ranking.
const MAX_ENTITY_CANDIDATES: usize = 101;
/// Finite hostile view parameters cannot create unbounded ray work.
pub(super) const MAX_BLOCK_SAMPLE_RAYS: usize = 4096;

/// Reusable two-tick candidate cache; mutation packets allocate outside evaluation.
#[derive(Resource, Debug)]
pub struct AimAssistFrame {
    pub target: Option<AimAssistTarget>,
    pub skipped_queries: u64,
    pub visibility_queries: u64,
    pub(super) entities: [Option<AimAssistCandidate>; MAX_ENTITY_CANDIDATES],
    pub(super) blocks: Vec<sim::CameraBlockHit>,
    frustum: Option<AimAssistFrustum>,
    held_item: Option<Arc<str>>,
    last_tick: Option<u64>,
    aim_tick: u32,
    revision: u64,
    pub(super) target_liquids: bool,
    pub(super) max_steps: u32,
}

impl Default for AimAssistFrame {
    /// Candidate capacity is committed before any frame is evaluated.
    fn default() -> Self {
        Self {
            target: None,
            skipped_queries: 0,
            visibility_queries: 0,
            entities: [None; MAX_ENTITY_CANDIDATES],
            blocks: Vec::with_capacity(MAX_BLOCK_SAMPLE_RAYS),
            frustum: None,
            held_item: None,
            last_tick: None,
            aim_tick: 0,
            revision: 0,
            target_liquids: false,
            max_steps: 0,
        }
    }
}

impl AimAssistFrame {
    /// The last completed simulation tick lets adapters consume catch-up poses in order.
    pub fn last_tick(&self) -> Option<u64> {
        self.last_tick
    }

    /// Interaction and action rotation share the target direction from the cached tick eye.
    #[must_use]
    pub fn interaction_direction(&self) -> Option<Vec3> {
        (self.target?.point - self.frustum?.origin).try_normalize()
    }

    /// Clear and policy replacement invalidate targets before the next simulation tick.
    pub fn synchronize(&mut self, state: &ServerAimAssist) {
        if self.revision != state.revision() || state.settings().is_none() {
            self.target = None;
            self.frustum = None;
            self.entities.fill(None);
            self.blocks.clear();
            self.last_tick = None;
            self.aim_tick = 0;
            self.revision = state.revision();
        }
    }

    /// Evaluates each observed simulation tick once and retains the result between ticks.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate<'a>(
        &mut self,
        state: &ServerAimAssist,
        world: &WorldAuthority,
        blocks: &PaletteWorld<'_>,
        tick: u64,
        origin: Vec3,
        forward: Vec3,
        local_actor: Option<&ActorSnapshot>,
        held_item: Option<&Arc<str>>,
        block_identifier: impl Fn(u32) -> Option<&'a str>,
        block_tags: impl Fn(u32) -> &'a [Arc<str>],
    ) {
        self.synchronize(state);
        let Some(settings) = state.settings() else {
            return;
        };
        if self.last_tick == Some(tick) {
            return;
        }
        self.last_tick = Some(tick);
        self.aim_tick = self.aim_tick.wrapping_add(1);
        let odd = self.aim_tick & 1 != 0;
        if odd {
            self.frustum =
                AimAssistFrustum::new(origin, forward, settings.view_angle, settings.distance);
            self.held_item = held_item.cloned();
            self.max_steps = settings.distance as u32;
            self.target_liquids = state.category(self.held_item.as_deref()).target_liquids
                && match blocks.camera_eye_in_liquid(vector(origin)) {
                    Ok(in_liquid) => !in_liquid,
                    Err(_) => {
                        self.skipped_queries += 1;
                        false
                    }
                };
            self.blocks.clear();
            self.entities.fill(None);
            self.capture_entities(state, world, local_actor);
            if self.target.is_some_and(|target| matches!(target.kind, TargetKind::Actor(id) if world.actor(id).is_none_or(|actor| actor.status.dead))) {
                self.target = None;
            }
        }
        let Some(frustum) = self.frustum else {
            self.skipped_queries += 1;
            return;
        };
        self.capture_blocks(blocks, frustum, settings.distance, odd);
        if odd {
            return;
        }
        self.target = None;
        let preset_index = metadata_index(local_actor, AIM_ASSIST_PRESET_METADATA_KEY);
        let category_index = metadata_index(local_actor, AIM_ASSIST_CATEGORY_METADATA_KEY);
        for index in 0..MAX_ENTITY_CANDIDATES {
            let Some(mut candidate) = self.entities[index] else {
                break;
            };
            if let TargetKind::Actor(id) = candidate.kind {
                candidate.priority = state.player_priority(
                    preset_index,
                    category_index,
                    metadata_index(world.actor(id), AIM_ASSIST_ACTOR_METADATA_KEY),
                );
            }
            let delta = candidate.point - frustum.origin;
            let visibility_end = candidate.point
                + if self.target_liquids {
                    Vec3::Y * 0.2
                } else {
                    Vec3::ZERO
                };
            candidate.obstructed = self.obstructed(blocks, frustum.origin, visibility_end);
            if !candidate.obstructed && settings.target_mode == CameraAimAssistTargetMode::Distance
            {
                candidate.obstructed = self.entities.iter().flatten().any(|other| {
                    other.kind != candidate.kind
                        && segment_intersects(frustum.origin, delta, other.minimum, other.maximum)
                });
            }
            self.keep_best(frustum.select(settings.target_mode, [candidate]));
        }
        let held_item = self.held_item.clone();
        self.select_blocks(
            blocks,
            frustum,
            settings.target_mode,
            state.category(held_item.as_deref()),
            block_identifier,
            block_tags,
        );
        if self.target.is_some_and(|target| {
            matches!(target.kind,
            TargetKind::Actor(id) if world.actor(id).is_none())
        }) {
            self.target = None;
        }
    }

    /// Captures visible actor geometry once for each two-tick search.
    fn capture_entities(
        &mut self,
        state: &ServerAimAssist,
        world: &WorldAuthority,
        local_actor: Option<&ActorSnapshot>,
    ) {
        let Some(frustum) = self.frustum else {
            return;
        };
        let preset_index = metadata_index(local_actor, AIM_ASSIST_PRESET_METADATA_KEY);
        let category_index = metadata_index(local_actor, AIM_ASSIST_CATEGORY_METADATA_KEY);
        let mut count = 0;
        for actor in world.remote_actors() {
            if actor.status.dead {
                continue;
            }
            match world.camera_aim_assist_eligible(actor) {
                Some(true) => {}
                Some(false) => continue,
                None => {
                    self.skipped_queries += 1;
                    continue;
                }
            }
            let actor_index = metadata_index(Some(actor), AIM_ASSIST_ACTOR_METADATA_KEY);
            let priority = if matches!(actor.kind, protocol::ActorKind::Player { .. }) {
                Some(state.player_priority(preset_index, category_index, actor_index))
            } else {
                state.actor_priority(preset_index, category_index, actor_index)
            };
            let Some(priority) = priority else {
                continue;
            };
            // Custom hitboxes aim at the box enclosing all of them.
            let Some((minimum, maximum)) = world
                .pick_hit_boxes(actor)
                .map(|(min, max)| (Vec3::from_array(min), Vec3::from_array(max)))
                .reduce(|(min, max), (other_min, other_max)| {
                    (min.min(other_min), max.max(other_max))
                })
            else {
                continue;
            };
            if !frustum.contains_box(minimum, maximum) {
                continue;
            }
            self.entities[count] = Some(AimAssistCandidate {
                kind: TargetKind::Actor(actor.runtime_id),
                minimum,
                maximum,
                point: (minimum + maximum) * 0.5,
                priority,
                obstructed: false,
            });
            count += 1;
            if count == MAX_ENTITY_CANDIDATES {
                break;
            }
        }
    }

    /// Keeps the earlier target when weighted scores tie.
    pub(super) fn keep_best(&mut self, candidate: Option<AimAssistTarget>) {
        if let Some(candidate) = candidate
            && self.target.is_none_or(|old| candidate.score < old.score)
        {
            self.target = Some(candidate);
        }
    }

    /// Missing collision evidence suppresses only the affected candidate.
    pub(super) fn obstructed(&mut self, world: &PaletteWorld<'_>, origin: Vec3, end: Vec3) -> bool {
        let delta = end - origin;
        self.visibility_queries += 1;
        if delta.length_squared() == 0.0 {
            return false;
        }
        match world.camera_aim_ray(
            vector(origin),
            vector(end),
            self.max_steps,
            true,
            self.target_liquids,
        ) {
            Ok(hit) => hit.is_some(),
            Err(_) => {
                self.skipped_queries += 1;
                true
            }
        }
    }
}

/// Missing or differently typed metadata uses the native zero table index.
fn metadata_index(actor: Option<&ActorSnapshot>, key: u32) -> i32 {
    match actor.and_then(|actor| actor.metadata.get(&key)) {
        Some(ActorMetadataValue::Int(value)) => *value,
        _ => 0,
    }
}

/// Converts camera coordinates for the shared collision query.
pub(super) fn vector(value: Vec3) -> sim::Vec3 {
    sim::Vec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
}

/// Segment-box clipping excludes intersections beyond the target center.
fn segment_intersects(origin: Vec3, delta: Vec3, minimum: Vec3, maximum: Vec3) -> bool {
    let mut near = 0.0_f32;
    let mut far = 1.0_f32;
    for axis in 0..3 {
        if delta[axis] == 0.0 {
            if origin[axis] < minimum[axis] || origin[axis] > maximum[axis] {
                return false;
            }
        } else {
            let first = (minimum[axis] - origin[axis]) / delta[axis];
            let second = (maximum[axis] - origin[axis]) / delta[axis];
            near = near.max(first.min(second));
            far = far.min(first.max(second));
            if near > far {
                return false;
            }
        }
    }
    near < 1.0 && far > 0.0
}
