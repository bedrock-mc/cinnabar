use crate::AssetError;

use super::super::{
    CompiledEntityAssets, EntityAssetKind, effective_geometry_bone_counts, invalid,
};
use super::{
    MAX_ENTITY_RIG_ANIMATIONS, MAX_ENTITY_RIG_BINDINGS, MAX_ENTITY_RIG_CONTROLLERS,
    MAX_ENTITY_RIG_GEOMETRIES, MolangOp, MolangSymbolKind, index_has_kind, molang_symbol_has_kind,
    range_in_bounds, validate_flattened_ranges,
};

pub(super) fn validate_rig_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.rig_bindings.len() > MAX_ENTITY_RIG_BINDINGS
        || compiled.rig_geometries.len() > MAX_ENTITY_RIG_GEOMETRIES
        || compiled.rig_animations.len() > MAX_ENTITY_RIG_ANIMATIONS
        || compiled.rig_controllers.len() > MAX_ENTITY_RIG_CONTROLLERS
    {
        return Err(invalid("entity rig binding count exceeds bound"));
    }
    let effective_bone_counts = effective_geometry_bone_counts(&compiled.geometries)?;
    for binding in &compiled.rig_animations {
        if !molang_symbol_has_kind(compiled, binding.name, &[MolangSymbolKind::Name])
            || binding.clip as usize >= compiled.animation_clips.len()
        {
            return Err(invalid("entity rig animation index is out of range"));
        }
    }
    for binding in &compiled.rig_controllers {
        if !molang_symbol_has_kind(compiled, binding.name, &[MolangSymbolKind::Name])
            || binding.controller as usize >= compiled.controllers.len()
        {
            return Err(invalid("entity rig controller index is out of range"));
        }
    }
    for binding in &compiled.rig_bindings {
        if !index_has_kind(
            &compiled.symbols,
            binding.entity_symbol,
            EntityAssetKind::Entity,
        ) || !index_has_kind(
            &compiled.symbols,
            binding.render_controller,
            EntityAssetKind::RenderController,
        ) || !range_in_bounds(
            binding.first_geometry,
            u32::from(binding.geometry_count),
            compiled.rig_geometries.len(),
        ) || binding.geometry_count == 0
        {
            return Err(invalid("entity rig binding index is out of range"));
        }
        let candidates = &compiled.rig_geometries[binding.first_geometry as usize
            ..binding.first_geometry as usize + binding.geometry_count as usize];
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            if candidate.geometry as usize >= compiled.geometries.len()
                || candidate.condition.is_none() != (candidate_index == 0)
                || candidate
                    .condition
                    .is_some_and(|condition| !is_boolean_expression(compiled, condition))
                || !range_in_bounds(
                    candidate.first_animation,
                    u32::from(candidate.animation_count),
                    compiled.rig_animations.len(),
                )
                || !range_in_bounds(
                    candidate.first_controller,
                    u32::from(candidate.controller_count),
                    compiled.rig_controllers.len(),
                )
            {
                return Err(invalid("entity rig geometry candidate is invalid"));
            }
            let geometry_bones = effective_bone_counts[candidate.geometry as usize];
            let animations = &compiled.rig_animations[candidate.first_animation as usize
                ..candidate.first_animation as usize + candidate.animation_count as usize];
            for animation in animations {
                validate_rig_clip_bones(compiled, animation.clip, geometry_bones)?;
            }
            let controllers = &compiled.rig_controllers[candidate.first_controller as usize
                ..candidate.first_controller as usize + candidate.controller_count as usize];
            for rig_controller in controllers {
                let controller = &compiled.controllers[rig_controller.controller as usize];
                let states = &compiled.controller_states[controller.first_state as usize
                    ..controller.first_state as usize + controller.state_count as usize];
                for state in states {
                    let animations = &compiled.controller_animations[state.first_animation as usize
                        ..state.first_animation as usize + state.animation_count as usize];
                    for animation in animations {
                        validate_rig_clip_bones(compiled, animation.clip, geometry_bones)?;
                    }
                }
            }
        }
    }
    validate_flattened_ranges(
        compiled
            .rig_bindings
            .iter()
            .map(|binding| (binding.first_geometry, u32::from(binding.geometry_count))),
        compiled.rig_geometries.len(),
        "rig geometry",
    )?;
    validate_flattened_ranges(
        compiled
            .rig_geometries
            .iter()
            .map(|binding| (binding.first_animation, u32::from(binding.animation_count))),
        compiled.rig_animations.len(),
        "rig animation",
    )?;
    validate_flattened_ranges(
        compiled.rig_geometries.iter().map(|binding| {
            (
                binding.first_controller,
                u32::from(binding.controller_count),
            )
        }),
        compiled.rig_controllers.len(),
        "rig controller",
    )?;
    Ok(())
}

fn is_boolean_expression(compiled: &CompiledEntityAssets, expression: u32) -> bool {
    let Some(expression) = compiled.molang_expressions.get(expression as usize) else {
        return false;
    };
    let last = expression
        .first_op
        .checked_add(u32::from(expression.op_count))
        .and_then(|end| end.checked_sub(1))
        .and_then(|index| compiled.molang_ops.get(index as usize));
    matches!(
        last,
        Some(
            MolangOp::Not
                | MolangOp::And
                | MolangOp::Or
                | MolangOp::Equal
                | MolangOp::NotEqual
                | MolangOp::Less
                | MolangOp::LessEqual
                | MolangOp::Greater
                | MolangOp::GreaterEqual
        )
    )
}

fn validate_rig_clip_bones(
    compiled: &CompiledEntityAssets,
    clip_index: u32,
    geometry_bones: usize,
) -> Result<(), AssetError> {
    let clip = &compiled.animation_clips[clip_index as usize];
    let channels = &compiled.animation_channels
        [clip.first_channel as usize..clip.first_channel as usize + clip.channel_count as usize];
    if channels
        .iter()
        .any(|channel| channel.bone as usize >= geometry_bones)
    {
        return Err(invalid(
            "entity animation channel bone is out of range for its effective rig geometry",
        ));
    }
    Ok(())
}
