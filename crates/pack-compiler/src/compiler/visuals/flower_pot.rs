use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn soil_material(
    records: &[RegistryRecord],
    inputs: &RuleInputs<'_>,
) -> Option<u32> {
    records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:dirt"
                && serde_json::from_str::<serde_json::Value>(&record.canonical_state)
                    .ok()
                    .is_some_and(|state| state.get("dirt_type").is_none_or(|kind| kind == "normal"))
        })
        .and_then(|record| inputs.material(record, BlockFace::Up))
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    soil: Option<u32>,
    templates: &mut BTreeMap<[u32; 7], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if record.name.as_ref() != "minecraft:flower_pot" {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((min, max, _)) = inputs.fallback.entry(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let Some(soil) = soil else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let materials = BlockFace::ALL.map(|face| {
        inputs
            .material(record, face)
            .unwrap_or(inputs.vanilla_fallback_material)
    });
    let key = [
        materials[0],
        materials[1],
        materials[2],
        materials[3],
        materials[4],
        materials[5],
        soil,
    ];
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let mut quads = cuboid_quads(materials, min, max).to_vec();
        // Dirt fills the opening at four pixels, below the six-pixel rim.
        // Quantize the small depth bias at the carrier's 1/256 boundary.
        let height = ((0.25_f32 - 0.001) * 256.0).round() as i16;
        let positions = [
            [min[0], height, max[2]],
            [max[0], height, max[2]],
            [max[0], height, min[2]],
            [min[0], height, min[2]],
        ];
        quads.push(ModelQuad {
            positions,
            uvs: positions.map(|[x, _, z]| [(x as u16) * 16, (z as u16) * 16]),
            material: soil,
            flags: 2,
        });
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, materials, template);
    // The added dirt is explicit; the body still uses the provisional envelope.
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

    use super::super::super::*;

    #[test]
    fn flower_pots_have_an_upward_dirt_floor_inside_the_rim() {
        let protocol = assets::active_content_registry_protocol();
        let registry = assets::read_registry_for_protocol(
            include_bytes!("../../../../assets/data/block-registry-v2193.bin"),
            protocol,
        )
        .unwrap();
        let records = registry
            .into_iter()
            .filter(|record| {
                matches!(
                    record.name.as_ref(),
                    "minecraft:flower_pot" | "minecraft:dirt" | "minecraft:stone" | "minecraft:air"
                )
            })
            .collect::<Vec<_>>();
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("textures/blocks")).unwrap();
        fs::write(root.join("blocks.json"), r#"{"flower_pot":{"textures":"pot"},"dirt":{"textures":"custom_soil"},"stone":{"textures":"stone"}}"#).unwrap();
        fs::write(root.join("textures/terrain_texture.json"), r#"{"texture_data":{"pot":{"textures":"textures/blocks/pot"},"custom_soil":{"textures":"textures/blocks/soil"},"stone":{"textures":"textures/blocks/stone"}}}"#).unwrap();
        fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
        for (name, colour) in [
            ("pot", [160, 60, 30, 255]),
            ("soil", [80, 50, 20, 255]),
            ("stone", [120, 120, 120, 255]),
        ] {
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(&colour.repeat(16 * 16), 16, 16, ExtendedColorType::Rgba8)
                .unwrap();
            fs::write(root.join(format!("textures/blocks/{name}.png")), png).unwrap();
        }
        let lights = vec![
            LightProperties::default();
            records
                .iter()
                .map(|record| record.sequential_id as usize + 1)
                .max()
                .unwrap()
        ];
        let (compiled, _) = compile_pack_inner(
            root,
            &records,
            &lights,
            CompiledBiomeAssets::diagnostic(),
            protocol,
        )
        .unwrap();
        let dirt = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:dirt")
            .unwrap();
        let soil = compiled.visuals[dirt.sequential_id as usize].faces[BlockFace::Up as usize];
        let pots = records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:flower_pot")
            .collect::<Vec<_>>();
        assert!(!pots.is_empty());
        let mut templates = std::collections::BTreeSet::new();
        for pot in pots {
            let visual = compiled.visuals[pot.sequential_id as usize];
            templates.insert(visual.model_template);
            let template = compiled.model_templates[visual.model_template as usize];
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            let floor = quads
                .iter()
                .find(|quad| quad.material == soil && quad.positions.iter().all(|p| p[1] == 64))
                .expect("the pot needs a dirt floor below its rim");
            assert!(
                floor
                    .positions
                    .iter()
                    .all(|p| (80..=176).contains(&p[0]) && (80..=176).contains(&p[2]))
            );
            let [a, b, c, _] = floor.positions;
            let normal_y = i32::from(b[2] - a[2]) * i32::from(c[0] - a[0])
                - i32::from(b[0] - a[0]) * i32::from(c[2] - a[2]);
            assert!(normal_y > 0, "floor must be visible from above");
            assert_eq!(
                floor.flags & (MODEL_QUAD_FLAG_FACE_MASK << 4),
                0,
                "support blocks must not cull the inset floor"
            );
            assert_eq!(quads.len(), 7);
        }
        assert_eq!(
            templates.len(),
            1,
            "occupied and empty pots share their body geometry"
        );
    }
}
