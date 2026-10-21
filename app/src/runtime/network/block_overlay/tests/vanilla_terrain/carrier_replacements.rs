use super::*;

/// Builds carrier layers and preserves their literal pixels at every mip level.
fn carrier(colors: &[[u8; 4]], faces: [u32; 6], flags: &[u32]) -> RuntimeAssets {
    let diagnostic = RuntimeAssets::diagnostic();
    let mut texture = diagnostic.texture_array().clone();
    texture.layers = colors.len() as u32;
    for mip in &mut texture.mips {
        mip.rgba8 = colors
            .iter()
            .flat_map(|color| color.repeat((mip.size * mip.size) as usize))
            .collect::<Vec<_>>()
            .into();
    }
    carrier_with_texture(texture, faces, flags)
}

/// Builds an immutable carrier around original pixel data and the supplied cube materials.
fn carrier_with_texture(
    texture: assets::TextureArray,
    faces: [u32; 6],
    flags: &[u32],
) -> RuntimeAssets {
    carrier_with_animation(texture, faces, flags, None, vec![])
}

/// Installs a supplied timeline on the first donor while keeping the other materials static.
fn carrier_with_animation(
    texture: assets::TextureArray,
    faces: [u32; 6],
    flags: &[u32],
    animation: Option<assets::Animation>,
    frames: Vec<assets::TextureRef>,
) -> RuntimeAssets {
    let layers = texture.layers;
    let mut cube = super::super::super::diagnostic_visual();
    cube.kind = VisualKind::Cube;
    cube.support = VisualSupport::Exact;
    cube.flags = assets::BlockFlags::CUBE_GEOMETRY | assets::BlockFlags::OCCLUDES_FULL_FACE;
    cube.faces = faces;
    let mut materials = std::iter::once(Material::unvaried())
        .chain(flags.iter().enumerate().map(|(index, &flags)| Material {
            texture: assets::TextureRef::new(0, index as u32 % layers).unwrap(),
            flags,
            ..Material::unvaried()
        }))
        .collect::<Vec<_>>();
    if animation.is_some() {
        materials[1].animation = 0;
        materials[1].texture = frames[0];
    }
    let compiled = assets::CompiledAssets {
        visuals: vec![cube; 2].into(),
        light_properties: vec![assets::LightProperties::OPAQUE_DARK; 2].into(),
        hashed: Box::new([]),
        materials: materials.into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: animation.into_iter().collect::<Vec<_>>().into(),
        animation_frames: frames.into(),
        texture_pages: vec![assets::TexturePage::new(texture)].into(),
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

/// Reads optional raster multipliers through the same public atlas operation as carrier creation.
fn tinted(mut pixel: [u8; 4], tint: Option<[u8; 3]>) -> [u8; 4] {
    if let Some(tint) = tint {
        pack_compile_input::apply_atlas_tint(&mut pixel, tint);
    }
    pixel
}

#[test]
fn carrier_fallback_preserves_baked_tint_and_applies_a_changed_winning_tint_once() {
    let pixel = [200, 100, 240, 128];
    let original_tint = [128, 255, 64];
    let base = carrier(
        &[pixel, tinted(pixel, Some(original_tint))],
        [2; 6],
        &[0, 0],
    );
    let keys = MaterialKeys::from_entries([(1, "pad"), (2, "pad")]).with_sources(
        [(
            "pad".into(),
            json!({"textures":{"path":"textures/pad","tint_color":"#80ff40"}}),
        )],
        [
            (
                1,
                MaterialTextureSource {
                    key: "pad".into(),
                    variant: 0,
                    path: "textures/pad".into(),
                    raster: "textures/pad.png".into(),
                    baked_tint: None,
                },
            ),
            (
                2,
                MaterialTextureSource {
                    key: "pad".into(),
                    variant: 0,
                    path: "textures/pad".into(),
                    raster: "textures/pad.png".into(),
                    baked_tint: Some(original_tint),
                },
            ),
        ],
        [],
    );
    for (entry, tint) in [
        (
            json!({"path":"textures/pad","tint_color":"#80ff40"}),
            Some(original_tint),
        ),
        (
            json!({"path":"textures/pad","tint_color":"#ff8040"}),
            Some([255, 128, 64]),
        ),
        (json!("textures/pad"), None),
    ] {
        let view = stack(&[(
            json!({"texture_data":{"pad":{"textures":entry}}}),
            json!({}),
            vec![],
        )]);
        if tint == Some(original_tint) {
            assert!(
                super::super::super::compile_block_overlay(
                    &view,
                    &CustomBlocks::default(),
                    false,
                    Some(&keys),
                    assets::MAX_TEXTURE_LAYERS as u32,
                    Some(&base)
                )
                .is_none()
            );
            continue;
        }
        let overlay = compile(&view, &base, &keys);
        let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
        for material in [1, 2] {
            assert_eq!(material_color(&runtime, material), tinted(pixel, tint));
        }
    }
    assert_eq!(material_color(&base, 2), tinted(pixel, Some(original_tint)));
}

#[test]
fn carrier_fallback_caches_different_tints_of_the_same_raw_raster_separately() {
    let pixel = [200, 100, 240, 128];
    let base = carrier(&[pixel], [1; 6], &[0]);
    let keys = MaterialKeys::from_entries([(1, "fixture")]).with_sources(
        [("fixture".into(), json!({"textures":"textures/raw"}))],
        [(
            1,
            MaterialTextureSource {
                key: "fixture".into(),
                variant: 0,
                path: "textures/raw".into(),
                raster: "textures/raw.png".into(),
                baked_tint: None,
            },
        )],
        [],
    );
    let view = stack(&[(
        json!({"texture_data":{"fixture":{"textures":{"variations":[
            {"path":"textures/raw","tint_color":"#80ff40","weight":1},
            {"path":"textures/raw","tint_color":"#ff8040","weight":1}
        ]}}}}),
        json!({}),
        vec![],
    )]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
    let selector = runtime.material(1);
    assert_eq!(selector.variation_count, 2);
    for (index, tint) in [[128, 255, 64], [255, 128, 64]].into_iter().enumerate() {
        assert_eq!(
            material_color(&runtime, selector.variation_start + index as u32),
            tinted(pixel, Some(tint))
        );
    }
}

#[test]
fn cube_bindings_resolve_new_catalog_keys_from_existing_carrier_raster_paths() {
    let pixel = [120, 80, 40, 255];
    let base = carrier(&[pixel], [1; 6], &[0, 0]);
    let keys = keys();
    let view = stack(&[(
        json!({"texture_data":{"replacement":{"textures":"textures/base_a"}}}),
        json!({"fixture":{"textures":"replacement","carried_textures":"replacement"}}),
        vec![],
    )]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
    let visual = runtime.resolve(NetworkIdMode::Sequential, 0);
    for face in BlockFace::ALL {
        assert_eq!(
            material_color(&runtime, visual.face(face).material_id()),
            pixel
        );
        assert_ne!(visual.face(face).material_id(), 1);
    }
    let carried = &overlay.vanilla_carried[0];
    let sheet =
        render_model::equipment::blocks::overlay_face_sheet(&overlay.overlay, carried.faces, None)
            .unwrap();
    assert!(
        sheet
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|sample| *sample == pixel)
    );
}

#[test]
fn scalar_and_per_face_pillar_bindings_recompute_rotation_in_both_directions() {
    for (mapping, rotations) in [
        ([2, 3, 1, 0, 4, 5], [false, false, true, true, true, true]),
        ([0, 1, 5, 4, 2, 3], [true, true, true, true, false, false]),
    ] {
        for original_per_face in [false, true] {
            let flags = rotations.map(|rotate| {
                if original_per_face && rotate {
                    assets::MATERIAL_FLAG_ROTATE_UV
                } else {
                    0
                }
            });
            let base = carrier(&[[100, 120, 140, 255]], [1, 2, 3, 4, 5, 6], &flags);
            let original_keys = keys();
            let mut blocks = original_keys.block_sources().to_vec();
            blocks[0].source_faces = mapping;
            blocks[0].source_rotations = rotations;
            let keys = MaterialKeys::from_entries((1..=6).map(|id| (id, "state"))).with_sources(
                [("state".into(), json!({"textures":"textures/base_a"}))],
                (1..=6).map(|id| {
                    (
                        id,
                        MaterialTextureSource {
                            key: "state".into(),
                            variant: 0,
                            path: "textures/base_a".into(),
                            raster: "textures/base_a.png".into(),
                            baked_tint: None,
                        },
                    )
                }),
                blocks,
            );
            for per_face in [false, true] {
                let binding = if per_face {
                    json!({"west":"replacement","east":"replacement","down":"replacement","up":"replacement","north":"replacement","south":"replacement"})
                } else {
                    json!("replacement")
                };
                let view = stack(&[(
                    json!({"texture_data":{"replacement":{"textures":"textures/new"}}}),
                    json!({"fixture":{"textures":binding,"carried_textures":"replacement"}}),
                    vec![("textures/new", [140, 90, 50, 255])],
                )]);
                let overlay = compile(&view, &base, &keys);
                let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
                let visual = runtime.resolve(NetworkIdMode::Sequential, 0);
                for face in BlockFace::ALL {
                    let material = runtime.material(visual.face(face).material_id());
                    assert_eq!(
                        material.flags & assets::MATERIAL_FLAG_ROTATE_UV != 0,
                        per_face && rotations[face as usize]
                    );
                }
                assert!(
                    overlay.vanilla_carried[0]
                        .faces
                        .iter()
                        .all(|&id| overlay.overlay.materials[id as usize].flags
                            & assets::MATERIAL_FLAG_ROTATE_UV
                            == 0)
                );
            }
        }
    }
}

/// Supplies a PNG containing exactly the same pixels as the immutable carrier layer.
fn raster_pack(terrain: Value, blocks: Value, pixels: &assets::TextureMip) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},"modules":[{"type":"resources"}]}).to_string();
    let bytes = png(pixels.size, pixels.size, |x, y| {
        let start = ((y * pixels.size + x) * 4) as usize;
        pixels.rgba8[start..start + 4].try_into().unwrap()
    });
    for (path, bytes) in [
        ("manifest.json", manifest.into_bytes()),
        (
            "textures/terrain_texture.json",
            terrain.to_string().into_bytes(),
        ),
        ("blocks.json", blocks.to_string().into_bytes()),
        ("textures/base_a.png", bytes),
    ] {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        writer.finish().unwrap().into_inner(),
    );
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ))
}

