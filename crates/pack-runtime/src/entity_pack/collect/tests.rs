use std::io::Write;

use serde_json::json;

#[test]
fn shared_equipment_overrides_are_collected_without_json_references() {
    let base = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(base.path().join("textures/misc")).unwrap();
    for identifier in assets::entity::shared_equipment_textures() {
        std::fs::create_dir_all(base.path().join(identifier).parent().unwrap()).unwrap();
        let png = format!("{identifier}.png");
        let tga = format!("{identifier}.tga");
        std::fs::write(base.path().join(&png), b"startup glint").unwrap();
        let view = resource_pack::LayeredPackView::tracked(crate::test_support::stack(&[(
            tga.as_str(),
            b"server glint",
        )]));
        let files = super::collect_files(&view, None, Some(base.path()));
        assert!(
            files
                .iter()
                .any(|(path, bytes)| path.as_ref() == tga && bytes == b"server glint")
        );
        assert!(!files.iter().any(|(path, _)| path.as_ref() == png));
        let removed = resource_pack::LayeredPackView::new(crate::test_support::stack(&[]));
        let files = super::collect_files(&removed, None, Some(base.path()));
        assert!(
            files
                .iter()
                .any(|(path, bytes)| path.as_ref() == png && bytes == b"startup glint")
        );
    }
}

#[test]
fn texture_only_packs_do_not_inherit_loose_attachable_bindings() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("attachables")).unwrap();
    std::fs::write(
        root.path().join("attachables/fixture.json"),
        json!({
            "format_version": "1.10.0", "minecraft:attachable": {"description": {"identifier": "fixture:armor",
                "textures": {"default": "textures/models/armor/fixture"}}}
        })
        .to_string(),
    )
    .unwrap();
    let view = resource_pack::LayeredPackView::new(crate::test_support::stack(&[(
        "textures/models/armor/fixture.png",
        b"pack pixels",
    )]));
    assert!(super::collect_files(&view, None, Some(root.path())).is_empty());
}

#[test]
fn authored_attachables_are_collected_without_loose_base_bindings() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("attachables")).unwrap();
    let native = json!({"minecraft:attachable":{"description":{
        "identifier":"fixture:armor.player", "item":{"fixture:armor":"1"},
        "textures":{"default":"textures/models/armor/fixture"}
    }}});
    std::fs::write(
        root.path().join("attachables/native.player.json"),
        native.to_string(),
    )
    .unwrap();
    // The authored definition binds the same item under a different definition id and path.
    let authored = json!({"minecraft:attachable":{"description":{
        "identifier":"fixture:custom", "item":{"fixture:armor":"1"},
        "textures":{"default":"textures/models/armor/custom"}
    }}})
    .to_string();
    let view = resource_pack::LayeredPackView::tracked(crate::test_support::stack(&[
        ("attachables/custom.json", authored.as_bytes()),
        ("textures/models/armor/fixture.tga", b"pack pixels"),
    ]));
    let files = super::collect_files(&view, None, Some(root.path()));
    let attachables = files
        .iter()
        .filter(|(path, _)| path.starts_with("attachables/"))
        .collect::<Vec<_>>();
    assert_eq!(attachables.len(), 1);
    assert_eq!(attachables[0].0.as_ref(), "attachables/custom.json");
}

