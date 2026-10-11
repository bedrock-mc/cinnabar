use super::*;
use crate::actor_animation::render_frame::camera;

impl VariableLayout {
    /// Binds immutable clip reads and controller weights once per runtime asset catalog.
    pub(super) fn bind_input_reads(&mut self, assets: &RuntimeEntityAssets) {
        self.clip_reads = assets
            .animation_clips()
            .iter()
            .map(|clip| {
                let mut reads = std::collections::BTreeSet::new();
                let first = clip.first_channel as usize;
                for channel in
                    &assets.animation_channels()[first..first + clip.channel_count as usize]
                {
                    let first = channel.first_keyframe as usize;
                    for keyframe in &assets.animation_keyframes()
                        [first..first + channel.keyframe_count as usize]
                    {
                        for &expression in keyframe.expressions.iter().flatten() {
                            let expression = &assets.molang_expressions()[expression as usize];
                            let first = expression.first_op as usize;
                            for op in &assets.molang_ops()
                                [first..first + usize::from(expression.op_count)]
                            {
                                let symbol = match op {
                                    MolangOp::LoadVariable(symbol)
                                    | MolangOp::LoadQuery(symbol) => *symbol,
                                    MolangOp::CallQuery(call) => call.symbol,
                                    MolangOp::Coalesce(branch) => branch.symbol,
                                    _ => continue,
                                };
                                if assets.molang_symbols()[symbol as usize].kind
                                    != MolangSymbolKind::Temporary
                                {
                                    reads.insert(symbol);
                                }
                            }
                        }
                    }
                }
                reads.into_iter().collect::<Vec<_>>().into_boxed_slice()
            })
            .collect();
        self.clip_inputs = self
            .clip_reads
            .iter()
            .map(|reads| PresentationReads {
                camera: reads
                    .iter()
                    .any(|&symbol| camera::camera_symbol(assets, symbol, false)),
                swing: reads
                    .iter()
                    .any(|&symbol| camera::camera_symbol(assets, symbol, true)),
            })
            .collect();
        self.controller_inputs = assets
            .controllers()
            .iter()
            .map(|controller| {
                let first = controller.first_state as usize;
                let mut inputs = PresentationReads::default();
                for state in assets.controller_states()
                    [first..first + usize::from(controller.state_count)]
                    .iter()
                {
                    let first = state.first_animation as usize;
                    for animation in &assets.controller_animations()
                        [first..first + usize::from(state.animation_count)]
                    {
                        if let Some(expression) = animation.weight {
                            inputs.camera |=
                                camera::camera_expression(assets, expression as usize, false);
                            inputs.swing |=
                                camera::camera_expression(assets, expression as usize, true);
                        }
                    }
                }
                inputs
            })
            .collect();
    }

    /// Shared authored variable and query reads; expression-local temporaries are excluded.
    pub(in crate::actor_animation) fn clip_reads(&self, clip: usize) -> &[u32] {
        self.clip_reads.get(clip).map_or(&[], Box::as_ref)
    }

    /// Constant-time classification of one contributing clip's presentation inputs.
    pub(in crate::actor_animation) fn clip_inputs(&self, clip: usize) -> PresentationReads {
        self.clip_inputs.get(clip).copied().unwrap_or_default()
    }

    /// Cached presentation dependencies in a controller's authored animation weights.
    pub(in crate::actor_animation) fn controller_inputs(
        &self,
        controller: usize,
    ) -> PresentationReads {
        self.controller_inputs
            .get(controller)
            .copied()
            .unwrap_or_default()
    }
}
