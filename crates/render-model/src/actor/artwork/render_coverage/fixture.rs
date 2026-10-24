use assets::{ActorArtworkBinding, ActorPoseMode, ActorTexture};
use sha2::{Digest, Sha256};

/// Compiles an original multi-candidate rig with complete, shared texture artwork.
pub(super) fn carrier() -> (Box<[u8]>, Vec<ActorTexture>, Vec<ActorArtworkBinding>) {
    let scratch = tempfile::tempdir().unwrap();
    for family in [
        "entity",
        "models/entity",
        "animations",
        "animation_controllers",
        "render_controllers",
        "textures/entity",
    ] {
        std::fs::create_dir_all(scratch.path().join(family)).unwrap();
    }
    let entity = serde_json::json!({
        "format_version": "1.10.0",
        "minecraft:client_entity": {"description": {
            "identifier": "test:coverage",
            "materials": {"default": "entity_alphatest"},
            "textures": {"default": "textures/entity/coverage"},
            "geometry": {"default": "geometry.first", "alternate": "geometry.second"},
            "render_controllers": ["controller.render.coverage"]
        }}
    });
    let geometries: Vec<_> = ["geometry.first", "geometry.second"]
        .map(|identifier| serde_json::json!({
            "description": {"identifier": identifier, "texture_width": 16, "texture_height": 16},
            "bones": [{"name": "body", "cubes": [{"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}]}]
        }))
        .into();
    let controller = serde_json::json!({
        "format_version": "1.8.0",
        "render_controllers": {"controller.render.coverage": {
            "arrays": {"geometries": {"Array.body": ["Geometry.default", "Geometry.alternate"]}},
            "geometry": "Array.body[query.variant]",
            "materials": [{"*": "Material.default"}],
            "textures": ["Texture.default"]
        }}
    });
    for (path, document) in [
        ("entity/coverage.entity.json", entity),
        (
            "models/entity/coverage.geo.json",
            serde_json::json!({
                "format_version": "1.12.0", "minecraft:geometry": geometries
            }),
        ),
        (
            "render_controllers/coverage.render_controllers.json",
            controller,
        ),
    ] {
        std::fs::write(scratch.path().join(path), document.to_string()).unwrap();
    }
    let pixels = vec![255; 16 * 16 * 4];
    image::RgbaImage::from_raw(16, 16, pixels.clone())
        .unwrap()
        .save(scratch.path().join("textures/entity/coverage.png"))
        .unwrap();
    let compiled = pack_entity_compiler::compile_entity_assets(
        scratch.path(),
        include_bytes!("../../../../../../assets/vanilla-source.json"),
    )
    .unwrap();
    let source = compiled
        .sources
        .iter()
        .position(|source| source.path.as_ref() == "textures/entity/coverage.png")
        .unwrap() as u32;
    let textures = vec![ActorTexture {
        source,
        width: 16,
        height: 16,
        pixel_sha256: Sha256::digest(&pixels).into(),
        rgba8: pixels.into(),
    }];
    let rig = &compiled.rig_bindings[0];
    assert!(rig.geometry_count > 1);
    let bindings = (rig.first_geometry..rig.first_geometry + u32::from(rig.geometry_count))
        .map(|candidate| ActorArtworkBinding {
            rig: 0,
            geometry_candidate: candidate,
            entity_symbol: rig.entity_symbol,
            geometry: compiled.rig_geometries[candidate as usize].geometry,
            render_controller: rig.render_controller.unwrap(),
            texture: 0,
            material: "entity_alphatest".into(),
            pose_mode: ActorPoseMode::CompiledLiteral,
        })
        .collect();
    (
        assets::encode_entity_blob(&compiled).unwrap(),
        textures,
        bindings,
    )
}
