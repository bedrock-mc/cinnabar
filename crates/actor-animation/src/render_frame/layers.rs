//! Bounded read-only sampling of an actor's rendered layers and owner properties.
use super::*;

impl ActorAnimationStore {
    /// Samples layers and owner equipment scale without committing animation state.
    /// Exhausted evaluations retain the completed tick's outputs under the shared budget.
    pub fn render_layers(
        &self,
        actor: &dyn AnimatedActor,
        partial_tick: f32,
        camera_rotation: [f32; 2],
        camera_position: [f32; 3],
        remaining_ops: &mut usize,
        sample_skin: bool,
    ) -> Option<ActorRenderLayers<'_>> {
        let lifetime = self.runtime_to_lifetime.get(&actor.runtime_id())?;
        let state = self.rigs.get(lifetime)?;
        if lifetime.spawn_revision != actor.spawn_revision() {
            return None;
        }
        let completed = || ActorRenderLayers {
            render: Cow::Borrowed(state.render.as_slice()),
            skin: Cow::Borrowed(state.skin_layers.as_slice()),
            held_item_scale: state.held_item_scale,
        };
        let fallback = || {
            self.frame_fallbacks
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            completed()
        };
        let Some(frame) = state.render_frame.as_ref().filter(|frame| {
            (partial_tick > 0.0
                || state.samples_camera_poses
                || state.samples_swing_poses
                || frame.needs_swell_sampling(actor)
                || frame.needs_rotation_sampling()
                || frame.needs_walk_sampling())
                && (!state.culled
                    || state.samples_camera_poses
                    || state.held_item_scale.is_some()
                    || frame.samples_rig_scale(actor))
                && !state.reset_pending
        }) else {
            return Some(completed());
        };
        if *remaining_ops == 0 {
            return Some(fallback());
        }
        let (assets, layout) = if state.pack {
            let pack = self.pack.as_ref()?;
            (pack.assets.as_ref(), pack.layout.as_ref())
        } else {
            (self.assets.as_deref()?, self.layout.as_ref())
        };
        // Local swings follow the physics clock; remote swings blend their last two ticks.
        let swing_source = state
            .local_swing
            .map(|progress| (progress.bedrock, progress.frame_alpha))
            .or_else(|| state.remote_swing().map(|pair| (pair, None)));
        let swing = swing_source.map(|(pair, alpha)| {
            LocalSwingProgress::interpolate(pair, alpha.unwrap_or(partial_tick))
        });
        let swing_changed = state.samples_swing_poses
            && swing_source.is_some_and(|(pair, _)| {
                pair[0] != pair[1] || swing != Some(frame.motion.input.attack_time)
            });
        let swell_changed = frame.needs_swell_sampling(actor);
        let rotation_changed = frame.needs_rotation_sampling();
        let walk_changed = frame.needs_walk_sampling();
        let motion_changed = swell_changed || rotation_changed || walk_changed;
        if !state.samples_render_frames && !swing_changed && !motion_changed {
            return Some(completed());
        }
        let camera_inputs_changed = std::iter::once(&frame.motion)
            .chain(frame.previous_motion.iter())
            .any(|motion| {
                camera_rotation != motion.context.camera_rotation
                    || camera_position != motion.context.camera_position
            });
        let pose_inputs_changed = camera_inputs_changed
            || rotation_changed
            || walk_changed
            || partial_tick != frame.motion.context.frame_alpha;
        let previous_motion = frame.previous_motion.as_ref().unwrap_or(&frame.motion);
        // Both endpoints read the frame's rotation and walk, retaining their other inputs.
        let sample_frame = |input: ActorTickInput| {
            let current = frame.motion.input;
            let input = if rotation_changed {
                input.sample_rotation(frame.previous_rotation, current, partial_tick)
            } else {
                input
            };
            if walk_changed {
                input.sample_walk(frame.previous_walk, current, partial_tick)
            } else {
                input
            }
        };
        let current_input = sample_frame(frame.motion.input);
        let previous_input = sample_frame(previous_motion.input);
        let mut context = frame.motion.context.clone();
        context.frame_alpha = partial_tick;
        context.camera_rotation = camera_rotation;
        context.camera_position = camera_position;
        let evaluator = evaluation::Evaluator {
            assets,
            pack: state.pack,
            layout,
            program: None,
            actor,
            input: &current_input,
            context: &context,
            anim_tick: frame.motion.anim_tick,
            anim_time: None,
            swell_amount: None,
            presentation_alpha: None,
            query_history: None,
            life_tick: self.completed_tick.saturating_sub(state.lifetime_epoch),
            finished: (false, false),
            state_time: 0.0,
            bones: state.posed_bones(),
            bone_names: state.posed_bone_names(),
        };
        let mut budget = EvalBudget {
            actor_left: MAX_MOLANG_OPS_PER_ACTOR_FRAME,
            world_left: remaining_ops,
            work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_FRAME,
            transitions_left: 0,
            used: 0,
            stack: OperandStack::lease(),
            static_draw: None,
        };
        let mut retained = frame.retained.lock().unwrap();
        let retained = if cache::enabled()
            && (!state.complete_spear_variables || layout.engine.spear.is_empty())
        {
            Some(retained.get_or_insert_with(|| retained::Sample::new(&evaluator, state, frame)))
        } else {
            None
        };
        let mut retained = retained;
        if let Some(retained) = retained.as_mut()
            && let Some(layers) = retained.get(
                retained::Key::new(&evaluator, state, motion_changed, swing, sample_skin),
                &evaluator,
                &frame.motion.variables,
                state,
                &budget,
            )
        {
            return Some(layers);
        }
        let mut variables = frame.motion.scratch.lease(&frame.motion.variables);
        if let Some(swing) = swing {
            variables.set(layout.engine.attack_time, swing);
        }
        let rig = assets.rig_bindings().get(state.rig_binding)?;
        let Ok(held_item_scale) =
            tick::scale::evaluate_held_item(&evaluator, rig, &mut variables, &mut budget)
        else {
            return Some(fallback());
        };
        if let Some(script) = rig.pre_animation
            && evaluator
                .run(script as usize, &mut variables, 0.0, &mut budget)
                .is_err()
        {
            return Some(fallback());
        }
        if state.complete_spear_variables {
            layout.engine.spear.apply(
                &mut variables,
                actor,
                &context,
                &current_input,
                swing.unwrap_or(current_input.attack_time),
            );
        }
        tick::set_item_rotation_factor(&layout.engine, &mut variables);
        let body_swing_changed = frame.samples_swing_poses && swing_changed;
        // Camera-dependent channels resample both endpoints with the live camera, so a camera
        // frame still blends tick motion instead of drawing one latest-tick pose.
        let camera_resample = state.samples_camera_poses && camera_inputs_changed;
        let isolated_motion = frame.motion_poses.is_some()
            && (motion_changed || body_swing_changed || camera_resample);
        let endpoint_contexts = isolated_motion.then(|| {
            [previous_motion, &frame.motion].map(|motion| {
                let mut context = motion.context.clone();
                context.camera_rotation = camera_rotation;
                context.camera_position = camera_position;
                context
            })
        });
        let mut endpoints: Option<(MotionEndpoint<'_>, MotionEndpoint<'_>)> =
            if let Some([previous_context, current_context]) = endpoint_contexts.as_ref() {
                let amount = actor.creeper_swell_amount(partial_tick);
                let Ok(previous) = previous_motion.sample(
                    evaluation::Evaluator {
                        input: &previous_input,
                        ..evaluator
                    },
                    state,
                    (amount, swing),
                    camera_resample.then_some(previous_context),
                    false,
                    &mut budget,
                ) else {
                    return Some(fallback());
                };
                let Ok(current) = frame.motion.sample(
                    evaluator,
                    state,
                    (amount, swing),
                    camera_resample.then_some(current_context),
                    true,
                    &mut budget,
                ) else {
                    return Some(fallback());
                };
                Some((previous, current))
            } else {
                None
            };
        let scale = if let Some((_, current)) = &endpoints {
            current.scale
        } else if motion_changed {
            let Ok(scale) =
                tick::scale::evaluate_model(&evaluator, rig, &mut variables, &mut budget)
            else {
                return Some(fallback());
            };
            scale
        } else {
            None
        };
        let sampled_scale = frame
            .motion
            .sampling
            .as_ref()
            .filter(|_| motion_changed)
            .and_then(|sampling| sampling.sampled_scale(rig, scale, state.scale));
        let sampled_clips = if !isolated_motion
            && (swing_changed
                || (frame.samples_camera_poses && pose_inputs_changed)
                || (motion_changed
                    && state
                        .motion_sampling
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
                state.motion_sampling.as_deref().filter(|_| motion_changed),
                !motion_changed,
                &mut budget,
            ) else {
                return Some(fallback());
            };
            Some(clips)
        } else {
            None
        };
        if sampled_clips.is_none()
            && endpoints.is_none()
            && frame.selection_effects.apply(&mut variables).is_err()
        {
            return Some(fallback());
        }
        let mut sampled_local = if let Some((_, current)) = endpoints.as_mut() {
            let Ok(local) = pose::sample_clips_reused(
                &current.evaluator,
                &mut current.variables,
                &state.bones,
                &state.bone_names,
                &current.clips,
                &mut budget,
            ) else {
                return Some(fallback());
            };
            Some(local)
        } else if (frame.samples_camera_poses && pose_inputs_changed)
            || body_swing_changed
            || motion_changed
        {
            let Ok(local) = pose::sample_clips_reused(
                &evaluator,
                &mut variables,
                &state.bones,
                &state.bone_names,
                sampled_clips.as_deref().unwrap_or(&frame.motion.clips),
                &mut budget,
            ) else {
                return Some(fallback());
            };
            Some(local)
        } else {
            None
        };
        let clips = endpoints
            .as_ref()
            .map(|(_, current)| current.clips.as_ref())
            .or(sampled_clips.as_deref())
            .unwrap_or(&frame.motion.clips);
        let sampled_motion_mask = (sampled_clips.is_some() || endpoints.is_some())
            .then_some(state.motion_sampling.as_ref())
            .flatten()
            .map(|sampling| {
                if camera_resample {
                    return motion_sampling::full_mask(state.bone_names.len());
                }
                sampling.mask(
                    assets,
                    &state.bone_names,
                    clips
                        .iter()
                        .chain(
                            endpoints
                                .as_ref()
                                .map_or(&[][..], |(previous, _)| previous.clips.as_ref()),
                        )
                        .copied(),
                    None,
                )
            });
        let sampled_previous_local = if let Some((previous, _)) = endpoints.as_mut() {
            let Some(history) = frame.motion_poses.as_ref() else {
                return Some(fallback());
            };
            let Some(local) = sampled_local.as_mut() else {
                return Some(fallback());
            };
            let Ok(sampled_previous) = pose::sample_clips_reused(
                &previous.evaluator,
                &mut previous.variables,
                &state.bones,
                &state.bone_names,
                &previous.clips,
                &mut budget,
            ) else {
                return Some(fallback());
            };
            Some(history.sample(local, &sampled_previous, sampled_motion_mask.as_deref()))
        } else {
            None
        };
        let motion_previous = match &sampled_previous_local {
            Some(local) => {
                let Some(pose) = frame.compositions.lock().unwrap().compose(
                    None,
                    true,
                    state.posed_bones().len(),
                    local,
                    || state.compose(local),
                ) else {
                    return Some(fallback());
                };
                Some(pose)
            }
            None => None,
        };
        let pose = match &sampled_local {
            Some(local) => {
                let Some(pose) = frame.compositions.lock().unwrap().compose(
                    None,
                    false,
                    state.posed_bones().len(),
                    local,
                    || state.compose(local),
                ) else {
                    return Some(fallback());
                };
                Some(pose)
            }
            None => None,
        };
        let geometry = assets
            .rig_geometries()
            .get(state.geometry_binding)?
            .geometry;
        if sampled_local.is_none() && frame.retained_pose_effects.apply(&mut variables).is_err() {
            return Some(fallback());
        }
        if let Some((_, current)) = &endpoints {
            current.variables.publish_writes(&mut variables);
        }
        let render_rig = render::RenderRig {
            binding: state.rig_binding,
            geometry,
            bone_names: state.posed_bone_names(),
            skeletons: &state.layer_skeletons,
        };
        let Ok(Some(mut layers)) = render::evaluate_render_cached(
            &evaluator,
            &mut variables,
            render_rig,
            &mut budget,
            Some(&mut frame.layers.lock().unwrap()),
        ) else {
            return Some(fallback());
        };
        if state
            .motion_sampling
            .as_ref()
            .is_some_and(|sampling| sampling.samples_render_writes())
            && let Some((previous, current)) = endpoints.as_mut()
        {
            // Authored writes retain each geometry endpoint's ordinary inputs.
            for endpoint in [previous, current] {
                if !matches!(
                    render::evaluate_render(
                        &endpoint.evaluator,
                        &mut endpoint.variables,
                        render_rig,
                        &mut budget,
                    ),
                    Ok(Some(_))
                ) {
                    return Some(fallback());
                }
            }
        }
        let (mut layer_endpoints, clips, previous_clips) = match endpoints.as_mut() {
            Some((previous, current)) => (
                Some((
                    (&previous.evaluator, &mut *previous.variables),
                    (&current.evaluator, &mut *current.variables),
                )),
                current.clips.as_ref(),
                previous.clips.as_ref(),
            ),
            None => (
                None,
                sampled_clips.as_deref().unwrap_or(&frame.motion.clips),
                &[][..],
            ),
        };
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
            let layer_inputs_changed = if pose.is_none()
                && previous.is_some()
                && let Some(geometry) = layer.geometry
            {
                match camera::layer_needs_pose_sampling(
                    &evaluator,
                    clips,
                    geometry,
                    camera::LayerInputs {
                        variables: &variables,
                        completed: &state.variables,
                        presentation_changed: pose_inputs_changed,
                    },
                    &mut budget,
                ) {
                    Ok(changed) => changed,
                    Err(_) => return Some(fallback()),
                }
            } else {
                false
            };
            let sampled = match (&pose, layer.geometry) {
                (Some(pose), None) => Some(Arc::clone(pose)),
                (_, Some(geometry))
                    if pose.is_some() || previous.is_none() || layer_inputs_changed =>
                {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        sampled_geometries.entry(geometry)
                    {
                        let (layer_evaluator, layer_variables) = layer_endpoints
                            .as_mut()
                            .map_or((&evaluator, &mut *variables), |(_, current)| {
                                (current.0, &mut *current.1)
                            });
                        let Ok(mut local) = render::sample_layer_local(
                            layer_evaluator,
                            layer_variables,
                            &state.layer_skeletons,
                            clips,
                            geometry,
                            &mut budget,
                        ) else {
                            return Some(fallback());
                        };
                        let Some(Some(skeleton)) = state.layer_skeletons.get(&geometry) else {
                            return Some(fallback());
                        };
                        let sampled_mask = (sampled_clips.is_some() || layer_endpoints.is_some())
                            .then_some(state.motion_sampling.as_ref())
                            .flatten()
                            .map(|sampling| {
                                if camera_resample {
                                    return motion_sampling::full_mask(skeleton.names.len());
                                }
                                sampling.mask(
                                    assets,
                                    &skeleton.names,
                                    clips.iter().chain(previous_clips).copied(),
                                    Some(geometry),
                                )
                            });
                        let previous_local = if let Some((previous, _)) = layer_endpoints.as_mut() {
                            let Ok(sampled_previous) = render::sample_layer_local(
                                previous.0,
                                &mut *previous.1,
                                &state.layer_skeletons,
                                previous_clips,
                                geometry,
                                &mut budget,
                            ) else {
                                return Some(fallback());
                            };
                            Some(match frame.motion_layers.get(&geometry) {
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
                        let Some(current) = frame.compositions.lock().unwrap().compose(
                            Some(geometry),
                            false,
                            skeleton.bones.len(),
                            &local,
                            || pose::compose_pose(&skeleton.bones, &local),
                        ) else {
                            return Some(fallback());
                        };
                        let previous = match previous_local {
                            Some(local) => {
                                let Some(previous) = frame.compositions.lock().unwrap().compose(
                                    Some(geometry),
                                    true,
                                    skeleton.bones.len(),
                                    &local,
                                    || pose::compose_pose(&skeleton.bones, &local),
                                ) else {
                                    return Some(fallback());
                                };
                                previous
                            }
                            None => Arc::clone(&current),
                        };
                        entry.insert((previous, current));
                    }
                    sampled_geometries
                        .get(&geometry)
                        .map(|(_, current)| Arc::clone(current))
                }
                _ => None,
            };
            if let Some(pose) = sampled {
                layer.previous_pose = match layer.geometry {
                    None => motion_previous
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
                return Some(fallback());
            }
            if let Some(previous) = previous
                && previous.hidden_bones == layer.hidden_bones
            {
                layer.hidden_bones = Arc::clone(&previous.hidden_bones);
            }
        }
        let skin = match sampled_local.as_deref().filter(|_| sample_skin) {
            Some(local) if !state.skin_layers.is_empty() => skin_layers::sample(
                state,
                &evaluator,
                &variables,
                (local, sampled_previous_local.as_deref()),
                Some(&layers),
                &mut budget,
            )
            .map(Cow::Owned),
            _ if sample_skin && !state.skin_layers.is_empty() => {
                skin_layers::refresh(state, &evaluator, &variables, &layers, &mut budget)
            }
            _ => Ok(Cow::Borrowed(state.skin_layers.as_slice())),
        };
        let complete = skin.is_ok();
        let skin = skin.unwrap_or(Cow::Borrowed(state.skin_layers.as_slice()));
        if complete && let Some(retained) = retained {
            retained.store(&layers, &skin, held_item_scale);
        }
        Some(ActorRenderLayers {
            render: Cow::Owned(layers),
            skin,
            held_item_scale,
        })
    }
}
