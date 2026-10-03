use std::{io::Write, sync::Arc};

use resource_pack::LayeredPackView;

use super::{BlockIcons, compile_session_icons, custom_block_items};
use crate::ui_runtime::presentation::SessionIcon;

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |_, y| image::Rgba([y as u8, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn view() -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000002";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let catalog = r#"{"texture_data": {"test:gem": {"textures": "textures/items/gem"},
        "test:strip": {"textures": ["textures/items/strip"]},
        "test:huge": {"textures": "textures/items/huge"}}}"#;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.into_bytes()),
        ("textures/item_texture.json", catalog.as_bytes().to_vec()),
        ("textures/items/gem.png", png(16, 16)),
        ("textures/items/strip.png", png(16, 48)),
        ("textures/items/huge.png", png(128, 64)),
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

// Keys resolve through item_texture.json; strips keep frame one and big icons shrink.
#[test]
fn icon_keys_resolve_to_bounded_sprites() {
    let key = |identifier: &str, key: &str| (Arc::<str>::from(identifier), Arc::<str>::from(key));
    let icons = compile_session_icons(
        &view(),
        &[
            key("lifeboat:gem", "test:gem"),
            key("lifeboat:strip", "test:strip"),
            key("lifeboat:huge", "test:huge"),
            key("lifeboat:missing", "test:absent"),
        ],
        BlockIcons::default(),
    )
    .expect("icons");
    let sizes = icons
        .icons
        .iter()
        .map(|icon| (icon.identifier.as_ref(), icon.width, icon.height))
        .collect::<Vec<_>>();
    assert_eq!(
        sizes,
        [
            ("lifeboat:gem", 16, 16),
            ("lifeboat:huge", 64, 32),
            ("lifeboat:strip", 16, 16)
        ]
    );
    let strip = icons
        .icons
        .iter()
        .find(|icon| icon.identifier.as_ref() == "lifeboat:strip")
        .unwrap();
    assert_eq!(strip.rgba8[(15 * 16) * 4], 15, "first frame rows only");
}

fn stack(packs: &[&[(&str, Vec<u8>)]]) -> LayeredPackView {
    let archives = packs
        .iter()
        .enumerate()
        .map(|(index, files)| {
            let id = format!("00000000-0000-0000-0000-{index:012}");
            let manifest = format!(
                r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
            );
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (path, bytes) in std::iter::once(("manifest.json", manifest.into_bytes()))
                .chain(files.iter().map(|(path, bytes)| (*path, bytes.clone())))
            {
                writer
                    .start_file(path, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(&bytes).unwrap();
            }
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                writer.finish().unwrap().into_inner(),
            )
        })
        .collect();
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(archives),
    ))
}

// Keys merge across every pack, fall back to textures/items, and misses say why.
#[test]
fn custom_item_icons_merge_across_packs_and_explain_misses() {
    let key = |identifier: &str, key: &str| (Arc::<str>::from(identifier), Arc::<str>::from(key));
    let base_catalog = br#"{"texture_data":{"a_key":{"textures":"textures/items/a"}}}"#.to_vec();
    let top_catalog = br#"{"texture_data":{"b_key":{"textures":"textures/items/b"},"dead_key":{"textures":"textures/items/none"}}}"#.to_vec();
    let view = stack(&[
        &[
            ("textures/item_texture.json", base_catalog),
            ("textures/items/a.png", png(16, 16)),
        ],
        &[
            ("textures/item_texture.json", top_catalog),
            ("textures/items/b.png", png(16, 16)),
            ("textures/items/loose.png", png(16, 16)),
        ],
    ]);
    let icons = compile_session_icons(
        &view,
        &[
            key("t:a", "a_key"),
            key("t:b", "b_key"),
            key("t:loose", "loose"),
            key("t:dead", "dead_key"),
            key("t:absent", "missing_key"),
        ],
        BlockIcons::default(),
    )
    .expect("icons");
    let mut resolved: Vec<_> = icons
        .icons
        .iter()
        .map(|i| i.identifier.to_string())
        .collect();
    resolved.sort();
    assert_eq!(resolved, ["t:a", "t:b", "t:loose"]);
    assert!(icons.misses["t:dead"].contains("no readable image"));
    assert!(icons.misses["t:absent"].contains("not in the merged item_texture.json"));
}

