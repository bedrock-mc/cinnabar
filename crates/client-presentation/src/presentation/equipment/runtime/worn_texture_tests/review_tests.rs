//! Owner selection and partially placeable worn models retain their admitted draw.
use super::*;

#[test]
fn worn_selection_keeps_startup_artwork_when_controller_textures_are_unresolved() {
    let (mut runtime, pages) = startup_runtime(coat_pack("v.coat=0;"));
    let body = player_body(&mut runtime);
    let mut files = coat_pack("v.coat=1;");
    files.retain(|(path, _)| !path.starts_with("textures/"));
    let (entities, catalog) = pack_catalog(files);
    assert!(!entities.geometries().is_empty());
    assert!(catalog.textures().is_empty());
    runtime.set_pack_layer(Some((entities, Some(catalog), Default::default())));
    let drawn = layers(&mut runtime, &body, &owner(), &helmet(), 1);
    assert_eq!(
        drawn.len(),
        1,
        "unresolved layers retain the startup artwork"
    );
    assert_eq!(selected_pixel(&pages, &drawn[0]), PLAIN);
}

#[test]
fn worn_selection_respects_a_valid_disabled_controller_and_hidden_bones() {
    for (condition, active, missing) in [
        ("0", false, false),
        ("0", false, true),
        ("q.owner_identifier ==", false, true),
        ("1", true, false),
    ] {
        let (mut runtime, base_pages) = startup_runtime(coat_pack("v.coat=0;"));
        let body = player_body(&mut runtime);
        let mut files = coat_pack("v.coat=1;");
        let mut definition: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
        definition["minecraft:attachable"]["description"]["render_controllers"] =
            serde_json::json!([{"controller.render.test_coat": condition}]);
        files[0].1 = serde_json::to_vec(&definition).unwrap();
        let mut controller: serde_json::Value = serde_json::from_slice(&files[2].1).unwrap();
        controller["render_controllers"]["controller.render.test_coat"]["part_visibility"] =
            serde_json::json!([{"*":"0"}]);
        files[2].1 = serde_json::to_vec(&controller).unwrap();
        if missing {
            files.retain(|(path, _)| !path.starts_with("textures/"));
        }
        let (entities, catalog) = pack_catalog(files);
        let (pages, locations) =
            base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
        let drawn = layers(&mut runtime, &body, &owner(), &helmet(), 1);
        if active {
            assert_eq!(drawn.len(), 1);
            assert_eq!(selected_pixel(&pages, &drawn[0]), PAINTED);
            assert!(
                drawn[0]
                    .submission
                    .input
                    .current_bones
                    .iter()
                    .all(|bone| bone.translation_scale[3] == 0.0)
            );
        } else {
            assert!(
                drawn.is_empty(),
                "an inactive controller must not reveal startup armour"
            );
        }
    }
}

/// Gives a mapped helmet independent artwork and an owner predicate.
fn mapped_pack(predicate: &str) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = coat_pack("v.coat=1;");
    let mut definition: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let description = &mut definition["minecraft:attachable"]["description"];
    description["identifier"] = "fixture:mapped_helmet".into();
    description["item"] = serde_json::json!({"minecraft:iron_helmet": predicate});
    description["textures"]["default"] = "textures/models/mapped_helmet".into();
    description["textures"]["painted"] = "textures/models/mapped_helmet".into();
    files[0].1 = serde_json::to_vec(&definition).unwrap();
    files[4].0 = "textures/models/mapped_helmet.png".into();
    files
}

#[test]
fn missing_mapped_artwork_resolves_startup_default_with_pack_precedence() {
    const OVERRIDE: [u8; 4] = [60, 90, 210, 255];
    for (custom_available, override_available, override_uploaded, expected) in [
        (false, true, true, OVERRIDE),
        (false, false, false, PLAIN),
        (false, true, false, PLAIN),
        (true, true, true, PAINTED),
    ] {
        let startup = coat_pack("v.coat=0;");
        let (_, startup_catalog) = pack_catalog(startup.clone());
        let fallback_texture = startup_catalog
            .binding("minecraft:iron_helmet")
            .unwrap()
            .texture
            .identifier
            .clone();
        let (mut runtime, base_pages) = startup_runtime(startup);
        let body = player_body(&mut runtime);
        let mut files = mapped_pack("1");
        let override_path = files[3].0.clone();
        let custom_path = files[4].0.clone();
        files[3].1 = texture(OVERRIDE);
        files.retain(|(path, _)| {
            (custom_available || path != &custom_path)
                && (override_available || path != &override_path)
        });
        let (entities, catalog) = pack_catalog(files);
        let (pages, mut locations) =
            base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        if !override_uploaded {
            for (texture, location) in catalog.textures().iter().zip(&mut locations) {
                if texture.identifier == fallback_texture {
                    *location = None;
                }
            }
        }
        runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
        let owner = owner();
        for tick in [1, 2] {
            let drawn = layers(&mut runtime, &body, &owner, &helmet(), tick);
            assert_eq!(drawn.len(), 1);
            assert_eq!(selected_pixel(&pages, &drawn[0]), expected);
        }
    }
}

