use super::super::*;
use super::context::{ModelStorage, diagnostic_visual, push_model_template, set_model_visual};
use super::dispatcher::CompileRuleResult;
use super::flowerbed::rotate_flowerbed_position;
use super::state::{exact_tagged_int, exact_tagged_string};

const LEAF_LITTER_HASHES: [u32; 32] = [
    0x834b_5ffc,
    0xa149_5c55,
    0x576a_f652,
    0x5bdd_1223,
    0xb79b_b5b0,
    0x590c_4349,
    0xe883_5116,
    0x4261_2d47,
    0x200a_28f1,
    0x6d49_e4fe,
    0xaa10_c7fb,
    0x0564_5e28,
    0xc43e_17cd,
    0xd627_b70a,
    0x9484_bf17,
    0xc665_5304,
    0xfba6_861a,
    0xc19c_81d7,
    0xeb43_47dc,
    0xdc27_3719,
    0x3930_96fe,
    0x3626_336b,
    0x2c41_9290,
    0x2f3d_304d,
    0xabb9_eec3,
    0xd98b_f63c,
    0x00a5_5ba9,
    0x58b1_545a,
    0x790d_b0a7,
    0x7909_09c0,
    0x5f78_9f1d,
    0x3518_cb9e,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::compiler) struct LeafLitterState {
    pub(in crate::compiler) growth: u32,
    pub(in crate::compiler) orientation: u32,
}

pub(in crate::compiler) fn is_leaf_litter_name(name: &str) -> bool {
    name == "minecraft:leaf_litter"
}

pub(in crate::compiler) fn exact_leaf_litter_state(
    record: &RegistryRecord,
) -> Option<LeafLitterState> {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if state.len() != 2 {
        return None;
    }
    let growth = exact_tagged_int(state.get("growth")?, 7)?;
    let direction = exact_tagged_string(state.get("minecraft:cardinal_direction")?)?;
    let orientation = match direction {
        "south" => 0,
        "west" => 1,
        "north" => 2,
        "east" => 3,
        _ => return None,
    };
    const ORIENTATION_MASK: u8 = 1 << (ModelStateField::Orientation as u8 - 1);
    const GROWTH_MASK: u8 = 1 << (ModelStateField::Growth as u8 - 1);
    if record.model_state.mask() != ORIENTATION_MASK | GROWTH_MASK
        || record.model_state.get(ModelStateField::Orientation) != Some(orientation)
        || record.model_state.get(ModelStateField::Growth) != Some(growth)
    {
        return None;
    }
    Some(LeafLitterState {
        growth,
        orientation,
    })
}

pub(in crate::compiler) fn is_leaf_litter_record(record: &RegistryRecord) -> bool {
    let Some(state) = exact_leaf_litter_state(record) else {
        return false;
    };
    let index = state.orientation * 8 + state.growth;
    let Some(&network_hash) = LEAF_LITTER_HASHES.get(index as usize) else {
        return false;
    };
    is_leaf_litter_name(&record.name)
        && record.sequential_id == 46 + index
        && record.network_hash == network_hash
        && record.model_family == ModelFamily::Layer
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.face_coverage == 0
        && record.collision_seed.shape_id == 0
        && record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.is_empty()
        && record.provenance
            == assets::RegistryProvenance::PMMP
                | assets::RegistryProvenance::DRAGONFLY
                | assets::RegistryProvenance::PRISMARINE
                | assets::RegistryProvenance::VALENTINE
}

pub(in crate::compiler) fn leaf_litter_inventory_is_exact(records: &[RegistryRecord]) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_leaf_litter_name(&record.name))
        .collect::<Vec<_>>();
    if selected.len() != 32 {
        return false;
    }
    let mut seen = [false; 32];
    for record in selected {
        if !is_leaf_litter_record(record) {
            return false;
        }
        let Some(state) = exact_leaf_litter_state(record) else {
            return false;
        };
        let index = (state.orientation * 8 + state.growth) as usize;
        if seen[index] {
            return false;
        }
        seen[index] = true;
    }
    seen.into_iter().all(|present| present)
}

