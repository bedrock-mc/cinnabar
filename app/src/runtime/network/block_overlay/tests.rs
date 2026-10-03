use std::{io::Write, sync::Arc};

use assets::{BlockFace, NetworkIdMode, RuntimeAssets, VisualKind};
use protocol::{
    CustomBlock, CustomBlockVisuals, CustomBlocks, CustomMaterialInstance, CustomPermutation,
    CustomStateAxis, CustomStateValue, CustomTransformation, CustomVisualComponents,
};
use resource_pack::LayeredPackView;

use super::{OverlayGaps, compile_block_overlay};

fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode fixture png");
    bytes.into_inner()
}

const GEOMETRY: &str = r#"{"format_version": "1.12.0", "minecraft:geometry": [{
    "description": {"identifier": "geometry.gen", "texture_width": 32, "texture_height": 32},
    "bones": [{"name": "block", "pivot": [0, 0, 0], "cubes": [
        {"origin": [-8, 0, -8], "size": [16, 16, 16], "uv": {
            "north": {"uv": [0, 0], "uv_size": [16, 16]},
            "east": {"uv": [16, 0], "uv_size": [16, 16]},
            "south": {"uv": [16, 0], "uv_size": [16, 16]},
            "west": {"uv": [16, 0], "uv_size": [16, 16]},
            "up": {"uv": [16, 32], "uv_size": [16, -16]},
            "down": {"uv": [0, 32], "uv_size": [16, -16]}}},
        {"origin": [-4, 16, -4], "size": [8, 1, 8], "uv": {"up": {"uv": [20, 20], "uv_size": [8, 8]}}},
        {"origin": [0, 0, 0], "size": [1, 1, 1], "rotation": [0, 45, 0], "uv": [0, 0]}
    ]}]}]}"#;

fn view() -> LayeredPackView {
    view_with_geometry(GEOMETRY.as_bytes())
}

/// Builds the normal overlay fixture with an alternate geometry document.
fn view_with_geometry(geometry: &[u8]) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let terrain = r#"{"texture_data": {
        "lucky": {"textures": "textures/blocks/lucky"},
        "gen": {"textures": ["textures/blocks/gen"]}}}"#;
    let flipbook = r#"[{"flipbook_texture": "textures/blocks/gen", "atlas_tile": "gen",
        "frames": [1, 0], "ticks_per_frame": 15}]"#;
    let lucky = png(16, 16, |x, _| [x as u8 * 16, 200, 0, 255]);
    let gen_strip = png(32, 64, |_, y| {
        if y < 32 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    });
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.as_bytes()),
        ("textures/terrain_texture.json", terrain.as_bytes()),
        ("textures/flipbook_textures.json", flipbook.as_bytes()),
        ("textures/blocks/lucky.png", &lucky),
        ("textures/blocks/gen.png", &gen_strip),
        ("models/blocks/gen.geo.json", geometry),
    ] {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
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

fn materials(texture: &str) -> Option<Box<[CustomMaterialInstance]>> {
    Some(Box::new([CustomMaterialInstance {
        name: "*".into(),
        texture: texture.into(),
        render_method: None,
        tint_method: None,
    }]))
}

fn block(name: &str, state_count: u32, visual: CustomBlockVisuals) -> CustomBlock {
    CustomBlock {
        name: name.into(),
        state_count,
        collides: true,
        collision_box: None,
        selection: Default::default(),
        visual: Arc::new(visual),
    }
}

fn turn(quarters: i32) -> CustomVisualComponents {
    CustomVisualComponents {
        transformation: Some(CustomTransformation {
            rotation: [0, quarters, 0],
            scale: [1.0; 3],
            translation: [0.0; 3],
        }),
        ..CustomVisualComponents::default()
    }
}

fn generator() -> CustomBlock {
    let direction = |value: &str, quarters| CustomPermutation {
        condition: format!("q.block_state('minecraft:cardinal_direction') == '{value}'").into(),
        components: turn(quarters),
    };
    block(
        "test:generator",
        4,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("geometry.gen".into()),
                materials: materials("gen"),
                ..CustomVisualComponents::default()
            },
            permutations: Box::new([direction("west", 1), direction("north", 0)]),
            state_axes: Box::new([CustomStateAxis {
                name: "minecraft:cardinal_direction".into(),
                values: ["south", "west", "north", "east"]
                    .map(|value| CustomStateValue::String(value.into()))
                    .into(),
            }]),
        },
    )
}