// A catalog path that already names its image resolves, as on the retail client.
#[test]
fn catalog_paths_with_an_image_extension_resolve() {
    let catalog = br#"{"texture_data":{"zeqa.training":{"textures":"textures/items/zeqa/hub/main/training.png"},"upper":{"textures":"textures/items/upper.PNG"}}}"#.to_vec();
    let view = stack(&[&[
        ("textures/item_texture.json", catalog),
        ("textures/items/zeqa/hub/main/training.png", png(16, 16)),
        ("textures/items/upper.PNG", png(16, 16)),
    ]]);
    let key = |identifier: &str, key: &str| (Arc::<str>::from(identifier), Arc::<str>::from(key));
    let icons = compile_session_icons(
        &view,
        &[
            key("zeqa:item.training", "zeqa.training"),
            key("t:upper", "upper"),
        ],
        BlockIcons::default(),
    )
    .expect("icons");
    assert!(icons.misses.is_empty(), "{:?}", icons.misses);
    assert_eq!(icons.icons.len(), 2);
}

// A custom block item draws as its block even when a short-name guess would resolve.
#[test]
fn block_items_beat_short_name_guesses() {
    let key = |identifier: &str, key: &str| (Arc::<str>::from(identifier), Arc::<str>::from(key));
    let block = SessionIcon {
        identifier: "t:crate".into(),
        metadata: 0,
        width: 32,
        height: 32,
        rgba8: vec![7; 32 * 32 * 4].into(),
    };
    let blocks = BlockIcons {
        icons: vec![block],
        misses: vec![("t:broken".into(), "no drawable visual".into())],
        ..Default::default()
    };
    let icons = compile_session_icons(
        &view(),
        &[
            key("t:crate", "test:gem"),
            key("t:broken", "test:gem"),
            key("t:gem", "test:gem"),
        ],
        blocks,
    )
    .expect("icons");
    let crate_icon = icons
        .icons
        .iter()
        .find(|icon| icon.identifier.as_ref() == "t:crate")
        .expect("block icon");
    assert_eq!((crate_icon.width, crate_icon.rgba8[0]), (32, 7));
    assert_eq!(icons.icons.len(), 2, "t:broken keeps no sprite");
    assert!(icons.misses["t:broken"].contains("no drawable visual"));
}

// A registry item named after a custom block is that block's item; others are not.
#[test]
fn registry_items_named_after_custom_blocks_are_block_items() {
    let mut game_data = protocol::GameData {
        start_game: Default::default(),
        item_registry: Default::default(),
        biome_definitions: None,
        entity_identifiers: None,
        creative_content: None,
    };
    for name in ["t:crate", "t:sword"] {
        game_data.item_registry.item_data.push(Default::default());
        game_data
            .item_registry
            .item_data
            .last_mut()
            .unwrap()
            .item_name = name.into();
    }
    let blocks = protocol::CustomBlocks {
        blocks: vec![protocol::CustomBlock {
            name: "t:crate".into(),
            state_count: 1,
            collides: true,
            collision_box: None,
            selection: Default::default(),
            visual: Default::default(),
        }]
        .into(),
        skipped: 0,
    };
    let pairs = custom_block_items(&game_data, &blocks);
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        (pairs[0].0.as_ref(), pairs[0].1.as_ref()),
        ("t:crate", "t:crate")
    );
}

// Real cached packs (`CINNABAR_PACKCACHE_DIR`): every item_texture.json key a pack declares
// resolves to a bounded icon through the same path a `minecraft:icon` component takes.
#[test]
fn packcache_item_icon_keys_resolve_when_requested() {
    let Some(dir) = std::env::var_os("CINNABAR_PACKCACHE_DIR") else {
        eprintln!(
            "skipping packcache_item_icon_keys_resolve_when_requested: fixture unavailable; requires CINNABAR_PACKCACHE_DIR containing offline cached packs"
        );
        return;
    };
    let (mut declared, mut resolved) = (0usize, 0usize);
    for entry in std::fs::read_dir(dir).expect("packcache dir").flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "zip") {
            continue;
        }
        let Some(view) = super::super::local_pack::local_pack_view_at(&path) else {
            continue;
        };
        let keys =
            super::super::resource_packs::texture_key_paths(&view, "textures/item_texture.json")
                .into_keys()
                .map(|key| {
                    (
                        Arc::<str>::from(format!("pack:{key}")),
                        Arc::<str>::from(key),
                    )
                })
                .collect::<Vec<_>>();
        for chunk in keys.chunks(256) {
            declared += chunk.len();
            resolved += compile_session_icons(&view, chunk, BlockIcons::default())
                .map_or(0, |icons| icons.icons.len());
        }
    }
    eprintln!("{resolved} of {declared} cached-pack item icon keys resolved");
    assert!(resolved * 10 >= declared * 8, "{resolved} of {declared}");
}

