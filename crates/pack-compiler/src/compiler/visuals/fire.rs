use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

const FLAME_HEIGHT: f32 = 1.4;

pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    matches!(
        record.name.as_ref(),
        "minecraft:fire" | "minecraft:soul_fire"
    ) && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 2], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record) {
        // Preserve both face bindings: the burning camera uses down independently.
        let key = [
            materials[BlockFace::Up as usize],
            materials[BlockFace::Down as usize],
        ];
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                supported_quads(key),
                assets::MODEL_TEMPLATE_FLAG_FIRE,
                storage.templates,
                storage.quads,
            )?;
            for alternate in [false, true] {
                for flip_u in [false, true] {
                    for mask in 0..assets::FIRE_ATTACHMENT_MASK_COUNT as u8 {
                        let offset =
                            assets::fire_attachment_template_offset(mask, alternate, flip_u);
                        let index = push_model_template(
                            attached_quads(key, mask, alternate, flip_u),
                            assets::MODEL_TEMPLATE_FLAG_FIRE,
                            storage.templates,
                            storage.quads,
                        )?;
                        debug_assert_eq!(index, template + offset);
                    }
                }
            }
            templates.insert(key, template);
            template
        };
        set_model_visual(&mut visual, materials, template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

// Quantize Vanilla's fire coordinates only at the carrier's 1/256 boundary.
fn quad(positions: [[f32; 3]; 4], material: u32, flip_u: bool) -> ModelQuad {
    let [a, b] = if flip_u { [0, 4096] } else { [4096, 0] };
    ModelQuad {
        positions: positions.map(|p| p.map(|value| (value * 256.0).round() as i16)),
        uvs: [[a, 0], [a, 4096], [b, 4096], [b, 0]],
        material,
        // Native alpha_test terrain is double-sided, with white flat colour.
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    }
}

fn supported_quads([up, down]: [u32; 2]) -> Vec<ModelQuad> {
    let h = FLAME_HEIGHT;
    vec![
        quad(
            [
                [0.2, h, 1.0],
                [0.7, 0.0, 1.0],
                [0.7, 0.0, 0.0],
                [0.2, h, 0.0],
            ],
            up,
            false,
        ),
        quad(
            [
                [0.8, h, 0.0],
                [0.3, 0.0, 0.0],
                [0.3, 0.0, 1.0],
                [0.8, h, 1.0],
            ],
            up,
            false,
        ),
        quad(
            [
                [1.0, h, 0.8],
                [1.0, 0.0, 0.3],
                [0.0, 0.0, 0.3],
                [0.0, h, 0.8],
            ],
            down,
            false,
        ),
        quad(
            [
                [0.0, h, 0.2],
                [0.0, 0.0, 0.7],
                [1.0, 0.0, 0.7],
                [1.0, h, 0.2],
            ],
            down,
            false,
        ),
        quad(
            [
                [0.1, h, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.1, h, 1.0],
            ],
            down,
            true,
        ),
        quad(
            [
                [0.9, h, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [0.9, h, 0.0],
            ],
            down,
            true,
        ),
        quad(
            [
                [0.0, h, 0.9],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, h, 0.9],
            ],
            up,
            true,
        ),
        quad(
            [
                [1.0, h, 0.1],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, h, 0.1],
            ],
            up,
            true,
        ),
    ]
}

fn attached_quads([up, down]: [u32; 2], mask: u8, alternate: bool, flip_u: bool) -> Vec<ModelQuad> {
    let b = 1.0 / 16.0;
    let t = FLAME_HEIGHT + b;
    let material = if alternate { down } else { up };
    let sides = [
        (
            [[0.2, t, 1.0], [0.0, b, 1.0], [0.0, b, 0.0], [0.2, t, 0.0]],
            flip_u,
        ),
        (
            [[0.8, t, 0.0], [1.0, b, 0.0], [1.0, b, 1.0], [0.8, t, 1.0]],
            !flip_u,
        ),
        (
            [[0.0, t, 0.2], [0.0, b, 0.0], [1.0, b, 0.0], [1.0, t, 0.2]],
            flip_u,
        ),
        (
            [[1.0, t, 0.8], [1.0, b, 1.0], [0.0, b, 1.0], [0.0, t, 0.8]],
            !flip_u,
        ),
    ];
    let mut quads = Vec::new();
    for (side, (positions, flip)) in sides.into_iter().enumerate() {
        if mask & (1 << side) != 0 {
            let front = quad(positions, material, flip);
            let mut back = front;
            back.positions.reverse();
            back.uvs.reverse();
            quads.extend([front, back]);
        }
    }
    if mask & 16 != 0 {
        // Roof orientation uses the cell above the original fire position.
        let roof = if alternate {
            [
                [
                    [0.0, 0.8, 0.0],
                    [1.0, 1.0, 0.0],
                    [1.0, 1.0, 1.0],
                    [0.0, 0.8, 1.0],
                ],
                [
                    [1.0, 0.8, 1.0],
                    [0.0, 1.0, 1.0],
                    [0.0, 1.0, 0.0],
                    [1.0, 0.8, 0.0],
                ],
            ]
        } else {
            [
                [
                    [0.0, 0.8, 1.0],
                    [0.0, 1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [1.0, 0.8, 1.0],
                ],
                [
                    [1.0, 0.8, 0.0],
                    [1.0, 1.0, 1.0],
                    [0.0, 1.0, 1.0],
                    [0.0, 0.8, 0.0],
                ],
            ]
        };
        quads.extend([quad(roof[0], up, false), quad(roof[1], down, false)]);
    }
    quads
}

#[cfg(test)]
#[path = "fire_tests.rs"]
mod tests;
