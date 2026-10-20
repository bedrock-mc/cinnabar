use super::*;
use assets::Material;
use assets::material_keys::{BlockTextureSource, MaterialKeys, MaterialTextureSource};
use serde_json::{Value, json};

/// One pack's terrain catalog, blocks catalog, and flat-colour rasters by path.
type PackLayer<'a> = (Value, Value, Vec<(&'a str, [u8; 4])>);

/// Packs contain only original tiny catalogs and flat color rasters.
fn stack(layers: &[PackLayer<'_>]) -> LayeredPackView {
    let archives = layers.iter().enumerate().rev().map(|(index, (terrain, blocks, colors))| {
        let id = format!("00000000-0000-0000-0000-{:012}", index + 1);
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},"modules":[{"type":"resources"}]}).to_string();
        for (path, bytes) in [("manifest.json", manifest.into_bytes()),
            ("textures/terrain_texture.json", terrain.to_string().into_bytes()),
            ("blocks.json", blocks.to_string().into_bytes())] {
            writer.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(&bytes).unwrap();
        }
        for &(path, color) in colors {
            writer.start_file(format!("{path}.png"), zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(&png(16, 16, |_, _| color)).unwrap();
        }
        bedrock_packs::ResourcePackArchive::unencrypted(id.parse().unwrap(), "1.0.0".into(), String::new(), writer.finish().unwrap().into_inner())
    }).collect();
    LayeredPackView::new(resource_pack::validate_handoff(
        bedrock_packs::ResourcePackHandoff::from_archives(archives),
    ))
}

