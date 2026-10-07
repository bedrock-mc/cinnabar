use super::super::*;
use super::context::{
    CuboidTemplateKey, ModelStorage, RuleInputs, diagnostic_visual, intern_cuboid_template,
    intern_snow_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

const FULL: i16 = 256;
/// One snow layer is two pixels tall.
const SNOW_LAYER: i16 = FULL / assets::TOP_SNOW_LAYER_COUNT as i16;
/// Repeaters and comparators sit on a two-pixel base.
const REDSTONE_BASE: i16 = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::compiler) struct NamedShape {
    /// Box height in 1/256 block; `FULL` compiles to an occluding cube.
    pub(in crate::compiler) top: i16,
    /// Side art has transparent rows the box cuts away, so the atlas needs alpha.
    pub(in crate::compiler) cutout: bool,
    /// Labeled approximation: missing parts or state-keyed art the pack route cannot select.
    pub(in crate::compiler) provisional: bool,
}

const fn shape(top: i16, cutout: bool, provisional: bool) -> NamedShape {
    NamedShape {
        top,
        cutout,
        provisional,
    }
}

/// Registry states the pack draws as a plain cube or a floor-anchored box.
pub(in crate::compiler) fn named_shape(record: &RegistryRecord) -> Option<NamedShape> {
    if !matches!(record.contributor_role, ContributorRole::Primary)
        || record.flags.contains(BlockFlags::AIR)
    {
        return None;
    }
    let name = record.name.strip_prefix("minecraft:")?;
    // Stateless literal cubes (redstone lamps, moss, sculk, nylium, ...) belong to `literal.rs`.
    Some(match name {
        "sculk_catalyst" | "chiseled_sulfur" | "polished_sulfur" | "sulfur_bricks"
        | "chiseled_cinnabar" | "polished_cinnabar" | "cinnabar_bricks" => {
            shape(FULL, false, false)
        }
        // The pack keys one texture per oxidation level; lit and powered art is state-keyed.
        _ if name.ends_with("copper_bulb") => shape(FULL, false, true),
        // Side art is opaque only from the second pixel row down.
        "grass_path" => shape(15 * 16, true, false),
        "snow_layer" => {
            let height = canonical_state_u32(&record.canonical_state, "height")?;
            if height >= u32::from(assets::TOP_SNOW_LAYER_COUNT) {
                return None;
            }
            shape((height as i16 + 1) * SNOW_LAYER, false, false)
        }
        // Base slab only; the facing rotation of the top and the torches are not modelled.
        "unpowered_repeater"
        | "powered_repeater"
        | "unpowered_comparator"
        | "powered_comparator" => shape(REDSTONE_BASE, false, true),
        _ => return None,
    })
}

pub(in crate::compiler) fn is_named_block(record: &RegistryRecord) -> bool {
    named_shape(record).is_some()
}

/// Atlas alpha flag for a named block's art; the enchanting table base is compiled by the
/// entity-drawn rule but shares the cut-away side rows.
pub(in crate::compiler) fn named_block_material_flags(record: &RegistryRecord) -> Option<u32> {
    if record.name.as_ref() == "minecraft:enchanting_table" {
        return Some(MATERIAL_FLAG_ALPHA_CUTOUT);
    }
    named_shape(record).map(|shape| {
        if shape.cutout {
            MATERIAL_FLAG_ALPHA_CUTOUT
        } else {
            0
        }
    })
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<CuboidTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    let Some(shape) = named_shape(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record) {
        if shape.top >= FULL {
            visual.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
            visual.faces = materials;
            visual.kind = VisualKind::Cube;
        } else {
            let max = [FULL, shape.top, FULL];
            let template = if record.name.as_ref() == "minecraft:snow_layer" {
                intern_snow_template(materials, max, templates, storage.templates, storage.quads)?
            } else {
                intern_cuboid_template(
                    materials,
                    [0; 3],
                    max,
                    templates,
                    storage.templates,
                    storage.quads,
                )?
            };
            set_model_visual(&mut visual, materials, template);
        }
        if shape.provisional {
            visual.support = VisualSupport::VanillaFallback;
        }
        if record.name.as_ref() == "minecraft:snow_layer" {
            visual.variant |= assets::BLOCK_VISUAL_VARIANT_TOP_SNOW;
        }
    }
    Ok(CompileRuleResult::Compiled(visual))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::RegistryProvenance;

    fn record(name: &str, state: &str) -> RegistryRecord {
        RegistryRecord {
            sequential_id: 1,
            network_hash: 1,
            name: name.into(),
            canonical_state: state.into(),
            flags: BlockFlags::empty(),
            model_family: ModelFamily::Unknown,
            contributor_role: ContributorRole::Primary,
            model_state: Default::default(),
            face_coverage: 0,
            collision_seed: Default::default(),
            provenance: RegistryProvenance::PMMP,
        }
    }

    #[test]
    fn snow_layers_are_two_pixels_each_and_the_eighth_is_a_full_cube() {
        let layer = |height: u32| {
            named_shape(&record(
                "minecraft:snow_layer",
                &format!(r#"{{"height":{{"type":"int","value":{height}}}}}"#),
            ))
        };
        let count = u32::from(assets::TOP_SNOW_LAYER_COUNT);
        assert_eq!(layer(0).map(|s| s.top), Some(SNOW_LAYER));
        assert_eq!(layer(count / 2 - 1).map(|s| s.top), Some(FULL / 2));
        assert_eq!(layer(count - 1).map(|s| s.top), Some(FULL));
        assert_eq!(layer(count), None);
    }

    #[test]
    fn copper_bulbs_and_repeaters_are_provisional_and_plain_cubes_are_not() {
        for name in [
            "copper_bulb",
            "waxed_oxidized_copper_bulb",
            "powered_repeater",
        ] {
            let shape = named_shape(&record(&format!("minecraft:{name}"), "{}")).expect(name);
            assert!(shape.provisional, "{name}");
        }
        let bricks = named_shape(&record("minecraft:sulfur_bricks", "{}")).expect("bricks");
        assert_eq!((bricks.top, bricks.provisional), (FULL, false));
        assert_eq!(named_shape(&record("minecraft:stone", "{}")), None);
    }

    #[test]
    fn literal_cube_names_are_owned_by_the_literal_rule() {
        for name in [
            "redstone_lamp",
            "moss_block",
            "sculk",
            "crimson_nylium",
            "target",
        ] {
            let state = record(&format!("minecraft:{name}"), "{}");
            assert_eq!(named_shape(&state), None, "{name}");
            assert!(super::super::literal::is_literal_cube(&state), "{name}");
        }
    }
}
