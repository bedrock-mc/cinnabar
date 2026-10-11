use super::*;

/// Camera queries affect presentation immediately without changing a clip's clock or state.
pub(in crate::actor_animation) fn needs_camera_sampling(
    assets: &RuntimeEntityAssets,
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
) -> bool {
    needs_pose_sampling(
        assets,
        rig_binding,
        geometry_binding,
        controllers,
        false,
        true,
    )
}

/// Contributing body inputs admit sampling independently of selected layer variants.
pub(in crate::actor_animation) fn active_pose_inputs(
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
pub(in crate::actor_animation) fn presentation_expressions(
    assets: &RuntimeEntityAssets,
    rig_binding: usize,
    geometry_binding: usize,
) -> evaluation::PresentationReads {
    evaluation::PresentationReads {
        camera: needs_pose_sampling(assets, rig_binding, geometry_binding, &[], false, false),
        swing: needs_pose_sampling(assets, rig_binding, geometry_binding, &[], true, false),
    }
}

/// Attack-time expressions sample the local swing at the physical frame fraction.
pub(in crate::actor_animation) fn needs_swing_sampling(
    assets: &RuntimeEntityAssets,
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
) -> bool {
    needs_pose_sampling(
        assets,
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
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
    swing: bool,
    include_clips: bool,
) -> bool {
    let samples_clip =
        |clip: usize| include_clips && camera_clip_with_layers(assets, rig_binding, clip, swing);
    if swing
        && include_clips
        && super::sampling::render_expressions(assets, rig_binding)
            .into_iter()
            .any(|expression| camera_expression(assets, expression as usize, true))
    {
        return true;
    }
    if assets.render_layers(rig_binding).iter().any(|layer| {
        layer
            .light_color_multiplier
            .is_some_and(|expression| camera_expression(assets, expression as usize, swing))
    }) {
        return true;
    }
    if assets
        .rig_bindings()
        .get(rig_binding)
        .and_then(|rig| rig.pre_animation)
        .is_some_and(|script| camera_expression(assets, script as usize, swing))
    {
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
                    .is_some_and(|expression| camera_expression(assets, expression as usize, swing))
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
                    .is_some_and(|expression| camera_expression(assets, expression as usize, swing))
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
                                    camera_expression(assets, expression as usize, swing)
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
        if (inputs.presentation_changed && camera_symbol(evaluator.assets, symbol, false))
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
    rig_binding: usize,
    clip: usize,
    swing: bool,
) -> bool {
    if camera_clip(assets, clip, swing) {
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
                        .is_some_and(|mapped| camera_clip(assets, mapped as usize, swing))
                })
            })
    })
}

/// Checks authored clip channels for the selected presentation input.
fn camera_clip(assets: &RuntimeEntityAssets, index: usize, swing: bool) -> bool {
    let Some(clip) = assets.animation_clips().get(index) else {
        return false;
    };
    let first = clip.first_channel as usize;
    assets
        .animation_channels()
        .get(first..first + clip.channel_count as usize)
        .is_some_and(|channels| {
            channels.iter().any(|channel| {
                let first = channel.first_keyframe as usize;
                assets
                    .animation_keyframes()
                    .get(first..first + channel.keyframe_count as usize)
                    .is_some_and(|keyframes| {
                        keyframes.iter().any(|keyframe| {
                            keyframe.expressions.iter().flatten().any(|&expression| {
                                camera_expression(assets, expression as usize, swing)
                            })
                        })
                    })
            })
        })
}

/// Checks a compiled expression for the selected presentation input.
pub(in crate::actor_animation) fn camera_expression(
    assets: &RuntimeEntityAssets,
    index: usize,
    swing: bool,
) -> bool {
    let Some(expression) = assets.molang_expressions().get(index) else {
        return false;
    };
    let first = expression.first_op as usize;
    assets
        .molang_ops()
        .get(first..first + usize::from(expression.op_count))
        .is_some_and(|ops| ops.iter().any(|op| camera_op(assets, op, swing)))
}

/// Identifies the presentation input consumed by one authored instruction.
fn camera_op(assets: &RuntimeEntityAssets, op: &MolangOp, swing: bool) -> bool {
    let symbol = match op {
        MolangOp::LoadVariable(symbol) => *symbol,
        MolangOp::Coalesce(branch) => branch.symbol,
        MolangOp::LoadQuery(symbol) if !swing => *symbol,
        MolangOp::CallQuery(call) if !swing => call.symbol,
        _ => return false,
    };
    camera_symbol(assets, symbol, swing)
}

/// Identifies a cached presentation read without revisiting its instruction stream.
pub(in crate::actor_animation) fn camera_symbol(
    assets: &RuntimeEntityAssets,
    symbol: u32,
    swing: bool,
) -> bool {
    assets
        .molang_symbols()
        .get(symbol as usize)
        .is_some_and(|symbol| {
            if swing {
                return symbol.identifier.as_ref() == "variable.attack_time"
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
                    symbol.identifier.as_ref(),
                    "query.camera_distance_range_lerp"
                        | "query.camera_rotation"
                        | "query.distance_from_camera"
                        | "query.rotation_to_camera"
                )
        })
}
