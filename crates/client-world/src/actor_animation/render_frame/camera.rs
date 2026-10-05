use super::*;

/// Camera queries affect presentation immediately without changing a clip's clock or state.
pub(in crate::actor_animation) fn needs_camera_sampling(
    assets: &RuntimeEntityAssets,
    geometry_binding: usize,
    controllers: &[ControllerState],
) -> bool {
    let Some(geometry) = assets.rig_geometries().get(geometry_binding) else {
        return false;
    };
    let first = geometry.first_animation as usize;
    if assets
        .rig_animations()
        .get(first..first + usize::from(geometry.animation_count))
        .is_some_and(|bindings| {
            bindings
                .iter()
                .any(|binding| camera_clip(assets, binding.clip as usize))
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
                            animations.iter().any(|animation| match animation.target {
                                assets::EntityControllerAnimationTarget::Clip(clip) => {
                                    camera_clip(assets, clip as usize)
                                }
                                assets::EntityControllerAnimationTarget::Controller(_) => false,
                            })
                        })
                })
            })
    })
}

fn camera_clip(assets: &RuntimeEntityAssets, index: usize) -> bool {
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
                            keyframe
                                .expressions
                                .iter()
                                .flatten()
                                .any(|&expression| camera_expression(assets, expression as usize))
                        })
                    })
            })
        })
}

fn camera_expression(assets: &RuntimeEntityAssets, index: usize) -> bool {
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
                    MolangOp::LoadQuery(symbol) => *symbol,
                    MolangOp::CallQuery(call) => call.symbol,
                    _ => return false,
                };
                assets
                    .molang_symbols()
                    .get(symbol as usize)
                    .is_some_and(|symbol| symbol.identifier.as_ref() == "query.camera_rotation")
            })
        })
}
