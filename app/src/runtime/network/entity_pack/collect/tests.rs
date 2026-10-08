use std::io::Write;

use serde_json::json;

#[test]
fn resource_pack_texture_only_armor_inherits_its_vanilla_attachable() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("attachables")).unwrap();
    let path = "attachables/fixture.player.json";
    let attachable = json!({"minecraft:attachable":{"description":{
        "identifier":"fixture:armor.player", "item":{"fixture:armor":"1"},
        "textures":{"default":"textures/models/armor/fixture"},
        "geometry":{"default":"geometry.fixture.armor"},
        "render_controllers":["controller.render.armor"]
    }}});
    std::fs::write(
        root.path().join(path),
        serde_json::to_vec(&attachable).unwrap(),
    )
    .unwrap();
    let texture = "textures/models/armor/fixture.png";
    let view =
        resource_pack::LayeredPackView::tracked(super::super::super::pack_reload_tests::stack(&[
            (texture, b"pack pixels"),
        ]));
    let files = super::collect_files(&view, None, Some(root.path()));
    assert!(
        files.iter().any(|(name, bytes)| name.as_ref() == path
            && serde_json::from_slice::<serde_json::Value>(bytes).unwrap() == attachable),
        "texture-only packs must retain the base attachable that uses their image"
    );
    assert!(
        files
            .iter()
            .any(|(name, bytes)| name.as_ref() == texture && bytes == b"pack pixels")
    );
    let reads = view.dependencies().unwrap().snapshot();
    assert!(reads.contains(&resource_pack::PackDependency::Directory(
        "textures/".into()
    )));
    assert!(reads.iter().any(|dependency| matches!(dependency,
        resource_pack::PackDependency::File { path, .. } if path == texture
    )));
    let tga_view =
        resource_pack::LayeredPackView::new(super::super::super::pack_reload_tests::stack(&[(
            "textures/models/armor/fixture.tga",
            b"TGA pack pixels",
        )]));
    let tga_files = super::collect_files(&tga_view, None, Some(root.path()));
    assert!(tga_files.iter().any(|(name, _)| name.as_ref() == path));
    assert!(
        tga_files
            .iter()
            .any(|(name, bytes)| name.ends_with(".tga") && bytes == b"TGA pack pixels")
    );
    let empty =
        resource_pack::LayeredPackView::new(super::super::super::pack_reload_tests::stack(&[]));
    assert!(
        !super::collect_files(&empty, None, Some(root.path()))
            .iter()
            .any(|(name, _)| name.starts_with("attachables/")),
        "removing the texture removes the pack binding"
    );
}

#[test]
fn authored_attachable_keeps_precedence_over_texture_only_inheritance() {
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
    let view =
        resource_pack::LayeredPackView::tracked(super::super::super::pack_reload_tests::stack(&[
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
        protocol::ResourcePackHandoff::from_archives(vec![lower, upper]),
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
        resource_pack::LayeredPackView::new(super::super::super::pack_reload_tests::stack(files))
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
    let view =
        resource_pack::LayeredPackView::new(super::super::super::pack_reload_tests::stack(&[
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
    let view =
        resource_pack::LayeredPackView::new(super::super::super::pack_reload_tests::stack(&[]));
    assert!(super::texture_file(&view, Some(&base), "textures/../../outside").is_none());
}
