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
    let page = compiled.overlay.texture.as_ref().unwrap();
    assert_eq!(page.mips[0].size, 512);
    for (visual, (_, width, height)) in compiled.overlay.visuals.iter().zip(images) {
        let layer = compiled.overlay.materials[visual.faces[0] as usize]
            .texture
            .layer() as usize;
        assert_eq!(
            compiled.overlay.texture_source_sizes[layer],
            [width as u16, height as u16]
        );
        let pixels = &page.mips[0].rgba8[layer * 512 * 512 * 4..];
        let next = (512 / width * 4) as usize;
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
        let page = compiled.overlay.texture.as_ref().unwrap();
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
fn over_budget_route_keeps_other_terrain_at_full_quality() {
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
    let page = compiled.overlay.texture.as_ref().unwrap();
    assert_eq!(page.mips[0].size, 512);
    assert!(
        page.mips.iter().map(|mip| mip.rgba8.len()).sum::<usize>()
            <= assets::texture::policy::MAX_SUBSCRIBER_BYTES
    );
    assert!(compiled.gaps.missing_textures > 0);
    assert_eq!(compiled.overlay.texture_source_sizes[1], [512; 2]);
    let last = compiled.overlay.visuals.last().unwrap();
    assert_eq!(
        compiled.overlay.materials[last.faces[0] as usize]
            .texture
            .layer(),
        0
    );
}
