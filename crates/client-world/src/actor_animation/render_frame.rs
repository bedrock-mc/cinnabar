use std::borrow::Cow;

use super::*;

pub(super) mod camera;
mod clips;
pub(super) mod sampling;
pub(super) mod swell;
pub(super) mod swell_endpoint;

use swell_endpoint::{SwellEndpoint, SwellMotion};

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
    scale_layers: HashMap<u64, Option<Cow<'a, [RenderTextureLayer]>>>,
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
            scale_layers: HashMap::new(),
        }
    }

    /// Samples authored scale before admission and caches its layers for the same draw.
    /// Ordinary rigs retain their tick scale without evaluating layer expressions.
    pub fn sample_rig_scale(&mut self, mut rig: ActorRigSnapshot<'a>) -> ActorRigSnapshot<'a> {
        let id = rig.actor.runtime_id;
        if self.store.samples_rig_scale(id) {
            let layers = self.scale_layers.entry(id).or_insert_with(|| {
                self.store
                    .render_layers(id, self.alpha, &mut self.remaining_ops, false)
                    .map(|layers| layers.render)
            });
            if let Some(scale) = layers
                .as_ref()
                .and_then(|layers| layers.iter().find_map(|layer| layer.sampled_scale))
            {
                rig.scale = scale[0];
                rig.axis_scale = [scale[1], scale[2], scale[3]];
            }
        }
        rig
    }

    /// Samples authored frame queries without committing variables, clocks or poses.
    /// Unsupported or exhausted evaluations retain the completed tick's layers.
    pub fn layers(&mut self, runtime_id: u64) -> Option<Cow<'a, [RenderTextureLayer]>> {
        if let Some(layers) = self.scale_layers.remove(&runtime_id) {
            return layers;
        }
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
    pub motion: SwellMotion,
    pub previous_motion: Option<SwellMotion>,
    pub swell_poses: Option<SwellPoses>,
    pub swell_layers: BTreeMap<u32, SwellPoses>,
    /// Swell samples used by the retained motion endpoints.
    pub swelling: [f32; 2],
}

impl FrameState {
    fn samples_rig_scale(&self, actor: &ActorSnapshot) -> bool {
        self.needs_swell_sampling(actor)
            && self
                .motion
                .sampling
                .as_ref()
                .is_some_and(|sampling| sampling.samples_rig_scale())
    }

    fn needs_swell_sampling(&self, actor: &ActorSnapshot) -> bool {
        self.swell_poses.is_some()
            && std::iter::once(&self.motion)
                .chain(self.previous_motion.iter())
                .any(|motion| motion.sampling.as_ref().is_some_and(|s| s.uses_swell()))
            && (actor.creeper_swell_changes()
                || self.swelling[0] != self.swelling[1]
                || actor.creeper_swell_amount(self.motion.context.frame_alpha) != self.swelling[1])
    }

    pub(super) fn hold_motion(&mut self) {
        self.swelling[0] = self.swelling[1];
        if let Some(previous) = self.previous_motion.as_mut() {
            previous.clone_from(&self.motion);
        }
        for poses in self
            .swell_poses
            .iter_mut()
            .chain(self.swell_layers.values_mut())
        {
            poses.previous.clone_from(&poses.current);
            poses.previous_mask.clone_from(&poses.mask);
        }
    }
}

