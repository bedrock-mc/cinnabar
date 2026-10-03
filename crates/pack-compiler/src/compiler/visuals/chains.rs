use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, ThinTemplateKey, diagnostic_visual, push_model_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;

const FAMILY: u8 = 3;
/// Half-width of each link plane in 1/256 block units (three pixels across).
const HALF_WIDTH: i16 = 24;
const CENTRE: i16 = 128;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<ThinTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_chain(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(axis) = chain_axis(record)
    {
        // The pack keys the first link sprite on the axis ends and the second on the sides.
        let [end, side] = match axis {
            0 => [BlockFace::West, BlockFace::Up],
            1 => [BlockFace::Up, BlockFace::West],
            _ => [BlockFace::North, BlockFace::Up],
        };
        let pair = [materials[end as usize], materials[side as usize]];
        let key = ThinTemplateKey {
            family: FAMILY,
            shape: axis,
            materials: pair,
        };
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                chain_quads(pair, axis).to_vec(),
                0,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(key, template);
            template
        };
        set_model_visual(&mut visual, materials, template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// Returns 0, 1, or 2 for the x, y, or z axis.
pub(in crate::compiler) fn chain_axis(record: &RegistryRecord) -> Option<u8> {
    match canonical_state_str(&record.canonical_state, "pillar_axis")?.as_ref() {
        "x" => Some(0),
        "y" => Some(1),
        "z" => Some(2),
        _ => None,
    }
}

/// Two crossed two-sided link planes running along `axis`.
pub(in crate::compiler) fn chain_quads(materials: [u32; 2], axis: u8) -> [ModelQuad; 2] {
    let (lo, hi) = (CENTRE - HALF_WIDTH, CENTRE + HALF_WIDTH);
    // Built along +y, then the length axis is swapped with the requested one.
    let planes: [([[i16; 3]; 4], usize, u32); 2] = [
        (
            [
                [lo, 0, CENTRE],
                [hi, 0, CENTRE],
                [hi, 256, CENTRE],
                [lo, 256, CENTRE],
            ],
            0,
            6,
        ),
        (
            [
                [CENTRE, 0, lo],
                [CENTRE, 0, hi],
                [CENTRE, 256, hi],
                [CENTRE, 256, lo],
            ],
            2,
            4,
        ),
    ];
    let [first, second] = planes;
    [(first, 0), (second, 1)].map(|((positions, across, face), plane)| {
        let uvs = positions.map(|position| {
            let u = (position[across] - lo) * 16;
            [u as u16, (4096 - i32::from(position[1]) * 16) as u16]
        });
        ModelQuad {
            positions: positions.map(|[x, y, z]| match axis {
                0 => [y, x, z],
                2 => [x, z, y],
                _ => [x, y, z],
            }),
            uvs,
            material: materials[plane],
            flags: MODEL_QUAD_FLAG_TWO_SIDED | face,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_three_pixels_wide_and_span_the_block_along_the_axis() {
        for axis in 0..3 {
            let quads = chain_quads([1, 2], axis);
            for quad in quads {
                let length = quad.positions.iter().map(|p| p[usize::from(axis)]);
                assert_eq!((length.clone().min(), length.max()), (Some(0), Some(256)));
                assert!(quad.uvs.iter().all(|uv| uv[0] <= 768 && uv[1] <= 4096));
                assert_ne!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            }
            assert_eq!((quads[0].material, quads[1].material), (1, 2));
        }
    }
}
