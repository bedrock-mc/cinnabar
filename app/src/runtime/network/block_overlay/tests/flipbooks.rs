use super::*;
use assets::texture::flipbook::FlipbookSource;

/// Compiles an overlay without a carrier donor and with an explicit physical layer limit.
fn compile_with_limit(
    view: &LayeredPackView,
    blocks: &CustomBlocks,
    hashed: bool,
    keys: Option<&assets::material_keys::MaterialKeys>,
    layers: u32,
) -> Option<super::super::CompiledBlockOverlay> {
    super::super::compile_block_overlay(view, blocks, hashed, keys, layers, None)
}

/// Admits a pack containing only the synthetic files named by the test.
fn pack(files: &[(&str, &[u8])]) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000019";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                writer.finish().unwrap().into_inner(),
            ),
        ]),
    ))
}

/// Returns a frame's first pixel from the compiled overlay's physical page.
fn pixel(overlay: &assets::BlockOverlay, frame: assets::TextureRef) -> [u8; 4] {
    let mip = &overlay.textures[frame.page() as usize - 1].mips[0];
    let start = frame.layer() as usize * (mip.size * mip.size * 4) as usize;
    mip.rgba8[start..start + 4].try_into().unwrap()
}

/// Builds a tiny immutable carrier with one base animation and a replaceable material.
fn animated_base() -> Arc<RuntimeAssets> {
    use assets::*;
    let diagnostic = RuntimeAssets::diagnostic();
    let compiled = CompiledAssets {
        visuals: vec![BlockVisual::diagnostic(
            BlockFlags::empty(),
            ContributorRole::Primary,
        )]
        .into(),
        light_properties: vec![LightProperties::default()].into(),
        hashed: Box::new([]),
        materials: vec![
            diagnostic.material(0),
            Material {
                animation: 0,
                ..diagnostic.material(0)
            },
        ]
        .into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: vec![Animation {
            frame_start: 0,
            frame_count: 2,
            ticks_per_frame: 7,
            atlas_index: 0,
            atlas_tile_variant: 0,
            replicate: 1,
            flags: 0,
        }]
        .into(),
        animation_frames: vec![TextureRef::DIAGNOSTIC; 2].into(),
        texture_pages: diagnostic.texture_pages().to_vec().into(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    Arc::new(RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap())
}

#[test]
fn raster_only_pack_replaces_all_base_frames_and_removal_restores_the_carrier() {
    let source: FlipbookSource = serde_json::from_value(serde_json::json!({
        "flipbook_texture":"textures/water", "atlas_tile":"water",
        "frames":[1,0,1], "ticks_per_frame":7, "blend_frames":false
    }))
    .unwrap();
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "water")])
        .with_aliases([("water", "textures/water")])
        .with_flipbooks(&[source]);
    let strip = png(16, 32, |_, y| {
        if y < 16 {
            [250, 20, 10, 255]
        } else {
            [10, 30, 240, 255]
        }
    });
    let compiled = Arc::new(
        compile_block_overlay(
            &pack(&[("textures/water.png", &strip)]),
            &CustomBlocks::default(),
            false,
            Some(&keys),
        )
        .unwrap(),
    );
    let animation = &compiled.overlay.animations[0];
    assert_eq!(
        (
            animation.frame_count,
            animation.ticks_per_frame,
            animation.flags
        ),
        (3, 7, 0)
    );
    assert_eq!(
        compiled
            .overlay
            .animation_frames
            .iter()
            .map(|&frame| pixel(&compiled.overlay, frame))
            .collect::<Vec<_>>(),
        [[10, 30, 240, 255], [250, 20, 10, 255], [10, 30, 240, 255]]
    );
    let base = animated_base();
    let session =
        super::super::super::resource_packs::session_runtime_assets(&base, None, Some(&compiled));
    assert_eq!(session.material(1).texture.page(), 1);
    assert_eq!(session.material(1).animation, 1);
    assert_eq!(
        session.animations()[1].ticks_per_frame,
        base.animations()[0].ticks_per_frame
    );

    let removed = compile_block_overlay(&pack(&[]), &CustomBlocks::default(), false, Some(&keys));
    assert!(removed.is_none());
    let restored = super::super::super::resource_packs::session_runtime_assets(&base, None, None);
    assert!(Arc::ptr_eq(&restored, &base));
    assert_eq!(restored.material(1).animation, 0);
    assert_eq!(restored.material(1).texture.page(), 0);
}

