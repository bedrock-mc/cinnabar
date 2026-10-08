use super::{MANIFEST, pack, write};
use serde_json::{Value, json};

fn state(material: &str, definitions: Option<&Value>) -> Value {
    let fixture = pack(0, material, false);
    if let Some(definitions) = definitions {
        write(
            fixture.path(),
            "materials/entity.material",
            &serde_json::to_vec(definitions).unwrap(),
        );
    }
    let compiled = pack_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.render.layers.len(), 1);
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    let runtime = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
    serde_json::to_value(runtime.render_data().layers[0]).unwrap()["material_state"].clone()
}

#[test]
fn actor_material_states_distinguish_authored_one_sided_and_nocull_alpha_test() {
    assert_eq!(
        state("entity_alphatest_one_sided", None),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":true})
    );
    assert_eq!(
        state("entity_alphatest", None),
        json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true})
    );
}

#[test]
fn skeleton_alias_uses_double_sided_alpha_test_without_material_definitions() {
    let expected = json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true});
    assert_eq!(state("skeleton", None), expected);
    assert_eq!(state("skeleton.skinning", None), expected);
}

#[test]
fn wither_body_and_armor_keep_their_material_states_without_material_definitions() {
    assert_eq!(
        state("wither_boss", None),
        json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true})
    );
    // Blended armor selects the transparent pass after inherited defines.
    let armor = json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true,
        "additive":true});
    assert_eq!(state("wither_boss_armor", None), armor);
    assert_eq!(state("charged_creeper", None), armor);

    let definitions = json!({"materials":{
        "wither_boss_armor:charged_creeper":{
            "-states":["DisableCulling"],
            "blendSrc":"SourceAlpha","blendDst":"OneMinusSrcAlpha"
        }
    }});
    assert_eq!(
        state("wither_boss_armor", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":true,"depth_write":true})
    );
}

#[test]
fn emissive_materials_keep_alpha_as_lighting_weight_and_authored_additive_inheritance() {
    assert_eq!(
        state("entity_emissive_alpha", None),
        json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true,"emissive":true})
    );
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_reflection:entity_emissive":{
            "+defines":["USE_UV_ANIM","ALPHA_TEST"],
            "+states":["Blending","DisableCulling"],
            "blendSrc":"One","blendDst":"One"
        },
        "fixture_child:fixture_reflection":{"+defines":["USE_OVERLAY"]},
        "fixture_ordinary:fixture_child":{"-defines":["USE_EMISSIVE"],
            "blendSrc":"SourceAlpha","blendDst":"OneMinusSrcAlpha"}
    }});
    let expected = json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true,
        "emissive":true,"additive":true});
    assert_eq!(state("fixture_reflection", Some(&definitions)), expected);
    assert_eq!(state("fixture_child", Some(&definitions)), expected);
    assert_eq!(
        state("fixture_ordinary", Some(&definitions)),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true})
    );
}

#[test]
fn additive_material_child_can_weight_source_alpha_without_replacing_destination() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_glow:entity_emissive":{
            "+states":["Blending","DisableCulling","DisableDepthWrite"],
            "blendSrc":"One","blendDst":"One"
        },
        "fixture_faded:fixture_glow":{"blendSrc":"SourceAlpha"},
        "fixture_inherited:fixture_faded":{"+defines":["USE_UV_ANIM"]},
        "fixture_opaque:fixture_faded":{"blendSrc":"One"},
        "fixture_standard:fixture_faded":{"blendDst":"OneMinusSrcAlpha"}
    }});
    let expected = json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":false,
        "emissive":true,"additive":true,"additive_alpha":true});
    assert_eq!(state("fixture_faded", Some(&definitions)), expected);
    assert_eq!(state("fixture_inherited", Some(&definitions)), expected);
    assert_eq!(
        state("fixture_opaque", Some(&definitions)),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":false,
            "emissive":true,"additive":true})
    );
    assert_eq!(
        state("fixture_standard", Some(&definitions)),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":false,"emissive":true})
    );
}