pub(in crate::compiler) fn leaf_litter_material_descriptor(
    pack: &PackSources,
) -> Option<(Descriptor, Box<str>)> {
    let key = pack.blocks.get_exact_leaf_litter()?;
    if key != "leaf_litter" {
        return None;
    }
    let path = pack.terrain.get_exact_singleton_plain(key)?;
    if path != "textures/blocks/leaf_litter"
        || pack.flipbooks.iter().any(|flipbook| {
            flipbook.atlas_tile.as_ref() == key || flipbook.texture_path.as_ref() == path
        })
    {
        return None;
    }
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.into(),
            flags: MATERIAL_FLAG_ALPHA_CUTOUT
                | MATERIAL_FLAG_FOLIAGE_TINT
                | MATERIAL_FLAG_DRY_FOLIAGE,
        },
        key.into(),
    ))
}

pub(in crate::compiler) fn leaf_litter_source_alpha_is_exact(
    root: &Path,
    pack: &PackSources,
) -> bool {
    let Some((descriptor, key)) = leaf_litter_material_descriptor(pack) else {
        return false;
    };
    let Ok(path) = static_texture_path(root, &descriptor.path, &key) else {
        return false;
    };
    let Ok(rgba8) = decode_static_texture(&path, &key) else {
        return false;
    };
    let mut transparent = false;
    let mut opaque = false;
    for pixel in rgba8.chunks_exact(4) {
        match pixel[3] {
            0 => transparent = true,
            u8::MAX => opaque = true,
            _ => return false,
        }
    }
    transparent && opaque
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    admitted: bool,
    material_by_descriptor: &BTreeMap<Descriptor, u32>,
    pack: &PackSources,
    templates: &mut BTreeMap<[u32; 3], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_leaf_litter_name(&record.name) {
        return Ok(CompileRuleResult::NoMatch);
    }
    if !admitted {
        return Ok(CompileRuleResult::Reject);
    }
    let Some(state) = exact_leaf_litter_state(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let Some((descriptor, _)) = leaf_litter_material_descriptor(pack) else {
        return Ok(CompileRuleResult::Reject);
    };
    let Some(&material) = material_by_descriptor.get(&descriptor) else {
        return Ok(CompileRuleResult::Reject);
    };
    const LAYOUT_BY_GROWTH: [u32; 8] = [0, 1, 2, 3, 3, 3, 3, 3];
    let layout = LAYOUT_BY_GROWTH[state.growth as usize];
    let key = [material, layout, state.orientation];
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            leaf_litter_quads(material, layout, state.orientation)?,
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, [material; 6], template);
    Ok(CompileRuleResult::Compiled(visual))
}

fn leaf_litter_quads(
    material: u32,
    layout: u32,
    orientation: u32,
) -> Result<Vec<ModelQuad>, AssetError> {
    type Quad = ([[i16; 3]; 4], [[u16; 2]; 4]);
    const FIRST: Quad = (
        [[0, 4, 0], [128, 4, 0], [128, 4, 128], [0, 4, 128]],
        [[0, 0], [2048, 0], [2048, 2048], [0, 2048]],
    );
    const HALF: Quad = (
        [[0, 4, 0], [128, 4, 0], [128, 4, 256], [0, 4, 256]],
        [[0, 0], [2048, 0], [2048, 4096], [0, 4096]],
    );
    const THIRD: Quad = (
        [[128, 4, 128], [256, 4, 128], [256, 4, 256], [128, 4, 256]],
        [[2048, 2048], [4096, 2048], [4096, 4096], [2048, 4096]],
    );
    const FULL: Quad = (
        [[0, 4, 0], [256, 4, 0], [256, 4, 256], [0, 4, 256]],
        [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
    );
    let source: &[Quad] = match layout {
        0 => &[FIRST],
        1 => &[HALF],
        2 => &[HALF, THIRD],
        3 => &[FULL],
        _ => {
            return Err(AssetError::InvalidCompiledAssets {
                detail: format!("leaf-litter layout {layout} is outside the pinned range").into(),
            });
        }
    };
    source
        .iter()
        .map(|&(positions, uvs)| {
            let mut rotated = positions;
            for position in &mut rotated {
                *position = rotate_flowerbed_position(*position, orientation)?;
            }
            Ok(ModelQuad {
                positions: rotated,
                uvs,
                material,
                flags: MODEL_QUAD_FLAG_TWO_SIDED,
            })
        })
        .collect()
}