/// Two block states keep different material ids even when they share one terrain key.
fn base() -> RuntimeAssets {
    let diagnostic = RuntimeAssets::diagnostic();
    let mut cube = super::super::diagnostic_visual();
    cube.kind = VisualKind::Cube;
    cube.support = VisualSupport::Exact;
    cube.flags = assets::BlockFlags::CUBE_GEOMETRY | assets::BlockFlags::OCCLUDES_FULL_FACE;
    cube.faces = [1; 6];
    let mut second = cube;
    second.faces = [2; 6];
    let compiled = assets::CompiledAssets {
        visuals: vec![cube, second].into(),
        light_properties: vec![assets::LightProperties::OPAQUE_DARK; 2].into(),
        hashed: Box::new([]),
        materials: vec![Material::unvaried(); 3].into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: diagnostic.texture_pages().to_vec().into(),
        biomes: assets::CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap()
}

/// Synthetic carrier metadata retains state and block binding facts after packing.
fn keys() -> MaterialKeys {
    MaterialKeys::from_entries([(1, "state"), (2, "state")])
        .with_aliases([("state", "textures/base_a")])
        .with_sources(
            [(
                "state".into(),
                json!({"textures":["textures/base_a","textures/base_b"]}),
            )],
            [
                (
                    1,
                    MaterialTextureSource {
                        key: "state".into(),
                        variant: 0,
                        path: "textures/base_a".into(),
                        raster: "textures/base_a.png".into(),
                        baked_tint: None,
                    },
                ),
                (
                    2,
                    MaterialTextureSource {
                        key: "state".into(),
                        variant: 1,
                        path: "textures/base_b".into(),
                        raster: "textures/base_b.png".into(),
                        baked_tint: None,
                    },
                ),
            ],
            [
                BlockTextureSource {
                    visual: 0,
                    name: "minecraft:fixture".into(),
                    binding_name: "fixture".into(),
                    source_faces: [0, 1, 2, 3, 4, 5],
                    source_rotations: [false; 6],
                    cube_binding_eligible: true,
                    carried_keys: std::array::from_fn(|_| None),
                    carried_fallback_keys: std::array::from_fn(|_| Some("state".into())),
                },
                BlockTextureSource {
                    visual: 1,
                    name: "minecraft:other".into(),
                    binding_name: "other".into(),
                    source_faces: [0, 1, 2, 3, 4, 5],
                    source_rotations: [false; 6],
                    cube_binding_eligible: true,
                    carried_keys: std::array::from_fn(|_| None),
                    carried_fallback_keys: std::array::from_fn(|_| Some("state".into())),
                },
            ],
        )
}

/// Reads the actual published texture page at one material's first pixel.
fn material_color(runtime: &RuntimeAssets, material: u32) -> [u8; 4] {
    let material = runtime.material(material);
    let page = &runtime.texture_pages()[material.texture.page() as usize].texture;
    let mip = &page.mips[0];
    let start = material.texture.layer() as usize * (mip.size * mip.size * 4) as usize;
    mip.rgba8[start..start + 4].try_into().unwrap()
}

/// Compiles against the explicit immutable base instead of process-global fixtures.
fn compile(
    view: &LayeredPackView,
    base: &RuntimeAssets,
    keys: &MaterialKeys,
) -> super::super::CompiledBlockOverlay {
    super::super::compile_block_overlay(
        view,
        &CustomBlocks::default(),
        false,
        Some(keys),
        assets::MAX_TEXTURE_LAYERS as u32,
        Some(base),
    )
    .unwrap()
}

#[test]
fn terrain_state_variants_and_removal_preserve_their_own_rasters() {
    let base = base();
    let keys = keys();
    let red = [200, 10, 20, 255];
    let blue = [20, 30, 200, 255];
    let view = stack(&[(
        json!({"texture_data":{"state":{"textures":["textures/a","textures/b"]}}}),
        json!({}),
        vec![("textures/a", red), ("textures/b", blue)],
    )]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
    assert_eq!(material_color(&runtime, 1), red);
    assert_eq!(material_color(&runtime, 2), blue);
    assert_eq!(
        runtime
            .resolve(NetworkIdMode::Sequential, 0)
            .face(BlockFace::Up)
            .material_id(),
        1
    );
    assert_eq!(
        runtime
            .resolve(NetworkIdMode::Sequential, 1)
            .face(BlockFace::Up)
            .material_id(),
        2
    );
    let empty = stack(&[(json!({}), json!({}), vec![])]);
    assert!(
        super::super::compile_block_overlay(
            &empty,
            &CustomBlocks::default(),
            false,
            Some(&keys),
            assets::MAX_TEXTURE_LAYERS as u32,
            Some(&base)
        )
        .is_none()
    );
    assert_eq!(base.material(1).texture.page(), 0);
    assert_eq!(base.material(2).texture.page(), 0);
}

#[test]
fn weighted_choices_survive_publication_and_raster_only_layers() {
    let base = base();
    let keys = keys();
    let first = [10, 90, 20, 255];
    let second = [80, 20, 10, 255];
    let replacement = [30, 40, 220, 255];
    let catalog = json!({"texture_data":{"state":{"textures":[{"variations":[{"path":"textures/a","weight":1},{"path":"textures/b","weight":3}]},"textures/a"]}}});
    let lower = (
        catalog,
        json!({}),
        vec![("textures/a", first), ("textures/b", second)],
    );
    let view = stack(&[
        lower.clone(),
        (json!({}), json!({}), vec![("textures/b", replacement)]),
    ]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
    let selector = runtime.material(1);
    assert_eq!(selector.variation_count, 2);
    let leaves = &runtime.materials()[selector.variation_start as usize..][..2];
    assert_eq!(
        leaves
            .iter()
            .map(|material| f32::from_bits(material.variation_weight))
            .collect::<Vec<_>>(),
        [0.25, 0.75]
    );
    assert_eq!(material_color(&runtime, selector.variation_start), first);
    assert_eq!(
        material_color(&runtime, selector.variation_start + 1),
        replacement
    );
    let restored = compile(&stack(&[lower]), &base, &keys);
    let restored = base.with_block_overlay(2, &restored.overlay).unwrap();
    assert_eq!(
        material_color(&restored, restored.material(1).variation_start + 1),
        second
    );
}

#[test]
fn block_bindings_are_specific_to_the_visual_and_carried_faces() {
    let base = base();
    let keys = keys();
    let top = [20, 200, 30, 255];
    let side = [180, 20, 30, 255];
    let carried = [40, 30, 220, 255];
    for (textures, carried_textures) in [
        (json!("side"), json!("carry")),
        (
            json!({"up":"top","down":"side","side":"side"}),
            json!({"up":"carry","down":"carry","side":"carry"}),
        ),
        (
            json!({"up":"top","down":"side","west":"side","east":"side","north":"side","south":"side"}),
            json!({"up":"carry","down":"carry","west":"carry","east":"carry","north":"carry","south":"carry"}),
        ),
    ] {
        let upper = (
            json!({"texture_data":{"top":{"textures":"textures/top"},"side":{"textures":"textures/side"},"carry":{"textures":"textures/carry"}}}),
            json!({"fixture":{"textures":textures,"carried_textures":carried_textures}}),
            vec![
                ("textures/top", top),
                ("textures/side", side),
                ("textures/carry", carried),
            ],
        );
        let overlay = compile(&stack(&[upper]), &base, &keys);
        let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
        let block = runtime.resolve(NetworkIdMode::Sequential, 0);
        assert_eq!(
            material_color(&runtime, block.face(BlockFace::Up).material_id()),
            if textures.is_string() { side } else { top }
        );
        assert_eq!(
            material_color(&runtime, block.face(BlockFace::North).material_id()),
            side
        );
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, 1)
                .face(BlockFace::Up)
                .material_id(),
            2
        );
        let faces = overlay.vanilla_carried[0].faces;
        let sheet =
            render_model::equipment::blocks::overlay_face_sheet(&overlay.overlay, faces, None)
                .unwrap();
        assert!(
            sheet
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == carried)
        );
        let icon = pack_entity_compiler::overlay_cube_icon(&overlay.overlay, faces, None).unwrap();
        assert!(
            icon.rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[2] > pixel[0] && pixel[3] != 0)
        );
        assert_eq!(
            base.resolve(NetworkIdMode::Sequential, 0)
                .face(BlockFace::Up)
                .material_id(),
            1
        );
    }
}