#[test]
fn shader_blending_define_does_not_enable_experience_orb_raster_blending() {
    assert_eq!(
        state("experience_orb", None),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":true})
    );
}

#[test]
fn actor_material_states_keep_slime_outer_blended_culled_and_depth_writing() {
    assert_eq!(
        state("slime_outer", None),
        json!({"alpha_test":false,"cull":true,"blend":true,"depth_write":true})
    );
}

#[test]
fn wind_material_uses_the_native_double_sided_blended_route_without_depth_writes() {
    let expected = json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":false});
    assert_eq!(state("breeze_wind", None), expected);
    // Native material inheritance still includes ALPHA_TEST, but Blending selects
    // the separate transparent Actor pass after the inherited declaration is read.
    let definitions = json!({"materials": {
        "version": "1.0.0",
        "fixture_wind:entity_static": {
            "+defines": ["ALPHA_TEST", "USE_UV_ANIM"],
            "+states": ["Blending", "DisableCulling", "DisableDepthWrite"]
        },
        "fixture_child:fixture_wind": {"+defines": ["ALPHA_TEST"]},
        "fixture_cutout:fixture_child": {"-states": ["Blending"]}
    }});
    assert_eq!(state("fixture_child", Some(&definitions)), expected);
    assert_eq!(
        state("fixture_cutout", Some(&definitions)),
        json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":false})
    );
}

fn per_bone_pack(rules: Value, alternate: bool) -> tempfile::TempDir {
    let fixture = pack(0, "entity_alphatest", false);
    write(
        fixture.path(),
        "entity/example.entity.json",
        &serde_json::to_vec(&json!({"format_version":"1.8.0","minecraft:client_entity":{
            "description":{
                "identifier":"minecraft:example",
                "geometry":{"default":"geometry.example","other":"geometry.other"},
                "materials":{"default":"entity_alphatest","glass":"slime_outer"},
                "textures":{"default":"textures/entity/example"},
                "render_controllers":["controller.render.example"]
            }
        }}))
        .unwrap(),
    );
    let bone = |name: &str, parent: Option<&str>| {
        let mut bone = json!({"name":name,"cubes":[{"origin":[0,0,0],"size":[0,2,7],"uv":[0,0]}]});
        if let Some(parent) = parent {
            bone["parent"] = json!(parent);
        }
        bone
    };
    write(
        fixture.path(),
        "models/entity/example.geo.json",
        &serde_json::to_vec(&json!({"format_version":"1.12.0","minecraft:geometry":[
            {"description":{"identifier":"geometry.example","texture_width":16,"texture_height":16},
             "bones":[bone("root",None),bone("magnifyingGlass",Some("root")),bone("handle",Some("magnifyingGlass"))]},
            {"description":{"identifier":"geometry.other","texture_width":16,"texture_height":16},
             "bones":[bone("root",None),bone("otherGlass",Some("root"))]}
        ]})).unwrap(),
    );
    write(
        fixture.path(),
        "render_controllers/example.json",
        &serde_json::to_vec(&json!({"format_version":"1.8.0","render_controllers":{
            "controller.render.example":{
                "geometry":if alternate {"query.is_alive ? Geometry.default : Geometry.other"} else {"Geometry.default"},
                "materials":rules,"textures":["Texture.default"],
                "part_visibility":[{"root":false}]
            }
        }})).unwrap(),
    );
    fixture
}

fn visible(
    compiled: &assets::CompiledEntityAssets,
    layer: &assets::EntityRenderLayer,
    name: &str,
) -> bool {
    let mut visible = true;
    let first = layer.first_visibility as usize;
    for rule in &compiled.render.visibility[first..first + usize::from(layer.visibility_count)] {
        if rule.pattern.as_ref() != "*" && !rule.pattern.eq_ignore_ascii_case(name) {
            continue;
        }
        let expression = compiled.molang_expressions[rule.condition as usize];
        let operations = &compiled.molang_ops[expression.first_op as usize
            ..expression.first_op as usize + usize::from(expression.op_count)];
        visible = match operations {
            [assets::MolangOp::Push(value)] => value.get() != 0.0,
            _ => panic!("fixture visibility must be constant"),
        };
    }
    visible
}

