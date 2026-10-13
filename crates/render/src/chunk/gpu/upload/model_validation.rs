use crate::chunk::*;

/// Checks that local model references consume exactly their lighting and draw records.
#[cfg(test)]
pub(in crate::chunk) fn validate_local_model_streams(
    model_refs: &[PackedModelRef],
    model_lighting: &[PackedQuadLighting],
    model_draw_refs: &[PackedModelDrawRef],
    model_templates: &[ModelTemplate],
) -> bool {
    let present = [
        !model_refs.is_empty(),
        !model_lighting.is_empty(),
        !model_draw_refs.is_empty(),
    ];
    if present.iter().any(|&value| value) && !present.iter().all(|&value| value) {
        return false;
    }
    if !present[0] {
        return true;
    }

    let mut draw_index = 0;
    let mut expected_lighting_base = 0_usize;
    for (model_ref_index, model_ref) in model_refs.iter().copied().enumerate() {
        let Ok(model_ref_index) = u32::try_from(model_ref_index) else {
            return false;
        };
        let words = model_ref.words();
        let lighting_base = words[2] as usize;
        let visible_mask = words[3];
        let Some(template) = model_templates.get(words[1] as usize) else {
            return false;
        };
        let Ok(template_quad_count) = usize::try_from(template.quad_count) else {
            return false;
        };
        if !(1..=32).contains(&template_quad_count)
            || visible_mask == 0
            || !model_lighting_base_matches(words, model_lighting, expected_lighting_base)
        {
            return false;
        }
        let valid_mask = if template_quad_count == 32 {
            u32::MAX
        } else {
            (1_u32 << template_quad_count) - 1
        };
        if visible_mask & !valid_mask != 0 {
            return false;
        }
        let Some(lighting_end) = lighting_base.checked_add(template_quad_count) else {
            return false;
        };
        if lighting_end > model_lighting.len() {
            return false;
        }
        expected_lighting_base = lighting_end;
        let mut visible = words[3];
        while visible != 0 {
            let quad_index = visible.trailing_zeros();
            let Some(draw_ref) = model_draw_refs.get(draw_index).copied() else {
                return false;
            };
            let draw_words = draw_ref.words();
            if draw_words != [model_ref_index, quad_index]
                || quad_index >= 32
                || lighting_base
                    .checked_add(quad_index as usize)
                    .is_none_or(|index| index >= model_lighting.len())
            {
                return false;
            }
            draw_index += 1;
            visible &= visible - 1;
        }
    }
    draw_index == model_draw_refs.len() && expected_lighting_base == model_lighting.len()
}

/// Checks that opaque and blend draws exactly partition the visible model quads.
pub(in crate::chunk) fn validate_partitioned_model_streams(
    model_refs: &[PackedModelRef],
    model_lighting: &[PackedQuadLighting],
    opaque_draw_refs: &[PackedModelDrawRef],
    blend_draw_refs: &[PackedModelDrawRef],
    model_templates: &[ModelTemplate],
    model_quads: &[assets::ModelQuad],
    materials: &[Material],
) -> bool {
    let any_draw = !opaque_draw_refs.is_empty() || !blend_draw_refs.is_empty();
    if model_refs.is_empty() != model_lighting.is_empty() || model_refs.is_empty() == any_draw {
        return false;
    }
    if model_refs.is_empty() {
        return true;
    }

    let mut opaque_index = 0;
    let mut blend_index = 0;
    let mut expected_lighting_base = 0_usize;
    for (model_ref_index, model_ref) in model_refs.iter().copied().enumerate() {
        let Ok(model_ref_index) = u32::try_from(model_ref_index) else {
            return false;
        };
        let words = model_ref.words();
        let lighting_base = words[2] as usize;
        let visible_mask = words[3];
        let Some(template) = model_templates.get(words[1] as usize) else {
            return false;
        };
        let Ok(template_quad_count) = usize::try_from(template.quad_count) else {
            return false;
        };
        let template_start = template.quad_start as usize;
        let Some(template_end) = template_start.checked_add(template_quad_count) else {
            return false;
        };
        let Some(template_quads) = model_quads.get(template_start..template_end) else {
            return false;
        };
        if !(1..=32).contains(&template_quad_count)
            || visible_mask == 0
            || !model_lighting_base_matches(words, model_lighting, expected_lighting_base)
        {
            return false;
        }
        let valid_mask = if template_quad_count == 32 {
            u32::MAX
        } else {
            (1_u32 << template_quad_count) - 1
        };
        if visible_mask & !valid_mask != 0 {
            return false;
        }
        let Some(lighting_end) = lighting_base.checked_add(template_quad_count) else {
            return false;
        };
        if lighting_end > model_lighting.len() {
            return false;
        }
        expected_lighting_base = lighting_end;

        let mut visible = visible_mask;
        while visible != 0 {
            let quad_index = visible.trailing_zeros();
            let Some(quad) = template_quads.get(quad_index as usize) else {
                return false;
            };
            let is_blend = if quad.material == assets::DIAGNOSTIC_MATERIAL {
                false
            } else {
                let Some(material) = materials.get(quad.material as usize) else {
                    return false;
                };
                material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND != 0
            };
            let (draw_refs, draw_index) = if is_blend {
                (blend_draw_refs, &mut blend_index)
            } else {
                (opaque_draw_refs, &mut opaque_index)
            };
            let Some(draw_ref) = draw_refs.get(*draw_index).copied() else {
                return false;
            };
            if draw_ref.words() != [model_ref_index, quad_index]
                || lighting_base
                    .checked_add(quad_index as usize)
                    .is_none_or(|index| index >= model_lighting.len())
            {
                return false;
            }
            *draw_index += 1;
            visible &= visible - 1;
        }
    }
    opaque_index == opaque_draw_refs.len()
        && blend_index == blend_draw_refs.len()
        && expected_lighting_base == model_lighting.len()
}

/// Requires contiguous lighting, with exactly two finite displacement records for flagged models.
fn model_lighting_base_matches(
    words: [u32; 4],
    lighting: &[PackedQuadLighting],
    expected: usize,
) -> bool {
    if words[0] & meshing::MODEL_REF_FLAG_RANDOM_OFFSET == 0 {
        return words[2] as usize == expected;
    }
    let Some(base) = expected.checked_add(2) else {
        return false;
    };
    let Some(prefix) = lighting.get(expected..base) else {
        return false;
    };
    words[2] as usize == base
        && prefix[1].samples()[2..] == [0, 0]
        && PackedQuadLighting::offset_from_prefix([prefix[0], prefix[1]])
            .iter()
            .all(|value| value.is_finite())
}
