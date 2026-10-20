//! Per-controller samples owned by a completed animation frame.
use super::*;
use crate::render_frame::{cache::Inputs, sampling};

#[derive(Debug)]
struct Layer {
    inputs: Inputs,
    color: [f32; 4],
    output: Option<LayerOutput>,
}

/// Keeps pure controller results; rebuilding the owning rig discards its immutable layer plans.
#[derive(Debug, Default)]
pub(crate) struct RenderCache {
    layers: Vec<Layer>,
}

impl RenderCache {
    /// Reuses only pure controllers; side-effecting controllers keep their original execution.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sample(
        &mut self,
        index: usize,
        evaluator: &Evaluator<'_>,
        variables: &mut MolangVariables,
        rig: RenderRig<'_>,
        layer: &assets::EntityRenderLayer,
        color: [f32; 4],
        multitexture: bool,
        budget: &mut EvalBudget<'_>,
    ) -> Result<LayerOutput, EvalError> {
        if !crate::render_frame::cache::enabled() {
            return evaluate_layer(
                evaluator,
                variables,
                rig,
                layer,
                color,
                multitexture,
                budget,
            );
        }
        if index == self.layers.len() {
            self.layers.push(Layer {
                inputs: Inputs::new(
                    evaluator.assets,
                    sampling::render_layer_expressions(evaluator.assets, layer).expressions,
                ),
                color,
                output: None,
            });
        }
        let cached = &mut self.layers[index];
        if !cached.inputs.pure() {
            return evaluate_layer(
                evaluator,
                variables,
                rig,
                layer,
                color,
                multitexture,
                budget,
            );
        }
        let unchanged = cached.inputs.update(evaluator, variables, budget)
            && cached.color.map(f32::to_bits) == color.map(f32::to_bits);
        if unchanged && let Some(output) = &cached.output {
            return Ok(output.clone());
        }
        // A failed evaluation cannot make the previous result valid for the new inputs.
        cached.output = None;
        cached.color = color;
        let output = evaluate_layer(
            evaluator,
            variables,
            rig,
            layer,
            color,
            multitexture,
            budget,
        )?;
        cached.output = Some(output.clone());
        Ok(output)
    }
}