#[test]
fn actor_material_rules_partition_bones_without_revealing_hidden_parts_or_hiding_children() {
    let fixture = per_bone_pack(
        json!([
            {"*":"Material.default"},{"magnifyingGlass":"Material.glass"}
        ]),
        false,
    );
    let compiled = pack_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.render.layers.len(), 2);
    for name in ["root", "magnifyingGlass", "handle"] {
        let states: Vec<_> = compiled
            .render
            .layers
            .iter()
            .filter(|layer| visible(&compiled, layer, name))
            .map(|layer| layer.material_state.unwrap())
            .collect();
        match name {
            "root" => assert!(states.is_empty(), "authored visibility stays hidden"),
            "magnifyingGlass" => assert_eq!(
                states,
                vec![assets::EntityRenderMaterialState {
                    alpha_test: false,
                    cull: true,
                    blend: true,
                    depth_write: true,
                    ..Default::default()
                }]
            ),
            _ => assert_eq!(
                states,
                vec![assets::EntityRenderMaterialState {
                    alpha_test: true,
                    cull: false,
                    blend: false,
                    depth_write: true,
                    ..Default::default()
                }]
            ),
        }
    }
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    assets::RuntimeEntityAssets::decode(&bytes).unwrap();
}

#[test]
fn actor_material_rules_use_the_last_match_and_cover_alternate_geometry_bones() {
    let fixture = per_bone_pack(
        json!([
            {"*":"Material.default"},{"magnifyingGlass":"Material.glass"},
            {"otherGlass":"Material.glass"}
        ]),
        true,
    );
    let compiled = pack_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.render.layers.len(), 2);
    for name in ["magnifyingGlass", "otherGlass"] {
        let layers: Vec<_> = compiled
            .render
            .layers
            .iter()
            .filter(|layer| visible(&compiled, layer, name))
            .collect();
        assert_eq!(layers.len(), 1);
        assert!(layers[0].material_state.unwrap().blend);
        assert_eq!(layers[0].geometry_count, 2);
    }
    let fixture = per_bone_pack(
        json!([
            {"*":"Material.default"},{"magnifyingGlass":"Material.glass"},
            {"*":"Material.default"}
        ]),
        false,
    );
    let compiled = pack_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.render.layers.len(), 1);
    assert!(!compiled.render.layers[0].material_state.unwrap().blend);
    assert!(visible(
        &compiled,
        &compiled.render.layers[0],
        "magnifyingGlass"
    ));
}

#[test]
fn actor_material_states_select_inherited_blending_without_losing_depth_write() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_text:entity_alphatest":{"-defines":["FANCY"]},
        "fixture_plate:fixture_text":{"+states":["Blending"]}
    }});
    assert_eq!(
        state("fixture_plate", Some(&definitions)),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true})
    );
}

#[test]
fn actor_material_states_apply_explicit_depth_and_culling_overrides_independently() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_plate:entity_alphatest":{
            "+states":["Blending","DisableDepthWrite"],
            "-states":["DisableCulling"]
        },
        "fixture_opaque:fixture_plate":{
            "-states":["Blending"],"-defines":["ALPHA_TEST"]
        }
    }});
    assert_eq!(
        state("fixture_plate", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":true,"depth_write":false})
    );
    assert_eq!(
        state("fixture_opaque", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":false})
    );
}

#[test]
fn actor_material_states_replacement_excludes_add_remove_for_the_same_family() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture:entity_alphatest":{
            "states":["DisableDepthWrite"],
            "+states":["Blending"],
            "-states":["DisableDepthWrite"],
            "defines":["ALPHA_TEST"],
            "-defines":["ALPHA_TEST"]
        }
    }});
    assert_eq!(
        state("fixture", Some(&definitions)),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":false})
    );
}