#[test]
fn sixteen_static_atlas_indices_share_one_layer_at_the_device_limit() {
    let keys = assets::material_keys::MaterialKeys::from_entries(
        (1..=16).map(|material| (material, "mushroom")),
    )
    .with_aliases([("mushroom", "textures/static")])
    .with_flipbook_selectors((0..16).map(|index| (index + 1, (index, 0))))
    .with_flipbook_destinations([("mushroom", [1; 16])]);
    let raster = png(16, 16, |_, _| [180, 90, 30, 255]);
    let compiled = compile_with_limit(
        &pack(&[("textures/static.png", &raster)]),
        &CustomBlocks::default(),
        false,
        Some(&keys),
        2,
    )
    .unwrap();
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 2);
    assert_eq!(compiled.gaps.saturated_textures, 0);
    assert!(compiled.overlay.animations.is_empty());
    assert_eq!(compiled.overlay.material_overrides.len(), 16);
    let texture = compiled.overlay.material_overrides[0].texture;
    assert_eq!(pixel(&compiled.overlay, texture), [180, 90, 30, 255]);
    for (index, replacement) in compiled.overlay.material_overrides.iter().enumerate() {
        assert_eq!(replacement.material, index as u32 + 1);
        assert_eq!(
            keys.flipbook_selector(replacement.material),
            (index as u32, 0)
        );
        assert_eq!(replacement.texture, texture);
        assert_eq!(replacement.animation, assets::NO_ANIMATION);
    }
}

#[test]
fn missing_flipbook_rasters_reuse_static_fallback_for_every_selector_at_capacity() {
    let sources = (0..3)
        .map(|index| {
            serde_json::from_value::<FlipbookSource>(serde_json::json!({
                "atlas_tile":"tile", "atlas_index":index,
                "flipbook_texture":format!("textures/absent_{index}")
            }))
            .unwrap()
        })
        .collect::<Vec<_>>();
    let keys = assets::material_keys::MaterialKeys::from_entries(
        (1..=3).map(|material| (material, "tile")),
    )
    .with_aliases([("tile", "textures/static")])
    .with_flipbooks(&sources)
    .with_flipbook_selectors((0..3).map(|index| (index + 1, (index, 0))))
    .with_flipbook_destinations([("tile", [1; 3])]);
    let raster = png(16, 16, |_, _| [180, 90, 30, 255]);
    let view = pack(&[("textures/static.png", &raster)]);
    let catalog = super::super::textures::TextureCatalog::new(&view, Some(&keys));
    for index in 0..3 {
        assert!(catalog.flipbook("tile", (index, 0)).is_some());
    }
    let compiled =
        compile_with_limit(&view, &CustomBlocks::default(), false, Some(&keys), 2).unwrap();
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 2);
    assert_eq!(compiled.gaps.saturated_textures, 0);
    assert!(compiled.overlay.animations.is_empty());
    assert_eq!(compiled.overlay.material_overrides.len(), sources.len());
    let texture = compiled.overlay.material_overrides[0].texture;
    assert_eq!(pixel(&compiled.overlay, texture), [180, 90, 30, 255]);
    for (index, replacement) in compiled.overlay.material_overrides.iter().enumerate() {
        assert_eq!(replacement.material, index as u32 + 1);
        assert_eq!(replacement.texture, texture);
        assert_eq!(replacement.animation, assets::NO_ANIMATION);
    }
}

#[test]
fn independent_sources_and_full_selectors_keep_distinct_replicated_frames() {
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile"), (2, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbook_selectors([(1, (1, 0)), (2, (1, 1))])
        .with_flipbook_destinations([("tile", [1, 2])]);
    let first = png(32, 64, |x, y| {
        if x < 16 && y % 32 < 16 {
            if y < 32 {
                [230, 10, 20, 255]
            } else {
                [20, 10, 230, 255]
            }
        } else {
            [0, 0, 0, 255]
        }
    });
    let second = png(32, 64, |x, y| {
        if x < 16 && y % 32 < 16 {
            if y < 32 {
                [230, 210, 20, 255]
            } else {
                [20, 210, 230, 255]
            }
        } else {
            [0, 0, 0, 255]
        }
    });
    let static_tile = png(16, 16, |_, _| [0, 180, 0, 255]);
    let descriptions = br#"[
        {"atlas_tile":"tile","atlas_index":1,"atlas_tile_variant":0,"flipbook_texture":"textures/first","replicate":2,"frames":[1,0],"ticks_per_frame":3},
        {"atlas_tile":"tile","atlas_index":1,"atlas_tile_variant":1,"flipbook_texture":"textures/second","replicate":2,"ticks_per_frame":5,"blend_frames":false}
    ]"#;
    let compiled = compile_block_overlay(
        &pack(&[
            ("textures/static.png", &static_tile),
            ("textures/first.png", &first),
            ("textures/second.png", &second),
            ("textures/flipbook_textures.json", descriptions),
        ]),
        &CustomBlocks::default(),
        false,
        Some(&keys),
    )
    .unwrap();
    assert_eq!(compiled.overlay.material_overrides.len(), 2);
    for (index, expected) in [[20, 10, 230, 255], [230, 210, 20, 255]]
        .into_iter()
        .enumerate()
    {
        let replacement = &compiled.overlay.material_overrides[index];
        let animation = &compiled.overlay.animations[replacement.animation as usize];
        assert_eq!(
            (
                animation.atlas_index,
                animation.atlas_tile_variant,
                animation.replicate
            ),
            (1, index as u32, 2)
        );
        assert_eq!(animation.ticks_per_frame, if index == 0 { 3 } else { 5 });
        assert_eq!(
            animation.flags,
            if index == 0 {
                assets::ANIMATION_FLAG_BLEND
            } else {
                0
            }
        );
        assert_eq!(pixel(&compiled.overlay, replacement.texture), expected);
        assert_eq!(
            compiled.overlay.texture_source_sizes[replacement.texture.layer() as usize],
            [16, 16]
        );
    }
}

