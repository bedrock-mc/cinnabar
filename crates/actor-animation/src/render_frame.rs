use std::borrow::Cow;

use super::*;

pub(crate) mod cache;
pub(super) mod camera;
mod clips;
mod composition;
mod layers;
pub(super) mod motion_endpoint;
pub(super) mod motion_sampling;
mod retained;
mod rotation;
pub(super) mod sampling;

use motion_endpoint::{MotionEndpoint, MotionEndpointState};

/// Completed slices stay borrowed; sampled layers own only their frame's changed pose data.
#[derive(Clone)]
pub struct ActorRenderLayers<'a> {
    pub render: Cow<'a, [RenderTextureLayer]>,
    pub skin: Cow<'a, [SkinRenderLayer]>,
    /// Main-hand size evaluated with this frame's owner variables and query inputs.
    pub held_item_scale: Option<f32>,
}

/// A bounded render-layer evaluation frame, borrowing tick-owned actor and rig state.
pub struct ActorRenderFrame<'a> {
    store: &'a ActorAnimationStore,
    actors: &'a dyn AnimatedActors,
    camera_rotation: [f32; 2],
    camera_position: [f32; 3],
    alpha: f32,
    remaining_ops: usize,
    sampled_layers: HashMap<u64, Option<ActorRenderLayers<'a>>>,
}

impl<'a> ActorRenderFrame<'a> {
    /// Samples `store`'s rigs for `actors` from the view `[pitch, yaw]` and position.
    pub fn new(
        store: &'a ActorAnimationStore,
        actors: &'a dyn AnimatedActors,
        camera_rotation: [f32; 2],
        camera_position: [f32; 3],
        alpha: f32,
    ) -> Self {
        Self {
            store,
            actors,
            camera_rotation,
            camera_position,
            alpha: if alpha.is_finite() {
                alpha.clamp(0.0, 1.0)
            } else {
                0.0
            },
            remaining_ops: MAX_MOLANG_OPS_PER_RENDER_FRAME,
            sampled_layers: HashMap::new(),
        }
    }

    fn samples_rig_scale(&self, runtime_id: u64) -> bool {
        self.actors
            .animated_actor(runtime_id)
            .is_some_and(|actor| self.store.samples_rig_scale(actor))
    }

    fn may_use_always_depth_material(&self, runtime_id: u64) -> bool {
        self.actors
            .animated_actor(runtime_id)
            .is_some_and(|actor| self.store.may_use_always_depth_material(actor))
    }

    /// Whether any controller branch can replace the base mesh, including frame selections.
    /// Unknown rigs fail open. The cached skeleton keys include inactive geometry choices,
    /// so this check neither evaluates expressions nor consumes the frame's work budget.
    pub fn may_replace_body_geometry(&self, runtime_id: u64) -> bool {
        let Some(actor) = self.actors.animated_actor(runtime_id) else {
            return true;
        };
        let Some(state) = self
            .store
            .runtime_to_lifetime
            .get(&runtime_id)
            .filter(|lifetime| lifetime.spawn_revision == actor.spawn_revision())
            .and_then(|lifetime| self.store.rigs.get(lifetime))
        else {
            return true;
        };
        let assets = if state.pack {
            self.store.pack.as_ref().map(|pack| pack.assets.as_ref())
        } else {
            self.store.assets.as_deref()
        };
        let Some(base) =
            assets.and_then(|assets| assets.rig_geometries().get(state.geometry_binding))
        else {
            return true;
        };
        !state.initialized
            || state
                .layer_skeletons
                .first_key_value()
                .is_some_and(|(geometry, _)| *geometry != base.geometry)
            || state
                .layer_skeletons
                .last_key_value()
                .is_some_and(|(geometry, _)| *geometry != base.geometry)
    }