#[test]
fn changing_mob_geometry_keeps_both_owner_armour_variants() {
    let item = "fixture:chestplate";
    let texture = "textures/items/fixture_armor";
    let player_geometry = "geometry.fixture.player.chestplate";
    let mob_geometry = "geometry.fixture.mob.chestplate";
    let geometry = |identifier: &str, size: [u8; 3]| {
        json!({
        "format_version": "1.12.0", "minecraft:geometry": [{
            "description": {"identifier": identifier, "texture_width": 4, "texture_height": 2},
            "bones": [{"name": "body", "cubes": [{"origin": [0, 0, 0], "size": size, "uv": [0, 0]}]}]
        }]
    }).to_string().into_bytes()
    };
    let attachable = |identifier: &str, geometry: &str, texture: &str| {
        json!({
            "format_version": "1.10.0", "minecraft:attachable": {"description": {
                "identifier": identifier, "geometry": {"default": geometry},
                "textures": {"default": texture}, "materials": {"default": "armor"},
                "render_controllers": ["controller.render.fixture"]
            }}
        })
    };
    let png = |color: [u8; 4]| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4, 2, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    let original = png([10, 20, 30, 255]);
    let pixels = png([40, 50, 60, 255]);
    // The dependencies and texture can change in the same variant or different variants.
    for shared_texture in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let mob_texture = if shared_texture {
            texture
        } else {
            "textures/items/fixture_mob"
        };
        let mob = attachable(item, mob_geometry, mob_texture);
        let mut player = attachable(&format!("{item}.player"), player_geometry, texture);
        player["minecraft:attachable"]["description"]["item"] =
            json!({(item): "query.owner_identifier == 'minecraft:player'"});
        let unrelated = attachable("fixture:unrelated", player_geometry, texture);
        let mut files: Vec<(Box<str>, Vec<u8>)> = vec![
            ("attachables/fixture.json".into(), mob.to_string().into_bytes()),
            ("attachables/fixture.player.json".into(), player.to_string().into_bytes()),
            ("attachables/unrelated.json".into(), unrelated.to_string().into_bytes()),
            ("models/entity/mob.json".into(), geometry(mob_geometry, [2, 2, 2])),
            ("models/entity/player.json".into(), geometry(player_geometry, [1, 1, 1])),
            (format!("{texture}.png").into(), original.clone()),
            ("render_controllers/fixture.json".into(), json!({
                "format_version": "1.8.0", "render_controllers": {"controller.render.fixture": {
                    "geometry": "Geometry.default", "materials": [{"*": "Material.default"}], "textures": ["Texture.default"]
                }}
            }).to_string().into_bytes()),
        ];
        if !shared_texture {
            files.push((format!("{mob_texture}.png").into(), original.clone()));
        }
        for (path, bytes) in &files {
            let path = root.path().join(path.as_ref());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let vanilla = pack_entity_compiler::compile_vanilla_entity_refs(root.path()).unwrap();
        let startup = pack_entity_compiler::compile_actor_pack(files)
            .unwrap()
            .unwrap();
        assert_eq!(
            startup
                .equipment_bindings
                .iter()
                .filter(|binding| { binding.identifier.as_ref() == item })
                .count(),
            2
        );
        assert_eq!(
            startup
                .equipment_bindings
                .iter()
                .find(|binding| binding.attachable_identifier() == format!("{item}.player"))
                .unwrap()
                .geometry
                .identifier
                .as_ref(),
            player_geometry
        );
        let replacement = geometry(mob_geometry, [3, 4, 5]);
        let view = resource_pack::LayeredPackView::new(crate::test_support::stack(&[
            (&format!("{texture}.png"), &pixels),
            ("models/entity/replacement.json", &replacement),
        ]));
        let compiled = pack_entity_compiler::compile_actor_pack(super::collect_files(
            &view,
            Some(&vanilla),
            Some(root.path()),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(
            compiled
                .equipment_bindings
                .iter()
                .filter(|binding| { binding.identifier.as_ref() == item })
                .count(),
            2
        );
        let binding = compiled
            .equipment_bindings
            .iter()
            .find(|binding| binding.attachable_identifier() == format!("{item}.player"))
            .unwrap();
        assert_eq!(binding.geometry.identifier.as_ref(), player_geometry);
        assert!(
            compiled
                .equipment_bindings
                .iter()
                .all(|binding| binding.identifier.as_ref() != "fixture:unrelated")
        );
        let selected = compiled
            .entities
            .geometries
            .iter()
            .find(|geometry| geometry.identifier.as_ref() == player_geometry)
            .unwrap();
        assert_eq!(
            selected.bones[0].cubes[0].size.map(|value| value.get()),
            [1.0; 3]
        );
        let mob = compiled
            .entities
            .geometries
            .iter()
            .find(|geometry| geometry.identifier.as_ref() == mob_geometry)
            .unwrap();
        assert_eq!(
            mob.bones[0].cubes[0].size.map(|value| value.get()),
            [3.0, 4.0, 5.0]
        );
        let artwork = compiled
            .equipment_textures
            .iter()
            .find(|image| image.identifier.as_ref() == texture)
            .unwrap();
        assert_eq!(&artwork.rgba8[..4], &[40, 50, 60, 255]);
    }
}

fn archive(id: u128, material: serde_json::Value) -> protocol::ResourcePackArchive {
    let id = format!("00000000-0000-0000-0000-{id:012x}");
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
        "modules":[{"type":"resources"}]});
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, value) in [
        ("manifest.json", manifest),
        ("materials/entity.material", material),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&value).unwrap()).unwrap();
    }
    protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        zip.finish().unwrap().into_inner(),
    )
}