#[test]
fn omitted_variant_reaches_every_destination_without_crossing_atlas_indices() {
    let view = pack(&[(
        "textures/flipbook_textures.json",
        br#"[
        {"atlas_tile":"tile","atlas_index":1,"flipbook_texture":"textures/strip"}
    ]"#,
    )]);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbook_destinations([("tile", [1, 3])]);
    let catalog = super::super::textures::TextureCatalog::new(&view, Some(&keys));
    assert!(catalog.flipbook("tile", (1, 0)).is_some());
    assert!(catalog.flipbook("tile", (1, 2)).is_some());
    assert!(catalog.flipbook("tile", (0, 0)).is_none());
}

#[test]
fn explicit_frames_clamp_to_the_last_physical_frame_before_replication() {
    let source: FlipbookSource = serde_json::from_value(serde_json::json!({
        "atlas_tile":"tile", "flipbook_texture":"textures/strip", "frames":[9,0], "replicate":2
    }))
    .unwrap();
    let strip = pack_runtime::pack_textures::DecodedTexture {
        width: 4,
        height: 8,
        rgba8: (0..8)
            .flat_map(|y| (0..4).flat_map(move |x| [x, y, 9, 255]))
            .collect(),
    };
    let frames = super::super::textures::flipbook_frames(&strip, &source, 2, 16);
    assert_eq!((frames.frames[0].width, frames.frames[0].height), (2, 2));
    assert_eq!(&frames.frames[0].rgba8[..4], &[0, 4, 9, 255]);
    assert_eq!(&frames.frames[1].rgba8[..4], &[0, 0, 9, 255]);
}

#[test]
fn invalid_uv_variants_fall_back_in_creation_order_and_invalid_indices_skip() {
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbook_destinations([("tile", [2])]);
    let view = pack(&[(
        "textures/flipbook_textures.json",
        br#"[
        {"atlas_tile":"tile","atlas_tile_variant":9,"flipbook_texture":"textures/first"},
        {"atlas_tile":"tile","atlas_tile_variant":4,"flipbook_texture":"textures/second"},
        {"atlas_tile":"tile","atlas_index":1,"flipbook_texture":"textures/invalid"}
    ]"#,
    )]);
    let catalog = super::super::textures::TextureCatalog::new(&view, Some(&keys));
    assert_eq!(
        catalog
            .flipbook("tile", (0, 0))
            .unwrap()
            .texture_path
            .as_ref(),
        "textures/second"
    );
    assert!(catalog.flipbook("tile", (0, 1)).is_none());
    assert_eq!(catalog.normalize_selector("tile", (1, 0)), (0, 0));
    assert_eq!(
        catalog
            .flipbook("tile", (1, 0))
            .unwrap()
            .texture_path
            .as_ref(),
        "textures/second",
        "an invalid authored index must not replace the valid description"
    );
}