    /// Samples one actor's layers against the frame's shared expression budget.
    fn sample(
        store: &'a ActorAnimationStore,
        actors: &'a dyn AnimatedActors,
        (camera_rotation, camera_position, alpha): ([f32; 2], [f32; 3], f32),
        runtime_id: u64,
        remaining_ops: &mut usize,
        sample_skin: bool,
    ) -> Option<ActorRenderLayers<'a>> {
        store.render_layers(
            actors.animated_actor(runtime_id)?,
            alpha,
            camera_rotation,
            camera_position,
            remaining_ops,
            sample_skin,
        )
    }

    /// Retains one body, render and persona result for every consumer of this frame.
    fn cached_layers(&mut self, runtime_id: u64) -> Option<&ActorRenderLayers<'a>> {
        self.sampled_layers
            .entry(runtime_id)
            .or_insert_with(|| {
                Self::sample(
                    self.store,
                    self.actors,
                    (self.camera_rotation, self.camera_position, self.alpha),
                    runtime_id,
                    &mut self.remaining_ops,
                    true,
                )
            })
            .as_ref()
    }

    /// Samples authored scale before admission and caches its layers for the same draw.
    /// Ordinary rigs retain their tick scale without evaluating layer expressions.
    pub fn sample_rig_scale(&mut self, mut rig: ActorRigSnapshot<'a>) -> ActorRigSnapshot<'a> {
        let id = rig.actor.runtime_id;
        if self.samples_rig_scale(id)
            && let Some(scale) = self
                .cached_layers(id)
                .and_then(|layers| layers.render.iter().find_map(|layer| layer.sampled_scale))
        {
            rig.scale = scale[0];
            rig.axis_scale = [scale[1], scale[2], scale[3]];
        }
        rig
    }

    /// Shares the owner's sampled item scale with equipment under this frame's budget.
    pub fn sample_held_item_scale<'rig>(
        &mut self,
        mut rig: ActorRigSnapshot<'rig>,
    ) -> ActorRigSnapshot<'rig> {
        if rig.held_item_scale.is_some()
            && let Some(layers) = self.cached_layers(rig.actor.runtime_id)
        {
            rig.held_item_scale = layers.held_item_scale;
        }
        rig
    }

    /// Shares the frame's sampled layers without committing variables, clocks or poses.
    /// Unsupported or exhausted evaluations retain the completed tick's layers.
    pub fn layers(&mut self, runtime_id: u64) -> Option<Cow<'a, [RenderTextureLayer]>> {
        self.cached_layers(runtime_id)
            .map(|layers| layers.render.clone())
    }

    /// Samples possible Always rigs and retains the same frame layers for the eventual draw.
    /// Ordinary materials do not spend the shared frame budget during occlusion checks.
    pub fn has_always_depth_material(&mut self, runtime_id: u64) -> bool {
        self.may_use_always_depth_material(runtime_id)
            && self.cached_layers(runtime_id).is_some_and(|layers| {
                layers
                    .render
                    .iter()
                    .any(|layer| layer.material_state.is_some_and(|state| state.depth_always))
            })
    }

    /// Expression budget left for this frame's remaining samples.
    #[cfg(any(test, feature = "test-support"))]
    pub fn remaining_ops(&self) -> usize {
        self.remaining_ops
    }

    /// Caps the budget left for this frame's remaining samples.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_remaining_ops(&mut self, ops: usize) {
        self.remaining_ops = ops;
    }

    /// Samples the native body and its persona skeletons once under the same frame budget.
    pub fn layers_with_skin(&mut self, runtime_id: u64) -> Option<ActorRenderLayers<'a>> {
        self.cached_layers(runtime_id).cloned()
    }
}

#[derive(Debug)]
pub(super) struct FrameState {
    pub layers: std::sync::Mutex<render::cache::RenderCache>,
    pub compositions: std::sync::Mutex<composition::Compositions>,
    pub retained: std::sync::Mutex<Option<retained::Sample>>,
    pub motion: MotionEndpointState,
    /// Previous simulation tick, even when that tick did not evaluate a visual pose.
    pub previous_rotation: ActorTickInput,
    /// Previous simulation tick for walk sampling, kept even where rotation is not sampled.
    pub previous_walk: ActorTickInput,
    pub samples_camera_poses: bool,
    pub samples_swing_poses: bool,
    /// Completed selection and clock writes feed independently sampled layer channels.
    pub selection_effects: evaluation::MolangEffects,
    /// Completed channel writes survive frames that retain ordinary pose endpoints.
    pub retained_pose_effects: evaluation::MolangEffects,
    pub previous_motion: Option<MotionEndpointState>,
    pub motion_poses: Option<MotionPoses>,
    pub motion_layers: BTreeMap<u32, MotionPoses>,
    /// Swell samples used by the retained motion endpoints.
    pub swelling: [f32; 2],
}