#[test]
fn carrier_and_pack_sources_share_grid_tint_and_carried_art_preparation() {
    let mut texture = RuntimeAssets::diagnostic().texture_array().clone();
    for mip in &mut texture.mips {
        let size = mip.size;
        mip.rgba8 = (0..size)
            .flat_map(|y| {
                (0..size).flat_map(move |x| match (x < size / 2, y < size / 2) {
                    (true, true) => [200, 0, 0, 255],
                    (false, true) => [0, 200, 0, 255],
                    (true, false) => [0, 0, 200, 255],
                    _ => [200, 200, 200, 255],
                })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
    }
    let base = carrier_with_texture(texture.clone(), [1; 6], &[0, 0]);
    let keys = keys();
    let terrain = json!({"texture_data":{
        "quad":{"quad":1,"textures":"./textures/base_a.png"},
        "plain":{"quad":0,"textures":"textures/base_a"},
        "tinted":{"quad":1,"textures":{"path":"textures/base_a","tint_color":"#80ffff"}}
    }});
    let blocks = json!({"fixture":{"textures":"tinted","carried_textures":"quad"},"other":{"textures":"plain","carried_textures":"plain"}});
    let pack = compile(
        &raster_pack(terrain.clone(), blocks.clone(), &texture.mips[0]),
        &base,
        &keys,
    );
    let fallback = compile(&stack(&[(terrain, blocks, vec![])]), &base, &keys);
    let runtime = base.with_block_overlay(2, &fallback.overlay).unwrap();
    let packed_runtime = base.with_block_overlay(2, &pack.overlay).unwrap();
    for (id, grid) in [(0, 1), (1, 0)] {
        let material = runtime
            .resolve(NetworkIdMode::Sequential, id)
            .face(BlockFace::Up)
            .material_id();
        assert_eq!(
            runtime.texture_source_grid(runtime.material(material).texture),
            grid
        );
        let packed_material = packed_runtime
            .resolve(NetworkIdMode::Sequential, id)
            .face(BlockFace::Up)
            .material_id();
        assert_eq!(
            packed_runtime.texture_source_grid(packed_runtime.material(packed_material).texture),
            grid
        );
        assert_eq!(
            material_color(&runtime, material),
            material_color(&packed_runtime, packed_material)
        );
    }
    let world = runtime
        .resolve(NetworkIdMode::Sequential, 0)
        .face(BlockFace::Up)
        .material_id();
    assert_eq!(material_color(&runtime, world), [100, 0, 0, 255]);
    let faces = fallback
        .vanilla_carried
        .iter()
        .find(|entry| entry.visual == 0)
        .unwrap()
        .faces;
    let sheet = render_model::equipment::blocks::overlay_face_sheet(&fallback.overlay, faces, None)
        .unwrap();
    let packed_faces = pack
        .vanilla_carried
        .iter()
        .find(|entry| entry.visual == 0)
        .unwrap()
        .faces;
    let packed_sheet =
        render_model::equipment::blocks::overlay_face_sheet(&pack.overlay, packed_faces, None)
            .unwrap();
    assert_eq!(sheet, packed_sheet);
    assert!(
        sheet
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [200, 0, 0, 255])
    );
    let icon = pack_entity_compiler::overlay_cube_icon(&fallback.overlay, faces, None).unwrap();
    assert!(
        icon.rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .all(|pixel| pixel[1] == 0 && pixel[2] == 0)
    );
}

#[test]
fn overlay_color_carried_bindings_skip_rebinding_without_blocking_raster_updates() {
    let base = carrier(&[[100, 120, 140, 128]], [1; 6], &[0, 0]);
    let raster = carrier(&[[20, 30, 200, 128]], [1; 6], &[0, 0]);
    for carried in [
        json!({"path":"textures/base_a","overlay_color":"#20ff30"}),
        json!({"variations":[{"path":"textures/base_a","overlay_color":"#20ff30","weight":1}]}),
    ] {
        let terrain = json!({"texture_data":{"world":{"textures":"textures/base_a"},"carry":{"textures":carried}}});
        let blocks = json!({"fixture":{"textures":"world","carried_textures":"carry"}});
        for (has_raster, view) in [
            (false, stack(&[(terrain.clone(), blocks.clone(), vec![])])),
            (
                true,
                raster_pack(terrain, blocks, &raster.texture_array().mips[0]),
            ),
        ] {
            let compiled = super::super::super::compile_block_overlay(
                &view,
                &CustomBlocks::default(),
                false,
                Some(&keys()),
                assets::MAX_TEXTURE_LAYERS as u32,
                Some(&base),
            );
            if !has_raster {
                assert!(compiled.is_none());
                continue;
            }
            let compiled = compiled.expect("independent base raster replacement");
            assert!(compiled.overlay.face_overrides.is_empty());
            assert!(
                compiled
                    .vanilla_carried
                    .iter()
                    .all(|route| route.visual != 0)
            );
            assert_eq!(compiled.overlay.material_overrides.len(), 1);
            assert_eq!(compiled.overlay.material_overrides[0].material, 1);
            let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
            assert_eq!(
                runtime.resolve(assets::NetworkIdMode::Sequential, 0),
                base.resolve(assets::NetworkIdMode::Sequential, 0),
            );
            assert_eq!(material_color(&runtime, 1), [20, 30, 200, 128]);
            assert_eq!(runtime.material(1).texture.page(), 1);
            assert_eq!(material_color(&base, 1), [100, 120, 140, 128]);
            assert_eq!(runtime.material(2), base.material(2));
        }
    }
}

#[test]
fn carrier_sources_resolve_explicit_png_and_relative_path_spellings() {
    let pixel = [90, 120, 30, 255];
    let base = carrier(&[pixel], [1; 6], &[0, 0]);
    for path in [
        "textures/base_a.png",
        "./textures/base_a.png",
        "././textures/base_a",
    ] {
        let view = stack(&[(
            json!({"texture_data":{"replacement":{"textures":path}}}),
            json!({"fixture":{"textures":"replacement","carried_textures":"replacement"}}),
            vec![],
        )]);
        let overlay = compile(&view, &base, &keys());
        let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
        for face in BlockFace::ALL {
            let material = runtime
                .resolve(NetworkIdMode::Sequential, 0)
                .face(face)
                .material_id();
            assert_ne!(material, 1);
            assert_eq!(material_color(&runtime, material), pixel);
        }
    }
}

/// A raster path, its width and height, and the colours of its two frames.
type TimelineImage<'a> = (&'a str, u32, u32, [u8; 4], [u8; 4]);

