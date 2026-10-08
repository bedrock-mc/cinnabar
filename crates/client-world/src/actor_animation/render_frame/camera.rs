use super::*;

/// Camera queries affect presentation immediately without changing a clip's clock or state.
pub(in crate::actor_animation) fn needs_camera_sampling(
    assets: &RuntimeEntityAssets,
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
) -> bool {
    needs_pose_sampling(assets, rig_binding, geometry_binding, controllers, false)
}

/// Attack-time expressions sample the local swing at the physical frame fraction.
pub(in crate::actor_animation) fn needs_swing_sampling(
    assets: &RuntimeEntityAssets,
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
) -> bool {
    needs_pose_sampling(assets, rig_binding, geometry_binding, controllers, true)
}

/// Locates camera queries or attack-time variables across every reachable pose expression.
fn needs_pose_sampling(
    assets: &RuntimeEntityAssets,
    rig_binding: usize,
    geometry_binding: usize,
    controllers: &[ControllerState],
    swing: bool,
) -> bool {
    if swing
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
                    || camera_clip(assets, binding.clip as usize, swing)
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
                                        camera_clip(assets, clip as usize, swing)
                                    }
                                    assets::EntityControllerAnimationTarget::Controller(_) => false,
                                }
                            })
                        })
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
fn camera_expression(assets: &RuntimeEntityAssets, index: usize, swing: bool) -> bool {
    let Some(expression) = assets.molang_expressions().get(index) else {
        return false;
    };
    let first = expression.first_op as usize;
    assets
        .molang_ops()
        .get(first..first + usize::from(expression.op_count))
        .is_some_and(|ops| {
            ops.iter().any(|op| {
                let symbol = match op {
                    MolangOp::LoadVariable(symbol) => *symbol,
                    MolangOp::LoadQuery(symbol) if !swing => *symbol,
                    MolangOp::CallQuery(call) if !swing => call.symbol,
                    _ => return false,
                };
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
            })
        })
}