impl FrameState {
    fn samples_rig_scale(&self, actor: &dyn AnimatedActor) -> bool {
        (self.needs_swell_sampling(actor)
            || self.needs_rotation_sampling()
            || self.needs_walk_sampling())
            && self
                .motion
                .sampling
                .as_ref()
                .is_some_and(|sampling| sampling.samples_rig_scale())
    }

    fn needs_swell_sampling(&self, actor: &dyn AnimatedActor) -> bool {
        self.motion_poses.is_some()
            && std::iter::once(&self.motion)
                .chain(self.previous_motion.iter())
                .any(|motion| motion.sampling.as_ref().is_some_and(|s| s.uses_swell()))
            && (actor.creeper_swell_changes()
                || self.swelling[0] != self.swelling[1]
                || actor.creeper_swell_amount(self.motion.context.frame_alpha) != self.swelling[1])
    }

    /// A changing tick pair requires angle sampling even at partial tick zero.
    fn needs_rotation_sampling(&self) -> bool {
        self.motion
            .sampling
            .as_ref()
            .is_some_and(|sampling| sampling.uses_rotation())
            && self.previous_rotation.rotation_changed(self.motion.input)
    }

    /// A changing stride or speed requires walk sampling between ticks.
    fn needs_walk_sampling(&self) -> bool {
        self.motion
            .sampling
            .as_ref()
            .is_some_and(|sampling| sampling.uses_walk())
            && self.previous_walk.walk_changed(self.motion.input)
    }

    pub(super) fn hold_motion(&mut self) {
        *self.retained.get_mut().unwrap() = None;
        self.previous_rotation = self.motion.input;
        self.previous_walk = self.motion.input;
        self.swelling[0] = self.swelling[1];
        if let Some(previous) = self.previous_motion.as_mut() {
            previous.clone_from(&self.motion);
        }
        for poses in self
            .motion_poses
            .iter_mut()
            .chain(self.motion_layers.values_mut())
        {
            poses.previous.clone_from(&poses.current);
            poses.previous_mask.clone_from(&poses.mask);
        }
    }
}

#[derive(Debug)]
pub(super) struct MotionPoses {
    pub previous: Vec<pose::LocalDelta>,
    pub current: Vec<pose::LocalDelta>,
    pub mask: Vec<[u8; 3]>,
    pub previous_mask: Vec<[u8; 3]>,
}

/// Carries sampled channel endpoints across ticks and retains them during draw-only refreshes.
pub(super) fn carry_motion_history(
    previous: Option<&mut FrameState>,
    next: &mut FrameState,
    advance: bool,
) {
    /// Moves matching pose history or seeds a newly admitted skeleton from its current pose.
    fn carry(previous: Option<&mut MotionPoses>, next: &mut MotionPoses, advance: bool) {
        if let Some(previous) =
            previous.filter(|previous| previous.current.len() == next.current.len())
        {
            next.previous_mask = if advance {
                std::mem::take(&mut previous.mask)
            } else {
                std::mem::take(&mut previous.previous_mask)
            };
            next.previous = if advance {
                std::mem::take(&mut previous.current)
            } else {
                std::mem::take(&mut previous.previous)
            };
        }
        if next.previous.is_empty() {
            next.previous.clone_from(&next.current);
        }
        if next.previous_mask.is_empty() {
            next.previous_mask.clone_from(&next.mask);
        }
    }
    next.swelling[0] = previous.as_ref().map_or(next.swelling[1], |frame| {
        frame.swelling[usize::from(advance)]
    });
    if !advance && let Some(previous) = previous.as_ref() {
        next.previous_rotation = previous.previous_rotation;
        next.previous_walk = previous.previous_walk;
    }
    let mut previous = previous;
    if next.motion_poses.is_some() {
        next.previous_motion = previous.as_deref_mut().map(|frame| {
            if advance {
                std::mem::take(&mut frame.motion)
            } else {
                frame
                    .previous_motion
                    .take()
                    .unwrap_or_else(|| frame.motion.clone())
            }
        });
    }
    if let Some(body) = next.motion_poses.as_mut() {
        carry(
            previous
                .as_deref_mut()
                .and_then(|frame| frame.motion_poses.as_mut()),
            body,
            advance,
        );
    }
    for (geometry, poses) in &mut next.motion_layers {
        carry(
            previous
                .as_deref_mut()
                .and_then(|frame| frame.motion_layers.get_mut(geometry)),
            poses,
            advance,
        );
    }
}

