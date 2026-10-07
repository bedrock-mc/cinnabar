use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

/// Ice, slime, honey, tinted glass, and powder snow as a unit cube on the transparent-cube path.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 6], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_translucent_cube(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record) {
        let template = if let Some(&template) = templates.get(&materials) {
            template
        } else {
            let template = push_model_template(
                native_cube_quads(materials).to_vec(),
                MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(materials, template);
            template
        };
        set_model_visual(&mut visual, materials, template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

fn native_cube_quads(materials: [u32; 6]) -> [ModelQuad; 6] {
    super::geometry::vanilla_cuboid_quads(materials, [0, 0, 0], [256, 256, 256])
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::RegistryProvenance;

    fn record(name: &str, flags: BlockFlags) -> RegistryRecord {
        RegistryRecord {
            sequential_id: 1,
            network_hash: 1,
            name: name.into(),
            canonical_state: "{}".into(),
            flags,
            model_family: ModelFamily::Unknown,
            contributor_role: ContributorRole::Primary,
            model_state: Default::default(),
            face_coverage: 0,
            collision_seed: Default::default(),
            provenance: RegistryProvenance::PMMP,
        }
    }

    #[test]
    fn admits_only_the_named_translucent_cubes() {
        for name in [
            "ice",
            "frosted_ice",
            "slime",
            "honey_block",
            "tinted_glass",
            "powder_snow",
        ] {
            let record = record(&format!("minecraft:{name}"), BlockFlags::empty());
            assert!(is_translucent_cube(&record), "{name}");
        }
        assert!(!is_translucent_cube(&record(
            "minecraft:glass",
            BlockFlags::empty()
        )));
        assert!(!is_translucent_cube(&record(
            "minecraft:ice",
            BlockFlags::AIR
        )));
    }

    #[test]
    fn powder_snow_stays_opaque_and_the_rest_blend() {
        assert_eq!(translucent_cube_material_flags("minecraft:powder_snow"), 0);
        assert_eq!(
            translucent_cube_material_flags("minecraft:ice"),
            MATERIAL_FLAG_ALPHA_BLEND
        );
    }

    #[test]
    fn transparent_cube_uvs_follow_native_opposing_face_axes() {
        for quad in native_cube_quads([1; 6]) {
            for ([x, y, z], uv) in quad.positions.into_iter().zip(quad.uvs) {
                let [x, y, z] = [x, y, z].map(|v| v as u16 * 16);
                let expected = match quad.flags {
                    1 => [x, 4096 - z],
                    2 => [x, z],
                    3 => [z, 4096 - y],
                    4 => [4096 - z, 4096 - y],
                    5 => [4096 - x, 4096 - y],
                    6 => [x, 4096 - y],
                    _ => unreachable!(),
                };
                assert_eq!(uv, expected, "native cube face {}", quad.flags);
            }
        }
    }
}