#[test]
fn missing_artwork_has_no_recursive_fallback_or_unadmitted_startup_default() {
    for same_missing_path in [false, true] {
        let mut startup = coat_pack("v.coat=0;");
        let mut definition: serde_json::Value = serde_json::from_slice(&startup[0].1).unwrap();
        let description = &mut definition["minecraft:attachable"]["description"];
        if same_missing_path {
            for value in description["textures"]
                .as_object_mut()
                .unwrap()
                .values_mut()
            {
                *value = "textures/models/mapped_helmet".into();
            }
        } else {
            description["identifier"] = "fixture:unrelated_helmet".into();
        }
        startup[0].1 = serde_json::to_vec(&definition).unwrap();
        let (mut runtime, _) = startup_runtime(startup);
        let body = player_body(&mut runtime);
        let mut files = mapped_pack("1");
        files.retain(|(path, _)| !path.starts_with("textures/"));
        let (entities, catalog) = pack_catalog(files);
        runtime.set_pack_layer(Some((entities, Some(catalog), Vec::new())));
        assert!(layers(&mut runtime, &body, &owner(), &helmet(), 1).is_empty());
    }
}

#[test]
fn mapped_armour_reuses_startup_geometry_without_losing_its_owner_condition() {
    for predicate in [
        "q.owner_identifier == 'minecraft:player'",
        "q.owner_identifier == 'minecraft:armor_stand'",
        "q.owner_identifier ==",
    ] {
        let (mut runtime, base_pages) = startup_runtime(coat_pack("v.coat=0;"));
        let body = player_body(&mut runtime);
        let mut files = mapped_pack(predicate);
        files.retain(|(path, _)| {
            !path.starts_with("models/") && !path.starts_with("render_controllers/")
        });
        let (entities, catalog) = pack_catalog(files);
        let (pages, locations) =
            base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
        let drawn = layers(&mut runtime, &body, &owner(), &helmet(), 1);
        assert_eq!(drawn.len(), 1);
        let expected = if predicate.contains("minecraft:player") {
            PAINTED
        } else {
            PLAIN
        };
        assert_eq!(selected_pixel(&pages, &drawn[0]), expected, "{predicate}");
    }
}

#[test]
fn owner_selection_observes_the_rendered_item_use_inputs() {
    let predicate = "q.is_using_item && q.main_hand_item_use_duration == 12 && q.main_hand_item_max_duration == 32 && q.get_animation_frame == 3";
    let (mut runtime, base_pages) = startup_runtime(coat_pack("v.coat=0;"));
    let body = player_body(&mut runtime);
    let (entities, catalog) = pack_catalog(mapped_pack(predicate));
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
    let mut owner = owner();
    owner.metadata.insert(0, ActorMetadataValue::Flags(1 << 4));
    let mut input = helmet();
    let mut main = input.armor[0].clone().unwrap();
    main.use_facts = client_world::EffectiveItemUse {
        ticks: Some(32),
        animation: assets::item_use::ItemUseAnimation::Bow,
        ..Default::default()
    };
    input.main = Some(main);
    let names = [Box::<str>::from("body")];
    let mut rig = owner_rig(&owner, &names, 1);
    for (ticks, expected) in [(0, PLAIN), (20, PAINTED), (0, PLAIN)] {
        rig.hand[1].use_ticks = ticks;
        let drawn = runtime
            .layers_for(
                &body,
                &input,
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 1.0,
                    delta_seconds: 0.05,
                }),
            )
            .to_vec();
        assert_eq!(
            selected_pixel(&pages, &drawn[0]),
            expected,
            "use ticks {ticks}"
        );
    }
}

