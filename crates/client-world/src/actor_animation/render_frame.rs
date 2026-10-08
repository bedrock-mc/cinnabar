use std::borrow::Cow;

use super::*;

pub(super) mod camera;
mod clips;
pub(super) mod sampling;

/// Completed slices stay borrowed; sampled layers own only their frame's changed pose data.
pub struct ActorRenderLayers<'a> {
    pub render: Cow<'a, [RenderTextureLayer]>,
    pub skin: Cow<'a, [SkinRenderLayer]>,
}

/// A bounded render-layer evaluation frame, borrowing tick-owned actor and rig state.
pub struct ActorRenderFrame<'a> {
    store: &'a crate::actor_store::ActorStore,
    alpha: f32,
    remaining_ops: usize,
}

impl<'a> ActorRenderFrame<'a> {
    pub(crate) fn new(store: &'a crate::actor_store::ActorStore, alpha: f32) -> Self {
        Self {
            store,
            alpha: if alpha.is_finite() {
                alpha.clamp(0.0, 1.0)
            } else {
                0.0
            },
            remaining_ops: MAX_MOLANG_OPS_PER_RENDER_FRAME,
        }
    }

    /// Samples authored frame queries without committing variables, clocks or poses.
    /// Unsupported or exhausted evaluations retain the completed tick's layers.
    pub fn layers(&mut self, runtime_id: u64) -> Option<Cow<'a, [RenderTextureLayer]>> {
        self.store
            .render_layers(runtime_id, self.alpha, &mut self.remaining_ops, false)
            .map(|layers| layers.render)
    }

    /// Samples the native body and its persona skeletons once under the same frame budget.
    pub fn layers_with_skin(&mut self, runtime_id: u64) -> Option<ActorRenderLayers<'a>> {
        self.store
            .render_layers(runtime_id, self.alpha, &mut self.remaining_ops, true)
    }
}

#[derive(Debug)]
pub(super) struct FrameState {
    pub variables: MolangVariables,
    pub context: ActorTickContext,
    pub input: ActorTickInput,
    pub anim_tick: u64,
    pub clips: Vec<tick::WeightedClip>,
    pub swell_poses: Option<SwellPoses>,
    pub swell_layers: BTreeMap<u32, SwellPoses>,
    /// Swell samples used by the retained motion endpoints.
    pub swelling: [f32; 2],
}

impl FrameState {
    fn needs_swell_sampling(&self, actor: &ActorSnapshot) -> bool {
        self.swell_poses.is_some()
            && (actor.creeper_swell_changes()
                || self.swelling[0] != self.swelling[1]
                || actor.creeper_swell_amount(self.context.frame_alpha) != self.swelling[1])
    }

    pub(super) fn hold_motion(&mut self) {
        self.swelling[0] = self.swelling[1];
        for poses in self
            .swell_poses
            .iter_mut()
            .chain(self.swell_layers.values_mut())
        {
            poses.previous.clone_from(&poses.current);
        }
    }
}

#[derive(Debug)]
pub(super) struct SwellPoses {
    pub previous: Vec<pose::LocalDelta>,
    pub current: Vec<pose::LocalDelta>,
}

pub(super) fn carry_swell_history(
    previous: Option<&mut FrameState>,
    next: &mut FrameState,
    advance: bool,
) {
    fn carry(previous: Option<&mut SwellPoses>, next: &mut SwellPoses, advance: bool) {
        if let Some(previous) =
            previous.filter(|previous| previous.current.len() == next.current.len())
        {
            next.previous = if advance {
                std::mem::take(&mut previous.current)
            } else {
                std::mem::take(&mut previous.previous)
            };
        }
        if next.previous.is_empty() {
            next.previous.clone_from(&next.current);
        }
    }
    next.swelling[0] = previous.as_ref().map_or(next.swelling[1], |frame| {
        frame.swelling[usize::from(advance)]
    });
    let mut previous = previous;
    if let Some(body) = next.swell_poses.as_mut() {
        carry(
            previous
                .as_deref_mut()
                .and_then(|frame| frame.swell_poses.as_mut()),
            body,
            advance,
        );
    }
    for (geometry, poses) in &mut next.swell_layers {
        carry(
            previous
                .as_deref_mut()
                .and_then(|frame| frame.swell_layers.get_mut(geometry)),
            poses,
            advance,
        );
    }
}