#[test]
fn scalar_terrain_replacement_remaps_indexed_materials_to_the_valid_animation() {
    let keys =
        assets::material_keys::MaterialKeys::from_entries([(1, "barrel_top"), (2, "barrel_top")])
            .with_aliases([("barrel_top", "textures/base")])
            .with_flipbook_selectors([(1, (0, 0)), (2, (1, 1))])
            .with_flipbook_destinations([("barrel_top", [2, 2])]);
    let strip = png(16, 32, |_, y| {
        if y < 16 {
            [230, 10, 20, 255]
        } else {
            [20, 10, 230, 255]
        }
    });
    let raster = png(16, 16, |_, _| [0, 180, 0, 255]);
    let view = pack(&[
        ("textures/static.png", &raster),
        ("textures/strip.png", &strip),
        ("textures/terrain_texture.json", br#"{"texture_data":{"barrel_top":{"textures":"textures/static"}}}"#),
        ("textures/flipbook_textures.json", br#"[
            {"atlas_tile":"barrel_top","atlas_index":0,"flipbook_texture":"textures/strip","ticks_per_frame":7},
            {"atlas_tile":"barrel_top","atlas_index":1,"flipbook_texture":"textures/strip","ticks_per_frame":11}
        ]"#),
    ]);
    let compiled =
        compile_block_overlay(&view, &CustomBlocks::default(), false, Some(&keys)).unwrap();
    assert_eq!(compiled.overlay.material_overrides.len(), 2);
    for replacement in &compiled.overlay.material_overrides {
        assert_ne!(replacement.animation, assets::NO_ANIMATION);
        let animation = &compiled.overlay.animations[replacement.animation as usize];
        assert_eq!(
            (animation.atlas_index, animation.atlas_tile_variant),
            (0, 0)
        );
        assert_eq!((animation.frame_count, animation.ticks_per_frame), (2, 7));
        assert_eq!(
            pixel(&compiled.overlay, replacement.texture),
            [230, 10, 20, 255]
        );
    }
    assert_eq!(keys.flipbook_selector(2), (1, 1));
}

#[test]
fn matching_animation_frames_share_storage_at_capacity_with_separate_destination_metadata() {
    let keys =
        assets::material_keys::MaterialKeys::from_entries([(1, "tile"), (2, "tile"), (3, "alias")])
            .with_aliases([("tile", "textures/static"), ("alias", "textures/static")])
            .with_flipbook_selectors([(1, (0, 0)), (2, (0, 1)), (3, (1, 0))])
            .with_flipbook_destinations([("tile", vec![2]), ("alias", vec![1, 1])]);
    let strip = png(16, 32, |_, y| {
        if y < 16 {
            [230, 10, 20, 255]
        } else {
            [20, 10, 230, 255]
        }
    });
    let view = pack(&[
        ("textures/strip.png", &strip),
        ("textures/flipbook_textures.json", br#"[
            {"atlas_tile":"tile","flipbook_texture":"textures/strip","frames":[1,0],"ticks_per_frame":3},
            {"atlas_tile":"alias","atlas_index":1,"flipbook_texture":"textures/strip","frames":[1,0],"ticks_per_frame":5,"blend_frames":false}
        ]"#),
    ]);
    let compiled =
        compile_with_limit(&view, &CustomBlocks::default(), false, Some(&keys), 3).unwrap();
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 3);
    assert_eq!(compiled.gaps.saturated_textures, 0);
    assert_eq!(compiled.overlay.material_overrides.len(), 3);
    assert_eq!(compiled.overlay.animations.len(), 3);
    assert_eq!(compiled.overlay.animation_frames.len(), 2);
    let texture = compiled.overlay.material_overrides[0].texture;
    for replacement in &compiled.overlay.material_overrides {
        assert_eq!(replacement.texture, texture);
        assert_ne!(replacement.animation, assets::NO_ANIMATION);
        let animation = &compiled.overlay.animations[replacement.animation as usize];
        assert_eq!(
            (animation.atlas_index, animation.atlas_tile_variant),
            keys.flipbook_selector(replacement.material)
        );
        assert_eq!(
            animation.ticks_per_frame,
            if replacement.material == 3 { 5 } else { 3 }
        );
        assert_eq!(
            animation.flags,
            if replacement.material == 3 {
                0
            } else {
                assets::ANIMATION_FLAG_BLEND
            }
        );
        let start = animation.frame_start as usize;
        let end = start + animation.frame_count as usize;
        assert_eq!(
            compiled.overlay.animation_frames[start..end]
                .iter()
                .map(|&frame| pixel(&compiled.overlay, frame))
                .collect::<Vec<_>>(),
            [[20, 10, 230, 255], [230, 10, 20, 255]]
        );
    }
}

