//! Block-item thumbnails, face sheets and prepared icon handoff.

use super::*;

// Block items draw their block's first state: cubes and models get thumbnails, a
// diagnostic visual is a miss, and hashed sessions index by `hashed_states`.
#[test]
fn custom_block_items_draw_their_default_state() {
    use super::super::super::item_icons::custom_block_icons;
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
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let icons = custom_block_icons(&overlay, &blocks, false, &items, None);
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
    let opaque = lucky
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] == 255);
    assert!(opaque.clone().count() > 300, "a filled cube silhouette");
    assert!(
        opaque.clone().all(|pixel| pixel[2] == 0),
        "lucky's red-green texels"
    );
    assert_eq!(icons.icons[2].rgba8, lucky.rgba8);

    let hashed = CustomBlocks {
        blocks: vec![generator()].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let overlay = compile_block_overlay(&view(), &hashed, true, None)
        .expect("overlay")
        .overlay;
    let icons = custom_block_icons(&overlay, &hashed, true, &items[1..2], None);
    assert_eq!(icons.icons.len(), 1);
}

// A full-cube block item also carries the six-face sheet vanilla block items draw as a GPU cube,
// with 16-texel faces however large the overlay's shared tile is; model shapes carry none.
#[test]
fn full_cube_block_items_carry_a_sixteen_texel_face_sheet() {
    use super::super::super::item_icons::custom_block_icons;
    let pair = |item: &str, block: &str| (Arc::<str>::from(item), Arc::<str>::from(block));
    let items = [
        pair("test:lucky", "test:lucky"),
        pair("test:generator", "test:generator"),
        pair("test:lucky_placer", "test:lucky"),
    ];
    let compiled = compiled();
    assert_eq!(compiled.overlay.visuals[0].support, VisualSupport::Exact);
    assert!(
        compiled
            .overlay
            .textures
            .iter()
            .any(|shard| shard.mips[0].size > u32::from(assets::BLOCK_ITEM_FACE_SIDE)),
        "exercise a shared atlas tile larger than the block-item face"
    );
    let blocks = CustomBlocks {
        blocks: vec![
            block("test:lucky", 1, CustomBlockVisuals::default()),
            generator(),
        ]
        .into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let icons = custom_block_icons(&compiled.overlay, &blocks, false, &items, None);
    let sheets = icons
        .block_sheets
        .iter()
        .map(|sheet| sheet.identifier.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(sheets, ["test:lucky", "test:lucky_placer"]);
    let sheet = &icons.block_sheets[0];
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
    assert_eq!((sheet.width, sheet.height), (width, height));
    let side = u32::from(assets::BLOCK_ITEM_FACE_SIDE);
    let columns = u32::from(assets::BLOCK_ITEM_SHEET_GRID[0]);
    for face in 0..6 {
        for (x, y) in [(0, 0), (7, 3), (15, 15)] {
            let (column, row) = (face % columns * side + x, face / columns * side + y);
            let at = ((row * width + column) * 4) as usize;
            assert_eq!(
                &sheet.rgba8[at..at + 4],
                &[x as u8 * 16, 200, 0, 255],
                "face {face} texel {x},{y} is lucky's own texel"
            );
        }
    }
}

#[test]
fn cube_items_keep_fallback_support_and_reshaped_cubes_draw_block_sheets() {
    use super::super::super::item_icons::custom_block_icons;

    let identity = CustomTransformation {
        rotation: [0; 3],
        scale: [1.0; 3],
        translation: [0.0; 3],
    };
    let material = |render_method: Option<&str>,
                    tint_method: Option<&str>|
     -> Option<Box<[CustomMaterialInstance]>> {
        Some(Box::new([CustomMaterialInstance {
            name: "*".into(),
            texture: "lucky".into(),
            render_method: render_method.map(Into::into),
            tint_method: tint_method.map(Into::into),
            ambient_occlusion: None,
            face_dimming: None,
        }]))
    };
    for (case, transformation, materials, thumbnail) in [
        (
            "rotation",
            Some(CustomTransformation {
                rotation: [0, 1, 0],
                ..identity
            }),
            material(None, None),
            true,
        ),
        ("blend", None, material(Some("blend"), None), true),
        (
            "unknown method",
            None,
            material(Some("unknown"), None),
            true,
        ),
        (
            "unresolved tint",
            None,
            material(None, Some("grass")),
            false,
        ),
    ] {
        let blocks = CustomBlocks {
            blocks: vec![block(
                "test:cube",
                1,
                CustomBlockVisuals {
                    base: CustomVisualComponents {
                        geometry: Some(super::super::FULL_BLOCK.into()),
                        materials,
                        transformation,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into(),
            ..Default::default()
        };
        let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
        assert_eq!(
            compiled.overlay.visuals[0].support,
            VisualSupport::VanillaFallback,
            "{case}"
        );
        let icons = custom_block_icons(
            &compiled.overlay,
            &blocks,
            false,
            &[("test:cube".into(), "test:cube".into())],
            None,
        );
        assert_eq!(
            icons.icons.len(),
            usize::from(thumbnail),
            "{case}: only an untinted fallback has a drawable thumbnail"
        );
        assert_eq!(icons.misses.len(), usize::from(!thumbnail), "{case}");
        assert!(icons.block_sheets.is_empty(), "{case}");
    }
    // Scale and translation reshape the cube, so it draws as a model with a block sheet.
    for (case, transformation) in [
        (
            "scale",
            CustomTransformation {
                scale: [0.5; 3],
                ..identity
            },
        ),
        (
            "translation",
            CustomTransformation {
                translation: [0.25, 0.0, 0.0],
                ..identity
            },
        ),
    ] {
        let blocks = CustomBlocks {
            blocks: vec![block(
                "test:cube",
                1,
                CustomBlockVisuals {
                    base: CustomVisualComponents {
                        geometry: Some(super::super::FULL_BLOCK.into()),
                        materials: material(None, None),
                        transformation: Some(transformation),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into(),
            ..Default::default()
        };
        let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
        assert_eq!(
            compiled.overlay.visuals[0].kind,
            VisualKind::Model,
            "{case}"
        );
        let icons = custom_block_icons(
            &compiled.overlay,
            &blocks,
            false,
            &[("test:cube".into(), "test:cube".into())],
            None,
        );
        assert!(icons.misses.is_empty(), "{case}");
        assert_eq!(icons.block_sheets.len(), 1, "{case}");
        assert_eq!(
            icons.block_sheets[0].identifier.as_ref(),
            "test:cube",
            "{case}"
        );
    }
}

// Cached packs supply scalar cube textures independently of custom visual components.
#[test]
fn packcache_custom_block_items_draw_when_requested() {
    use super::super::super::item_icons::custom_block_icons;
    let Some(dir) = std::env::var_os("CINNABAR_PACKCACHE_DIR") else {
        eprintln!(
            "skipping packcache_custom_block_items_draw_when_requested: fixture unavailable; requires CINNABAR_PACKCACHE_DIR containing offline cached packs"
        );
        return;
    };
    let mut eligible = 0usize;
    let mut checked = 0usize;
    for entry in std::fs::read_dir(dir).expect("packcache dir").flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some((_, version)) = name
            .strip_suffix(".zip")
            .and_then(|stem| stem.split_once('_'))
        else {
            continue;
        };
        if version.is_empty() {
            continue;
        }
        let Some(view) = super::super::super::local_pack::local_pack_view_at(&path) else {
            continue;
        };
        let custom = view
            .merged_json_object("blocks.json", None)
            .iter()
            .filter_map(|(block_name, entry)| {
                entry.get("textures")?.as_str()?;
                block_name
                    .contains(':')
                    .then(|| block(block_name, 1, CustomBlockVisuals::default()))
            })
            .collect::<Vec<_>>();
        if custom.is_empty() {
            continue;
        }
        eligible += custom.len();
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
        let icons = custom_block_icons(&compiled.overlay, &blocks, false, &items, None);
        let textured = items.len() - compiled.gaps.missing_textures as usize;
        assert!(
            icons.icons.len() >= textured,
            "{name}: {} of {textured} textured blocks drew; misses {:?}",
            icons.icons.len(),
            icons.misses
        );
        checked += icons.icons.len();
    }
    if eligible == 0 {
        eprintln!(
            "skipping packcache_custom_block_items_draw_when_requested: fixture unavailable; no admitted scalar custom block bindings in cached packs"
        );
        return;
    }
    assert!(checked > 0, "fixture must contain drawable custom blocks");
    eprintln!("{checked} packcache custom block item icons drawn");
}

// Icons compile beside other subscribers but only once blocks supply their thumbnails.
#[test]
fn join_preparation_icons_carry_the_compiled_block_sheets() {
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
    let inputs = Arc::new(pack_runtime::PackInputs {
        blocks: CustomBlocks {
            blocks: vec![lucky].into(),
            vanilla_blocks: Default::default(),
            skipped: 0,
        },
        block_items: vec![("test:lucky".into(), "test:lucky".into())],
        ..Default::default()
    });
    let application = super::super::super::resource_packs::prepare_validated_application(
        view().shared_stack(),
        inputs,
    );
    assert!(application.block_overlay.is_some());
    let icons = application.item_icons.expect("block item icons");
    assert_eq!(icons.block_sheets.len(), 1);
    assert_eq!(icons.block_sheets[0].identifier.as_ref(), "test:lucky");
}

#[test]
fn authored_inventory_visuals_use_their_own_geometry_and_materials() {
    let blocks = CustomBlocks {
        blocks: vec![block(
            "fixture:shaped",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("geometry.gen".into()),
                    materials: materials("gen"),
                    transformation: Some(CustomTransformation {
                        rotation: [0, 1, 0],
                        scale: [0.5; 3],
                        translation: [0.25; 3],
                    }),
                    ..Default::default()
                },
                item_visual: Some(CustomVisualComponents {
                    geometry: Some("minecraft:geometry.full_block".into()),
                    materials: materials("lucky").map(|mut materials| {
                        materials[0].face_dimming = Some(false);
                        materials
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )]
        .into(),
        ..Default::default()
    };
    let view = view();
    let world = compile_block_overlay(&view, &blocks, false, None).unwrap();
    assert_eq!(world.overlay.visuals[0].kind, VisualKind::Model);
    let (items, definitions) = super::super::compile_block_item_overlay(
        &view,
        &blocks,
        None,
        assets::MAX_TEXTURE_LAYERS as u32,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.overlay.visuals.len(), 1);
    assert_eq!(items.overlay.visuals[0].kind, VisualKind::Cube);
    assert_eq!(definitions.blocks[0].visual.base.transformation, None);
    let icons = crate::runtime::network::item_icons::custom_block_icons(
        &items.overlay,
        &definitions,
        false,
        &[("fixture:shaped".into(), "fixture:shaped".into())],
        None,
    );
    assert_eq!(icons.icons.len(), 1);
    assert_eq!(icons.block_sheets.len(), 1);
    assert!(icons.misses.is_empty());
    assert!(
        icons.icons[0]
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .all(|pixel| pixel[2] == 0)
    );
}

#[test]
fn borrowed_base_page_textures_draw_custom_inventory_icons() {
    use super::super::super::item_icons::custom_block_icons;
    let base = RuntimeAssets::diagnostic();
    let mut overlay = compiled().overlay;
    overlay.materials[1].texture = assets::TextureRef::new(0, 0).unwrap();
    overlay.visuals[0].faces = [1; 6];
    overlay.textures.clear();
    let blocks = CustomBlocks {
        blocks: vec![block("fixture:borrowed", 1, CustomBlockVisuals::default())].into(),
        ..Default::default()
    };
    let icons = custom_block_icons(
        &overlay,
        &blocks,
        false,
        &[("fixture:borrowed".into(), "fixture:borrowed".into())],
        Some(&base),
    );
    assert_eq!(icons.icons.len(), 1);
    assert_eq!(icons.block_sheets.len(), 1);
    assert!(icons.misses.is_empty());
}

#[test]
fn shared_terrain_catalog_preserves_inventory_output_and_replays_reads() {
    let stack = view().shared_stack();
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        ..Default::default()
    };
    let shared = super::super::SharedTextures::default();
    let world_view = resource_pack::LayeredPackView::tracked(stack.clone());
    let _ = super::super::compile_block_overlay_with_textures(
        &world_view,
        &blocks,
        false,
        None,
        assets::MAX_TEXTURE_LAYERS as u32,
        None,
        Some(&shared),
    );
    let inventory_view = resource_pack::LayeredPackView::tracked(stack.clone());
    let compiled = super::super::compile_block_overlay_with_textures(
        &inventory_view,
        &blocks,
        false,
        None,
        assets::MAX_TEXTURE_LAYERS as u32,
        None,
        Some(&shared),
    )
    .unwrap();
    let fresh_view = resource_pack::LayeredPackView::tracked(stack);
    let fresh = super::super::compile_block_overlay_with_textures(
        &fresh_view,
        &blocks,
        false,
        None,
        assets::MAX_TEXTURE_LAYERS as u32,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        shared.catalog_builds.get(),
        1,
        "inventory reuses the parsed terrain tables"
    );
    assert_eq!(compiled, fresh);
    assert_eq!(
        inventory_view.dependencies().unwrap().snapshot(),
        fresh_view.dependencies().unwrap().snapshot()
    );
}