#[derive(Debug)]
pub(super) struct SwellPoses {
    pub previous: Vec<pose::LocalDelta>,
    pub current: Vec<pose::LocalDelta>,
    pub mask: Vec<[u8; 3]>,
    pub previous_mask: Vec<[u8; 3]>,
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
    let mut previous = previous;
    if next.swell_poses.is_some() {
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
    fn sample(
        &self,
        current: &mut [pose::LocalDelta],
        sampled_previous: &[pose::LocalDelta],
        mask: Option<&[[u8; 3]]>,
    ) -> Vec<pose::LocalDelta> {
        let mut previous = self.previous.clone();
        for (
            (((((sample, completed), previous), sampled_previous), sampled_mask), current_mask),
            previous_mask,
        ) in current
            .iter_mut()
            .zip(&self.current)
            .zip(&mut previous)
            .zip(sampled_previous)
            .zip(mask.unwrap_or(&self.mask))
            .zip(&self.mask)
            .zip(&self.previous_mask)
        {
            for (axis, bits) in sampled_mask.iter().enumerate() {
                let bits = bits | current_mask[axis] | previous_mask[axis];
                if bits & swell::TRANSLATION != 0 {
                    previous.translation[axis] = sampled_previous.translation[axis];
                } else {
                    sample.translation[axis] = completed.translation[axis];
                }
                if bits & swell::ROTATION != 0 {
                    previous.rotation[axis] = sampled_previous.rotation[axis];
                } else {
                    sample.rotation[axis] = completed.rotation[axis];
                }
                if bits & swell::SCALE != 0 {
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
                    (sampled | current | previous) & swell::ROTATION_FRAME != 0
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
    /// Whether admission needs a frame scale rather than the completed tick's scale.
    pub(crate) fn samples_rig_scale(&self, actor: &ActorSnapshot) -> bool {
        self.runtime_to_lifetime
            .get(&actor.runtime_id)
            .filter(|lifetime| lifetime.spawn_revision == actor.spawn_revision)
            .and_then(|lifetime| self.rigs.get(lifetime))
            .and_then(|state| state.render_frame.as_ref())
            .is_some_and(|frame| frame.samples_rig_scale(actor))
    }

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
                && (!state.culled || state.samples_camera_poses || frame.samples_rig_scale(actor))
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
            && swing.is_some_and(|value| value != frame.motion.input.attack_time);
        let swell_changed = frame.needs_swell_sampling(actor);
        if !state.samples_render_frames && !swing_changed && !swell_changed {
            return Some(completed());
        }
        let camera_inputs_changed = std::iter::once(&frame.motion)
            .chain(frame.previous_motion.iter())
            .any(|motion| {
                camera_rotation != motion.context.camera_rotation
                    || camera_position != motion.context.camera_position
            });
        let pose_inputs_changed =
            camera_inputs_changed || partial_tick != frame.motion.context.frame_alpha;
        let mut context = frame.motion.context.clone();
        context.frame_alpha = partial_tick;
        context.camera_rotation = camera_rotation;
        context.camera_position = camera_position;
        let evaluator = evaluation::Evaluator {
            assets,
            layout,
            program: None,
            actor,
            input: &frame.motion.input,
            context: &context,
            anim_tick: frame.motion.anim_tick,
            anim_time: None,
            swell_amount: None,
            presentation_alpha: None,
            query_history: None,
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
        let mut variables = frame.motion.variables.clone();
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
        if state.complete_spear_variables {
            layout.engine.spear.apply(
                &mut variables,
                actor,
                &context,
                &frame.motion.input,
                swing.unwrap_or(frame.motion.input.attack_time),
            );
        }
        tick::set_item_rotation_factor(&layout.engine, &mut variables);
        let isolated_swell = swell_changed
            && !(state.samples_camera_poses && camera_inputs_changed)
            && !swing_changed;
        let mut endpoints: Option<(SwellEndpoint<'_>, SwellEndpoint<'_>)> = if isolated_swell {
            let amount = actor.creeper_swell_amount(partial_tick);
            let previous = frame.previous_motion.as_ref().unwrap_or(&frame.motion);
            let Ok(previous) = previous.sample(evaluator, state, amount, false, &mut budget) else {
                return Some(completed());
            };
            let Ok(current) = frame
                .motion
                .sample(evaluator, state, amount, true, &mut budget)
            else {
                return Some(completed());
            };
            Some((previous, current))
        } else {
            None
        };
        let scale = if let Some((_, current)) = &endpoints {
            current.scale
        } else if swell_changed {
            let Ok(scale) = tick::evaluate_scale(&evaluator, rig, &mut variables, &mut budget)
            else {
                return Some(completed());
            };
            scale
        } else {
            None
        };
        let sampled_scale = frame
            .motion
            .sampling
            .as_ref()
            .filter(|_| swell_changed)
            .and_then(|sampling| sampling.sampled_scale(rig, scale, state.scale));
        let sampled_clips = if !isolated_swell
            && (swing_changed
                || (swell_changed
                    && state
                        .swell_sampling
                        .as_ref()
                        .is_some_and(|sampling| sampling.samples_clips())))
        {
            let Ok(clips) = clips::sample(
                &evaluator,
                &mut variables,
                state,
                clips::ClipHistory {
                    clips: &frame.motion.clips,
                    clocks: &state.clip_clocks,
                    controllers: &state.controllers,
                    journal: &frame.motion.journal,
                    server_effects: &frame.motion.server_effects,
                },
                state.swell_sampling.as_deref().filter(|_| swell_changed),
                &mut budget,
            ) else {
                return Some(completed());
            };
            Some(clips)
        } else {
            None
        };
        let mut sampled_local = if let Some((_, current)) = endpoints.as_mut() {
            let Ok(local) = pose::sample_clips(
                &current.evaluator,
                &mut current.variables,
                &state.bones,
                &state.bone_names,
                &current.clips,
                &mut budget,
            ) else {
                return Some(completed());
            };
            Some(local)
        } else if (state.samples_camera_poses && pose_inputs_changed)
            || swing_changed
            || swell_changed
        {
            let Ok(local) = pose::sample_clips(
                &evaluator,
                &mut variables,
                &state.bones,
                &state.bone_names,
                sampled_clips.as_deref().unwrap_or(&frame.motion.clips),
                &mut budget,
            ) else {
                return Some(completed());
            };
            Some(local)
        } else {
            None
        };
        let clips = endpoints
            .as_ref()
            .map(|(_, current)| current.clips.as_slice())
            .or(sampled_clips.as_deref())
            .unwrap_or(&frame.motion.clips);
        let sampled_swell_mask = (sampled_clips.is_some() || endpoints.is_some())
            .then_some(state.swell_sampling.as_ref())
            .flatten()
            .map(|sampling| {
                sampling.mask(
                    assets,
                    &state.bone_names,
                    clips
                        .iter()
                        .chain(
                            endpoints
                                .as_ref()
                                .map_or(&[][..], |(previous, _)| previous.clips.as_slice()),
                        )
                        .copied(),
                    None,
                )
            });
        let swell_previous = if let Some((previous, _)) = endpoints.as_mut() {
            let Some(history) = frame.swell_poses.as_ref() else {
                return Some(completed());
            };
            let Some(local) = sampled_local.as_mut() else {
                return Some(completed());
            };
            let Ok(sampled_previous) = pose::sample_clips(
                &previous.evaluator,
                &mut previous.variables,
                &state.bones,
                &state.bone_names,
                &previous.clips,
                &mut budget,
            ) else {
                return Some(completed());
            };
            let previous = history.sample(local, &sampled_previous, sampled_swell_mask.as_deref());
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
        if let Some((_, current)) = &endpoints {
            current.variables.publish_writes(&mut variables);
        }
        let render_rig = render::RenderRig {
            binding: state.rig_binding,
            geometry,
            bone_names: state.posed_bone_names(),
            skeletons: &state.layer_skeletons,
        };
        let Ok(mut layers) =
            render::evaluate_render(&evaluator, &mut variables, render_rig, &mut budget)
        else {
            return Some(completed());
        };
        if state
            .swell_sampling
            .as_ref()
            .is_some_and(|sampling| sampling.samples_render_writes())
            && let Some((previous, current)) = endpoints.as_mut()
        {
            // Authored writes retain each geometry endpoint's ordinary inputs.
            for endpoint in [previous, current] {
                if render::evaluate_render(
                    &endpoint.evaluator,
                    &mut endpoint.variables,
                    render_rig,
                    &mut budget,
                )
                .is_err()
                {
                    return Some(completed());
                }
            }
        }
        let clips = endpoints
            .as_ref()
            .map(|(_, current)| current.clips.as_slice())
            .or(sampled_clips.as_deref())
            .unwrap_or(&frame.motion.clips);
        let mut sampled_geometries = BTreeMap::new();
        for (index, layer) in layers.iter_mut().enumerate() {
            layer.sampled_scale = sampled_scale;
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
                        let (layer_evaluator, layer_variables) = endpoints
                            .as_ref()
                            .map_or((&evaluator, &variables), |(_, current)| {
                                (&current.evaluator, &current.variables)
                            });
                        let Ok(mut local) = render::sample_layer_local(
                            layer_evaluator,
                            layer_variables,
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
                        let sampled_mask = (sampled_clips.is_some() || endpoints.is_some())
                            .then_some(state.swell_sampling.as_ref())
                            .flatten()
                            .map(|sampling| {
                                sampling.mask(
                                    assets,
                                    &skeleton.names,
                                    clips
                                        .iter()
                                        .chain(
                                            endpoints.as_ref().map_or(&[][..], |(previous, _)| {
                                                previous.clips.as_slice()
                                            }),
                                        )
                                        .copied(),
                                    Some(geometry),
                                )
                            });
                        let previous_local = if let Some((previous, _)) = endpoints.as_ref() {
                            let Ok(sampled_previous) = render::sample_layer_local(
                                &previous.evaluator,
                                &previous.variables,
                                &state.layer_skeletons,
                                &previous.clips,
                                geometry,
                                &mut budget,
                            ) else {
                                return Some(completed());
                            };
                            Some(match frame.swell_layers.get(&geometry) {
                                Some(history) => history.sample(
                                    &mut local,
                                    &sampled_previous,
                                    sampled_mask.as_deref(),
                                ),
                                None => sampled_previous,
                            })
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
