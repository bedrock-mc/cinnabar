use super::*;
use serde_json::json;

/// Compiles material groups and authored visibility for a two-bone controller fixture.
fn compile(controller: Value) -> (assets::EntityRenderData, MolangCompiler) {
    let description =
        json!({"materials": {"solid": "entity_alphatest", "blend": "entity_alphablend"}});
    let geometry: EntityGeometry = serde_json::from_value(json!({
        "identifier": "geometry.test", "source_index": 0, "texture_width": 1, "texture_height": 1,
        "bones": [{"name": "root", "cubes": []}, {"name": "eye", "parent": "root", "cubes": []}]
    }))
    .unwrap();
    let controller = controller.as_object().unwrap();
    let groups = resolve(
        description.as_object().unwrap(),
        controller,
        &MaterialStates::default(),
        &[geometry],
        [0],
        &mut None,
    )
    .unwrap();
    let mut molang = MolangCompiler::default();
    let mut visibility = Vec::new();
    for rule in controller
        .get("part_visibility")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for (pattern, value) in rule.as_object().unwrap() {
            if let Some(condition) =
                super::super::expression_text(value).and_then(|text| molang.compile(&text).ok())
            {
                visibility.push(EntityRenderVisibility {
                    pattern: pattern.to_ascii_lowercase().into(),
                    condition,
                });
            }
        }
    }
    let base: EntityRenderLayer = serde_json::from_value(json!({
        "rig": 0, "condition": null, "first_slot": 0, "slot_count": 0,
        "first_visibility": 0, "visibility_count": visibility.len(),
        "color": null, "overlay_color": null, "on_fire_color": null
    }))
    .unwrap();
    let mut layers = Vec::new();
    let mut selections = Vec::new();
    append(
        base,
        groups,
        Records {
            selections: &mut selections,
            layers: &mut layers,
            slots: &mut Vec::new(),
            candidates: &mut Vec::new(),
            visibility: &mut visibility,
            geometries: &mut Vec::new(),
        },
        &mut molang,
    )
    .unwrap();
    (
        assets::EntityRenderData {
            layers: layers.into(),
            visibility: visibility.into(),
            material_selections: selections.into(),
            ..Default::default()
        },
        molang,
    )
}

/// Reads expected raster words from the material policy shared by all compiler consumers.
fn material_word(name: &str) -> u32 {
    let rules = assets::facts::pack::rules();
    rules.raster(name).word(rules.material_state(name))
}

/// Resolves the raster words that actually draw one named bone for a fixture query value.
fn drawn(fixture: &(assets::EntityRenderData, MolangCompiler), bone: &str, value: f32) -> Vec<u32> {
    use assets::EntityRenderMaterialSelection as Node;
    let (data, molang) = fixture;
    let read = |index| {
        molang
            .evaluate_query(index, "query.is_invisible", value)
            .unwrap()
    };
    data.layers
        .iter()
        .filter(|layer| layer.condition.is_none_or(|index| read(index) != 0.0))
        .filter(|layer| {
            data.visibility[layer.first_visibility as usize
                ..layer.first_visibility as usize + usize::from(layer.visibility_count)]
                .iter()
                .filter(|rule| entity_render_pattern_matches(&rule.pattern, bone))
                .next_back()
                .is_none_or(|rule| read(rule.condition) != 0.0)
        })
        .map(|layer| {
            let Some(mut selected) = layer.material_selection else {
                return layer.material.word(layer.material_state);
            };
            loop {
                selected = match &data.material_selections[selected as usize] {
                    Node::Raster { material, state } => return material.word(*state),
                    Node::Branch { condition, yes, no } => {
                        if read(*condition) != 0.0 {
                            *yes
                        } else {
                            *no
                        }
                    }
                    Node::Array { index, choices } => {
                        choices[(read(*index).max(0.0) as usize) % choices.len()]
                    }
                };
            }
        })
        .collect()
}

#[test]
fn material_arrays_select_one_raster_with_wrapped_and_clamped_indices() {
    let fixture = compile(json!({
        "arrays": {"materials": {"Array.states": ["Material.solid", "Material.blend"]}},
        "materials": [{"*": "Array.states[query.is_invisible]"}]
    }));
    let solid = material_word("entity_alphatest");
    let blend = material_word("entity_alphablend");
    for (index, expected) in [
        (0.0, solid),
        (1.0, blend),
        (2.0, solid),
        (-1.0, solid),
        (1.9, blend),
    ] {
        assert_eq!(drawn(&fixture, "root", index), [expected]);
        assert_eq!(drawn(&fixture, "eye", index), [expected]);
    }
}

#[test]
fn material_ternaries_preserve_last_bone_rules_and_authored_hiding() {
    let fixture = compile(json!({
        "materials": [{"*": "query.is_invisible ? Material.blend : Material.solid"}, {"eye": "Material.solid"}],
        "part_visibility": [{"*": true}, {"root": false}]
    }));
    for value in [0.0, 1.0] {
        assert!(drawn(&fixture, "root", value).is_empty());
        assert_eq!(
            drawn(&fixture, "eye", value),
            [material_word("entity_alphatest")]
        );
    }
}

#[test]
fn material_selection_keeps_dynamic_authored_visibility() {
    let fixture = compile(json!({
        "materials": [{"*": "query.is_invisible ? Material.blend : Material.solid"}],
        "part_visibility": [{"*": "!query.is_invisible"}]
    }));
    assert_eq!(
        drawn(&fixture, "root", 0.0),
        [material_word("entity_alphatest")]
    );
    assert!(drawn(&fixture, "root", 1.0).is_empty());
}

#[test]
fn wide_equivalent_material_arrays_draw_each_bone_once() {
    let fixture = compile(json!({
        "arrays": {"materials": {"Array.same": vec!["Material.solid"; 64]}},
        "materials": [{"*": "Array.same[query.is_invisible]"}]
    }));
    for value in [0.0, 1.0, 2.0] {
        assert_eq!(
            drawn(&fixture, "eye", value),
            [material_word("entity_alphatest")]
        );
    }
}

#[test]
fn material_choices_preserve_admitted_visibility_scripts() {
    for text in ["return query.is_alive;", "query.is_alive;"] {
        let fixture = compile(json!({
            "materials": [{"*": "query.is_invisible ? Material.blend : Material.solid"}],
            "part_visibility": [{"*": text}]
        }));
        assert!(
            fixture
                .0
                .layers
                .iter()
                .all(|layer| layer.visibility_count == 1)
        );
    }
}

#[test]
fn static_single_material_needs_no_literal_bone_mask() {
    let mut controller = json!({"materials":[{"*":"Material.solid"},{"eye":"Material.blend"}]});
    let description = json!({"materials":{"solid":"entity_alphatest","blend":"entity_alphablend"}});
    let name = "a".repeat(MAX_ENTITY_RENDER_PATTERN_BYTES + 1);
    let geometry: EntityGeometry = serde_json::from_value(json!({
        "identifier":"geometry.test", "source_index":0, "texture_width":1, "texture_height":1,
        "bones":[{"name":name,"cubes":[]}]
    }))
    .unwrap();
    let groups = resolve(
        description.as_object().unwrap(),
        controller.as_object_mut().unwrap(),
        &MaterialStates::default(),
        &[geometry],
        [0],
        &mut None,
    )
    .unwrap();
    assert_eq!(groups.len(), 1);
    assert!(groups[0].hidden.is_empty());
}
