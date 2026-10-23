//! Original fixtures for resource-only changes to completed startup body definitions.
use super::*;
use serde_json::json;

/// Writes original resource documents and one tag-derived harness raster.
fn fixture() -> (tempfile::TempDir, String, String) {
    let root = tempfile::tempdir().unwrap();
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    let tag = facts.tag(assets::facts::HARNESS_TAG).unwrap();
    let item = facts
        .payload()
        .records
        .iter()
        .find(|record| record.fact.tags.contains(tag))
        .unwrap()
        .identifier
        .to_string();
    let color = item
        .strip_prefix("minecraft:")
        .unwrap()
        .strip_suffix("_harness")
        .unwrap();
    let texture = format!("textures/entity/harness/harness_{color}");
    write(root.path(), "models/entity/harness.geo.json", &model(1));
    write(
        root.path(),
        "render_controllers/harness.render_controllers.json",
        &controller(true),
    );
    let path = root.path().join(format!("{texture}.png"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, pixels(40)).unwrap();
    (root, item, texture)
}

/// Encodes a tiny original cuboid with a distinguishable width.
fn model(width: u8) -> Vec<u8> {
    json!({"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.harness","texture_width":8,"texture_height":8},
        "bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[width,1,1],"uv":[0,0]}]}]
    }]})
    .to_string()
    .into_bytes()
}

/// Selects an original controller visibility state independently of its geometry.
fn controller(visible: bool) -> Vec<u8> {
    json!({"format_version":"1.8.0","render_controllers":{"controller.render.harness":{
        "geometry":"Geometry.default","textures":["Texture.default"],
        "materials":[{"*":"Material.default"}],"part_visibility":[{"*":visible}]
    }}})
    .to_string()
    .into_bytes()
}

/// Encodes original solid pixels without installed artwork.
fn pixels(red: u8) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(8, 8, image::Rgba([red, 20, 30, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Writes an owned fixture source beneath its pack root.
fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn resource_only_harness_override_collects_completed_startup_binding_and_removes_it() {
    let (root, item, texture) = fixture();
    let vanilla = pack_entity_compiler::compile_vanilla_entity_refs(root.path()).unwrap();
    let geometry = model(3);
    let render = controller(false);
    let raster = pixels(99);
    let path = format!("{texture}.png");
    for replace_texture in [true, false] {
        let mut entries = vec![
            ("models/entity/replacement.json", geometry.as_slice()),
            ("render_controllers/replacement.json", render.as_slice()),
        ];
        if replace_texture {
            entries.push((path.as_str(), raster.as_slice()));
        }
        let view = LayeredPackView::new(crate::test_support::stack(&entries));
        let files = collect_files(&view, Some(&vanilla), Some(root.path()));
        let compiled = pack_entity_compiler::compile_actor_pack(files.clone())
            .unwrap()
            .unwrap();
        assert_eq!(
            compiled.equipment_bindings.len(),
            1,
            "resource-only changes need the startup binding"
        );
        assert_eq!(compiled.equipment_bindings[0].identifier.as_ref(), item);
        let selected = compiled
            .entities
            .geometries
            .iter()
            .find(|geometry| geometry.identifier.as_ref() == "geometry.harness")
            .unwrap();
        assert_eq!(selected.bones[0].cubes[0].size[0].get(), 3.0);
        let render: Value = files
            .iter()
            .find(|(path, _)| path.as_ref() == "render_controllers/_pack.json")
            .map(|(_, bytes)| serde_json::from_slice(bytes).unwrap())
            .unwrap();
        assert_eq!(
            render["render_controllers"]["controller.render.harness"]["part_visibility"][0]["*"],
            false
        );
        let image = compiled
            .equipment_textures
            .iter()
            .find(|image| image.identifier.as_ref() == texture)
            .unwrap();
        assert_eq!(
            &image.rgba8[..4],
            &[if replace_texture { 99 } else { 40 }, 20, 30, 255]
        );
    }
    let empty = LayeredPackView::new(crate::test_support::stack(&[]));
    let removed = collect_files(&empty, Some(&vanilla), Some(root.path()));
    assert!(
        !removed
            .iter()
            .any(|(path, _)| path.starts_with("attachables/")),
        "removal leaves startup definitions in charge"
    );
    let texture_only = LayeredPackView::new(crate::test_support::stack(&[(&path, &raster)]));
    assert!(
        !collect_files(&texture_only, Some(&vanilla), Some(root.path()))
            .iter()
            .any(|(path, _)| path.starts_with("attachables/")),
        "artwork-only changes preserve startup binding selection"
    );
}

#[test]
fn resource_only_harness_override_preserves_authored_startup_conditions_and_pack_precedence() {
    let (root, item, texture) = fixture();
    let authored = json!({"format_version":"1.10.0","minecraft:attachable":{"description":{
        "identifier":"fixture:conditional", "item":{(&item):"context.item_slot == 'body'"},
        "geometry":{"default":"geometry.harness"},"materials":{"default":"harness"},
        "textures":{"default":texture},"render_controllers":["controller.render.harness"]
    }}});
    let mut bytes = b"// Original conditional definition.\n".to_vec();
    bytes.extend(authored.to_string().into_bytes());
    write(root.path(), "attachables/conditional.json", &bytes);
    let vanilla = pack_entity_compiler::compile_vanilla_entity_refs(root.path()).unwrap();
    let geometry = model(3);
    let raster = pixels(99);
    let path = format!("{texture}.png");
    for supplied in [false, true] {
        let mut server = authored.clone();
        server["minecraft:attachable"]["description"]["identifier"] = json!("fixture:disabled");
        server["minecraft:attachable"]["description"]["item"] = json!({(&item):"0"});
        let server = server.to_string().into_bytes();
        let mut entries = vec![
            ("models/entity/replacement.json", geometry.as_slice()),
            (path.as_str(), raster.as_slice()),
        ];
        if supplied {
            entries.push(("attachables/server.json", server.as_slice()));
        }
        let view = LayeredPackView::new(crate::test_support::stack(&entries));
        let files = collect_files(&view, Some(&vanilla), Some(root.path()));
        let descriptions = files
            .iter()
            .filter(|(path, _)| path.starts_with("attachables/"))
            .map(|(_, bytes)| serde_json::from_slice::<Value>(bytes).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            descriptions.len(),
            1,
            "no synthesized fallback may bypass authored conditions"
        );
        assert_eq!(
            descriptions[0],
            if supplied {
                serde_json::from_slice(&server).unwrap()
            } else {
                authored.clone()
            }
        );
        let compiled = pack_entity_compiler::compile_actor_pack(files)
            .unwrap()
            .unwrap();
        assert_eq!(compiled.equipment_bindings.len(), 1);
        assert_eq!(
            compiled.equipment_bindings[0].attachable_identifier(),
            if supplied {
                "fixture:disabled"
            } else {
                "fixture:conditional"
            }
        );
        assert!(
            compiled
                .entities
                .rig_bindings
                .iter()
                .any(|binding| binding.item_condition.is_some())
        );
    }
}