// Every server item keeps its icon: a pack with more icons than the old 512 cap loses none.
#[test]
fn icon_count_is_bounded_by_the_item_registry_not_a_fixed_cap() {
    let key = |identifier: String| (Arc::<str>::from(identifier), Arc::<str>::from("test:gem"));
    let keys: Vec<_> = (0..600)
        .map(|index| key(format!("cosmetic:item_{index}")))
        .collect();
    let icons = compile_session_icons(&view(), &keys, BlockIcons::default()).expect("icons");
    assert_eq!(icons.icons.len(), 600);
}

#[test]
fn array_variants_keep_metadata_and_pack_item_declarations_override_registry_keys() {
    let view = stack(&[&[
        ("textures/item_texture.json", br#"{"texture_data":{"custom":{"textures":["textures/items/zero","textures/items/missing","textures/items/two"]}}}"#.to_vec()),
        ("items/example.json", br#"{"minecraft:item":{"description":{"identifier":"test:variant"},"components":{"minecraft:icon":{"textures":{"default":"custom"}}}}}"#.to_vec()),
        ("textures/items/zero.png", png(8, 8)),
        ("textures/items/two.png", png(16, 16)),
    ]]);
    let icons = compile_session_icons(
        &view,
        &[(Arc::from("test:variant"), Arc::from("old"))],
        BlockIcons::default(),
    )
    .unwrap();
    assert_eq!(
        icons
            .icons
            .iter()
            .map(|icon| (icon.identifier.as_ref(), icon.metadata, icon.width))
            .collect::<Vec<_>>(),
        [("test:variant", 0, 8), ("test:variant", 2, 16)]
    );
}

#[test]
fn upper_catalog_replaces_the_complete_variant_list() {
    let view = stack(&[
        &[("textures/item_texture.json", br#"{"texture_data":{"gem":{"textures":["textures/items/zero","textures/items/one"]}}}"#.to_vec()), ("textures/items/zero.png", png(8, 8)), ("textures/items/one.png", png(16, 16))],
        &[("textures/item_texture.json", br#"{"texture_data":{"gem":{"textures":"textures/items/one"}}}"#.to_vec())],
    ]);
    let icons = compile_session_icons(
        &view,
        &[(Arc::from("test:gem"), Arc::from("gem"))],
        BlockIcons::default(),
    )
    .unwrap();
    assert_eq!(icons.icons.len(), 1);
    assert_eq!((icons.icons[0].metadata, icons.icons[0].width), (0, 16));
}

#[test]
fn review_variant_decoding_respects_remaining_capacity_and_metadata_indices() {
    let paths = std::collections::HashMap::from([(
        "variants".to_owned(),
        vec![
            "textures/missing".to_owned(),
            "textures/items/gem".to_owned(),
            "textures/items/gem".to_owned(),
            "textures/items/gem".to_owned(),
        ],
    )]);
    let resolved = super::resolve_key(&view(), &paths, "variants", 1).unwrap();
    assert_eq!(
        resolved
            .iter()
            .map(|(metadata, _)| *metadata)
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn review_item_declaration_priority_survives_moves_between_files() {
    let declaration = |identifier: &str, key: &str| {
        serde_json::to_vec(&serde_json::json!({
        "minecraft:item": {"description": {"identifier":identifier}, "components":{"minecraft:icon":key}}
    })).unwrap()
    };
    let view = stack(&[
        &[
            ("items/a.json", declaration("t:a", "lower_a")),
            ("items/b.json", declaration("t:b", "lower_b")),
        ],
        &[
            ("items/a.json", declaration("t:b", "upper_b")),
            ("items/b.json", declaration("t:a", "upper_a")),
        ],
    ]);
    let keys = super::catalog::icon_keys(&view, &[]);
    assert_eq!(
        keys.iter()
            .map(|(id, key)| (id.as_ref(), key.as_ref()))
            .collect::<Vec<_>>(),
        [("t:a", "upper_a"), ("t:b", "upper_b")]
    );
}