#[test]
fn relative_flipbook_paths_keep_animation_and_share_canonical_storage() {
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile"), (2, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbook_selectors([(1, (0, 0)), (2, (0, 1))])
        .with_flipbook_destinations([("tile", [2])]);
    let strip = png(256, 512, |_, y| {
        if y < 256 {
            [230, 10, 20, 255]
        } else {
            [20, 10, 230, 255]
        }
    });
    let raster = png(16, 16, |_, _| [0, 180, 0, 255]);
    let view = pack(&[
        ("textures/static.png", &raster),
        ("textures/strip.png", &strip),
        ("textures/flipbook_textures.json", br#"[
            {"atlas_tile":"tile","atlas_tile_variant":0,"flipbook_texture":" ././textures/strip ","frames":[1,0],"ticks_per_frame":4},
            {"atlas_tile":"tile","atlas_tile_variant":1,"flipbook_texture":"textures/strip","frames":[1,0],"ticks_per_frame":6}
        ]"#),
    ]);
    let compiled =
        compile_with_limit(&view, &CustomBlocks::default(), false, Some(&keys), 3).unwrap();
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 3);
    assert_eq!(compiled.overlay.textures.first().unwrap().mips[0].size, 256);
    assert_eq!(compiled.gaps.saturated_textures, 0);
    assert_eq!(compiled.overlay.material_overrides.len(), 2);
    let texture = compiled.overlay.material_overrides[0].texture;
    for replacement in &compiled.overlay.material_overrides {
        assert_eq!(replacement.texture, texture);
        assert_eq!(
            compiled.overlay.texture_source_sizes[texture.layer() as usize],
            [256; 2]
        );
        assert_ne!(replacement.animation, assets::NO_ANIMATION);
        let animation = &compiled.overlay.animations[replacement.animation as usize];
        assert_eq!(animation.frame_count, 2);
        assert_eq!(
            animation.ticks_per_frame,
            if replacement.material == 1 { 4 } else { 6 }
        );
        assert_eq!(animation.atlas_tile_variant, replacement.material - 1);
        let start = animation.frame_start as usize;
        assert_eq!(
            compiled.overlay.animation_frames[start..start + 2]
                .iter()
                .map(|&frame| pixel(&compiled.overlay, frame))
                .collect::<Vec<_>>(),
            [[20, 10, 230, 255], [230, 10, 20, 255]]
        );
    }
}

#[test]
fn full_arrays_reuse_shared_frames_without_reading_uncached_image_payloads() {
    let fillers = (0..9)
        .map(|index| format!("b_fill{index}"))
        .collect::<Vec<_>>();
    let keys = assets::material_keys::MaterialKeys::from_entries(
        [
            (1, "a_fill"),
            (2, "a_fill"),
            (12, "z_static"),
            (13, "y_animated"),
        ]
        .into_iter()
        .map(|(id, key)| (id, key.to_owned()))
        .chain(
            fillers
                .iter()
                .enumerate()
                .map(|(index, key)| (index as u32 + 3, key.clone())),
        ),
    )
    .with_aliases(
        [
            ("a_fill", "textures/strip"),
            ("z_static", "textures/overflow_static"),
            ("y_animated", "textures/overflow_fallback"),
        ]
        .into_iter()
        .map(|(key, path)| (key.to_owned(), path.to_owned()))
        .chain(
            fillers
                .iter()
                .map(|key| (key.clone(), format!("textures/{key}"))),
        ),
    )
    .with_flipbook_selectors([(1, (0, 0)), (2, (0, 1))])
    .with_flipbook_destinations([
        ("a_fill", vec![2]),
        ("z_static", vec![1]),
        ("y_animated", vec![1]),
    ]);
    let strip = png(16, 32, |_, y| {
        if y < 16 {
            [230, 10, 20, 255]
        } else {
            [20, 10, 230, 255]
        }
    });
    let raster = png(16, 16, |_, _| [0, 180, 0, 255]);
    let filler_paths = fillers
        .iter()
        .map(|key| format!("textures/{key}.png"))
        .collect::<Vec<_>>();
    let mut files = vec![
        ("textures/strip.png", strip.as_slice()),
        ("textures/overflow_static.png", raster.as_slice()),
        ("textures/overflow_animated.png", strip.as_slice()),
        ("textures/overflow_fallback.png", raster.as_slice()),
        (
            "textures/flipbook_textures.json",
            br#"[
            {"atlas_tile":"a_fill","flipbook_texture":"textures/strip"},
            {"atlas_tile":"y_animated","flipbook_texture":"textures/overflow_animated"}
        ]"#
            .as_slice(),
        ),
    ];
    files.extend(
        filler_paths
            .iter()
            .map(|path| (path.as_str(), raster.as_slice())),
    );
    let stack = pack(&files);
    let view = LayeredPackView::tracked(stack.shared_stack());
    let compiled =
        compile_with_limit(&view, &CustomBlocks::default(), false, Some(&keys), 3).unwrap();
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 3);
    assert_eq!(compiled.gaps.saturated_textures, 2);
    assert_eq!(compiled.overlay.material_overrides.len(), 2 + fillers.len());
    assert_eq!(
        compiled.overlay.textures.len(),
        super::super::MAX_OVERLAY_SHARDS
    );
    assert!(
        compiled
            .overlay
            .textures
            .iter()
            .all(|shard| shard.layers == 3)
    );
    for (index, replacement) in compiled.overlay.material_overrides[..2].iter().enumerate() {
        assert_eq!(replacement.material, index as u32 + 1);
        assert_ne!(replacement.animation, assets::NO_ANIMATION);
        assert_eq!(
            replacement.texture,
            compiled.overlay.material_overrides[0].texture
        );
    }
    let inputs = view.dependencies().unwrap().snapshot();
    assert!(inputs.iter().any(|input| matches!(input,
        resource_pack::PackDependency::File { path, limit }
            if path == "textures/strip.png" && *limit == resource_pack::MAX_PACK_TEXTURE_BYTES
    )));
    assert!(!inputs.iter().any(|input| matches!(input,
        resource_pack::PackDependency::File { path, limit }
            if path.starts_with("textures/overflow_") && *limit == resource_pack::MAX_PACK_TEXTURE_BYTES
    )), "overflow routes must not read image payloads after capacity is exhausted");
}