/// Builds original rasters and optional flipbook metadata without using an installed carrier.
fn timeline_pack(
    terrain: Value,
    flipbooks: Value,
    images: &[TimelineImage<'_>],
) -> LayeredPackView {
    timeline_pack_with_bindings(terrain, flipbooks, images, json!({}))
}

/// Adds optional cube bindings to the original raster and flipbook fixture.
fn timeline_pack_with_bindings(
    terrain: Value,
    flipbooks: Value,
    images: &[TimelineImage<'_>],
    blocks: Value,
) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},"modules":[{"type":"resources"}]}).to_string();
    for (path, bytes) in [
        ("manifest.json", manifest.into_bytes()),
        ("blocks.json", blocks.to_string().into_bytes()),
        (
            "textures/terrain_texture.json",
            terrain.to_string().into_bytes(),
        ),
        (
            "textures/flipbook_textures.json",
            flipbooks.to_string().into_bytes(),
        ),
    ] {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    for &(path, width, height, first, second) in images {
        writer
            .start_file(
                format!("{path}.png"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(&png(
                width,
                height,
                |_, y| if y < width { first } else { second },
            ))
            .unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        writer.finish().unwrap().into_inner(),
    );
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ))
}

/// Retains a non-physical B/A/B timeline and an independently named static destination.
fn animated_donor() -> (RuntimeAssets, MaterialKeys) {
    let a = [200, 10, 20, 255];
    let b = [20, 30, 200, 255];
    let mut texture = RuntimeAssets::diagnostic().texture_array().clone();
    texture.layers = 2;
    for mip in &mut texture.mips {
        mip.rgba8 = [a, b]
            .into_iter()
            .flat_map(|color| color.repeat((mip.size * mip.size) as usize))
            .collect::<Vec<_>>()
            .into();
    }
    let refs = [1, 0, 1].map(|layer| assets::TextureRef::new(0, layer).unwrap());
    let animation = assets::Animation {
        frame_start: 0,
        frame_count: 3,
        ticks_per_frame: 2,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 1,
        flags: 0,
    };
    let base = carrier_with_animation(texture, [1; 6], &[0, 0], Some(animation), refs.into());
    let keys = MaterialKeys::from_entries([(1, "anim"), (2, "static")]).with_sources(
        [
            ("anim".into(), json!({"textures":"textures/anim"})),
            ("static".into(), json!({"textures":"textures/static"})),
        ],
        [
            (
                1,
                MaterialTextureSource {
                    key: "anim".into(),
                    variant: 0,
                    path: "textures/anim".into(),
                    raster: "textures/anim.png".into(),
                    baked_tint: None,
                },
            ),
            (
                2,
                MaterialTextureSource {
                    key: "static".into(),
                    variant: 0,
                    path: "textures/static".into(),
                    raster: "textures/static.png".into(),
                    baked_tint: None,
                },
            ),
        ],
        [],
    );
    (base, keys)
}

