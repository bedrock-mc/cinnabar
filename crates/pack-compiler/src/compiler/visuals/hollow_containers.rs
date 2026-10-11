//! Open vessels keep their inward walls, inset floors and separate fill surfaces.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Names whose texture slots describe vessel parts instead of ordinary cube faces.
pub(in crate::compiler) fn is_name(name: &str) -> bool {
    matches!(
        name,
        "minecraft:composter" | "minecraft:cauldron" | "minecraft:lava_cauldron"
    )
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum VesselState {
    Composter(u8),
    Cauldron { level: u8, liquid: u8 },
}

/// Admits the typed fill selectors while keeping unknown state products diagnostic.
pub(in crate::compiler) fn state(record: &RegistryRecord) -> Option<VesselState> {
    if record.contributor_role != ContributorRole::Primary || record.flags.contains(BlockFlags::AIR)
    {
        return None;
    }
    let values: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if record.name.as_ref() == "minecraft:composter" && values.len() == 1 {
        return Some(VesselState::Composter(super::state::exact_tagged_int(
            values.get("composter_fill_level")?,
            8,
        )? as u8));
    }
    if matches!(
        record.name.as_ref(),
        "minecraft:cauldron" | "minecraft:lava_cauldron"
    ) && values.len() == 2
    {
        let level = super::state::exact_tagged_int(values.get("fill_level")?, 6)? as u8;
        let liquid = match super::state::exact_tagged_string(values.get("cauldron_liquid")?)? {
            "water" => 0,
            "lava" => 1,
            "powder_snow" => 2,
            _ => return None,
        };
        return Some(VesselState::Cauldron { level, liquid });
    }
    None
}

/// Resolves an authored face slot and explicit array index, honoring static texture aliases.
fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
    variant: usize,
    flags: u32,
) -> Option<(Descriptor, Box<str>)> {
    let TextureKey { key, rotate_uv } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, variant)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            flags: flags
                | if rotate_uv {
                    MATERIAL_FLAG_ROTATE_UV
                } else {
                    0
                },
            state_variant,
            flipbook_selector: None,
        },
        key,
    ))
}

/// Lists body slots followed by the optional contents material for this state.
pub(in crate::compiler) fn descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<Vec<(Descriptor, Box<str>)>> {
    let state = state(record)?;
    let mut selected = Vec::new();
    for face in [BlockFace::North, BlockFace::Down, BlockFace::Up] {
        selected.push(descriptor(
            pack,
            record,
            face,
            0,
            MATERIAL_FLAG_ALPHA_CUTOUT,
        )?);
    }
    match state {
        VesselState::Composter(level) => {
            selected.push(descriptor(
                pack,
                record,
                if level == 0 {
                    BlockFace::Down
                } else {
                    BlockFace::Up
                },
                if level == 0 {
                    0
                } else if level == 8 {
                    2
                } else {
                    1
                },
                MATERIAL_FLAG_ALPHA_CUTOUT,
            )?);
            for face in [BlockFace::East, BlockFace::West, BlockFace::South] {
                selected.push(descriptor(
                    pack,
                    record,
                    face,
                    0,
                    MATERIAL_FLAG_ALPHA_CUTOUT,
                )?);
            }
        }
        VesselState::Cauldron { level, liquid } => {
            selected.push(descriptor(
                pack,
                record,
                BlockFace::South,
                0,
                MATERIAL_FLAG_ALPHA_CUTOUT,
            )?);
            if level > 0 {
                selected.push(if liquid != 0 {
                    let key: Box<str> = if liquid == 1 {
                        "still_lava"
                    } else {
                        "powder_snow"
                    }
                    .into();
                    let (path, state_variant) =
                        pack.terrain.get_clamped_untinted_variant(&key, 0)?;
                    (
                        Descriptor {
                            path: path.into(),
                            texture_key: key.clone(),
                            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
                            state_variant,
                            flipbook_selector: None,
                        },
                        key,
                    )
                } else {
                    descriptor(
                        pack,
                        record,
                        BlockFace::East,
                        0,
                        MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_CAULDRON_WATER,
                    )?
                });
            }
        }
    }
    Some(selected)
}

/// Compiles a bounded chain so detailed cauldron feet retain independent visibility masks.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(VesselState, Vec<u32>), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_name(&record.name) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(state) = state(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = descriptors(inputs.pack, record).and_then(|descriptors| {
        descriptors
            .into_iter()
            .map(|(descriptor, _)| inputs.material_by_descriptor.get(&descriptor).copied())
            .collect::<Option<Vec<_>>>()
    }) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (state, materials.clone());
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = super::container_geometry::quads(state, &materials);
        let head = storage.templates.len() as u32;
        let chunks = quads.chunks(assets::MAX_MODEL_TEMPLATE_QUADS);
        let count = chunks.len();
        for (index, chunk) in chunks.enumerate() {
            push_model_template(
                chunk.to_vec(),
                if index + 1 < count {
                    MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
                } else {
                    0
                },
                storage.templates,
                storage.quads,
            )?;
        }
        templates.insert(key, head);
        head
    };
    let [side, bottom, top, ..] = materials.as_slice() else {
        unreachable!("vessel materials");
    };
    let faces = if matches!(state, VesselState::Composter(_)) {
        [
            materials[5],
            materials[4],
            *bottom,
            *top,
            *side,
            materials[6],
        ]
    } else {
        [*side, *side, *bottom, *top, *side, *side]
    };
    set_model_visual(&mut visual, faces, template);
    // Block-actor water/potion colours require a separate runtime data path.
    if matches!(state, VesselState::Cauldron { .. }) {
        visual.support = VisualSupport::VanillaFallback;
    }
    Ok(CompileRuleResult::Compiled(visual))
}