#[test]
fn actor_material_collector_retains_custom_inheritance_and_highest_layer_child() {
    let lower = archive(
        1,
        json!({"materials":{
            "version":"1.0.0",
            "fixture:entity_alphatest":{"+states":["Blending"]},
            "unchanged:entity_alphatest":{"-defines":["FANCY"]}
        }}),
    );
    let upper = archive(
        2,
        json!({"materials":{
            "version":"1.0.0",
            "fixture:entity_alphatest_one_sided":{"+states":["DisableDepthWrite"]}
        }}),
    );
    let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![upper, lower]),
    ));
    let files = super::collect_files(&view, None, None);
    let definitions = files
        .iter()
        .find_map(|(path, bytes)| {
            path.starts_with("materials/").then(|| {
                serde_json::from_slice::<serde_json::Value>(bytes).unwrap()["materials"].clone()
            })
        })
        .expect("authored material definitions must reach entity compilation");
    assert_eq!(
        definitions["fixture:entity_alphatest_one_sided"],
        json!({"+states":["DisableDepthWrite"]})
    );
    assert_eq!(
        definitions["unchanged:entity_alphatest"],
        json!({"-defines":["FANCY"]})
    );
    assert!(definitions.get("fixture:entity_alphatest").is_none());
}

#[test]
fn server_entity_textures_inherit_vanilla_without_overriding_server_formats() {
    let base = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(base.path().join("textures/entity")).unwrap();
    std::fs::write(base.path().join("textures/entity/fixture.png"), b"vanilla").unwrap();
    let entity = br#"{"minecraft:client_entity":{"description":{"identifier":"fixture:actor","textures":{"default":"textures/entity/fixture"}}}}"#;
    let view = |files: &[(&str, &[u8])]| {
        resource_pack::LayeredPackView::new(crate::test_support::stack(files))
    };
    let inherited = super::collect_files(
        &view(&[("entity/fixture.json", entity)]),
        None,
        Some(base.path()),
    );
    assert!(inherited.iter().any(|(path, bytes)| {
        path.as_ref() == "textures/entity/fixture.png" && bytes == b"vanilla"
    }));
    let overridden = super::collect_files(
        &view(&[
            ("entity/fixture.json", entity),
            ("textures/entity/fixture.tga", b"server"),
        ]),
        None,
        Some(base.path()),
    );
    assert!(overridden.iter().any(|(path, bytes)| {
        path.as_ref() == "textures/entity/fixture.tga" && bytes == b"server"
    }));
    assert!(
        !overridden
            .iter()
            .any(|(path, _)| path.as_ref() == "textures/entity/fixture.png")
    );
}

