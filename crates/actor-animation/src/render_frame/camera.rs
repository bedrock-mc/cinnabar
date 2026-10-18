use super::*;
use assets::entity::EngineVariable as V;

/// Camera queries affect presentation immediately without changing a clip's clock or state.
pub(crate) fn needs_camera_sampling(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
) -> bool {
    needs_pose_sampling(
        assets,
        layout,
        rig_binding,
        geometry_binding,
        controllers,
        false,
        true,
    )
}

/// Contributing body inputs admit sampling independently of selected layer variants.
pub(crate) fn active_pose_inputs(
    layout: &VariableLayout,
    mut expressions: evaluation::PresentationReads,
    controllers: &[ControllerState],
    clips: &[tick::WeightedClip],
) -> evaluation::PresentationReads {
    for inputs in controllers
        .iter()
        .map(|c| layout.controller_inputs(c.controller))
        .chain(clips.iter().map(|c| layout.clip_inputs(c.clip)))
    {
        expressions.camera |= inputs.camera;
        expressions.swing |= inputs.swing;
    }
    expressions
}

/// Binds rig-level presentation inputs without animation-channel scans during actor ticks.
pub(crate) fn presentation_expressions(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    rig_binding: usize,
    geometry_binding: usize,
) -> evaluation::PresentationReads {
    evaluation::PresentationReads {
        camera: needs_pose_sampling(
            assets,
            layout,
            rig_binding,
            geometry_binding,
            &[],
            false,
            false,
        ),
        swing: needs_pose_sampling(
            assets,
            layout,
            rig_binding,
            geometry_binding,
            &[],
            true,
            false,
        ),
    }
}

/// Attack-time expressions sample the local swing at the physical frame fraction.
pub(crate) fn needs_swing_sampling(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
) -> bool {
    needs_pose_sampling(
        assets,
        layout,
        rig_binding,
        geometry_binding,
        controllers,
        true,
        true,
    )
}

/// Locates camera queries or attack-time variables across every reachable pose expression.
fn needs_pose_sampling(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
    swing: bool,
    include_clips: bool,
) -> bool {
    let samples_clip = |clip: usize| {
        include_clips && camera_clip_with_layers(assets, layout, rig_binding, clip, swing)
    };
    if swing
        && include_clips
        && super::sampling::render_expressions(assets, rig_binding)
            .into_iter()
            .any(|expression| camera_expression(layout, expression as usize, true))
    {
        return true;
    }
    if assets.render_layers(rig_binding).iter().any(|layer| {
        layer
            .light_color_multiplier
            .is_some_and(|expression| camera_expression(layout, expression as usize, swing))
    }) {
        return true;
    }
    if assets.rig_bindings().get(rig_binding).is_some_and(|rig| {
        [rig.pre_animation, rig.held_item_scale]
            .into_iter()
            .flatten()
            .any(|script| camera_expression(layout, script as usize, swing))
    }) {
        return true;
    }
    let Some(geometry) = assets.rig_geometries().get(geometry_binding) else {
        return false;
    };
    let first = geometry.first_animation as usize;
    if assets
        .rig_animations()
        .get(first..first + usize::from(geometry.animation_count))
        .is_some_and(|bindings| {
            bindings.iter().any(|binding| {
                binding
                    .weight
                    .is_some_and(|expression| camera_expression(layout, expression as usize, swing))
                    || samples_clip(binding.clip as usize)
            })
        })
    {
        return true;
    }
    let first = geometry.first_controller as usize;
    if assets
        .rig_controllers()
        .get(first..first + usize::from(geometry.controller_count))
        .is_some_and(|bindings| {
            bindings.iter().any(|binding| {
                binding
                    .weight
                    .is_some_and(|expression| camera_expression(layout, expression as usize, swing))
            })
        })
    {
        return true;
    }
    controllers.iter().any(|controller| {
        let Some(controller) = assets.controllers().get(controller.controller) else {
            return false;
        };
        let first = controller.first_state as usize;
        assets
            .controller_states()
            .get(first..first + usize::from(controller.state_count))
            .is_some_and(|states| {
                states.iter().any(|state| {
                    let first = state.first_animation as usize;
                    assets
                        .controller_animations()
                        .get(first..first + usize::from(state.animation_count))
                        .is_some_and(|animations| {
                            animations.iter().any(|animation| {
                                if animation.weight.is_some_and(|expression| {
                                    camera_expression(layout, expression as usize, swing)
                                }) {
                                    return true;
                                }
                                match animation.target {
                                    assets::EntityControllerAnimationTarget::Clip(clip) => {
                                        samples_clip(clip as usize)
                                    }
                                    assets::EntityControllerAnimationTarget::Controller(_) => false,
                                }
                            })
                        })
                })
            })
    })
}