#[test]
fn an_unplaceable_armour_bone_keeps_controller_texture_and_other_bones() {
    let mut files = coat_pack("v.coat=1;");
    let mut geometry: serde_json::Value = serde_json::from_slice(&files[1].1).unwrap();
    geometry["minecraft:geometry"][0]["bones"].as_array_mut().unwrap().extend([
        serde_json::json!({"name":"unplaceable", "binding":"q.item_slot_to_bone_name(c.item_slot)", "pivot":[0,24,0], "cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}),
        serde_json::json!({"name":"hidden_shell", "pivot":[0,24,0], "cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}),
    ]);
    files[1].1 = serde_json::to_vec(&geometry).unwrap();
    let mut controller: serde_json::Value = serde_json::from_slice(&files[2].1).unwrap();
    controller["render_controllers"]["controller.render.test_coat"]["part_visibility"] =
        serde_json::json!([{"hidden_shell":"0"}]);
    files[2].1 = serde_json::to_vec(&controller).unwrap();
    let (mut runtime, pages) = pack_runtime(files);
    let body = player_body(&mut runtime);
    let drawn = layers(&mut runtime, &body, &owner(), &helmet(), 1);
    assert_eq!(drawn.len(), 1);
    assert_eq!(selected_pixel(&pages, &drawn[0]), PAINTED);
    let poses = &drawn[0].submission.input.current_bones;
    assert_eq!(poses.len(), 3);
    assert!(poses[0].translation_scale[3] > 0.0);
    assert_eq!(poses[1].translation_scale[3], 0.0);
    assert_eq!(poses[2].translation_scale[3], 0.0);
}

#[test]
fn partial_artwork_preserves_available_missing_disabled_and_hidden_branches() {
    for (branch, active, hidden) in [
        (0, true, false),
        (1, true, false),
        (1, false, false),
        (0, true, true),
        (1, true, true),
    ] {
        let (mut runtime, base_pages) = startup_runtime(coat_pack("v.coat=0;"));
        let body = player_body(&mut runtime);
        let mut files = coat_pack(&format!("v.coat={branch};"));
        files.retain(|(path, _)| path.as_ref() != "textures/models/coat_painted.png");
        files[3].0 = "textures/models/coat_available.png".into();
        files[3].1 = texture(PAINTED);
        let mut definition: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
        definition["minecraft:attachable"]["description"]["textures"]["plain"] =
            "textures/models/coat_available".into();
        definition["minecraft:attachable"]["description"]["render_controllers"] =
            serde_json::json!([{"controller.render.test_coat": if active {"1"} else {"0"}}]);
        files[0].1 = serde_json::to_vec(&definition).unwrap();
        if hidden {
            let mut controller: serde_json::Value = serde_json::from_slice(&files[2].1).unwrap();
            controller["render_controllers"]["controller.render.test_coat"]["part_visibility"] =
                serde_json::json!([{"*":"0"}]);
            files[2].1 = serde_json::to_vec(&controller).unwrap();
        }
        let (entities, catalog) = pack_catalog(files);
        let (pages, locations) =
            base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
        let drawn = layers(&mut runtime, &body, &owner(), &helmet(), 1);
        if !active || (hidden && branch == 1) {
            assert!(
                drawn.is_empty(),
                "inactive or fully hidden missing artwork must stay hidden"
            );
        } else {
            assert_eq!(drawn.len(), 1, "active branch {branch} must retain artwork");
            assert_eq!(
                selected_pixel(&pages, &drawn[0]),
                if branch == 0 { PAINTED } else { PLAIN }
            );
            assert_eq!(
                drawn[0]
                    .submission
                    .input
                    .current_bones
                    .iter()
                    .all(|bone| bone.translation_scale[3] == 0.0),
                hidden
            );
        }
    }
}

#[test]
fn owner_context_worn_enchantments_select_geometry_and_preserve_intrinsic_glint() {
    let mut files = coat_pack("v.coat=0;");
    let mut model: serde_json::Value = serde_json::from_slice(&files[1].1).unwrap();
    let mut alternate = model["minecraft:geometry"][0].clone();
    alternate["description"]["identifier"] = "geometry.fixture.enchanted".into();
    alternate["bones"][0]["cubes"][0]["size"] = serde_json::json!([12, 8, 8]);
    model["minecraft:geometry"]
        .as_array_mut()
        .unwrap()
        .push(alternate);
    files[1].1 = serde_json::to_vec(&model).unwrap();
    let mut mapped: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let description = &mut mapped["minecraft:attachable"]["description"];
    description["identifier"] = "fixture:enchanted_helmet".into();
    description["item"] = serde_json::json!({"minecraft:iron_helmet":"v.is_enchanted"});
    description["geometry"]["default"] = "geometry.fixture.enchanted".into();
    files.push((
        "attachables/enchanted.json".into(),
        serde_json::to_vec(&mapped).unwrap(),
    ));
    let (entities, _) = pack_catalog(files.clone());
    let (mut runtime, _) = startup_runtime(files);
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("head")];
    let rig = owner_rig(&owner, &names, 1);
    for (enchanted, intrinsic) in [
        (false, false),
        (true, false),
        (false, true),
        (true, true),
        (false, false),
    ] {
        let stack = crate::presentation::equipment::input::canonical_fixture(
            "minecraft:iron_helmet",
            enchanted,
            intrinsic,
        );
        let worn =
            crate::presentation::equipment::input::worn_item(&stack, None, Default::default())
                .unwrap();
        let input = ActorEquipmentInput {
            armor: [Some(worn), None, None, None],
            ..Default::default()
        };
        let drawn = runtime
            .layers_for_profile(
                &body,
                &input,
                render_api::CombatVisuals::Legacy112,
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 1.0,
                    delta_seconds: 0.05,
                }),
            )
            .layers
            .to_vec();
        assert_eq!(drawn.len(), 1);
        let geometry = render_model::find_geometry_index(
            &entities,
            if enchanted {
                "geometry.fixture.enchanted"
            } else {
                "geometry.test.coat_helmet"
            },
        )
        .unwrap();
        assert_eq!(
            drawn[0].submission.input.rig,
            render_model::equipment_rig_id(geometry),
            "enchanted={enchanted} intrinsic={intrinsic}"
        );
        assert_eq!(
            drawn[0].submission.material.kind,
            if enchanted || intrinsic {
                assets::EntityRenderMaterial::Glint
            } else {
                assets::EntityRenderMaterial::Default
            }
        );
    }
}