#[test]
fn raster_only_world_and_carried_routes_reuse_layers_at_the_device_limit() {
    let base = base();
    let keys = keys();
    let colors = [[140, 20, 30, 255], [30, 40, 180, 255]];
    let view = stack(&[(
        json!({}),
        json!({}),
        vec![
            ("textures/base_a", colors[0]),
            ("textures/base_b", colors[1]),
        ],
    )]);
    let compiled = super::super::super::compile_block_overlay(
        &view,
        &CustomBlocks::default(),
        false,
        Some(&keys),
        3,
        Some(&base),
    )
    .unwrap();
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 3);
    assert_eq!(compiled.vanilla_carried.len(), 2);
    for (visual, color) in colors.into_iter().enumerate() {
        let faces = compiled
            .vanilla_carried
            .iter()
            .find(|entry| entry.visual == visual as u32)
            .unwrap()
            .faces;
        let sheet =
            render_model::equipment::blocks::overlay_face_sheet(&compiled.overlay, faces, None)
                .unwrap();
        assert!(
            sheet
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == color)
        );
        assert!(pack_entity_compiler::overlay_cube_icon(&compiled.overlay, faces, None).is_some());
    }
}

#[test]
fn baked_carrier_timelines_are_excluded_instead_of_reapplying_physical_indices() {
    let (base, keys) = animated_donor();
    let terrain = json!({"texture_data":{"anim":{"textures":"textures/anim"}}});
    let flipbooks = json!([{"atlas_tile":"anim","flipbook_texture":"textures/anim","frames":[1,0,1],"ticks_per_frame":2}]);
    let absent = timeline_pack(terrain.clone(), flipbooks.clone(), &[]);
    assert!(
        super::super::super::compile_block_overlay(
            &absent,
            &CustomBlocks::default(),
            false,
            Some(&keys),
            assets::MAX_TEXTURE_LAYERS as u32,
            Some(&base)
        )
        .is_none()
    );
    let present = timeline_pack(
        terrain,
        flipbooks,
        &[(
            "textures/anim",
            16,
            32,
            [200, 10, 20, 255],
            [20, 30, 200, 255],
        )],
    );
    let compiled = compile(&present, &base, &keys);
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    let animation = runtime.animations()[runtime.material(1).animation as usize];
    let frames = &runtime.animation_frames()[animation.frame_start as usize..]
        [..animation.frame_count as usize];
    let colors = frames
        .iter()
        .map(|reference| {
            let mip = &runtime.texture_pages()[reference.page() as usize]
                .texture
                .mips[0];
            let start = (reference.layer() * mip.size * mip.size * 4) as usize;
            mip.rgba8[start..start + 4].try_into().unwrap()
        })
        .collect::<Vec<[u8; 4]>>();
    assert_eq!(
        colors,
        [[20, 30, 200, 255], [200, 10, 20, 255], [20, 30, 200, 255]]
    );
}