#[test]
fn server_attachables_inherit_vanilla_render_dependencies() {
    let attachable = serde_json::to_vec(&json!({
        "minecraft:attachable": {"description": {
            "identifier": "fixture:held_item",
            "geometry": {"default": "geometry.fixture.held_item"},
            "render_controllers": [
                "controller.render.default",
                {"controller.render.fixture": "query.is_first_person"}
            ],
            "animations": {
                "pose": "animation.fixture.pose",
                "held": "controller.animation.fixture.held"
            },
            "animation_controllers": [{"legacy": "controller.animation.fixture.legacy"}]
        }}
    }))
    .unwrap();
    let authored_controller = json!({"geometry": "Geometry.default"});
    let render_controllers = serde_json::to_vec(&json!({"render_controllers": {
        "controller.render.fixture": authored_controller
    }}))
    .unwrap();
    let view = resource_pack::LayeredPackView::new(crate::test_support::stack(&[
        ("attachables/held_item.json", &attachable),
        ("render_controllers/fixture.json", &render_controllers),
    ]));
    let mut vanilla = assets::VanillaEntityRefs::new();
    let default_controller = json!({
        "geometry": "Geometry.default",
        "materials": [{"*": "Material.default"}],
        "textures": ["Texture.default"]
    });
    vanilla.render_controllers.insert(
        "controller.render.default".into(),
        default_controller.clone(),
    );
    vanilla.render_controllers.insert(
        "controller.render.fixture".into(),
        json!({"geometry": "Geometry.overridden"}),
    );
    let animation = json!({"loop": true, "bones": {"root": {"rotation": [0, 0, 0]}}});
    vanilla
        .animations
        .insert("animation.fixture.pose".into(), animation.clone());
    let animation_controller = json!({"initial_state": "default", "states": {"default": {}}});
    for name in [
        "controller.animation.fixture.held",
        "controller.animation.fixture.legacy",
    ] {
        vanilla
            .animation_controllers
            .insert(name.into(), animation_controller.clone());
    }
    let geometry = json!({"minecraft:geometry": [{
        "description": {"identifier": "geometry.fixture.held_item"},
        "bones": [{"name": "root", "pivot": [0, 0, 0]}]
    }]});
    vanilla
        .geometry_index
        .insert("geometry.fixture.held_item".into(), 0);
    vanilla.geometry_files.push(assets::VanillaGeometryFile {
        path: "models/entity/held_item.json".into(),
        text: geometry.to_string(),
    });

    let files = super::collect_files(&view, Some(&vanilla), None);
    let collected = files
        .iter()
        .filter_map(|(_, bytes)| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .collect::<Vec<_>>();
    let definition = |family: &str, name: &str| {
        collected
            .iter()
            .find_map(|source| source.get(family)?.get(name))
    };
    assert_eq!(
        definition("render_controllers", "controller.render.default"),
        Some(&default_controller),
        "attachables must retain referenced vanilla render controllers"
    );
    assert_eq!(
        definition("render_controllers", "controller.render.fixture"),
        Some(&authored_controller),
        "authored dependencies must keep precedence over vanilla"
    );
    assert_eq!(
        definition("animations", "animation.fixture.pose"),
        Some(&animation)
    );
    for name in [
        "controller.animation.fixture.held",
        "controller.animation.fixture.legacy",
    ] {
        assert_eq!(
            definition("animation_controllers", name),
            Some(&animation_controller)
        );
    }
    assert!(files.iter().any(|(_, bytes)| {
        super::geometry_identifiers(bytes)
            .into_iter()
            .any(|identifier| identifier == "geometry.fixture.held_item")
    }));
}

#[test]
fn vanilla_texture_lookup_does_not_leave_the_pack_root() {
    let parent = tempfile::tempdir().unwrap();
    let base = parent.path().join("base");
    std::fs::create_dir_all(base.join("textures")).unwrap();
    std::fs::write(parent.path().join("outside.png"), b"outside").unwrap();
    let view = resource_pack::LayeredPackView::new(crate::test_support::stack(&[]));
    assert!(super::texture_file(&view, Some(&base), "textures/../../outside").is_none());
}

#[test]
fn artwork_only_is_decided_from_each_items_authored_dependencies() {
    use super::attachables::binding_inputs_unchanged;
    use std::collections::BTreeSet;
    let document = json!({"format_version": "1.10.0", "minecraft:attachable": {"description": {
        "identifier": "fixture:coat.player", "item": {"fixture:coat": "1"},
        "geometry": {"default": "geometry.fixture.coat", "alternate": "geometry.fixture.alternate"},
        "materials": {"default": "armor_leather", "enchanted": "armor_leather_enchanted"},
        "render_controllers": [{"controller.render.coat": "1"}],
        "animations": {"idle": "animation.coat.idle", "controller": "controller.animation.coat",
            "wield_first_person": "animation.coat.first", "wield_third_person": "animation.coat.third"},
        "animation_controllers": [{"active": "controller.animation.coat.extra"}]
    }}});
    let inputs = pack_entity_compiler::EquipmentBindingDependencies::default();
    let authored = BTreeSet::new();
    let unrelated = BTreeSet::from(["geometry.other".into(), "controller.render.other".into()]);
    assert!(binding_inputs_unchanged(
        &document,
        &authored,
        &unrelated,
        None,
        Some(&inputs)
    ));
    for changed in [
        "geometry.fixture.coat",
        "geometry.fixture.alternate",
        "controller.render.coat",
        "animation.coat.idle",
        "animation.coat.first",
        "animation.coat.third",
        "controller.animation.coat",
        "controller.animation.coat.extra",
        "armor_leather",
        "armor_leather_enchanted",
    ] {
        assert!(
            !binding_inputs_unchanged(
                &document,
                &authored,
                &BTreeSet::from([changed.into()]),
                None,
                Some(&inputs)
            ),
            "{changed}"
        );
    }
    assert!(!binding_inputs_unchanged(
        &document,
        &BTreeSet::from(["fixture:coat".into()]),
        &BTreeSet::new(),
        None,
        Some(&inputs),
    ));
    let mut vanilla = assets::VanillaEntityRefs::new();
    vanilla.geometry_parent.insert(
        "geometry.fixture.coat".into(),
        "geometry.fixture.parent".into(),
    );
    let parent = BTreeSet::from(["geometry.fixture.parent".into()]);
    assert!(!binding_inputs_unchanged(
        &document,
        &authored,
        &parent,
        Some(&vanilla),
        Some(&inputs)
    ));
    assert!(!binding_inputs_unchanged(
        &document,
        &authored,
        &BTreeSet::new(),
        None,
        None
    ));
}

#[test]
fn artwork_only_checks_installed_material_parents_and_skinning_variants() {
    use super::attachables::binding_inputs_unchanged;
    use std::collections::BTreeSet;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("materials")).unwrap();
    std::fs::write(
        root.path().join("materials/fixture.material"),
        json!({"materials": {
            "coat:coat_parent": {}, "coat_parent:armor_leather": {}
        }})
        .to_string(),
    )
    .unwrap();
    let document = json!({"format_version": "1.10.0", "minecraft:attachable": {"description": {
        "identifier": "fixture:coat", "materials": {"default": "coat.skinning"}
    }}});
    for (declaration, fields, unchanged) in [
        ("armor_leather", json!({"defines": []}), false),
        ("coat_parent:entity", json!({}), false),
        (
            "+coat_parent:armor_leather",
            json!({"-defines": ["USE_COLOR_MASK"]}),
            false,
        ),
        (
            "coat:entity",
            json!({"+defines": ["USE_COLOR_MASK"]}),
            false,
        ),
        ("unrelated:entity", json!({"defines": []}), true),
    ] {
        let mut materials = serde_json::Map::new();
        materials.insert(declaration.into(), fields);
        let authored = json!({"materials": materials});
        let inputs = pack_entity_compiler::EquipmentBindingDependencies::load(
            Some(root.path()),
            Some(&authored),
        )
        .unwrap();
        assert_eq!(
            binding_inputs_unchanged(
                &document,
                &BTreeSet::new(),
                inputs.authored_materials(),
                None,
                Some(&inputs)
            ),
            unchanged,
            "{declaration}"
        );
    }
}