fn compiled() -> super::CompiledBlockOverlay {
    let lucky = block(
        "test:lucky",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: materials("lucky"),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let missing = block(
        "test:missing",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                materials: materials("absent"),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![lucky, generator(), missing].into(),
        skipped: 0,
    };
    compile_block_overlay(&view(), &blocks, false, None).expect("overlay")
}

// Full blocks become cubes on page 1; missing textures stay diagnostic and are counted.
#[test]
fn full_block_is_a_page_one_cube_and_missing_texture_is_diagnostic() {
    let compiled = compiled();
    let overlay = &compiled.overlay;
    assert_eq!(
        overlay.visuals.len(),
        6,
        "one lucky, four generator, one missing state"
    );
    let lucky = overlay.visuals[0];
    assert_eq!(lucky.kind, VisualKind::Cube);
    let material = overlay.materials[lucky.faces[BlockFace::Up as usize] as usize];
    assert_eq!((material.texture.page(), material.flags), (1, 0));
    assert_eq!(overlay.visuals[5].kind, VisualKind::Diagnostic);
    assert_eq!(
        compiled.gaps,
        OverlayGaps {
            missing_textures: 1,
            ..OverlayGaps::default()
        },
        "rotated cubes are rendered, not skipped"
    );
    let texture = overlay.texture.as_ref().expect("page");
    assert_eq!(
        texture.mips[0].size, 32,
        "largest source keeps its resolution"
    );
}

// Flipbook frames become layers in the listed order behind one animation.
#[test]
fn flipbook_texture_animates_listed_frames() {
    let compiled = compiled();
    let overlay = &compiled.overlay;
    let generator = overlay.visuals[1];
    let material = overlay.materials[generator.faces[0] as usize];
    let animation = overlay.animations[material.animation as usize];
    assert_eq!((animation.frame_count, animation.ticks_per_frame), (2, 15));
    let texture = overlay.texture.as_ref().unwrap();
    let layer_bytes = 32 * 32 * 4;
    let first_frame = overlay.animation_frames[animation.frame_start as usize].layer() as usize;
    let pixel = &texture.mips[0].rgba8[first_frame * layer_bytes..][..4];
    assert_eq!(
        pixel,
        [0, 0, 255, 255],
        "frame 1 of the strip is listed first"
    );
}

// Geometry faces keep their pixel UVs and a quarter turn moves the front to the west.
#[test]
fn geometry_quads_follow_uvs_and_placement_rotation() {
    let compiled = compiled();
    let overlay = &compiled.overlay;
    let north_state = overlay.visuals[3];
    assert_eq!(north_state.kind, VisualKind::Model);
    let template = overlay.model_templates[north_state.model_template as usize];
    assert_eq!(
        template.quad_count, 13,
        "six body faces, the top plate, and the six faces of the turned cube"
    );
    let quads = &overlay.model_quads[template.quad_start as usize..][..13];
    let front = quads
        .iter()
        .find(|quad| quad.flags & 7 == 5)
        .expect("north face");
    assert!(front.positions.iter().all(|corner| corner[2] == 0));
    assert!(front.uvs.iter().all(|uv| uv[0] <= 2048 && uv[1] <= 2048));
    assert_eq!(
        front.flags >> 4 & 7,
        5,
        "boundary faces cull against neighbours"
    );

    let west_state = overlay.visuals[2];
    let west_template = overlay.model_templates[west_state.model_template as usize];
    let west_quads = &overlay.model_quads[west_template.quad_start as usize..][..13];
    let turned_front = west_quads
        .iter()
        .find(|quad| quad.uvs.iter().all(|uv| uv[0] <= 2048 && uv[1] <= 2048))
        .expect("front face");
    assert!(turned_front.positions.iter().all(|corner| corner[0] == 0));
    assert_eq!(turned_front.flags & 7, 3, "front now faces west");
}

// The overlay resolves at the ids after the base carrier and nowhere else.
#[test]
fn overlay_extends_runtime_assets_after_base_ids() {
    let compiled = compiled();
    let base = RuntimeAssets::diagnostic();
    let session = base
        .with_block_overlay(1, &compiled.overlay)
        .expect("overlay applies");
    assert_eq!(session.texture_pages().len(), 2);
    let lucky = session.resolve(NetworkIdMode::Sequential, 1);
    assert!(lucky.is_known());
    assert_eq!(lucky.kind(), VisualKind::Cube);
    let material = session.material(lucky.face(BlockFace::North).material_id());
    assert_eq!(material.texture.page(), 1);
    assert!(!session.resolve(NetworkIdMode::Sequential, 7).is_known());
    assert!(base.with_block_overlay(2, &compiled.overlay).is_err());
}

// Only block_state equality conjunctions evaluate; anything else is unknown, not false.
#[test]
fn permutation_conditions_evaluate_only_supported_terms() {
    let direction = CustomStateValue::String("west".into());
    let open = CustomStateValue::Bool(true);
    let state = |name: &str| match name {
        "minecraft:cardinal_direction" => Some(&direction),
        "test:open" => Some(&open),
        _ => None,
    };
    let evaluate = |condition| super::condition::evaluate(condition, &state);
    assert_eq!(
        evaluate("q.block_state('minecraft:cardinal_direction') == 'west'"),
        Some(true)
    );
    assert_eq!(
        evaluate("query.block_state(\"minecraft:cardinal_direction\") != 'west'"),
        Some(false)
    );
    assert_eq!(
        evaluate(
            "(q.block_state('test:open') == true) && q.block_state('minecraft:cardinal_direction') == 'west'"
        ),
        Some(true)
    );
    assert_eq!(evaluate("q.block_state('test:open') == false"), Some(false));
    assert_eq!(evaluate("q.block_state('test:missing') == 1"), None);
    assert_eq!(evaluate("q.block_state('test:open') || true"), None);
    assert_eq!(evaluate("math.random(0, 1) > 0.5"), None);
}

// Flipbook framing is bounded before copies are cut and each frame is shrunk.
#[test]
fn flipbook_frames_are_capped_and_shrunk() {
    use super::textures::{DecodedTexture, Flipbook, flipbook_frames, shrink_to_max};
    let strip = DecodedTexture {
        width: 64,
        height: 64 * 8,
        rgba8: vec![7; (64 * 64 * 8 * 4) as usize].into_boxed_slice(),
    };
    let flipbook = Flipbook {
        frames: None,
        ticks_per_frame: 1,
        blend: true,
    };
    let frames = flipbook_frames(&strip, &flipbook, 3, 32);
    assert_eq!(frames.len(), 3, "frame count capped to the budget");
    assert!(
        frames.iter().all(|f| f.width == 32 && f.height == 32),
        "frames shrunk to max side"
    );
    assert!(flipbook_frames(&strip, &flipbook, 0, 32).is_empty());

    let big = DecodedTexture {
        width: 256,
        height: 128,
        rgba8: vec![9; 256 * 128 * 4].into_boxed_slice(),
    };
    let shrunk = shrink_to_max(&big, 64);
    assert_eq!(
        (shrunk.width, shrunk.height),
        (64, 32),
        "aspect preserved under the cap"
    );
    let small = DecodedTexture {
        width: 16,
        height: 16,
        rgba8: vec![1; 16 * 16 * 4].into_boxed_slice(),
    };
    assert_eq!(
        shrink_to_max(&small, 64).width,
        16,
        "small textures are untouched"
    );
}

// Explicit light components override the full-dampening, no-emission default.
#[test]
fn light_components_drive_state_light() {
    use protocol::{CustomBlockVisuals, CustomVisualComponents};
    let lit = block(
        "test:lamp",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: materials("lucky"),
                light_emission: Some(13),
                light_dampening: Some(2),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let plain = block(
        "test:plain",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: materials("lucky"),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![lit, plain].into(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).expect("overlay");
    let light = &compiled.overlay.light_properties;
    assert_eq!((light[0].emission(), light[0].filter()), (13, 2));
    assert_eq!(
        (light[1].emission(), light[1].filter()),
        (0, 15),
        "vanilla default"
    );
}

// Hashed sessions get one visual and one distinct hash per state combination.
#[test]
fn hashed_mode_emits_a_visual_and_hash_per_state() {
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&view(), &blocks, true, None).expect("overlay");
    assert_eq!(compiled.overlay.visuals.len(), 4);
    assert_eq!(compiled.overlay.hashes.len(), 4);
    let unique: std::collections::HashSet<_> = compiled.overlay.hashes.iter().collect();
    assert_eq!(unique.len(), 4);
    let base = RuntimeAssets::diagnostic();
    let session = base
        .with_block_overlay(1, &compiled.overlay)
        .expect("session assets");
    let hash = compiled.overlay.hashes[2];
    assert_eq!(session.sequential_id_for_hash(hash), Some(3));
}

// A pack redefining a vanilla terrain key repoints that key's base materials; unknown keys are ignored.
#[test]
fn vanilla_terrain_keys_override_base_materials() {
    let keys = assets::MaterialKeys::from_entries([(5, "lucky"), (6, "gen"), (7, "not_in_pack")]);
    let empty = CustomBlocks::default();
    let compiled = compile_block_overlay(&view(), &empty, false, Some(&keys)).expect("overrides");
    let mut materials = compiled
        .overlay
        .material_overrides
        .iter()
        .map(|replacement| replacement.material)
        .collect::<Vec<_>>();
    materials.sort_unstable();
    assert_eq!(materials, [5, 6]);
    let gen_override = compiled
        .overlay
        .material_overrides
        .iter()
        .find(|replacement| replacement.material == 6)
        .unwrap();
    assert_ne!(
        gen_override.animation,
        assets::NO_ANIMATION,
        "flipbook carries over"
    );
    assert!(compile_block_overlay(&view(), &empty, false, None).is_none());
}

// Block items draw their block's first state: cubes and models get thumbnails, a
// diagnostic visual is a miss, and hashed sessions index by `hashed_states`.
#[test]
fn custom_block_items_draw_their_default_state() {
    use super::super::item_icons::custom_block_icons;
    let pair = |item: &str, block: &str| (Arc::<str>::from(item), Arc::<str>::from(block));
    let items = [
        pair("test:lucky", "test:lucky"),
        pair("test:generator", "test:generator"),
        pair("test:missing", "test:missing"),
        pair("test:lucky_placer", "test:lucky"),
    ];
    let overlay = compiled().overlay;
    let blocks = CustomBlocks {
        blocks: vec![
            block("test:lucky", 1, CustomBlockVisuals::default()),
            generator(),
            block("test:missing", 1, CustomBlockVisuals::default()),
        ]
        .into(),
        skipped: 0,
    };
    let icons = custom_block_icons(&overlay, &blocks, false, &items);
    let drawn = icons
        .icons
        .iter()
        .map(|icon| icon.identifier.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(drawn, ["test:lucky", "test:generator", "test:lucky_placer"]);
    assert_eq!(icons.misses.len(), 1);
    assert_eq!(icons.misses[0].0.as_ref(), "test:missing");
    let lucky = &icons.icons[0];
    assert_eq!((lucky.width, lucky.height), (32, 32));
    let opaque = lucky.rgba8.chunks_exact(4).filter(|pixel| pixel[3] == 255);
    assert!(opaque.clone().count() > 300, "a filled cube silhouette");
    assert!(
        opaque.clone().all(|pixel| pixel[2] == 0),
        "lucky's red-green texels"
    );
    assert_eq!(icons.icons[2].rgba8, lucky.rgba8);

    let hashed = CustomBlocks {
        blocks: vec![generator()].into(),
        skipped: 0,
    };
    let overlay = compile_block_overlay(&view(), &hashed, true, None)
        .expect("overlay")
        .overlay;
    let icons = custom_block_icons(&overlay, &hashed, true, &items[1..2]);
    assert_eq!(icons.icons.len(), 1);
}

// Real cached packs (`CINNABAR_PACKCACHE_DIR`): each unencrypted pack's namespaced scalar-textured
// blocks, as full-block custom blocks, draw item thumbnails.
#[test]
fn packcache_custom_block_items_draw_when_requested() {
    use super::super::item_icons::custom_block_icons;
    let Some(dir) = std::env::var_os("CINNABAR_PACKCACHE_DIR") else {
        eprintln!(
            "skipping packcache_custom_block_items_draw_when_requested: fixture unavailable; requires CINNABAR_PACKCACHE_DIR containing offline cached packs"
        );
        return;
    };
    let mut checked = 0usize;
    for entry in std::fs::read_dir(dir).expect("packcache dir").flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some((id, version)) = name
            .strip_suffix(".zip")
            .and_then(|stem| stem.split_once('_'))
        else {
            continue;
        };
        if path.with_extension("key").exists() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let Some(blocks_json) = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
            .ok()
            .and_then(|mut archive| {
                let mut file = archive.by_name("blocks.json").ok()?;
                let mut text = Vec::new();
                std::io::Read::read_to_end(&mut file, &mut text).ok()?;
                serde_json::from_slice::<serde_json::Value>(&resource_pack::normalize_jsonc(&text)?)
                    .ok()
            })
        else {
            continue;
        };
        let custom = blocks_json
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(block_name, entry)| {
                let texture = entry.get("textures")?.as_str()?;
                block_name.contains(':').then(|| {
                    block(
                        block_name,
                        1,
                        CustomBlockVisuals {
                            base: CustomVisualComponents {
                                materials: materials(texture),
                                ..CustomVisualComponents::default()
                            },
                            ..CustomBlockVisuals::default()
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        if custom.is_empty() {
            continue;
        }
        let (Ok(pack_id), version) = (id.parse(), version.to_owned()) else {
            continue;
        };
        let archive =
            protocol::ResourcePackArchive::unencrypted(pack_id, version, String::new(), bytes);
        let view = LayeredPackView::new(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::from_archives(vec![archive]),
        ));
        let blocks = CustomBlocks::from_definitions(std::iter::empty());
        let blocks = CustomBlocks {
            blocks: custom.into(),
            ..blocks
        };
        let Some(compiled) = compile_block_overlay(&view, &blocks, false, None) else {
            continue;
        };
        let items = blocks
            .blocks
            .iter()
            .map(|block| (Arc::clone(&block.name), Arc::clone(&block.name)))
            .collect::<Vec<_>>();
        let icons = custom_block_icons(&compiled.overlay, &blocks, false, &items);
        let textured = items.len() - compiled.gaps.missing_textures as usize;
        assert!(
            icons.icons.len() >= textured,
            "{name}: {} of {textured} textured blocks drew; misses {:?}",
            icons.icons.len(),
            icons.misses
        );
        checked += icons.icons.len();
    }
    assert!(checked > 0, "fixture must contain drawable custom blocks");
    eprintln!("{checked} packcache custom block item icons drawn");
}

#[test]
fn review_geometry_skips_overflowing_cube_bounds() {
    let mut document: serde_json::Value = serde_json::from_str(GEOMETRY).unwrap();
    document["minecraft:geometry"][0]["bones"][0]["cubes"] = serde_json::json!([
        {"origin": [1e38, 0, 0], "size": [3e38, 1, 1], "uv": [0, 0]},
        {"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}
    ]);
    let parsed = super::geometry::parse_geometry_file(&serde_json::to_vec(&document).unwrap());
    assert_eq!(parsed[0].1.cubes.len(), 1);
    assert_eq!(parsed[0].1.skipped_cubes, 1);
    assert!(parsed[0].1.cubes.iter().all(|cube| {
        cube.min
            .iter()
            .chain(&cube.max)
            .all(|value| value.is_finite())
    }));
}

#[test]
fn review_geometry_catalog_accepts_json_escaped_identifiers() {
    let escaped = GEOMETRY.replace("geometry.gen", r"geometry\u002egen");
    let view = view_with_geometry(escaped.as_bytes());
    let wanted = std::collections::HashSet::from(["geometry.gen"]);
    let catalog = super::geometry::geometry_catalog(&view, &wanted);
    assert!(catalog.contains_key("geometry.gen"));
}
