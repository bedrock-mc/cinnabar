use super::*;

/// A pack naming one more distinct texture than the overlay's array can hold.
fn view_full_of_textures() -> LayeredPackView {
    let mut catalog = String::from(r#"{"texture_data": {"#);
    let mut files = Vec::new();
    for index in 0..=super::super::MAX_OVERLAY_LAYERS {
        if index > 0 {
            catalog.push(',');
        }
        catalog.push_str(&format!(
            r#""fill_{index}": {{"textures": "textures/blocks/fill_{index}"}}"#
        ));
        let png = png(16, 16, |x, y| {
            let seed = u8::try_from(
                (index + usize::try_from(x).unwrap_or(0) + usize::try_from(y).unwrap_or(0)) % 251,
            )
            .unwrap_or(0);
            [seed, u8::try_from(index % 251).unwrap_or(0), 128, 255]
        });
        files.push((format!("textures/blocks/fill_{index}.png"), png));
    }
    catalog.push_str("}}");
    let geometry = GEOMETRY.as_bytes();
    let id = "00000000-0000-0000-0000-000000000001";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("manifest.json".into(), manifest.into_bytes()),
        ("textures/terrain_texture.json".into(), catalog.into_bytes()),
        ("models/blocks/gen.geo.json".into(), geometry.to_vec()),
    ];
    entries.extend(files);
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in entries {
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
}

/// The overlay keeps pushing diagnostics only for routes its shards refused, so a
/// full texture budget is distinguishable from an unusable pack.
#[test]
fn a_full_texture_budget_names_the_routes_it_refused() {
    let view = view_full_of_textures();
    let mut blocks = Vec::new();
    let mut items = Vec::new();
    for index in 0..=super::super::MAX_OVERLAY_LAYERS {
        let name = format!("test:fill_{index}");
        blocks.push(block(
            &name,
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
        items.push((name.clone(), name));
    }
    let blocks = CustomBlocks {
        blocks: blocks.into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    // Four shards of 256 layers cannot hold the whole fixture, so the fifth
    // shard's routes are refused and named instead of degrading silently.
    let compiled = super::super::compile_block_overlay(&view, &blocks, false, None, 256, None)
        .expect("overlay");
    let layers: u32 = compiled
        .overlay
        .textures
        .iter()
        .map(|shard| shard.layers)
        .sum();
    assert_eq!(layers, 4 * 256);
    assert_eq!(compiled.overlay.textures.len(), 4);
    assert!(
        compiled.gaps.saturated_textures > 0,
        "a full budget must count the routes it refused, gaps: {:?}",
        compiled.gaps
    );
    assert_eq!(
        compiled.gaps.missing_textures, compiled.gaps.saturated_textures,
        "each refused route is its own diagnostic visual"
    );
}

#[test]
fn budget_rejected_runs_do_not_materialize_visuals_or_item_indices() {
    let admitted = block("test:admitted", 1, CustomBlockVisuals::default());
    let rejected = block(
        "test:rejected",
        u32::MAX,
        CustomBlockVisuals {
            state_identity_incomplete: true,
            materialization_disabled: true,
            ..Default::default()
        },
    );
    let definitions = CustomBlocks {
        blocks: Arc::from([admitted, rejected]),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view(), &definitions, hashed, None).unwrap();
        assert_eq!(compiled.overlay.visuals.len(), 1);
        assert_eq!(compiled.overlay.hashes.len(), 1);
        assert_eq!(compiled.gaps.incomplete_state_identities, 1);
        let icons = crate::runtime::network::item_icons::custom_block_icons(
            &compiled.overlay,
            &definitions,
            hashed,
            &[("test:rejected".into(), "test:rejected".into())],
            None,
        );
        assert!(icons.icons.is_empty());
        assert!(icons.block_sheets.is_empty());
        assert_eq!(icons.misses.len(), 1);
    }
}