impl SwellPoses {
    fn sample(&self, current: &mut [pose::LocalDelta]) -> Vec<pose::LocalDelta> {
        let mut previous = self.previous.clone();
        for ((sample, completed), previous) in
            current.iter_mut().zip(&self.current).zip(&mut previous)
        {
            // Swelling changes scale; ordinary motion retains both completed tick poses.
            previous.scale = sample.scale;
            sample.translation = completed.translation;
            sample.rotation = completed.rotation;
            sample.rotation_relative_to_entity = completed.rotation_relative_to_entity;
        }
        previous
    }
}

impl ActorAnimationStore {
    pub(crate) fn render_layers(
        &self,
        actor: &ActorSnapshot,
        partial_tick: f32,
        camera_rotation: [f32; 2],
        camera_position: [f32; 3],
        remaining_ops: &mut usize,
        sample_skin: bool,
    ) -> Option<ActorRenderLayers<'_>> {
        let lifetime = self.runtime_to_lifetime.get(&actor.runtime_id)?;
        let state = self.rigs.get(lifetime)?;
        if lifetime.spawn_revision != actor.spawn_revision {
            return None;
        }
        let completed = || ActorRenderLayers {
            render: Cow::Borrowed(state.render.as_slice()),
            skin: Cow::Borrowed(state.skin_layers.as_slice()),
        };
        let Some(frame) = state.render_frame.as_ref().filter(|frame| {
            (partial_tick > 0.0
                || state.samples_camera_poses
                || state.samples_swing_poses
                || frame.needs_swell_sampling(actor))
                && *remaining_ops > 0
                && (!state.culled || state.samples_camera_poses)
                && !state.reset_pending
        }) else {
            return Some(completed());
        };
        let (assets, layout) = if state.pack {
            let pack = self.pack.as_ref()?;
            (pack.assets.as_ref(), pack.layout.as_ref())
        } else {
            (self.assets.as_deref()?, self.layout.as_ref())
        };
        let swing = state
            .local_swing
            .map(|progress| progress.bedrock_progress(partial_tick));
        let swing_changed = state.samples_swing_poses
            && swing.is_some_and(|value| value != frame.input.attack_time);
        let swell_changed = frame.needs_swell_sampling(actor);
        if !state.samples_render_frames && !swing_changed && !swell_changed {
            return Some(completed());
        }
        let pose_inputs_changed = camera_rotation != frame.context.camera_rotation
            || camera_position != frame.context.camera_position
            || partial_tick != frame.context.frame_alpha;
        let mut context = frame.context.clone();
        context.frame_alpha = partial_tick;
        context.camera_rotation = camera_rotation;
        context.camera_position = camera_position;
        let evaluator = evaluation::Evaluator {
            assets,
            layout,
            program: None,
            actor,
            input: &frame.input,
            context: &context,
            anim_tick: frame.anim_tick,
            anim_time: None,
            life_tick: self.completed_tick.saturating_sub(state.lifetime_epoch),
            finished: (false, false),
            bones: state.posed_bones(),
            bone_names: state.posed_bone_names(),
        };
        let mut budget = EvalBudget {
            actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
            world_left: remaining_ops,
            work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
            transitions_left: 0,
            used: 0,
            stack: Vec::new(),
        };
        let mut variables = frame.variables.clone();
        if let Some(swing) = swing {
            variables.set(layout.engine.attack_time, swing);
        }
        let rig = assets.rig_bindings().get(state.rig_binding)?;
        if let Some(script) = rig.pre_animation
            && evaluator
                .run(script as usize, &mut variables, 0.0, &mut budget)
                .is_err()
        {
            return Some(completed());
        }
        tick::set_item_rotation_factor(&layout.engine, &mut variables);
        let sampled_clips = if swing_changed {
            let Ok(clips) =
                clips::sample(&evaluator, &mut variables, state, &frame.clips, &mut budget)
            else {
                return Some(completed());
            };
            Some(clips)
        } else {
            None
        };
        let clips = sampled_clips.as_deref().unwrap_or(&frame.clips);
        let mut sampled_local = if (state.samples_camera_poses && pose_inputs_changed)
            || swing_changed
            || swell_changed
        {
            let Ok(local) = pose::sample_clips(
                &evaluator,
                &mut variables,
                &state.bones,
                &state.bone_names,
                clips,
                &mut budget,
            ) else {
                return Some(completed());
            };
            Some(local)
        } else {
            None
        };
        let swell_previous = if swell_changed && !state.samples_camera_poses && !swing_changed {
            let Some(history) = frame.swell_poses.as_ref() else {
                return Some(completed());
            };
            let Some(local) = sampled_local.as_mut() else {
                return Some(completed());
            };
            let previous = history.sample(local);
            state.compose(&previous).map(Arc::<[BoneTransform]>::from)
        } else {
            None
        };
        let pose = match &sampled_local {
            Some(local) => {
                let Some(pose) = state.compose(local) else {
                    return Some(completed());
                };
                Some(Arc::<[BoneTransform]>::from(pose))
            }
            None => None,
        };
        let geometry = assets
            .rig_geometries()
            .get(state.geometry_binding)?
            .geometry;
        let Ok(mut layers) = render::evaluate_render(
            &evaluator,
            &mut variables,
            render::RenderRig {
                binding: state.rig_binding,
                geometry,
                bone_names: state.posed_bone_names(),
                skeletons: &state.layer_skeletons,
            },
            &mut budget,
        ) else {
            return Some(completed());
        };
        let mut sampled_geometries = BTreeMap::new();
        for (index, layer) in layers.iter_mut().enumerate() {
            let previous = state
                .render
                .get(index)
                .filter(|previous| previous.geometry == layer.geometry)
                .or_else(|| {
                    state
                        .render
                        .iter()
                        .find(|previous| previous.geometry == layer.geometry)
                });
            let sampled = match (&pose, layer.geometry) {
                (Some(pose), None) => Some(Arc::clone(pose)),
                (Some(_), Some(geometry)) => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        sampled_geometries.entry(geometry)
                    {
                        let Ok(mut local) = render::sample_layer_local(
                            &evaluator,
                            &variables,
                            &state.layer_skeletons,
                            clips,
                            geometry,
                            &mut budget,
                        ) else {
                            return Some(completed());
                        };
                        let Some(Some(skeleton)) = state.layer_skeletons.get(&geometry) else {
                            return Some(completed());
                        };
                        let previous_local = if swell_previous.is_some() {
                            frame
                                .swell_layers
                                .get(&geometry)
                                .map(|history| history.sample(&mut local))
                        } else {
                            None
                        };
                        let Some(current) = pose::compose_pose(&skeleton.bones, &local)
                            .map(Arc::<[BoneTransform]>::from)
                        else {
                            return Some(completed());
                        };
                        let previous = match previous_local {
                            Some(local) => {
                                let Some(previous) = pose::compose_pose(&skeleton.bones, &local)
                                else {
                                    return Some(completed());
                                };
                                Arc::from(previous)
                            }
                            None => Arc::clone(&current),
                        };
                        entry.insert((previous, current));
                    }
                    sampled_geometries
                        .get(&geometry)
                        .map(|(_, current)| Arc::clone(current))
                }
                (None, _) => None,
            };
            if let Some(pose) = sampled {
                layer.previous_pose = match layer.geometry {
                    None => swell_previous
                        .as_ref()
                        .map_or_else(|| Arc::clone(&pose), Arc::clone),
                    Some(geometry) => sampled_geometries
                        .get(&geometry)
                        .map_or_else(|| Arc::clone(&pose), |(previous, _)| Arc::clone(previous)),
                };
                layer.pose = pose;
            } else if let Some(previous) = previous {
                layer.previous_pose = Arc::clone(&previous.previous_pose);
                layer.pose = Arc::clone(&previous.pose);
            } else if layer.geometry.is_some() {
                return Some(completed());
            }
            if let Some(previous) = previous
                && previous.hidden_bones == layer.hidden_bones
            {
                layer.hidden_bones = Arc::clone(&previous.hidden_bones);
            }
        }
        let skin = match sampled_local.as_deref().filter(|_| sample_skin) {
            Some(local) if !state.skin_layers.is_empty() => {
                let Ok(skin) = skin_layers::sample(
                    state,
                    &evaluator,
                    &variables,
                    local,
                    Some(&layers),
                    &mut budget,
                ) else {
                    return Some(completed());
                };
                Cow::Owned(skin)
            }
            _ => Cow::Borrowed(state.skin_layers.as_slice()),
        };
        Some(ActorRenderLayers {
            render: Cow::Owned(layers),
            skin,
        })
    }
}

#[cfg(test)]
pub(in crate::actor_animation) mod tests;

#[cfg(test)]
mod orb_tests;

#[cfg(test)]
mod camera_tests;
