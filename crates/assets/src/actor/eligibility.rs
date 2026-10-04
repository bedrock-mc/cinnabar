//! Conservative rectangle eligibility; no implicit texture wrapping.
use crate::{EntityGeometry, EntityGeometryUv, validate_entity_geometry_inheritance};

// Inspect the exact linked programs, not a producer-supplied qualification flag.
pub fn neutral_actor_pose_mode(
    assets: &crate::RuntimeEntityAssets,
    candidate: usize,
) -> Option<super::ActorPoseMode> {
    let candidate = assets.rig_geometries().get(candidate)?;
    if usize::from(candidate.animation_count) + usize::from(candidate.controller_count) > 1 {
        return None;
    }
    let mut expression = false;
    for binding in assets.rig_animations().get(
        candidate.first_animation as usize
            ..candidate.first_animation as usize + usize::from(candidate.animation_count),
    )? {
        let clip = assets.animation_clips().get(binding.clip as usize)?;
        if clip.loop_mode != crate::EntityAnimationLoop::Loop {
            return None;
        }
        expression |= clip_has_expressions(assets, clip)?;
    }
    for binding in assets.rig_controllers().get(
        candidate.first_controller as usize
            ..candidate.first_controller as usize + usize::from(candidate.controller_count),
    )? {
        let controller = assets.controllers().get(binding.controller as usize)?;
        if controller.state_count != 1 || controller.initial_state != 0 {
            return None;
        }
        for state in assets.controller_states().get(
            controller.first_state as usize
                ..controller.first_state as usize + usize::from(controller.state_count),
        )? {
            if state.transition_count != 0 || state.on_entry.is_some() || state.on_exit.is_some() {
                return None;
            }
            for animation in assets.controller_animations().get(
                state.first_animation as usize
                    ..state.first_animation as usize + usize::from(state.animation_count),
            )? {
                let crate::EntityControllerAnimationTarget::Clip(clip) = animation.target else {
                    return None;
                };
                let clip = assets.animation_clips().get(clip as usize)?;
                if clip.loop_mode != crate::EntityAnimationLoop::Loop {
                    return None;
                }
                expression |= clip_has_expressions(assets, clip)?;
                if let Some(weight) = animation.weight {
                    let program = assets.molang_expressions().get(weight as usize)?;
                    let ops = assets.molang_ops().get(
                        program.first_op as usize
                            ..program.first_op as usize + usize::from(program.op_count),
                    )?;
                    expression |=
                        !matches!(ops, [crate::MolangOp::Push(value)] if value.get().is_finite());
                }
            }
        }
    }
    Some(if expression {
        super::ActorPoseMode::RestPose
    } else {
        super::ActorPoseMode::CompiledLiteral
    })
}

fn clip_has_expressions(
    assets: &crate::RuntimeEntityAssets,
    clip: &crate::EntityAnimationClip,
) -> Option<bool> {
    let channels = assets.animation_channels().get(
        clip.first_channel as usize..clip.first_channel as usize + clip.channel_count as usize,
    )?;
    for channel in channels {
        let keyframes = assets.animation_keyframes().get(
            channel.first_keyframe as usize
                ..channel.first_keyframe as usize + channel.keyframe_count as usize,
        )?;
        if keyframes
            .iter()
            .any(|keyframe| keyframe.expressions.iter().any(Option::is_some))
        {
            return Some(true);
        }
    }
    Some(false)
}

pub fn neutral_actor_geometry_uvs_are_supported(
    geometries: &[EntityGeometry],
    index: usize,
) -> bool {
    let Some(geometry) = geometries.get(index) else {
        return false;
    };
    let width = f32::from(geometry.texture_width);
    let height = f32::from(geometry.texture_height);
    if width == 0.0 || height == 0.0 {
        return false;
    }
    let Ok(parents) = validate_entity_geometry_inheritance(geometries) else {
        return false;
    };
    let rectangle = |origin: [f32; 2], size: [f32; 2]| {
        origin
            .into_iter()
            .zip(size)
            .zip([width, height])
            .all(|((first, size), bound)| {
                let last = first + size;
                first.is_finite()
                    && size.is_finite()
                    && last.is_finite()
                    && (0.0..=bound).contains(&first)
                    && (0.0..=bound).contains(&last)
            })
    };
    let mut current = index;
    for _ in 0..=geometries.len() {
        let Some(geometry) = geometries.get(current) else {
            return false;
        };
        // Ancestor cubes are checked conservatively even when later overridden.
        // This may reject unused art; it cannot admit an unverified wrap route.
        for (bone, cube) in geometry
            .bones
            .iter()
            .flat_map(|bone| bone.cubes.iter().map(move |cube| (bone, cube)))
        {
            let [x, y, z] = cube.size.map(|value| value.get());
            if [x, y, z]
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
            {
                return false;
            }
            let valid = match &cube.uv {
                EntityGeometryUv::Box(origin) => {
                    // Native Cube setup (26.50.26 RVA 01e5f1d0) offsets the side
                    // UVs by depth and the top/bottom UVs by depth in U. Fish
                    // fins have negative origins in unused, degenerate faces.
                    // Validate the faces with area, not the entire unfolded box.
                    let [x_uv, y_uv, z_uv] = [x, y, z].map(f32::trunc);
                    let [u, v] = origin.map(|value| value.get());
                    let inflate =
                        cube.inflate.get() + bone.inflate.map_or(0.0, |inflate| inflate.get());
                    let (origin, envelope) = if x + 2.0 * inflate == 0.0 {
                        ([u, v + z_uv], [2.0 * z_uv, y_uv])
                    } else if y + 2.0 * inflate == 0.0 {
                        ([u + z_uv, v], [2.0 * x_uv, z_uv])
                    } else if z + 2.0 * inflate == 0.0 {
                        // Preserve the camera-facing item plane's north rectangle.
                        ([u, v], [x_uv, y_uv])
                    } else {
                        ([u, v], [2.0 * x_uv + 2.0 * z_uv, y_uv + z_uv])
                    };
                    rectangle(origin, envelope)
                }
                EntityGeometryUv::Faces(faces) => [
                    &faces.north,
                    &faces.south,
                    &faces.east,
                    &faces.west,
                    &faces.up,
                    &faces.down,
                ]
                .into_iter()
                .zip(cube.face_uv_dimensions())
                .all(|(face, dimensions)| {
                    face.as_ref().is_none_or(|face| {
                        rectangle(
                            face.uv.map(|value| value.get()),
                            face.uv_size
                                .map_or(dimensions, |size| size.map(|value| value.get())),
                        )
                    })
                }),
            };
            if !valid {
                return false;
            }
        }
        let Some(parent) = parents.get(current).copied().flatten() else {
            return true;
        };
        current = parent;
    }
    false
}