#[test]
fn animation_failures_keep_static_visuals_and_cached_overrides() {
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile"), (2, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbook_selectors([(1, (0, 0)), (2, (0, 1))])
        .with_flipbook_destinations([("tile", [2])]);
    let blocks = CustomBlocks {
        blocks: vec![block(
            "test:tile",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("minecraft:geometry.full_block".into()),
                    materials: materials("tile"),
                    ..Default::default()
                },
                ..Default::default()
            },
        )]
        .into(),
        ..Default::default()
    };
    let raster = png(16, 16, |_, _| [11, 17, 23, 255]);
    for (width, height, ticks) in [(48, 32, 2), (32, 48, 2), (16, 32, u32::MAX)] {
        let invalid = png(width, height, |_, _| [200, 100, 0, 255]);
        let description = serde_json::to_vec(&serde_json::json!([{
            "atlas_tile":"tile", "flipbook_texture":"textures/invalid", "ticks_per_frame":ticks
        }]))
        .unwrap();
        // Keep both source frames available when checking timeline overflow.
        let layers = if ticks == u32::MAX { 4 } else { 2 };
        let compiled = compile_with_limit(
            &pack(&[
                ("textures/static.png", &raster),
                ("textures/invalid.png", &invalid),
                ("textures/flipbook_textures.json", &description),
            ]),
            &blocks,
            false,
            Some(&keys),
            layers,
        )
        .unwrap();
        assert_eq!(compiled.gaps.missing_textures, 0);
        assert_eq!(compiled.gaps.saturated_textures, 0);
        assert_eq!(compiled.overlay.textures.first().unwrap().layers, 2);
        assert!(compiled.overlay.animations.is_empty());
        assert_eq!(compiled.overlay.material_overrides.len(), 2);
        let texture = compiled.overlay.material_overrides[0].texture;
        assert_eq!(pixel(&compiled.overlay, texture), [11, 17, 23, 255]);
        for replacement in &compiled.overlay.material_overrides {
            assert_eq!(replacement.texture, texture);
            assert_eq!(replacement.animation, assets::NO_ANIMATION);
        }
        for &id in &compiled.overlay.visuals[0].faces {
            assert_ne!(id, 0);
            assert_eq!(compiled.overlay.materials[id as usize].texture, texture);
        }
    }
}

