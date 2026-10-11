//! Cake bases with one centered candle and its crossed wick.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

/// Identifies the undyed candle cake and the sixteen vanilla dye families.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    let Some(candle) = record
        .name
        .strip_prefix("minecraft:")
        .and_then(|name| name.strip_suffix("_cake"))
    else {
        return false;
    };
    matches!(
        candle,
        "candle"
            | "white_candle"
            | "orange_candle"
            | "magenta_candle"
            | "light_blue_candle"
            | "yellow_candle"
            | "lime_candle"
            | "pink_candle"
            | "gray_candle"
            | "light_gray_candle"
            | "cyan_candle"
            | "purple_candle"
            | "blue_candle"
            | "brown_candle"
            | "green_candle"
            | "red_candle"
            | "black_candle"
    )
}

/// Validates the single typed lit selector and the cake collision contract.
fn lit(record: &RegistryRecord) -> Option<u8> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct State {
        lit: Tagged,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Tagged {
        #[serde(rename = "type")]
        kind: String,
        value: u8,
    }
    let state: State = serde_json::from_str(&record.canonical_state).ok()?;
    (is_record(record)
        && state.lit.kind == "byte"
        && state.lit.value <= 1
        && record.model_family == ModelFamily::Unknown
        && record.model_state.mask() == 0
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.face_coverage == 0
        && super::cake::cake_collision_is_exact(record, 0))
    .then_some(state.lit.value)
}

/// Resolves the default cake and the corresponding candle's authored lit texture.
pub(in crate::compiler) fn descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<Vec<(Descriptor, Box<str>)>> {
    let lit = lit(record)?;
    let mut descriptors = super::cake::cake_material_descriptors(pack)?
        .into_iter()
        .collect::<Vec<_>>();
    let mut candle = record.clone();
    candle.name = record.name.strip_suffix("_cake")?.into();
    let TextureKey { key, rotate_uv } =
        resolve_texture_key(&pack.blocks, &candle, assets::BlockFace::Down);
    let key = key?;
    let (path, state_variant) = pack
        .terrain
        .get_clamped_untinted_variant(&key, usize::from(lit))?;
    descriptors.push((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: MATERIAL_FLAG_ALPHA_CUTOUT
                | if rotate_uv {
                    MATERIAL_FLAG_ROTATE_UV
                } else {
                    0
                },
        },
        key,
    ));
    Some(descriptors)
}

/// Compiles the composite before a collision-only cuboid can claim its identity.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 4], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    if lit(record).is_none() {
        return Ok(CompileRuleResult::Reject);
    }
    let mut visual = diagnostic_visual(record);
    let Some(descriptors) = descriptors(inputs.pack, record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(materials) = descriptors
        .into_iter()
        .map(|(descriptor, _)| inputs.material_by_descriptor.get(&descriptor).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let [side, bottom, top, _, candle] = materials.as_slice() else {
        unreachable!("cake and candle descriptors");
    };
    let key = [*side, *bottom, *top, *candle];
    let faces = [*side, *side, *bottom, *top, *side, *side];
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template =
            push_model_template(quads(faces, *candle), 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, faces, template);
    visual.support = VisualSupport::Exact;
    Ok(CompileRuleResult::Compiled(visual))
}

/// Emits the cake, six candle faces, and both sides of each diagonal wick plane.
fn quads(faces: [u32; 6], candle: u32) -> Vec<ModelQuad> {
    let mut quads =
        super::geometry::vanilla_cuboid_quads(faces, [16, 0, 16], [240, 128, 240]).to_vec();
    for (face, mut quad) in
        assets::BlockFace::ALL
            .into_iter()
            .zip(super::geometry::vanilla_cuboid_quads(
                [candle; 6],
                [112, 128, 112],
                [144, 224, 144],
            ))
    {
        let region = match face {
            assets::BlockFace::Down => [0, 14, 2, 16],
            assets::BlockFace::Up => [0, 6, 2, 8],
            _ => [0, 8, 2, 14],
        };
        for axis in 0..2 {
            let min = quad.uvs.iter().map(|uv| uv[axis]).min().unwrap();
            let max = quad.uvs.iter().map(|uv| uv[axis]).max().unwrap();
            for uv in &mut quad.uvs {
                uv[axis] = region[axis] * 256
                    + (u32::from(uv[axis] - min)
                        * u32::from((region[axis + 2] - region[axis]) * 256)
                        / u32::from(max - min)) as u16;
            }
        }
        quads.push(quad);
    }
    for positions in [
        [
            [120, 240, 120],
            [120, 224, 120],
            [136, 224, 136],
            [136, 240, 136],
        ],
        [
            [120, 240, 136],
            [120, 224, 136],
            [136, 224, 120],
            [136, 240, 120],
        ],
    ] {
        let quad = ModelQuad {
            positions,
            uvs: [[0, 1280], [0, 1536], [256, 1536], [256, 1280]],
            material: candle,
            flags: 0,
        };
        quads.push(quad);
        let mut back = quad;
        back.positions.reverse();
        back.uvs.reverse();
        quads.push(back);
    }
    quads
}
