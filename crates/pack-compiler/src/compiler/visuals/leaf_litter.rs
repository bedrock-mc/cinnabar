use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::state::{exact_tagged_int, exact_tagged_string};
use {super::super::*, assets::BlockFace};

/// Compiles state-selected ground patches independently from the carried item artwork.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 3], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if assets::facts::block_render_fact(&record.name).kind
        != assets::facts::BlockRenderKind::LeafLitter
        || record.contributor_role != ContributorRole::Primary
    {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some((growth, orientation)) = state(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(material) = inputs.material(record, BlockFace::Up) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let growth = growth.min(3);
    let key = [material, growth, orientation];
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            super::flowerbed::leaf_litter_quads(material, growth, orientation)?,
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, [material; 6], template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Admits the palette's growth and cardinal state, retaining saturated command-only growth.
fn state(record: &RegistryRecord) -> Option<(u32, u32)> {
    use assets::facts::BlockProperty as P;
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
    let growth = exact_tagged_int(state.get(P::Growth.name())?, 7)?;
    let orientation = match exact_tagged_string(state.get(P::MinecraftCardinalDirection.name())?)? {
        "south" => 0,
        "west" => 1,
        "north" => 2,
        "east" => 3,
        _ => return None,
    };
    Some((growth, orientation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_follow_cardinal_direction_and_keep_their_texture_quadrants() {
        for (orientation, bounds) in [
            (0, [128, 128, 256, 256]),
            (1, [0, 128, 128, 256]),
            (2, [0, 0, 128, 128]),
            (3, [128, 0, 256, 128]),
        ] {
            let quads = super::super::flowerbed::leaf_litter_quads(17, 0, orientation).unwrap();
            assert_eq!(quads.len(), 1);
            let quad = &quads[0];
            let xs = quad.positions.map(|position| position[0]);
            let zs = quad.positions.map(|position| position[2]);
            assert_eq!(
                [
                    *xs.iter().min().unwrap(),
                    *zs.iter().min().unwrap(),
                    *xs.iter().max().unwrap(),
                    *zs.iter().max().unwrap()
                ],
                bounds,
            );
            assert_eq!(quad.uvs, [[0, 0], [2048, 0], [2048, 2048], [0, 2048]]);
            assert!(quad.positions.iter().all(|position| position[1] == 4));
            assert_eq!(quad.material, 17);
        }
    }

    #[test]
    fn malformed_ground_cover_state_is_unresolved() {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let mut record = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:leaf_litter")
            .unwrap()
            .clone();
        for canonical in [
            r#"{}"#,
            r#"{"growth":{"type":"int","value":8},"minecraft:cardinal_direction":{"type":"string","value":"south"}}"#,
            r#"{"growth":{"type":"byte","value":0},"minecraft:cardinal_direction":{"type":"string","value":"south"}}"#,
            r#"{"growth":{"type":"int","value":0},"minecraft:cardinal_direction":{"type":"string","value":"up"}}"#,
        ] {
            record.canonical_state = canonical.into();
            assert_eq!(state(&record), None);
        }
    }
}