impl MotionPoses {
    fn sample(
        &self,
        current: &mut [pose::LocalDelta],
        sampled_previous: &[pose::LocalDelta],
        mask: Option<&[[u8; 3]]>,
    ) -> pose::PoseBuffer {
        let mut previous = pose::PoseBuffer::copy_of(&self.previous);
        for (
            (((((sample, completed), previous), sampled_previous), sampled_mask), current_mask),
            previous_mask,
        ) in current
            .iter_mut()
            .zip(&self.current)
            .zip(previous.iter_mut())
            .zip(sampled_previous)
            .zip(mask.unwrap_or(&self.mask))
            .zip(&self.mask)
            .zip(&self.previous_mask)
        {
            // Both endpoints compose their newly sampled attack delta.
            previous.legacy_swing = sampled_previous.legacy_swing;
            for (axis, bits) in sampled_mask.iter().enumerate() {
                let bits = bits | current_mask[axis] | previous_mask[axis];
                if bits & motion_sampling::TRANSLATION != 0 {
                    previous.translation[axis] = sampled_previous.translation[axis];
                } else {
                    sample.translation[axis] = completed.translation[axis];
                }
                if bits & motion_sampling::ROTATION != 0 {
                    previous.rotation[axis] = sampled_previous.rotation[axis];
                } else {
                    sample.rotation[axis] = completed.rotation[axis];
                }
                if bits & motion_sampling::SCALE != 0 {
                    previous.scale[axis] = sampled_previous.scale[axis];
                } else {
                    sample.scale[axis] = completed.scale[axis];
                }
            }
            if sampled_mask
                .iter()
                .zip(current_mask)
                .zip(previous_mask)
                .any(|((sampled, current), previous)| {
                    (sampled | current | previous) & motion_sampling::ROTATION_FRAME != 0
                })
            {
                previous.rotation_relative_to_entity = sampled_previous.rotation_relative_to_entity;
            } else {
                sample.rotation_relative_to_entity = completed.rotation_relative_to_entity;
            }
        }
        previous
    }
}

impl ActorAnimationStore {
    /// Checks the resolved rig's compiled layers, including inactive controller branches.
    pub fn may_use_always_depth_material(&self, actor: &dyn AnimatedActor) -> bool {
        self.runtime_to_lifetime
            .get(&actor.runtime_id())
            .filter(|lifetime| lifetime.spawn_revision == actor.spawn_revision())
            .and_then(|lifetime| self.rigs.get(lifetime))
            .is_some_and(|state| state.may_use_always_depth_material)
    }

    /// Whether admission needs a frame scale rather than the completed tick's scale.
    pub fn samples_rig_scale(&self, actor: &dyn AnimatedActor) -> bool {
        self.runtime_to_lifetime
            .get(&actor.runtime_id())
            .filter(|lifetime| lifetime.spawn_revision == actor.spawn_revision())
            .and_then(|lifetime| self.rigs.get(lifetime))
            .and_then(|state| state.render_frame.as_ref())
            .is_some_and(|frame| frame.samples_rig_scale(actor))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod orb_tests;