#[test]
fn static_destinations_never_inherit_an_animated_donors_timeline() {
    let (base, keys) = animated_donor();
    let terrain = json!({"texture_data":{"static":{"textures":"textures/anim"}}});
    let absent = timeline_pack(terrain.clone(), json!([]), &[]);
    assert!(
        super::super::super::compile_block_overlay(
            &absent,
            &CustomBlocks::default(),
            false,
            Some(&keys),
            assets::MAX_TEXTURE_LAYERS as u32,
            Some(&base)
        )
        .is_none()
    );
    let present = timeline_pack(
        terrain,
        json!([]),
        &[(
            "textures/anim",
            16,
            32,
            [200, 10, 20, 255],
            [20, 30, 200, 255],
        )],
    );
    let compiled = compile(&present, &base, &keys);
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    assert_eq!(runtime.material(2).animation, assets::NO_ANIMATION);
    assert_eq!(material_color(&runtime, 2), [200, 10, 20, 255]);
}

#[test]
fn failed_weighted_selectors_release_pixels_frames_and_cached_slots() {
    for (limit, animated, exhaust_layers) in [(2, false, true), (3, false, false), (3, true, true)]
    {
        for reuse_first in [false, true] {
            let base = base();
            let keys = MaterialKeys::from_entries([(1, "bad"), (2, "good")]).with_sources(
                [
                    ("bad".into(), json!({"textures":"textures/old_bad"})),
                    ("good".into(), json!({"textures":"textures/old_good"})),
                ],
                [
                    (
                        1,
                        MaterialTextureSource {
                            key: "bad".into(),
                            variant: 0,
                            path: "textures/old_bad".into(),
                            raster: "textures/old_bad.png".into(),
                            baked_tint: None,
                        },
                    ),
                    (
                        2,
                        MaterialTextureSource {
                            key: "good".into(),
                            variant: 0,
                            path: "textures/old_good".into(),
                            raster: "textures/old_good.png".into(),
                            baked_tint: None,
                        },
                    ),
                ],
                [],
            );
            let first = [40, 80, 200, 255];
            let good = [200, 30, 40, 255];
            let extra_paths = (0..limit as usize * super::super::super::MAX_OVERLAY_SHARDS)
                .map(|index| format!("textures/extra_{index}"))
                .collect::<Vec<_>>();
            let mut variations = vec![json!({"path":"textures/first","weight":1})];
            if exhaust_layers {
                // Distinct leaves exhaust all shards after the first source has been cached.
                variations.extend(
                    extra_paths
                        .iter()
                        .map(|path| json!({"path":path,"weight":1})),
                );
            } else {
                variations.push(json!({"path":"textures/missing","weight":1}));
            }
            let terrain = json!({"texture_data":{
                "bad":{"textures":{"variations":variations}},
                "good":{"textures":if reuse_first {"textures/first"} else {"textures/good"}}
            }});
            let flipbooks = if animated {
                json!([{"atlas_tile":"bad","atlas_tile_variant":0,"flipbook_texture":"textures/first","frames":[0,1],"ticks_per_frame":2}])
            } else {
                json!([])
            };
            let mut images = vec![
                (
                    "textures/first",
                    256,
                    if animated { 512 } else { 256 },
                    first,
                    if animated { good } else { first },
                ),
                ("textures/good", 16, 16, good, good),
            ];
            if exhaust_layers {
                images.extend(
                    extra_paths
                        .iter()
                        .map(|path| (path.as_str(), 16, 16, good, good)),
                );
            }
            let view = timeline_pack(terrain, flipbooks, &images);
            let compiled = super::super::super::compile_block_overlay(
                &view,
                &CustomBlocks::default(),
                false,
                Some(&keys),
                limit,
                Some(&base),
            )
            .unwrap();
            assert_eq!(compiled.overlay.material_overrides.len(), 1);
            assert_eq!(compiled.overlay.material_overrides[0].material, 2);
            assert_eq!(
                compiled
                    .overlay
                    .textures
                    .iter()
                    .map(|shard| shard.layers)
                    .sum::<u32>(),
                2
            );
            let replacement = compiled.overlay.material_overrides[0].texture;
            let shard = &compiled.overlay.textures[replacement.page() as usize - 1];
            assert!(replacement.layer() < shard.layers);
            assert_eq!(shard.mips[0].size, if reuse_first { 256 } else { 16 });
            assert!(compiled.overlay.animations.is_empty());
            assert!(compiled.overlay.animation_frames.is_empty());
            let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
            assert_eq!(
                material_color(&runtime, 2),
                if reuse_first { first } else { good }
            );
            assert_eq!(runtime.material(1), base.material(1));
        }
    }
}

mod catalog_repeats;
mod resolved_images;
mod review_regressions;

mod weighted_replacements;

mod filename_precedence;