#[test]
fn referenced_painting_texture_still_inherits_the_installed_raster() {
    let base = tempfile::tempdir().unwrap();
    let stem = assets::painting::PAINTING_ATLAS;
    let path = format!("{stem}.png");
    std::fs::create_dir_all(base.path().join("textures/painting")).unwrap();
    std::fs::write(base.path().join(&path), b"installed raster").unwrap();
    let entity = serde_json::json!({"minecraft:client_entity":{"description":{
        "identifier":"fixture:picture", "textures":{"default":stem}
    }}})
    .to_string();
    let view = resource_pack::LayeredPackView::new(crate::test_support::stack(&[(
        "entity/fixture.json",
        entity.as_bytes(),
    )]));
    let files = super::collect_files(&view, None, Some(base.path()));
    assert!(
        files
            .iter()
            .any(|(file, bytes)| file.as_ref() == path && bytes == b"installed raster")
    );
}

#[test]
fn many_file_collection_is_identical_on_one_and_four_threads() {
    let mut files = Vec::new();
    for index in 0..64 {
        files.push((
            format!("entity/model_{index:03}.json"),
            json!({
                "format_version":"1.10.0", "minecraft:client_entity":{"description":{
                    "identifier":format!("fixture:model_{index}"),
                    "geometry":{"default":format!("geometry.fixture_{index}")},
                    "textures":{"default":format!("textures/entity/model_{index}")},
                    "animations":{"idle":format!("animation.fixture_{index}.idle")}
                }}
            })
            .to_string()
            .into_bytes(),
        ));
        files.push((
            format!("models/entity/model_{index:03}.json"),
            json!({
                "format_version":"1.12.0", "minecraft:geometry":[{
                    "description":{"identifier":format!("geometry.fixture_{index}")},"bones":[]
                }]
            })
            .to_string()
            .into_bytes(),
        ));
        files.push((
            format!("animations/model_{index:03}.json"),
            json!({
                "format_version":"1.8.0", "animations":{
                    format!("animation.fixture_{index}.idle"):{"bones":{}}
                }
            })
            .to_string()
            .into_bytes(),
        ));
        files.push((
            format!("textures/entity/model_{index}.png"),
            vec![index as u8; 16],
        ));
    }
    let entries = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    let stack = crate::test_support::stack(&entries);
    let collect = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let view = resource_pack::LayeredPackView::tracked(std::sync::Arc::clone(&stack));
                let files = super::collect_files(&view, None, None);
                assert_eq!(
                    files
                        .iter()
                        .filter(|(path, _)| path.starts_with("entity/"))
                        .count(),
                    64
                );
                assert_eq!(
                    files
                        .iter()
                        .filter(|(path, _)| path.starts_with("textures/"))
                        .count(),
                    64
                );
                files
            })
    };
    assert_eq!(collect(1), collect(4));
}

