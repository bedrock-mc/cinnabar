use std::borrow::Cow;

use super::*;

pub(super) mod camera;

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
            .render_layers(runtime_id, self.alpha, &mut self.remaining_ops)
    }
}

#[derive(Debug)]
pub(super) struct FrameState {
    pub variables: MolangVariables,
    pub context: ActorTickContext,
    pub input: ActorTickInput,
    pub anim_tick: u64,
    pub clips: Vec<tick::WeightedClip>,
}

impl ActorAnimationStore {
    pub(crate) fn render_layers(
        &self,
        actor: &ActorSnapshot,
        partial_tick: f32,
        camera_rotation: [f32; 2],
        remaining_ops: &mut usize,
    ) -> Option<Cow<'_, [RenderTextureLayer]>> {
        let lifetime = self.runtime_to_lifetime.get(&actor.runtime_id)?;
        let state = self.rigs.get(lifetime)?;
        if lifetime.spawn_revision != actor.spawn_revision {
            return None;
        }
        let completed = || Cow::Borrowed(state.render.as_slice());
        let Some(frame) = state.render_frame.as_ref().filter(|_| {
            (partial_tick > 0.0 || state.samples_camera_poses)
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
        let mut context = frame.context.clone();
        context.frame_alpha = partial_tick;
        context.camera_rotation = camera_rotation;
        let evaluator = evaluation::Evaluator {
            assets,
            layout,
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
        let rig = assets.rig_bindings().get(state.rig_binding)?;
        if let Some(script) = rig.pre_animation
            && evaluator
                .run(script as usize, &mut variables, 0.0, &mut budget)
                .is_err()
        {
            return Some(completed());
        }
        let pose = if state.samples_camera_poses {
            let Ok(local) = pose::sample_clips(
                &evaluator,
                &mut variables,
                &state.bones,
                &frame.clips,
                &mut budget,
            ) else {
                return Some(completed());
            };
            let Some(pose) = state.compose(&local) else {
                return Some(completed());
            };
            Some(Arc::<[BoneTransform]>::from(pose))
        } else {
            None
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
        for layer in &mut layers {
            if let Some(pose) = &pose
                && layer.geometry.is_none()
            {
                layer.previous_pose = Arc::clone(pose);
                layer.pose = Arc::clone(pose);
                continue;
            }
            let previous = state
                .render
                .iter()
                .find(|previous| previous.geometry == layer.geometry);
            if let Some(previous) = previous {
                layer.previous_pose = Arc::clone(&previous.previous_pose);
                layer.pose = Arc::clone(&previous.pose);
                if previous.hidden_bones == layer.hidden_bones {
                    layer.hidden_bones = Arc::clone(&previous.hidden_bones);
                }
            } else if layer.geometry.is_some() {
                return Some(completed());
            }
        }
        Some(Cow::Owned(layers))
    }
}

pub(super) fn needs_frame_sampling(assets: &RuntimeEntityAssets, binding: usize) -> bool {
    let Some(rig) = assets.rig_bindings().get(binding) else {
        return false;
    };
    let mut expressions = Vec::new();
    expressions.extend(rig.pre_animation);
    let data = assets.render_data();
    for layer in assets.render_layers(binding) {
        expressions.extend(layer.condition);
        for channels in [
            layer.color,
            layer.overlay_color,
            layer.hurt_color,
            layer.on_fire_color,
            layer.uv_anim,
        ]
        .into_iter()
        .flatten()
        {
            expressions.extend(channels);
        }
        let first = layer.first_geometry as usize;
        if let Some(choices) = data
            .geometries
            .get(first..first + usize::from(layer.geometry_count))
        {
            expressions.extend(choices.iter().filter_map(|choice| choice.condition));
        }
        let first = layer.first_visibility as usize;
        if let Some(rules) = data
            .visibility
            .get(first..first + usize::from(layer.visibility_count))
        {
            expressions.extend(rules.iter().map(|rule| rule.condition));
        }
        let first = layer.first_slot as usize;
        if let Some(slots) = data.slots.get(first..first + usize::from(layer.slot_count)) {
            for slot in slots {
                let first = slot.first_candidate as usize;
                if let Some(choices) = data
                    .candidates
                    .get(first..first + usize::from(slot.candidate_count))
                {
                    expressions.extend(choices.iter().filter_map(|choice| choice.condition));
                }
            }
        }
    }
    expressions.into_iter().any(|expression| {
        let Some(expression) = assets.molang_expressions().get(expression as usize) else {
            return false;
        };
        let first = expression.first_op as usize;
        let Some(ops) = assets
            .molang_ops()
            .get(first..first + usize::from(expression.op_count))
        else {
            return false;
        };
        ops.iter().any(|op| {
            let symbol = match op {
                MolangOp::LoadQuery(symbol) => *symbol,
                MolangOp::CallQuery(call) => call.symbol,
                _ => return false,
            };
            assets
                .molang_symbols()
                .get(symbol as usize)
                .is_some_and(|symbol| {
                    matches!(
                        symbol.identifier.as_ref(),
                        "query.frame_alpha" | "query.life_time"
                    )
                })
        })
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod orb_tests;
