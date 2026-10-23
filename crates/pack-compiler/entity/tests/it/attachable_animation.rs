//! Modern held-item definitions keep the same bounded scripts, animation and render machinery.

use assets::{EntityAssetKind, RuntimeEntityAssets, encode_entity_blob};
use image::{ImageFormat, Rgba, RgbaImage};
use pack_entity_compiler::compile_actor_pack;
use serde_json::json;
use std::io::Cursor;

const IDENTIFIER: &str = "test:framed_attachable";

fn framed_pack() -> Vec<(Box<str>, Vec<u8>)> {
    pack_with_first_alias("default")
}

/// Builds four controller-selected raster frames with the requested standby resource alias.
fn pack_with_first_alias(first_alias: &str) -> Vec<(Box<str>, Vec<u8>)> {
    let names = [first_alias, "pull_0", "pull_1", "pull_2"];
    let geometry = names.iter().enumerate().map(|(index, name)| json!({
        "description": {"identifier": format!("geometry.test.{name}"), "texture_width": 16, "texture_height": 16},
        "bones": [{"name": "rightitem", "binding": "query.item_slot_to_bone_name(context.item_slot)",
            "texture_meshes": [{"local_pivot": [1,2,3], "position": [4,index,6], "rotation": [7,8,9], "texture": name}]}]
    })).collect::<Vec<_>>();
    let attachable = json!({"format_version":"1.10.0", "minecraft:attachable":{"description": {
        "identifier": IDENTIFIER, "materials":{"default":"entity_alphatest"},
        "textures": names.iter().map(|name| ((*name).to_owned(), json!(format!("textures/items/{name}")))).collect::<serde_json::Map<_, _>>(),
        "geometry": names.iter().map(|name| ((*name).to_owned(), json!(format!("geometry.test.{name}")))).collect::<serde_json::Map<_, _>>(),
        "animations":{"wield":"animation.test.wield", "pull":"animation.test.pull", "inactive":"animation.test.inactive"},
        "scripts":{"initialize":["variable.initial = 3;"], "pre_animation":["variable.charge = query.main_hand_item_use_duration;"],
            "animate":["wield", {"pull":"context.is_first_person && query.main_hand_item_use_duration > 0"}]},
        "render_controllers":["controller.render.test"]
    }}});
    let render = json!({"format_version":"1.10.0", "render_controllers":{"controller.render.test": {
        "arrays":{"textures":{"array.frames":names.iter().map(|name| format!("texture.{name}")).collect::<Vec<_>>()},
            "geometries":{"array.models":names.iter().map(|name| format!("geometry.{name}")).collect::<Vec<_>>()}},
        "geometry":"array.models[query.get_animation_frame]", "textures":["array.frames[query.get_animation_frame]"],
        "materials":[{"*":"material.default"}]
    }}});
    let animations = json!({"format_version":"1.10.0", "animations": {
        "animation.test.wield":{"loop":true,"bones":{"rightitem":{"position":["context.is_first_person ? -1 : 2",3,4],"rotation":[5,6,7]}}},
        "animation.test.pull":{"loop":true,"bones":{"rightitem":{"position":[0,"variable.charge",0]}}},
        "animation.test.inactive":{"loop":true,"bones":{"rightitem":{"position":[99,0,0]}}}
    }});
    let mut files = vec![
        (
            "attachables/test.json".into(),
            serde_json::to_vec(&attachable).unwrap(),
        ),
        (
            "models/entity/test.geo.json".into(),
            serde_json::to_vec(&json!({"format_version":"1.16.0", "minecraft:geometry":geometry}))
                .unwrap(),
        ),
        (
            "animations/test.json".into(),
            serde_json::to_vec(&animations).unwrap(),
        ),
        (
            "render_controllers/test.json".into(),
            serde_json::to_vec(&render).unwrap(),
        ),
    ];
    for (index, name) in names.iter().enumerate() {
        let mut png = Vec::new();
        RgbaImage::from_pixel(16, 16, Rgba([index as u8, 30, 40, 255]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        files.push((format!("textures/items/{name}.png").into(), png));
    }
    files
}

#[test]
fn named_alias_attachables_keep_their_equipment_and_controller_frames() {
    let mut files = pack_with_first_alias("standby");
    let source = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "attachables/test.json")
        .unwrap();
    let mut attachable: serde_json::Value = serde_json::from_slice(&source.1).unwrap();
    let description = &mut attachable["minecraft:attachable"]["description"];
    // These precede the consumed aliases in map order but never appear in the controller.
    description["geometry"]["aaa_unused"] = json!("geometry.test.pull_2");
    description["textures"]["aaa_unused"] = json!("textures/items/pull_2");
    description["textures"]["enchanted"] = json!("textures/items/pull_1");
    source.1 = serde_json::to_vec(&attachable).unwrap();

    let compiled = compile_actor_pack(files).unwrap().unwrap();
    let [binding] = compiled.equipment_bindings.as_slice() else {
        panic!("the named attachable must retain its equipment binding");
    };
    assert_eq!(binding.identifier.as_ref(), IDENTIFIER);
    assert_eq!(
        binding.geometry.identifier.as_ref(),
        "geometry.test.standby"
    );
    assert_eq!(
        binding.texture.identifier.as_ref(),
        "textures/items/standby"
    );
    assert_eq!(
        binding.geometry.resolution,
        assets::EntityDependencyResolution::Catalog
    );
    assert_eq!(compiled.equipment_textures.len(), 4);

    let runtime =
        RuntimeEntityAssets::decode(&encode_entity_blob(&compiled.entities).unwrap()).unwrap();
    let rig = runtime.attachable_rig_binding(IDENTIFIER).unwrap();
    let [layer] = runtime.render_layers(rig) else {
        panic!("the named attachable must retain its render controller");
    };
    let geometry_choices = &compiled.entities.render.geometries[layer.first_geometry as usize..]
        [..usize::from(layer.geometry_count)];
    let texture_slot = &compiled.entities.render.slots[layer.first_slot as usize];
    let texture_choices = &compiled.entities.render.candidates
        [texture_slot.first_candidate as usize..][..usize::from(texture_slot.candidate_count)];
    assert_eq!(geometry_choices.len(), 4);
    assert_eq!(texture_choices.len(), 4);
    for ((geometry, texture), alias) in geometry_choices
        .iter()
        .zip(texture_choices)
        .zip(["standby", "pull_0", "pull_1", "pull_2"])
    {
        assert_eq!(
            runtime.geometries()[geometry.geometry as usize]
                .identifier
                .as_ref(),
            format!("geometry.test.{alias}")
        );
        assert_eq!(
            runtime.sources()[texture.source as usize].path.as_ref(),
            format!("textures/items/{alias}.png")
        );
        assert!(geometry.condition.is_some() && texture.condition.is_some());
    }
}

#[test]
fn explicit_attachable_defaults_keep_their_catalog_resources() {
    let mut files = framed_pack();
    let source = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "render_controllers/test.json")
        .unwrap();
    let mut controller: serde_json::Value = serde_json::from_slice(&source.1).unwrap();
    let arrays = &mut controller["render_controllers"]["controller.render.test"]["arrays"];
    arrays["geometries"]["array.models"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    arrays["textures"]["array.frames"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    source.1 = serde_json::to_vec(&controller).unwrap();
    let compiled = compile_actor_pack(files).unwrap().unwrap();
    let binding = &compiled.equipment_bindings[0];
    assert_eq!(
        binding.geometry.identifier.as_ref(),
        "geometry.test.default"
    );
    assert_eq!(
        binding.texture.identifier.as_ref(),
        "textures/items/default"
    );
}

#[test]
fn named_alias_attachables_without_a_controller_keep_no_guessed_binding() {
    let files = pack_with_first_alias("standby")
        .into_iter()
        .filter(|(path, _)| !path.starts_with("render_controllers/"))
        .collect();
    let compiled = compile_actor_pack(files).unwrap().unwrap();
    assert!(compiled.equipment_bindings.is_empty());
}

#[test]
fn server_pack_use_profiles_do_not_inherit_compatibility_admissions() {
    let compiled = compile_actor_pack(framed_pack()).unwrap().unwrap();
    let runtime = RuntimeEntityAssets::from_compiled(compiled.entities).unwrap();
    assert_eq!(
        runtime.item_use_profile("minecraft:bow"),
        assets::item_use::ItemUseProfile::Unresolved,
        "an attachable pack cannot manufacture item-use facts for absent items"
    );
}

#[test]
fn attachable_scripts_roots_and_all_render_frames_survive_the_carriers() {
    let compiled = compile_actor_pack(framed_pack()).unwrap().unwrap();
    assert_eq!(compiled.skipped.unparsable, 0);
    assert_eq!(compiled.entities.rig_bindings.len(), 1);
    let rig = &compiled.entities.rig_bindings[0];
    assert_eq!(
        compiled.entities.symbols[rig.entity_symbol as usize].kind,
        EntityAssetKind::Attachable
    );
    assert!(rig.initialize.is_some() && rig.pre_animation.is_some());
    assert_eq!(
        rig.geometry_count, 5,
        "default plus four selectable frame candidates"
    );
    for geometry in &compiled.entities.rig_geometries {
        assert_eq!(
            geometry.animation_count, 2,
            "the inactive alias must not play"
        );
        let roots = &compiled.entities.rig_animations[geometry.first_animation as usize..][..2];
        assert!(roots[0].weight.is_none());
        assert!(roots[1].weight.is_some());
        assert_eq!([roots[0].order, roots[1].order], [0, 1]);
    }
    assert_eq!(compiled.entities.render.layers.len(), 1);
    assert_eq!(compiled.entities.render.geometries.len(), 4);
    assert_eq!(compiled.entities.render.candidates.len(), 4);
    assert!(
        compiled
            .entities
            .molang_symbols
            .iter()
            .any(|symbol| symbol.identifier.as_ref() == "query.get_animation_frame")
    );
    let bytes = encode_entity_blob(&compiled.entities).unwrap();
    let runtime = RuntimeEntityAssets::decode(&bytes).unwrap();
    assert_eq!(runtime.attachable_rig_binding(IDENTIFIER), Some(0));
    assert_eq!(runtime.attachable_rig_binding("test:missing"), None);
    assert_eq!(runtime.render_layers(0).len(), 1);
    assert_eq!(runtime.geometries(), compiled.entities.geometries.as_ref());
    assert_eq!(
        compiled.equipment_textures.len(),
        4,
        "not only the default texture"
    );
    for (index, name) in ["default", "pull_0", "pull_1", "pull_2"]
        .into_iter()
        .enumerate()
    {
        let texture = compiled
            .equipment_textures
            .iter()
            .find(|texture| texture.identifier.as_ref() == format!("textures/items/{name}"))
            .unwrap();
        assert_eq!(&texture.rgba8[..4], &[index as u8, 30, 40, 255]);
    }
}

#[test]
fn authored_texture_mesh_fields_and_binding_are_not_discarded() {
    let compiled = compile_actor_pack(framed_pack()).unwrap().unwrap();
    for geometry in &compiled.entities.geometries {
        let bone = &geometry.bones[0];
        assert_eq!(
            bone.binding.as_deref(),
            Some("query.item_slot_to_bone_name(context.item_slot)")
        );
        assert!(bone.cubes.is_empty());
        let mesh = &bone.texture_meshes[0];
        assert_eq!(mesh.local_pivot.map(|scalar| scalar.get()), [1.0, 2.0, 3.0]);
        assert_eq!(mesh.rotation.map(|scalar| scalar.get()), [7.0, 8.0, 9.0]);
        assert_eq!(mesh.position[0].get(), 4.0);
        assert_eq!(mesh.position[2].get(), 6.0);
        assert_eq!(
            geometry.identifier.as_ref(),
            format!("geometry.test.{}", mesh.texture)
        );
    }
}

#[test]
fn old_geometry_payloads_keep_omitted_empty_fields_canonical() {
    let compiled = compile_actor_pack(framed_pack()).unwrap().unwrap();
    let mut bone = compiled.entities.geometries[0].bones[0].clone();
    bone.binding = None;
    bone.texture_meshes = Box::new([]);
    let old = serde_json::to_value(&bone).unwrap();
    assert!(old.get("binding").is_none() && old.get("texture_meshes").is_none());
    assert_eq!(
        serde_json::from_value::<assets::EntityGeometryBone>(old).unwrap(),
        bone
    );
    let mesh = &compiled.entities.geometries[0].bones[0].texture_meshes[0];
    assert_eq!(mesh.scale, assets::EntityGeometryTextureMesh::DEFAULT_SCALE);
    assert_eq!(
        mesh.use_pixel_depth,
        assets::EntityGeometryTextureMesh::DEFAULT_USE_PIXEL_DEPTH
    );
    let old_mesh = serde_json::to_value(mesh).unwrap();
    assert!(old_mesh.get("scale").is_none() && old_mesh.get("use_pixel_depth").is_none());
    assert_eq!(
        serde_json::from_value::<assets::EntityGeometryTextureMesh>(old_mesh).unwrap(),
        *mesh
    );
}

#[test]
fn texture_mesh_nonunit_scale_pixel_depth_and_absent_position_round_trip() {
    let mut files = framed_pack();
    let source = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "models/entity/test.geo.json")
        .unwrap();
    let mut geometry: serde_json::Value = serde_json::from_slice(&source.1).unwrap();
    let mesh = &mut geometry["minecraft:geometry"][0]["bones"][0]["texture_meshes"][0];
    mesh["scale"] = json!([2, 3, 4]);
    mesh["use_pixel_depth"] = json!(false);
    mesh.as_object_mut().unwrap().remove("position");
    source.1 = serde_json::to_vec(&geometry).unwrap();
    let compiled = compile_actor_pack(files).unwrap().unwrap();
    assert_eq!(compiled.skipped.unparsable, 0);
    let runtime =
        RuntimeEntityAssets::decode(&encode_entity_blob(&compiled.entities).unwrap()).unwrap();
    let mesh = &runtime.geometries()[0].bones[0].texture_meshes[0];
    assert_eq!(mesh.scale.map(|scalar| scalar.get()), [2.0, 3.0, 4.0]);
    assert!(!mesh.use_pixel_depth);
    assert_eq!(mesh.position, [assets::EntityGeometryScalar::ZERO; 3]);
    let json = serde_json::to_value(mesh).unwrap();
    assert!(json.get("scale").is_some());
    assert_eq!(json["use_pixel_depth"], false);
}

#[test]
fn unsafe_or_excessive_texture_mesh_records_fail_closed() {
    let mut compiled = compile_actor_pack(framed_pack()).unwrap().unwrap().entities;
    let original = compiled.geometries[0].bones[0].texture_meshes[0].clone();
    compiled.geometries[0].bones[0].texture_meshes =
        vec![original.clone(); assets::MAX_ENTITY_GEOMETRY_TEXTURE_MESHES + 1].into();
    assert!(encode_entity_blob(&compiled).is_err());
    compiled.geometries[0].bones[0].texture_meshes = vec![original].into();
    compiled.geometries[0].bones[0].texture_meshes[0].texture = "".into();
    assert!(encode_entity_blob(&compiled).is_err());
    compiled.geometries[0].bones[0].texture_meshes[0].texture = "default".into();
    compiled.geometries[0].bones[0].texture_meshes[0].scale[0] =
        serde_json::from_value(json!(u32::MAX)).unwrap();
    assert!(
        encode_entity_blob(&compiled).is_err(),
        "non-finite scale must not enter a carrier"
    );
}

#[test]
fn equipment_frame_texture_ingestion_rejects_invalid_cross_indices() {
    let compiled = compile_actor_pack(framed_pack()).unwrap().unwrap();
    let mut entities = compiled.entities;
    entities.render.layers[0].rig = u32::MAX;
    assert!(
        pack_entity_compiler::compile_equipment_textures_for_assets_with(
            &entities,
            &compiled.equipment_bindings,
            &mut |_| panic!("indices must be checked before any source read"),
        )
        .is_err()
    );
}

#[test]
fn attachable_controller_blend_settings_survive_the_compiled_controller_pipeline() {
    let mut files = framed_pack();
    let source = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "attachables/test.json")
        .unwrap();
    let mut attachable: serde_json::Value = serde_json::from_slice(&source.1).unwrap();
    let description = &mut attachable["minecraft:attachable"]["description"];
    description["animations"]["controller"] = json!("controller.animation.test");
    description["scripts"]["animate"] = json!(["controller"]);
    source.1 = serde_json::to_vec(&attachable).unwrap();
    files.push(("animation_controllers/test.json".into(), serde_json::to_vec(&json!({"format_version":"1.10.0", "animation_controllers":{
        "controller.animation.test":{"initial_state":"default","states":{"default":{"animations":["wield",{"pull":"context.is_first_person"}],"blend_transition":0.2,"blend_via_shortest_path":true}}}
    }})).unwrap()));
    let compiled = compile_actor_pack(files).unwrap().unwrap();
    assert_eq!(compiled.entities.controllers.len(), 4);
    for state in &compiled.entities.controller_states {
        assert_eq!(state.blend_transition.get(), 0.2);
        assert!(state.blend_via_shortest_path);
    }
    let runtime =
        RuntimeEntityAssets::decode(&encode_entity_blob(&compiled.entities).unwrap()).unwrap();
    assert_eq!(
        runtime.controller_states(),
        compiled.entities.controller_states.as_ref()
    );
    for geometry in &compiled.entities.rig_geometries {
        assert_eq!(geometry.controller_count, 1);
        assert_eq!(geometry.animation_count, 0);
    }
}

#[test]
fn existing_attachable_fixture_keeps_compiled_bytes() {
    use sha2::{Digest, Sha256};
    let compiled = compile_actor_pack(framed_pack()).unwrap().unwrap();
    let entity = assets::encode_entity_blob(&compiled.entities).unwrap();
    let equipment = assets::encode_equipment_catalog_with_textures(
        compiled.identity,
        Sha256::digest(&entity).into(),
        &compiled.equipment_bindings,
        &compiled.equipment_textures,
    )
    .unwrap();
    let mut digest = Sha256::new();
    digest.update(entity);
    digest.update(equipment);
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "4c3520a5cea37284d6096d7f3d8a381bc8ff2d5688ff27de4dd268901c2d5276"
    );
}
