//! Count-selected sea pickle bodies and live crossed tips, using the pack's sprite.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Identifies primary sea pickles before the provisional envelope route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:sea_pickle"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Admits only the typed cluster and liveness selectors consumed by this model.
fn selectors(record: &RegistryRecord) -> Option<(u8, bool)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    let count = super::state::exact_tagged_int(state.get("cluster_count")?, 3)? as u8;
    let dead = super::state::exact_tagged_byte(state.get("dead_bit")?, 1)? != 0;
    Some((count, dead))
}

/// Compiles all cluster states with independent templates for dry and live presentation.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, u8, bool), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((count, dead)) = selectors(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    // This model uses the first block sprite for every body face and both tips.
    let Some(material) = inputs.material(record, BlockFace::Down) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (material, count, dead);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = quads(material, count, dead);
        let chunks = quads.chunks(assets::MAX_MODEL_TEMPLATE_QUADS);
        let parts = chunks.len();
        let mut template = None;
        for (index, chunk) in chunks.enumerate() {
            let part = push_model_template(
                chunk.to_vec(),
                if index + 1 < parts {
                    MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
                } else {
                    0
                },
                storage.templates,
                storage.quads,
            )?;
            template.get_or_insert(part);
        }
        let template = template.expect("sea pickle cluster has bodies");
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, [material; 6], template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Returns each body's horizontal centre and height in block coordinates.
fn bodies(count: u8) -> &'static [[f64; 3]] {
    match count {
        0 => &[[0.5, 0.5, 0.375]],
        1 => &[[0.35, 0.35, 0.375], [0.65, 0.65, 0.245]],
        2 => &[[0.5, 0.7, 0.245], [0.3, 0.3, 0.435], [0.6, 0.35, 0.375]],
        _ => &[
            [0.29, 0.31, 0.375],
            [0.71, 0.76, 0.245],
            [0.69, 0.27, 0.305],
            [0.25, 0.67, 0.435],
        ],
    }
}

/// Quantizes block coordinates into the carrier's existing model position format.
fn position(value: f64) -> i16 {
    (value * 256.0).round() as i16
}

/// Rebinds a cuboid's outside-view UV orientation to a sprite rectangle.
fn region(quad: &mut ModelQuad, rect: [u16; 4]) {
    let min: [u16; 2] =
        std::array::from_fn(|axis| quad.uvs.iter().map(|uv| uv[axis]).min().unwrap());
    let max: [u16; 2] =
        std::array::from_fn(|axis| quad.uvs.iter().map(|uv| uv[axis]).max().unwrap());
    for uv in &mut quad.uvs {
        *uv = std::array::from_fn(|axis| {
            rect[axis + usize::from(uv[axis] == max[axis] && min[axis] != max[axis]) * 2]
        });
    }
}

/// Emits closed bodies plus two double-sided crossed tips for each live pickle.
fn quads(material: u32, count: u8, dead: bool) -> Vec<ModelQuad> {
    let mut result = Vec::with_capacity((usize::from(count) + 1) * if dead { 7 } else { 9 });
    for &[x, z, height] in bodies(count) {
        let min = [position(x - 0.125), 0, position(z - 0.125)];
        let max = [position(x + 0.125), position(height), position(z + 0.125)];
        let side_top = ((1.0 - height) * 4096.0).round() as u16;
        let regions = [
            [3072, side_top, 4096, 4096],
            [1024, side_top, 2048, 4096],
            [2048, 1280, 3072, 256],
            [1024, 1280, 2048, 256],
            [0, side_top, 1024, 4096],
            [2048, side_top, 3072, 4096],
        ];
        for (mut quad, rect) in super::geometry::vanilla_cuboid_quads([material; 6], min, max)
            .into_iter()
            .zip(regions)
        {
            region(&mut quad, rect);
            result.push(quad);
        }
        let low = position(height - 0.05);
        let mut cap =
            super::geometry::vanilla_cuboid_quads([material; 6], min, max)[BlockFace::Up as usize];
        for vertex in &mut cap.positions {
            vertex[1] = low;
        }
        region(&mut cap, [2048, 1280, 3072, 256]);
        result.push(cap);
        if dead {
            continue;
        }
        let high = position(height + 0.15);
        let [west, east, north, south] = [x - 0.04, x + 0.04, z - 0.04, z + 0.04].map(position);
        for (positions, u) in [
            (
                [
                    [west, high, north],
                    [west, low, north],
                    [east, low, south],
                    [east, high, south],
                ],
                0,
            ),
            (
                [
                    [east, high, north],
                    [east, low, north],
                    [west, low, south],
                    [west, high, south],
                ],
                3072,
            ),
        ] {
            result.push(ModelQuad {
                positions,
                uvs: [[u, 0], [u, 1280], [u + 1024, 1280], [u + 1024, 0]],
                material,
                flags: MODEL_QUAD_FLAG_TWO_SIDED,
            });
        }
    }
    result
}