#[test]
fn wide_horizontal_strips_keep_every_frame_and_share_them_across_selectors() {
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile"), (2, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbook_selectors([(1, (0, 0)), (2, (0, 1))])
        .with_flipbook_destinations([("tile", [2])]);
    let strip = png(8192, 64, |x, y| {
        [(x / 64) as u8, (x % 64) as u8, y as u8, 255]
    });
    let raster = png(16, 16, |_, _| [0, 180, 0, 255]);
    let compiled = compile_with_limit(
        &pack(&[("textures/static.png", &raster), ("textures/horizontal.png", &strip),
            ("textures/flipbook_textures.json", br#"[{"atlas_tile":"tile","flipbook_texture":"textures/horizontal","ticks_per_frame":7}]"#)]),
        &CustomBlocks::default(), false, Some(&keys), 129,
    ).unwrap();
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 129);
    assert_eq!(compiled.overlay.textures.first().unwrap().mips[0].size, 64);
    assert_eq!(compiled.gaps.saturated_textures, 0);
    assert_eq!(compiled.overlay.material_overrides.len(), 2);
    assert_eq!(compiled.overlay.animation_frames.len(), 128);
    for replacement in &compiled.overlay.material_overrides {
        assert_ne!(replacement.animation, assets::NO_ANIMATION);
        let animation = &compiled.overlay.animations[replacement.animation as usize];
        assert_eq!((animation.frame_count, animation.ticks_per_frame), (128, 7));
        assert_eq!(animation.atlas_tile_variant, replacement.material - 1);
        let frames = &compiled.overlay.animation_frames[animation.frame_start as usize..][..128];
        for (index, &frame) in frames.iter().enumerate() {
            assert_eq!(pixel(&compiled.overlay, frame), [index as u8, 0, 0, 255]);
            assert_eq!(
                compiled.overlay.texture_source_sizes[frame.layer() as usize],
                [64; 2]
            );
        }
    }
}

#[test]
fn a_later_all_variant_description_overwrites_an_earlier_exact_destination() {
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbook_destinations([("tile", [2])]);
    let view = pack(&[(
        "textures/flipbook_textures.json",
        br#"[
        {"atlas_tile":"tile","atlas_tile_variant":1,"flipbook_texture":"textures/exact"},
        {"atlas_tile":"tile","flipbook_texture":"textures/all"}
    ]"#,
    )]);
    let catalog = super::super::textures::TextureCatalog::new(&view, Some(&keys));
    assert_eq!(
        catalog
            .flipbook("tile", (0, 1))
            .unwrap()
            .texture_path
            .as_ref(),
        "textures/all"
    );
}

#[test]
fn raster_only_replacements_normalize_nonzero_carrier_selectors() {
    for index in 1..=4 {
        let source: FlipbookSource = serde_json::from_value(serde_json::json!({
            "flipbook_texture":"textures/charged", "atlas_tile":"top",
            "atlas_index":index, "atlas_tile_variant":index,
            "frames":[1,0], "ticks_per_frame":9, "blend_frames":false
        }))
        .unwrap();
        let keys = assets::material_keys::MaterialKeys::from_entries([(1, "top")])
            .with_aliases([("top", "textures/charged")])
            .with_flipbooks(&[source])
            .with_flipbook_selectors([(1, (index, index))])
            .with_flipbook_destinations([("top", [1; 5])]);
        let strip = png(16, 32, |_, y| {
            if y < 16 {
                [250, 20, 10, 255]
            } else {
                [10, 30, 240, 255]
            }
        });
        let compiled = compile_block_overlay(
            &pack(&[("textures/charged.png", &strip)]),
            &CustomBlocks::default(),
            false,
            Some(&keys),
        )
        .unwrap();
        let replacement = &compiled.overlay.material_overrides[0];
        assert_ne!(
            replacement.animation,
            assets::NO_ANIMATION,
            "atlas index {index}"
        );
        let animation = &compiled.overlay.animations[replacement.animation as usize];
        assert_eq!(
            (animation.atlas_index, animation.atlas_tile_variant),
            (index, 0)
        );
        assert_eq!((animation.frame_count, animation.ticks_per_frame), (2, 9));
        assert_eq!(
            pixel(&compiled.overlay, replacement.texture),
            [10, 30, 240, 255]
        );
        assert_eq!(keys.flipbook_selector(1), (index, index));
    }
}

#[test]
fn variant_zero_override_keeps_the_other_variants_base_animation() {
    let source: FlipbookSource = serde_json::from_value(serde_json::json!({
        "flipbook_texture":"textures/base", "atlas_tile":"tile", "all_variants":true,
        "frames":[1,0], "ticks_per_frame":7, "blend_frames":false
    }))
    .unwrap();
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile"), (2, "tile")])
        .with_aliases([("tile", "textures/base")])
        .with_flipbooks(&[source])
        .with_flipbook_selectors([(1, (0, 0)), (2, (0, 1))])
        .with_flipbook_destinations([("tile", [2])]);
    let base = png(16, 32, |_, y| {
        if y < 16 {
            [20, 180, 10, 255]
        } else {
            [10, 80, 20, 255]
        }
    });
    let upper = png(16, 32, |_, y| {
        if y < 16 {
            [250, 20, 10, 255]
        } else {
            [10, 30, 240, 255]
        }
    });
    let view = pack(&[
        ("textures/base.png", &base), ("textures/upper.png", &upper),
        ("textures/flipbook_textures.json", br#"[{
            "atlas_tile":"tile", "atlas_tile_variant":0, "flipbook_texture":"textures/upper", "ticks_per_frame":3
        }]"#),
    ]);
    let compiled =
        compile_block_overlay(&view, &CustomBlocks::default(), false, Some(&keys)).unwrap();
    for (replacement, (ticks, color)) in compiled
        .overlay
        .material_overrides
        .iter()
        .zip([(3, [10, 30, 240, 255]), (7, [10, 80, 20, 255])])
    {
        assert_ne!(replacement.animation, assets::NO_ANIMATION);
        let animation = &compiled.overlay.animations[replacement.animation as usize];
        assert_eq!(
            (
                animation.frame_count,
                animation.ticks_per_frame,
                animation.flags
            ),
            (2, ticks, 0)
        );
        assert_eq!(pixel(&compiled.overlay, replacement.texture), color);
    }
    assert_eq!(compiled.overlay.material_overrides.len(), 2);
}

