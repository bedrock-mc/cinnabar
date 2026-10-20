use super::super::{MAX_OVERLAY_LAYERS, compile_block_overlay};
use super::*;

/// Large stacks retain supported routes and use diagnostic visuals only after capacity.
#[test]
fn larger_overlays_keep_every_texture_that_fits_the_device() {
    let previous = MAX_OVERLAY_LAYERS;
    assert!(
        previous >= 4096,
        "the overlay must hold more than one vanilla atlas"
    );
    let wanted = 4096;
    let view = {
        let mut catalog = String::from(r#"{"texture_data": {"#);
        let mut entries = Vec::new();
        for index in 0..wanted {
            if index > 0 {
                catalog.push(',');
            }
            catalog.push_str(&format!(
                r#""fill_{index}": {{"textures": "textures/blocks/fill_{index}"}}"#
            ));
            let pixels = png(16, 16, |x, y| {
                [
                    u8::try_from(index % 251).unwrap_or(0),
                    u8::try_from((x + y) % 251).unwrap_or(0),
                    128,
                    255,
                ]
            });
            entries.push((format!("textures/blocks/fill_{index}.png"), pixels));
        }
        catalog.push_str("}}");
        let id = "00000000-0000-0000-0000-000000000001";
        let manifest = format!(
            r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
        );
        let mut files: Vec<(String, Vec<u8>)> = vec![
            ("manifest.json".into(), manifest.into_bytes()),
            ("textures/terrain_texture.json".into(), catalog.into_bytes()),
            (
                "models/blocks/gen.geo.json".into(),
                GEOMETRY.as_bytes().to_vec(),
            ),
        ];
        files.extend(entries);
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, bytes) in files {
            writer
                .start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
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
    };
    let mut blocks = Vec::new();
    for index in 0..wanted {
        blocks.push(block(
            &format!("test:fill_{index}"),
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("minecraft:geometry.full_block".into()),
                    materials: materials(&format!("fill_{index}")),
                    ..CustomVisualComponents::default()
                },
                ..CustomBlockVisuals::default()
            },
        ));
    }
    let blocks = CustomBlocks {
        blocks: blocks.into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    for limit in [256, 2048, 4096, assets::MAX_TEXTURE_LAYERS as u32 + 1] {
        let compiled =
            compile_block_overlay(&view, &blocks, false, None, limit, None).expect("overlay");
        let per_page = limit.min(assets::MAX_TEXTURE_LAYERS as u32);
        let admitted = (wanted as u32 + 1).min(4 * per_page);
        let refused = wanted as u32 + 1 - admitted;
        assert_eq!(compiled.gaps.saturated_textures, refused);
        assert_eq!(compiled.gaps.missing_textures, refused);
        let shards = (admitted as usize).div_ceil(per_page as usize);
        assert_eq!(compiled.overlay.textures.len(), shards);
        let layers: u32 = compiled
            .overlay
            .textures
            .iter()
            .map(|shard| shard.layers)
            .sum();
        assert_eq!(layers, admitted);
        for shard in &compiled.overlay.textures {
            render_scene::TextureArrayLimits {
                max_layers: limit,
                max_dimension_2d: assets::MAX_TILE_SIZE,
            }
            .validate(shard.layers, shard.mips[0].size)
            .expect("uploadable overlay");
        }
        let runtime = RuntimeAssets::diagnostic()
            .with_block_overlay(1, &compiled.overlay)
            .expect("supported replacement is retained");
        assert_eq!(runtime.texture_pages().len() - 1, shards);
        for (index, visual) in compiled.overlay.visuals.iter().enumerate() {
            let reference = compiled.overlay.materials[visual.faces[0] as usize].texture;
            let flat = if index < admitted as usize - 1 {
                index as u32 + 1
            } else {
                0
            };
            assert_eq!(
                (reference.page(), reference.layer()),
                (
                    flat / per_page + 1,
                    if flat == 0 { 0 } else { flat % per_page }
                )
            );
        }
    }
}

#[test]
fn flipbooks_respect_device_layer_limits_across_shards() {
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        ..Default::default()
    };
    for limit in [1, 2, 3] {
        let compiled = compile_block_overlay(&view(), &blocks, false, None, limit, None).unwrap();
        assert_eq!(
            compiled
                .overlay
                .textures
                .iter()
                .map(|shard| shard.layers)
                .sum::<u32>(),
            3
        );
        assert!(
            compiled
                .overlay
                .textures
                .iter()
                .all(|shard| shard.layers <= limit)
        );
        assert_eq!(compiled.overlay.animations[0].frame_count, 2);
        assert!(
            compiled
                .overlay
                .animation_frames
                .iter()
                .all(|frame| frame.layer() < limit)
        );
        assert!(
            compiled
                .overlay
                .materials
                .iter()
                .all(|material| material.texture.layer() < limit)
        );
    }
    assert!(compile_block_overlay(&view(), &blocks, false, None, 0, None).is_none());
}