#[test]
fn bindings_can_reference_carrier_textures_without_replacing_rasters() {
    let base = base();
    let keys = keys();
    let view = stack(&[(
        json!({"texture_data":{"alias":{"textures":"textures/base_a"}}}),
        json!({"fixture":{"textures":"alias","carried_textures":"alias"}}),
        vec![],
    )]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
    let material = runtime
        .resolve(NetworkIdMode::Sequential, 0)
        .face(BlockFace::Up)
        .material_id();
    assert_eq!(material_color(&runtime, material), material_color(&base, 1));
    assert!(!overlay.vanilla_carried.is_empty());
}

#[test]
fn a_shared_material_can_use_a_later_alias_after_an_earlier_route_misses() {
    let base = base();
    let keys = MaterialKeys::from_entries([(1, "first"), (1, "second")])
        .with_aliases([("first", "textures/missing"), ("second", "textures/base")]);
    let pixel = [40, 150, 30, 255];
    let view = stack(&[(
        json!({"texture_data":{"second":{"textures":"textures/changed"}}}),
        json!({}),
        vec![("textures/changed", pixel)],
    )]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
    assert_eq!(material_color(&runtime, 1), pixel);
}

#[test]
fn an_untinted_state_does_not_inherit_the_other_states_tint() {
    let base = base();
    let keys = keys();
    let pixel = [200, 100, 50, 255];
    let view = stack(&[(
        json!({"texture_data":{"state":{"textures":[
        {"path":"textures/a","tint_color":"#ff0000"},"textures/b"]}}}),
        json!({}),
        vec![("textures/a", pixel), ("textures/b", pixel)],
    )]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
    assert_eq!(material_color(&runtime, 1), [200, 0, 0, 255]);
    assert_eq!(material_color(&runtime, 2), pixel);
}

#[test]
fn carried_only_keys_trigger_item_preparation_for_catalog_and_raster_changes() {
    let base = base();
    let keys = keys();
    let mut terrain = keys
        .terrain_entries()
        .map(|(key, entry)| (key.into(), entry.clone()))
        .collect::<Vec<_>>();
    terrain.push(("carried".into(), json!({"textures":"textures/carried"})));
    let materials = keys
        .material_sources()
        .map(|(id, source)| (id, source.clone()))
        .collect::<Vec<_>>();
    let mut blocks = keys.block_sources().to_vec();
    blocks[0].carried_keys = std::array::from_fn(|_| Some("carried".into()));
    let keys = keys.with_sources(terrain, materials, blocks);
    let pixel = [30, 40, 200, 255];
    for (catalog, path) in [
        (json!({}), "textures/carried"),
        (
            json!({"texture_data":{"carried":{"textures":"textures/new_carried"}}}),
            "textures/new_carried",
        ),
    ] {
        let view = stack(&[(catalog, json!({}), vec![(path, pixel)])]);
        let overlay = compile(&view, &base, &keys);
        assert!(overlay.overlay.material_overrides.is_empty());
        assert!(overlay.overlay.face_overrides.is_empty());
        let faces = overlay
            .vanilla_carried
            .iter()
            .find(|route| route.visual == 0)
            .unwrap()
            .faces;
        let sheet =
            render_model::equipment::blocks::overlay_face_sheet(&overlay.overlay, faces, None)
                .unwrap();
        assert!(
            sheet
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|sample| *sample == pixel)
        );
        assert_eq!(base.material(1).texture.page(), 0);
    }
}

mod exclusions;

mod carrier_replacements;