#[test]
fn horizontal_strip_replacement_retains_both_frames_in_timeline_order() {
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/strip")]);
    let strip = png(32, 16, |x, _| {
        if x < 16 {
            [250, 20, 10, 255]
        } else {
            [10, 30, 240, 255]
        }
    });
    let view = pack(&[
        ("textures/strip.png", &strip),
        ("textures/flipbook_textures.json", br#"[{
            "atlas_tile":"tile", "flipbook_texture":"textures/strip", "frames":[1,0], "ticks_per_frame":9
        }]"#),
    ]);
    let compiled =
        compile_block_overlay(&view, &CustomBlocks::default(), false, Some(&keys)).unwrap();
    let animation = &compiled.overlay.animations[0];
    assert_eq!((animation.frame_count, animation.ticks_per_frame), (2, 9));
    let mip = &compiled.overlay.textures.first().unwrap().mips[0];
    let bytes_per_frame = (mip.size * mip.size * 4) as usize;
    for (&frame, color) in compiled
        .overlay
        .animation_frames
        .iter()
        .zip([[10, 30, 240, 255], [250, 20, 10, 255]])
    {
        let start = frame.layer() as usize * bytes_per_frame;
        assert!(
            mip.rgba8[start..start + bytes_per_frame]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == color)
        );
    }
    assert_ne!(
        compiled.overlay.material_overrides[0].animation,
        assets::NO_ANIMATION
    );
}

#[test]
fn repeated_square_frames_leave_capacity_for_another_texture() {
    for frames in [
        vec![0, 0],
        vec![0; assets::texture::flipbook::MAX_FLIPBOOK_FRAMES],
    ] {
        let keys =
            assets::material_keys::MaterialKeys::from_entries([(1, "a_square"), (2, "b_other")])
                .with_aliases([
                    ("a_square", "textures/square"),
                    ("b_other", "textures/other"),
                ]);
        let square = png(16, 16, |_, _| [200, 10, 20, 255]);
        let other = png(16, 16, |_, _| [10, 200, 20, 255]);
        let description = serde_json::to_vec(&serde_json::json!([{
            "atlas_tile":"a_square","flipbook_texture":"textures/square","frames":frames,"ticks_per_frame":3
        }])).unwrap();
        let view = pack(&[
            ("textures/square.png", &square),
            ("textures/other.png", &other),
            ("textures/flipbook_textures.json", &description),
        ]);
        let compiled =
            compile_with_limit(&view, &CustomBlocks::default(), false, Some(&keys), 3).unwrap();
        assert_eq!(compiled.overlay.textures.first().unwrap().layers, 3);
        assert_eq!(compiled.overlay.material_overrides.len(), 2);
        let square = compiled
            .overlay
            .material_overrides
            .iter()
            .find(|replacement| replacement.material == 1)
            .unwrap();
        let other = compiled
            .overlay
            .material_overrides
            .iter()
            .find(|replacement| replacement.material == 2)
            .unwrap();
        assert_eq!(pixel(&compiled.overlay, square.texture), [200, 10, 20, 255]);
        assert_eq!(pixel(&compiled.overlay, other.texture), [10, 200, 20, 255]);
        let animation = &compiled.overlay.animations[square.animation as usize];
        assert_eq!(animation.frame_count as usize, frames.len());
        assert_eq!(animation.ticks_per_frame, 3);
        assert!(
            compiled
                .overlay
                .animation_frames
                .iter()
                .all(|&frame| frame == square.texture)
        );
    }
}