/// Inputs compared after the selected layer's render-controller expressions have run.
pub(super) struct LayerInputs<'a> {
    pub variables: &'a MolangVariables,
    pub completed: &'a MolangVariables,
    pub presentation_changed: bool,
}

/// Selected layers respond to presentation queries and changed authored reads independently.
pub(super) fn layer_needs_pose_sampling(
    evaluator: &evaluation::Evaluator<'_>,
    clips: &[tick::WeightedClip],
    geometry: u32,
    inputs: LayerInputs<'_>,
    budget: &mut EvalBudget<'_>,
) -> Result<bool, EvalError> {
    for active in clips {
        budget.charge_work()?;
        if let Some(mapped) = render::clip_for_layer(evaluator.assets, *active, geometry)
            && clip_inputs_changed(evaluator, mapped.clip, &inputs, budget)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Compares cached authored inputs within the frame work budget, independent of keyframe count.
fn clip_inputs_changed(
    evaluator: &evaluation::Evaluator<'_>,
    clip: usize,
    inputs: &LayerInputs<'_>,
    budget: &mut EvalBudget<'_>,
) -> Result<bool, EvalError> {
    for &symbol in evaluator.layout.clip_reads(clip) {
        budget.charge_work()?;
        if (inputs.presentation_changed && evaluator.layout.symbol_inputs(symbol).camera)
            || inputs
                .variables
                .read_changed(inputs.completed, evaluator.layout, symbol)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Detects camera capability across every geometry the render controllers can select.
fn camera_clip_with_layers(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    rig_binding: usize,
    clip: usize,
    swing: bool,
) -> bool {
    if camera_clip(layout, clip, swing) {
        return true;
    }
    let Some(clip) = assets.animation_clips().get(clip) else {
        return false;
    };
    assets.render_layers(rig_binding).iter().any(|layer| {
        let first = layer.first_geometry as usize;
        assets
            .render_data()
            .geometries
            .get(first..first + usize::from(layer.geometry_count))
            .is_some_and(|choices| {
                choices.iter().any(|choice| {
                    assets
                        .clip_for_geometry(clip.symbol, choice.geometry)
                        .is_some_and(|mapped| camera_clip(layout, mapped as usize, swing))
                })
            })
    })
}

/// Uses the shared assignment closure to classify a contributing clip.
fn camera_clip(layout: &VariableLayout, index: usize, swing: bool) -> bool {
    let inputs = layout.clip_inputs(index);
    if swing { inputs.swing } else { inputs.camera }
}

/// Uses the shared assignment closure to classify an authored expression.
fn camera_expression(layout: &VariableLayout, index: usize, swing: bool) -> bool {
    let inputs = layout.expression_inputs(index);
    if swing { inputs.swing } else { inputs.camera }
}

/// Identifies a cached presentation read without revisiting its instruction stream.
pub(crate) fn camera_symbol(assets: &RuntimeEntityAssets, symbol: u32, swing: bool) -> bool {
    assets
        .molang_symbols()
        .get(symbol as usize)
        .is_some_and(|symbol| {
            if swing {
                return symbol.identifier.as_ref() == V::AttackTime.name()
                    || symbol.identifier.starts_with("variable.fp_melee_spear_")
                    || symbol.identifier.starts_with("variable.tp_melee_spear_");
            }
            symbol
                .identifier
                .starts_with("variable.fp_melee_spear_use_")
                || symbol
                    .identifier
                    .starts_with("variable.tp_melee_spear_use_")
                || matches!(
                    assets::MolangQuery::from_name(&symbol.identifier),
                    Some(
                        assets::MolangQuery::CameraDistanceRangeLerp
                            | assets::MolangQuery::CameraRotation
                            | assets::MolangQuery::DistanceFromCamera
                            | assets::MolangQuery::RotationToCamera
                    )
                )
        })
}
