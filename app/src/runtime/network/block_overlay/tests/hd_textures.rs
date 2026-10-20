use super::*;

/// Builds original terrain detail patterns and explicitly animated strips in a tiny pack.
fn fixture(images: &[(&str, u32, u32)]) -> (LayeredPackView, CustomBlocks) {
    let id = "00000000-0000-0000-0000-000000000003";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let terrain: serde_json::Map<_, _> = images
        .iter()
        .map(|(key, _, _)| {
            (
                key.to_string(),
                serde_json::json!({"textures": format!("textures/blocks/{key}")}),
            )
        })
        .collect();
    let flipbooks: Vec<_> = images.iter().filter(|(_, width, height)| height > width)
        .map(|(key, _, _)| serde_json::json!({"atlas_tile":key,"flipbook_texture":format!("textures/blocks/{key}"),"ticks_per_frame":2})).collect();
    let mut files = vec![
        ("manifest.json".to_owned(), manifest.into_bytes()),
        (
            "textures/terrain_texture.json".to_owned(),
            serde_json::to_vec(&serde_json::json!({"texture_data":terrain})).unwrap(),
        ),
        (
            "textures/flipbook_textures.json".to_owned(),
            serde_json::to_vec(&flipbooks).unwrap(),
        ),
    ];
    for &(key, width, height) in images {
        files.push((
            format!("textures/blocks/{key}.png"),
            png(width, height, |x, y| {
                [
                    if x % 2 == 0 { 255 } else { 0 },
                    if y % 2 == 0 { 255 } else { 0 },
                    (y / width) as u8,
                    255,
                ]
            }),
        ));
    }
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in files {
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
    let view = LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ));
    let blocks = CustomBlocks {
        blocks: images
            .iter()
            .map(|(key, _, _)| {
                block(
                    &format!("test:{key}"),
                    1,
                    CustomBlockVisuals {
                        base: CustomVisualComponents {
                            geometry: Some("minecraft:geometry.full_block".into()),
                            materials: materials(key),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    (view, blocks)
}

#[test]
fn mixed_terrain_keeps_hd_dimensions_and_adjacent_detail_pixels() {
    let images = [("small", 16, 16), ("hd", 256, 256), ("larger", 512, 512)];
    let (view, blocks) = fixture(&images);
    let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
    assert_eq!(compiled.gaps.missing_textures, 0);
    // Best fit segregates scales: each texture keeps its own tile instead of
    // subsidizing the shared page up to the largest neighbour.
    let mut tiles: Vec<u32> = compiled
        .overlay
        .textures
        .iter()
        .map(|shard| shard.mips[0].size)
        .collect();
    tiles.sort_unstable();
    assert_eq!(tiles, vec![16, 256, 512]);
    let mut starts = Vec::with_capacity(compiled.overlay.textures.len());
    let mut flat = 0;
    for shard in &compiled.overlay.textures {
        starts.push(flat);
        flat += shard.layers as usize;
    }
    for (visual, (_, width, height)) in compiled.overlay.visuals.iter().zip(images) {
        let reference = compiled.overlay.materials[visual.faces[0] as usize].texture;
        let shard = reference.page() as usize - 1;
        let layer = reference.layer() as usize;
        assert_eq!(
            compiled.overlay.textures[shard].mips[0].size,
            width.max(height),
            "each scale keeps its own tile"
        );
        assert_eq!(
            compiled.overlay.texture_source_sizes[starts[shard] + layer],
            [width as u16, height as u16]
        );
        let tile = compiled.overlay.textures[shard].mips[0].size;
        let pixels =
            &compiled.overlay.textures[shard].mips[0].rgba8[layer * (tile * tile * 4) as usize..];
        let next = (tile / width * 4) as usize;
        assert_eq!(&pixels[..4], &[255, 255, 0, 255]);
        assert_eq!(&pixels[next..next + 4], &[0, 255, 0, 255]);
    }
    RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .unwrap();
}

#[test]
fn tall_strips_retain_first_and_last_full_resolution_frames() {
    for (width, height) in [(64, 2048), (128, 4096), (64, 8384)] {
        let (view, blocks) = fixture(&[("water", width, height)]);
        let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
        assert_eq!(compiled.gaps.missing_textures, 0);
        let animation = &compiled.overlay.animations[0];
        assert_eq!(animation.frame_count, height / width);
        assert_eq!(animation.ticks_per_frame, 2);
        let page = compiled.overlay.textures.first().unwrap();
        let layer_bytes = (width * width * 4) as usize;
        for frame in [0, animation.frame_count - 1] {
            let layer = compiled.overlay.animation_frames[frame as usize].layer() as usize;
            assert_eq!(
                compiled.overlay.texture_source_sizes[layer],
                [width as u16; 2]
            );
            assert_eq!(
                &page.mips[0].rgba8[layer * layer_bytes..][..4],
                &[255, 255, frame as u8, 255]
            );
        }
    }
}

#[test]
fn over_budget_routes_resize_without_diagnostic_faces() {
    let mut images = (0..48)
        .map(|i| (format!("hd{i}"), 512, 512))
        .collect::<Vec<_>>();
    images.push(("small".to_owned(), 16, 16));
    let borrowed = images
        .iter()
        .map(|(key, width, height)| (key.as_str(), *width, *height))
        .collect::<Vec<_>>();
    let (view, blocks) = fixture(&borrowed);
    let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
    assert_eq!(compiled.gaps.missing_textures, 0);
    assert_eq!(compiled.gaps.saturated_textures, 0);
    assert!(
        compiled
            .overlay
            .texture_source_sizes
            .iter()
            .any(|size| size[0] < 512)
    );
    assert!(
        compiled
            .overlay
            .textures
            .iter()
            .flat_map(|page| &page.mips)
            .map(|mip| mip.rgba8.len())
            .sum::<usize>()
            <= assets::texture::policy::terrain_texture_policy().storage_bytes()
    );
    let last = compiled.overlay.visuals.last().unwrap();
    assert!(
        last.faces
            .iter()
            .all(|&face| face != assets::DIAGNOSTIC_MATERIAL)
    );
}

#[test]
fn oversized_horizontal_terrain_without_animation_keeps_the_first_frame() {
    let (view, blocks) = fixture(&[("strip", 8192, 64)]);
    let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
    assert_eq!(compiled.gaps.missing_textures, 0);
    assert!(compiled.overlay.animations.is_empty());
    let material = compiled.overlay.materials[compiled.overlay.visuals[0].faces[0] as usize];
    assert_eq!(
        compiled.overlay.texture_source_sizes[material.texture.layer() as usize],
        [64; 2]
    );
    let mip = &compiled.overlay.textures.first().unwrap().mips[0];
    assert_eq!(mip.size, 64);
    let offset = material.texture.layer() as usize * 64 * 64 * 4;
    assert_eq!(
        &mip.rgba8[offset + (63 * 64 + 63) * 4..][..4],
        &[0, 0, 0, 255]
    );
}

#[test]
fn diagnostic_layer_is_budgeted_before_resizing_excess_textures() {
    let mut images = (0..4)
        .map(|index| (format!("medium{index}"), 256, 256))
        .collect::<Vec<_>>();
    images.extend((0..47).map(|index| (format!("large{index}"), 512, 512)));
    let borrowed = images
        .iter()
        .map(|(key, width, height)| (key.as_str(), *width, *height))
        .collect::<Vec<_>>();
    let (view, blocks) = fixture(&borrowed);
    let compiled = super::super::compile_block_overlay(&view, &blocks, false, None, 2048, None)
        .expect("an excess route must not discard the admitted overlay");
    assert_eq!(compiled.gaps.missing_textures, 0);
    let resized = compiled.overlay.visuals.last().unwrap();
    assert_eq!(resized.kind, VisualKind::Cube);
    assert!(
        resized
            .faces
            .iter()
            .all(|&face| face != assets::DIAGNOSTIC_MATERIAL),
        "byte pressure must keep every valid face textured"
    );
    assert!(
        compiled.overlay.visuals[..50]
            .iter()
            .all(|visual| visual.kind == VisualKind::Cube)
    );
    let bytes = compiled
        .overlay
        .textures
        .iter()
        .flat_map(|shard| &shard.mips)
        .map(|mip| mip.rgba8.len())
        .sum::<usize>();
    assert!(bytes <= assets::texture::policy::terrain_texture_policy().storage_bytes());
    RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .unwrap();
}

#[test]
fn growing_the_fourth_open_shard_charges_only_existing_and_incoming_layers() {
    let (view, blocks) = fixture(&[
        ("small", 16, 16),
        ("medium", 32, 32),
        ("larger", 64, 64),
        ("hd", 128, 128),
        ("largest", 256, 256),
    ]);
    let compiled =
        super::super::compile_block_overlay(&view, &blocks, false, None, 2048, None).unwrap();
    assert_eq!(compiled.gaps.missing_textures, 0);
    assert_eq!(
        compiled.overlay.textures.len(),
        super::super::MAX_OVERLAY_SHARDS
    );
    assert!(
        compiled
            .overlay
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Cube)
    );
    assert!(
        compiled
            .overlay
            .textures
            .iter()
            .flat_map(|shard| &shard.mips)
            .map(|mip| mip.rgba8.len())
            .sum::<usize>()
            <= 1_163_256
    );
    RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .unwrap();
}