#[test]
fn superseded_definition_documents_are_released_between_batches() {
    let files = (0..64).map(|index| (
        format!("animations/model_{index:03}.json"),
        json!({"animations":{"animation.fixture.shared":{"value":index,"padding":"x".repeat(1024)}}})
            .to_string().into_bytes(),
    )).collect::<Vec<_>>();
    let entries = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    let stack = crate::test_support::stack(&entries);
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap()
        .install(|| {
            super::PEAK_DEFINITION_DOCUMENTS.set(0);
            let view = resource_pack::LayeredPackView::new(stack);
            let merged = super::merged_definitions(&view, "animations/", "animations");
            assert_eq!(merged["animation.fixture.shared"]["value"], 63);
            assert!(
                (1..=5).contains(&super::PEAK_DEFINITION_DOCUMENTS.get()),
                "retained {} definition documents before merging",
                super::PEAK_DEFINITION_DOCUMENTS.get()
            );
        });
}

#[test]
fn entity_collection_memory_releases_superseded_payloads() {
    let files =
        (0..64)
            .map(|index| {
                (
        format!("entity/model_{index:03}.json"),
        json!({"minecraft:client_entity":{"description":{"identifier":"fixture:shared"}},
            "value":index,"padding":"x".repeat(1024)}).to_string().into_bytes(),
    )
            })
            .collect::<Vec<_>>();
    let largest = files.iter().map(|(_, bytes)| bytes.len()).max().unwrap();
    let entries = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    let stack = crate::test_support::stack(&entries);
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap()
        .install(|| {
            super::PEAK_ENTITY_PAYLOAD_BYTES.set(0);
            let view = resource_pack::LayeredPackView::new(stack);
            let collected = super::unique_entities(&view);
            assert_eq!(collected.files.len(), 1);
            let document: serde_json::Value =
                serde_json::from_slice(&collected.files[0].1).unwrap();
            assert_eq!(document["value"], 63);
            assert!(
                (collected.files[0].1.len()..=5 * largest)
                    .contains(&super::PEAK_ENTITY_PAYLOAD_BYTES.get()),
                "retained {} canonical entity bytes",
                super::PEAK_ENTITY_PAYLOAD_BYTES.get()
            );
        });
}

#[test]
fn geometry_collection_bounds_parallel_parse_input() {
    let padding = "x".repeat(3 * 1024 * 1024);
    let files = (0..6)
        .map(|index| {
            (
                format!("models/entity/large_{index}.json"),
                json!({"format_version":"1.12.0","minecraft:geometry":[{"description":
                    {"identifier":format!("geometry.large_{index}")}}],"padding":padding})
                .to_string()
                .into_bytes(),
            )
        })
        .collect::<Vec<_>>();
    let largest = files.iter().map(|(_, bytes)| bytes.len()).max().unwrap() as u64;
    let entries = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    let stack = crate::test_support::stack(&entries);
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap()
        .install(|| {
            super::PEAK_GEOMETRY_BATCH_BYTES.set(0);
            let view = resource_pack::LayeredPackView::new(stack);
            assert_eq!(super::unshadowed_geometry(&view).len(), 6);
            let peak = super::PEAK_GEOMETRY_BATCH_BYTES.get();
            assert!(
                peak <= (assets::MAX_ENTITY_SOURCE_BYTES as u64).max(largest),
                "parsed {peak} geometry input bytes in one parallel batch"
            );
        });
}
