//! Complete samples retained before scratch replay and motion-history copying.
use super::*;

/// Tick and controller identities are implicit in the owning completed frame.
#[derive(Debug, PartialEq)]
pub(super) struct Key {
    alpha: Option<u32>,
    camera: [u32; 5],
    swing: Option<u32>,
    swell: u32,
    legacy: bool,
    color: [u32; 4],
    skin: bool,
    skin_tick: Option<u64>,
}

impl Key {
    /// Includes frame time only for paths whose sampling control flow depends on it.
    pub(super) fn new(
        evaluator: &Evaluator<'_>,
        state: &ActorRigState,
        motion: bool,
        swing: Option<f32>,
        skin: bool,
    ) -> Self {
        let context = evaluator.context;
        Self {
            alpha: (state.samples_render_frames || motion).then_some(context.frame_alpha.to_bits()),
            camera: if state.samples_camera_poses {
                [
                    context.camera_rotation[0],
                    context.camera_rotation[1],
                    context.camera_position[0],
                    context.camera_position[1],
                    context.camera_position[2],
                ]
                .map(f32::to_bits)
            } else {
                [0; 5]
            },
            swing: swing.map(f32::to_bits),
            swell: evaluator
                .actor
                .creeper_swell_amount(context.frame_alpha)
                .to_bits(),
            legacy: crate::legacy_swing::ordinary(
                evaluator.actor,
                context,
                evaluator.input.swim_amount,
            ),
            color: crate::query::wolf::default_render_color(evaluator.actor).map(f32::to_bits),
            skin,
            skin_tick: (skin && !state.skin_layers.is_empty()).then_some(evaluator.life_tick),
        }
    }
}

#[derive(Debug)]
struct Output {
    render: Vec<RenderTextureLayer>,
    skin: Option<Vec<SkinRenderLayer>>,
    scale: Option<f32>,
}

#[derive(Debug)]
pub(crate) struct Sample {
    inputs: cache::Inputs,
    key: Option<Key>,
    output: Option<Output>,
    valid: bool,
}

impl Sample {
    /// Derives live query reads from every reachable expression and selected server clip.
    pub(super) fn new(
        evaluator: &Evaluator<'_>,
        state: &ActorRigState,
        frame: &FrameState,
    ) -> Self {
        let inputs = if let Some(sampling) = &frame.motion.sampling {
            cache::Inputs::from_query_plan(sampling.frame_queries())
        } else {
            let clips = frame
                .motion
                .clips
                .iter()
                .map(|clip| clip.clip)
                .collect::<Vec<_>>();
            let expressions = sampling::pose_expressions(
                evaluator.assets,
                state.rig_binding,
                state.geometry_binding,
                &state.controllers,
                &clips,
            );
            cache::Inputs::queries(evaluator.assets, expressions)
        };
        Self {
            inputs,
            key: None,
            output: None,
            valid: false,
        }
    }

    /// A new tick keeps storage but must produce its own controller, variable and pose results.
    pub(super) fn rebase(&mut self, queries: Option<Arc<[u32]>>) {
        self.inputs.reset_query_plan(queries);
        self.key = None;
        self.valid = false;
    }

    /// A failed or changed sample invalidates its previous output before replay begins.
    pub(super) fn get<'s>(
        &mut self,
        key: Key,
        evaluator: &Evaluator<'_>,
        variables: &MolangVariables,
        state: &'s ActorRigState,
        budget: &EvalBudget<'_>,
    ) -> Option<ActorRenderLayers<'s>> {
        let unchanged =
            self.inputs.update(evaluator, variables, budget) && self.key.as_ref() == Some(&key);
        self.key = Some(key);
        if unchanged
            && self.valid
            && let Some(output) = &self.output
        {
            return Some(ActorRenderLayers {
                render: Cow::Owned(output.render.clone()),
                skin: output
                    .skin
                    .as_ref()
                    .map_or(Cow::Borrowed(state.skin_layers.as_slice()), |skin| {
                        Cow::Owned(skin.clone())
                    }),
                held_item_scale: output.scale,
            });
        }
        self.valid = false;
        None
    }

    /// Stores only completed samples; borrowed persona layers remain tick-owned.
    pub(super) fn store(
        &mut self,
        render: &[RenderTextureLayer],
        sampled_skin: Option<&[SkinRenderLayer]>,
        scale: Option<f32>,
    ) {
        if !self.inputs.pure() {
            return;
        }
        let output = self.output.get_or_insert_with(|| Output {
            render: Vec::new(),
            skin: None,
            scale,
        });
        output.render.clear();
        output.render.extend_from_slice(render);
        match sampled_skin {
            Some(skin) => {
                let output = output.skin.get_or_insert_with(Vec::new);
                output.clear();
                output.extend_from_slice(skin);
            }
            None => output.skin = None,
        }
        output.scale = scale;
        self.valid = true;
    }
}
