use super::*;

fn independent_font(sides: &[u32]) -> Arc<RuntimeFontCatalog> {
    let pages = sides
        .iter()
        .enumerate()
        .map(|(index, &side)| {
            let pixels = vec![255; side as usize * side as usize * 4].into_boxed_slice();
            FontTexturePage {
                source_path: format!("font/independent-{index:02}.png").into(),
                source_bytes: pixels.len() as u32,
                source_sha256: Sha256::digest(&pixels).into(),
                pixels_sha256: Sha256::digest(&pixels).into(),
                width: side,
                height: side,
                pixels: FontPixels::Rgba8(pixels),
            }
        })
        .collect::<Vec<_>>();
    let glyph = GlyphMetrics {
        codepoint: '\u{fffd}',
        page: 0,
        uv: [0, 0, 8, 16],
        bearing: [0, -14],
        advance_64: 8 * 64,
    };
    let mut glyphs = vec![
        GlyphMetrics {
            codepoint: 'A',
            uv: [16, 0, 24, 16],
            ..glyph
        },
        glyph,
    ];
    if sides.len() > 1 {
        glyphs.push(GlyphMetrics {
            codepoint: '一',
            page: 1,
            uv: [32, 0, 40, 16],
            ..glyph
        });
    }
    glyphs.sort_by_key(|g| g.codepoint);
    let bytes = encode_font_catalog([7; 32], &glyphs, &pages).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, [7; 32]).unwrap())
}

fn independent_icons(count: usize, side: u16) -> Arc<RuntimeIconCatalog> {
    let sprite = assets::IconSprite {
        width: side,
        height: side,
        rgba8: vec![255; usize::from(side).pow(2) * 4].into(),
    };
    let sprites = vec![sprite; count];
    let entries = (0..count)
        .map(|i| assets::IconEntry {
            identifier: format!("minecraft:fixture_{i:04}").into(),
            metadata: 0,
            sprite: i as u32,
        })
        .collect::<Vec<_>>();
    Arc::new(
        RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([5; 32], &sprites, &entries).unwrap(),
        )
        .unwrap(),
    )
}

#[test]
fn full_icon_catalog_and_reserved_dynamic_pages_are_admitted_together() {
    let font = independent_font(&[1024, 2048, 2048, 2048]);
    let presentation = UiPresentationRuntime::with_hud_and_icons(
        Arc::clone(&font),
        fixture_hud(),
        independent_icons(735, 16),
    )
    .unwrap();
    assert!(presentation.textures.plan().bytes() <= render_model::MAX_UI_TEXTURE_BYTES);
    assert_eq!(presentation.icon_refs.as_ref().unwrap().len(), 735);
    // The largest icon carrier still fits beside the CJK font; the planner refuses whole
    // catalogs past the byte budget (see render's `planner_checks_entire_catalog_and_all_limits`).
    let large =
        UiPresentationRuntime::with_hud_and_icons(font, fixture_hud(), independent_icons(900, 64))
            .unwrap();
    assert_eq!(large.icon_refs.as_ref().unwrap().len(), 900);
    assert!(large.textures.plan().bytes() <= render_model::MAX_UI_TEXTURE_BYTES);
}

#[test]
fn ordinary_cube_thumbnail_pages_share_the_complete_static_budget() {
    // A 1024-route ceiling is six 256px pages at the retained 18px pitch.
    // This exercises the actual merged font/HUD/icon allocator, not a
    // separately budgeted icon cache or a truncated prefix.
    let font = independent_font(&[1024, 2048, 2048, 2048]);
    let presentation = UiPresentationRuntime::with_hud_and_icons(
        Arc::clone(&font),
        fixture_hud(),
        independent_icons(1024, 16),
    )
    .unwrap();
    assert_eq!(presentation.icon_refs.as_ref().unwrap().len(), 1024);
    assert!(presentation.textures.dynamic_start() < presentation.textures.pages().len());
    assert!(presentation.textures.plan().bytes() <= render_model::MAX_UI_TEXTURE_BYTES);
    assert!(
        UiPresentationRuntime::with_hud_and_icons(font, fixture_hud(), independent_icons(900, 64))
            .is_ok(),
        "the merged font, HUD and icon catalog fits the shared budget whole"
    );
}

#[test]
fn projected_nametag_glyphs_keep_logical_page_order_without_shadow() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::new(independent_font(&[1024, 2048])).unwrap();
    let runtime = UiRuntime::new(1);
    presentation.set_nametag_anchors(vec![super::super::nametags::tests::anchor("A一A")]);
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    // The world glyphs now rasterize into the retained GPU line atlas, not HUD vertices.
    let scene = presentation.nametag_scene();
    assert_eq!(scene.records.len(), 2);
    assert_eq!(scene.records[1].color, [1.0; 4]);
    assert_eq!(scene.atlas.len(), 1);
    assert!(
        scene.atlas[0]
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .all(|pixel| *pixel == [255; 4])
    );
    for batch in input.batches.iter() {
        let logical = batch.texture_page as usize;
        let physical = input.textures.plan().locations()[logical];
        assert_eq!(
            input.textures.plan().buckets()[physical.bucket].dimensions,
            input.textures.pages()[logical].resident_dimensions()
        );
    }
}

#[test]
fn mixed_native_font_pages_keep_source_pixels_with_bounded_gpu_residency() {
    let font = independent_font(&[1024, 2048, 2048, 2048]);
    let presentation = UiPresentationRuntime::new(Arc::clone(&font)).unwrap();
    let font_bytes: usize = presentation.textures.pages()[..font.pages().len()]
        .iter()
        .map(|page| {
            let [width, height] = page.resident_dimensions();
            width as usize * height as usize * page.format().bytes_per_texel()
        })
        .sum();
    let page_bytes = |side: u32| side as usize * side as usize * 4;
    let small_page_bytes = page_bytes(render_model::UI_DYNAMIC_PAGE_SIDE);
    let dynamic_bytes = render_model::UI_LOCAL_FONT_PAGE_OFFSET * small_page_bytes
        + page_bytes(render_model::UI_LOCAL_FONT_PAGE_SIDE)
        + render_model::MAX_UI_FALLBACK_FONT_PAGES
            * render_model::UI_FALLBACK_FONT_PAGE_SIDE as usize
            * render_model::UI_FALLBACK_FONT_PAGE_SIDE as usize;
    assert_eq!(
        presentation.textures.plan().bytes(),
        font_bytes
            + small_page_bytes
            + dynamic_bytes
            + render_model::MAX_UI_ART_PAGES * page_bytes(render_model::UI_ART_PAGE_SIDE)
    );
    for (index, source) in font.pages().iter().enumerate() {
        let page = &presentation.textures.pages()[index];
        assert_eq!(page.dimensions(), [source.width, source.height]);
        assert_eq!(page.pixels().as_ptr(), source.pixels.bytes().as_ptr());
    }
}

#[test]
fn actual_producer_publish_and_extraction_keep_revision_and_publication_identity_joined() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::new(independent_font(&[1024, 2048])).unwrap();
    let runtime = UiRuntime::new(1);
    let stats = UiRenderStats::default();
    let mut scene = UiRenderScene::default();
    let mut previous_revision = 0;
    for frame in 0..100_u32 {
        // Alternate sizes so every frame carries a changed payload.
        let size = [800 + frame % 2, 600];
        let input = presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(
            input.revision > previous_revision,
            "actual producer assigns a new revision to each changed frame"
        );
        previous_revision = input.revision;
        scene.publish(input.clone(), &stats).unwrap();
        let publication = Arc::clone(scene.input.as_ref().unwrap());
        scene.publish(input, &stats).unwrap();
        assert!(
            Arc::ptr_eq(scene.input.as_ref().unwrap(), &publication),
            "equivalent republish preserves accepted Arc"
        );
        // Render-world extraction clones the scene.
        let extracted = scene.clone();
        assert!(Arc::ptr_eq(extracted.input.as_ref().unwrap(), &publication));
    }
}

/// An unchanged frame must keep its revision and publication so the GPU upload fast path fires.
#[test]
fn unchanged_frames_keep_revision_and_accepted_publication() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::new(independent_font(&[256])).unwrap();
    let runtime = UiRuntime::new(1);
    let stats = UiRenderStats::default();
    let mut scene = UiRenderScene::default();
    let build = |presentation: &mut UiPresentationRuntime, size| {
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    };
    let first = build(&mut presentation, [800, 600]);
    scene.publish(first.clone(), &stats).unwrap();
    let publication = Arc::clone(scene.input.as_ref().unwrap());
    for _ in 0..10 {
        let again = build(&mut presentation, [800, 600]);
        assert_eq!(again, first);
        assert!(Arc::ptr_eq(&again.vertices, &first.vertices));
        scene.publish(again, &stats).unwrap();
        assert!(Arc::ptr_eq(scene.input.as_ref().unwrap(), &publication));
    }
    let resized = build(&mut presentation, [801, 600]);
    assert_eq!(resized.revision, first.revision + 1);
    let back = build(&mut presentation, [800, 600]);
    assert_eq!(back.revision, resized.revision + 1);
    assert_eq!(back.vertices, first.vertices);
}

#[test]
fn actual_preview_updates_share_static_pages_and_retire_superseded_scenes() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::new(independent_font(&[256])).unwrap();
    presentation.set_player_preview_skin(None, player_preview::PlayerPreviewPose::default());
    let retained = Arc::clone(&presentation.textures);
    let static_pointer = retained.pages()[0].pixels().as_ptr();
    let retained_dynamic = retained.pages()[retained.dynamic_start()].pixels().to_vec();
    let stats = UiRenderStats::default();
    let mut main = UiRenderScene::default();
    let mut extracted = UiRenderScene::default();
    assert!(extracted.input.is_none());
    let runtime = UiRuntime::new(1);
    let mut prior = None;
    for frame in 1..=100 {
        // The view pitch redraws the hand rasters; world yaw no longer turns the model.
        presentation.set_player_preview_skin(
            None,
            player_preview::PlayerPreviewPose::new(40.0, 40.0, frame as f32, false),
        );
        assert_eq!(
            presentation.textures.pages()[0].pixels().as_ptr(),
            static_pointer
        );
        let input = presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [800, 600],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        main.publish(input, &stats).unwrap();
        // Same single-target replacement used by Bevy's ExtractResource.
        extracted = main.clone();
        if let Some(old) = prior.take() {
            assert!(std::sync::Weak::upgrade(&old).is_none());
        }
        prior = Some(Arc::downgrade(&presentation.textures));
        for index in 0..presentation.textures.dynamic_start() {
            assert!(std::ptr::eq(
                presentation.textures.pages()[index].pixels(),
                retained.pages()[index].pixels()
            ));
        }
    }
    assert_ne!(presentation.textures.identity(), retained.identity());
    assert_eq!(
        retained.pages()[retained.dynamic_start()].pixels(),
        retained_dynamic
    );
    let before = Arc::clone(&presentation.textures);
    presentation.set_player_preview_skin(
        None,
        player_preview::PlayerPreviewPose::new(40.0, 40.0, 100.0, false),
    );
    assert!(Arc::ptr_eq(&before, &presentation.textures));
    assert!(extracted.input.is_some());
}

/// Mouse look must not rebuild the dynamic page while neither the paper doll nor CPU hands show.
#[test]
fn hidden_preview_defers_pose_changes_until_shown() {
    let mut presentation = UiPresentationRuntime::new(independent_font(&[256])).unwrap();
    let pose =
        |yaw: f32, pitch: f32| player_preview::PlayerPreviewPose::new(yaw, yaw, pitch, false);
    presentation.sync_player_preview(None, pose(0.0, 0.0), false, false, 0.0);
    let drawn = Arc::clone(&presentation.textures);
    for frame in 1..=100 {
        let turn = frame as f32;
        presentation.sync_player_preview(None, pose(turn, turn / 4.0), false, false, 0.0);
        // Yaw alone does not move the CPU hands.
        presentation.sync_player_preview(None, pose(turn, 0.0), false, true, 0.0);
    }
    assert!(Arc::ptr_eq(&drawn, &presentation.textures));
    presentation.sync_player_preview(None, pose(100.0, 0.0), true, false, 0.0);
    assert!(!Arc::ptr_eq(&drawn, &presentation.textures));
    let shown = Arc::clone(&presentation.textures);
    presentation.sync_player_preview(None, pose(100.0, 5.0), false, true, 0.0);
    assert!(!Arc::ptr_eq(&shown, &presentation.textures));
    let hands = Arc::clone(&presentation.textures);
    presentation.sync_player_preview(
        Some(&vec![255; 64 * 64 * 4]),
        pose(7.0, 5.0),
        false,
        false,
        0.0,
    );
    assert!(
        !Arc::ptr_eq(&hands, &presentation.textures),
        "skin changes still redraw"
    );
}

#[test]
fn resize_and_session_reset_do_not_reload_static_pixels_or_retain_dynamic_ownership() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::new(independent_font(&[256])).unwrap();
    let runtime = UiRuntime::new(1);
    presentation.set_player_preview_skin(None, player_preview::PlayerPreviewPose::default());
    let first = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let static_pixel = first.textures.pages()[0].pixels().as_ptr();
    let resized = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1200, 800],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(Arc::ptr_eq(&first.textures, &resized.textures));
    assert!(resized.revision > first.revision);
    let reset = presentation
        .build(
            &player_runtime,
            &UiRuntime::new(2),
            0,
            [1200, 800],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert_eq!(reset.textures.pages()[0].pixels().as_ptr(), static_pixel);
    assert_eq!(
        reset.textures.static_identity(),
        first.textures.static_identity()
    );
    // Session pages clear; the art pages keep the launcher's title logo.
    let dynamic = reset.textures.dynamic_start();
    assert!(
        reset.textures.pages()[dynamic..dynamic + render_model::MAX_UI_DYNAMIC_PAGES]
            .iter()
            .all(|p| p.pixels().iter().all(|&v| v == 0))
    );
    assert!(presentation.player_preview_icon.is_none());
    assert!(reset.revision > resized.revision);
    let stats = UiRenderStats::default();
    let mut scene = UiRenderScene::default();
    scene.publish(reset, &stats).unwrap();
    assert!(scene.publish(first, &stats).is_err());
    assert!(scene.input.is_none());
    assert_eq!(stats.snapshot().accepted_revision, None);
}

#[test]
fn preview_changes_do_not_reread_menu_files_or_copy_cached_menu_pages() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ui-independent-art-{}-{unique}.png",
        std::process::id()
    ));
    image::RgbaImage::from_pixel(96, 96, image::Rgba([40, 80, 120, 255]))
        .save(&path)
        .unwrap();
    let mut presentation = UiPresentationRuntime::new(independent_font(&[256])).unwrap();
    let name = path.to_string_lossy().into_owned();
    presentation.sync_menu_artwork(vec![(name.clone(), 512)]);
    presentation.finish_menu_artwork();
    let menu = presentation.menu_artwork_icon(&name).unwrap();
    let retained = Arc::clone(&presentation.textures);
    let menu_pixels = retained.pages()[menu.page as usize].pixels().as_ptr();
    std::fs::remove_file(&path).unwrap();
    for frame in 0..100 {
        presentation.set_player_preview_skin(
            None,
            player_preview::PlayerPreviewPose::new(frame as f32, 20.0, 0.0, false),
        );
        assert_eq!(presentation.menu_artwork_icon(&name), Some(menu));
        assert_eq!(
            presentation.textures.pages()[menu.page as usize]
                .pixels()
                .as_ptr(),
            menu_pixels
        );
        assert!(
            presentation.menu_artwork_loader.pending().is_none(),
            "admitted artwork retires the pending raster owner"
        );
    }
    let preview_pixels = presentation.textures.pages()[presentation.textures.dynamic_start()]
        .pixels()
        .as_ptr();
    presentation.sync_menu_artwork(Vec::new());
    presentation.finish_menu_artwork();
    assert!(presentation.menu_artwork_icon(&name).is_none());
    assert_eq!(
        presentation.textures.pages()[presentation.textures.dynamic_start()]
            .pixels()
            .as_ptr(),
        preview_pixels
    );
}

// A changed art set decodes on the worker: the frame returns before the art
// exists, and a later frame installs it.
#[test]
fn menu_art_decodes_off_the_frame() {
    let path = std::env::temp_dir().join(format!("ui-worker-art-{}.png", std::process::id()));
    image::RgbaImage::from_pixel(2048, 2048, image::Rgba([200, 40, 40, 255]))
        .save(&path)
        .unwrap();
    let mut presentation = UiPresentationRuntime::new(independent_font(&[256])).unwrap();
    let name = path.to_string_lossy().into_owned();
    presentation.sync_menu_artwork(vec![(name.clone(), 512)]);
    assert!(presentation.menu_artwork_icon(&name).is_none());
    presentation.finish_menu_artwork();
    let icon = presentation.menu_artwork_icon(&name).expect("installed");
    assert_eq!(icon.uv[2] - icon.uv[0], 512);
    std::fs::remove_file(&path).unwrap();
}

// Server icons pack onto the last dynamic page and win over the vanilla atlas.
#[test]
fn session_icons_pack_onto_the_last_dynamic_page() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let icons = Arc::new(SessionIcons {
        icons: vec![
            SessionIcon {
                identifier: "test:gem".into(),
                metadata: 0,
                width: 2,
                height: 1,
                rgba8: vec![255, 0, 0, 255, 0, 255, 0, 255].into(),
            },
            SessionIcon {
                identifier: "test:bad".into(),
                metadata: 0,
                width: 4,
                height: 4,
                rgba8: vec![0; 3].into(),
            },
        ],
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    let icon = presentation.item_icon("test:gem", 0).expect("session icon");
    let dynamic_start = presentation.textures.dynamic_start();
    assert_eq!(
        usize::from(icon.page),
        dynamic_start + dynamic_textures::SESSION_ICON_PAGE
    );
    assert_eq!(icon.uv, [1, 1, 3, 2], "one-pixel gutter around the sprite");
    assert!(
        presentation.item_icon("test:bad", 0).is_none(),
        "malformed sprites are left out"
    );

    session_icons::observe(&mut presentation, None);
    assert!(presentation.item_icon("test:gem", 0).is_none());
}

// Server glyph sheets land on the trailing dynamic pages and only the session font resolves them.
#[test]
fn session_glyph_sheets_extend_the_font_and_reset_with_the_session() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    assert!(presentation.font.glyph('\u{e005}').is_none());
    presentation.set_nametag_anchors(vec![super::super::nametags::NametagAnchor {
        runtime_id: 1,
        position: bevy::math::Vec3::new(0.0, 2.0, 0.0),
        lines: vec![Arc::from("\u{e005}")],
        depth_tested: false,
        text_alpha: 1.0,
        distance: 5.0,
    }]);
    let before = presentation.nametag_scene();
    let mut rgba8 = vec![0u8; 128 * 128 * 4];
    for y in 0..8usize {
        for x in 0..8usize {
            let at = ((y * 128) + 5 * 8 + x) * 4;
            rgba8[at..at + 4].copy_from_slice(&[255; 4]);
        }
    }
    let sheets = Arc::new(SessionGlyphSheets {
        named: Default::default(),
        prepared: Default::default(),
        cells: assets::extract_cells(&assets::GlyphSheet {
            high_byte: 0xe0,
            width: 128,
            height: 128,
            rgba8: rgba8.into(),
        }),
    });
    session_glyphs::observe(&mut presentation, Some(&sheets));
    let changed = presentation.nametag_scene();
    assert!(!Arc::ptr_eq(
        &before.atlas[0].rgba8,
        &changed.atlas[0].rgba8
    ));
    assert_ne!(before.records[1].rect, changed.records[1].rect);
    let glyph = *presentation.font.glyph('\u{e005}').expect("sheet glyph");
    let dynamic_start = presentation.textures.dynamic_start();
    assert_eq!(usize::from(glyph.page), dynamic_start + 10);
    assert_eq!(glyph.advance_64, 18 * 64);
    let page = &presentation.textures.pages()[usize::from(glyph.page)];
    let [left, top, ..] = glyph.uv;
    assert_eq!(
        page.pixels()
            [(usize::from(top) * page.dimensions()[0] as usize + usize::from(left)) * 4 + 3],
        255
    );
    assert!(presentation.base_font.glyph('\u{e005}').is_none());

    session_glyphs::observe(&mut presentation, None);
    assert!(presentation.font.glyph('\u{e005}').is_none());
    let restored = presentation.nametag_scene();
    assert_eq!(restored.records[1].rect, before.records[1].rect);
    assert_eq!(restored.atlas[0].rgba8, before.atlas[0].rgba8);
    assert_eq!(
        presentation.textures.pages().len(),
        dynamic_start + render_model::MAX_UI_DYNAMIC_PAGES + render_model::MAX_UI_ART_PAGES
    );
}

#[test]
fn startup_named_font_survives_session_alias_updates_without_changing_default_metrics() {
    let base = independent_font(&[64]);
    let private = independent_font(&[32]);
    let startup = Arc::new(
        base.with_named_font(ui::mod_panel::FONT_NAME, &private)
            .unwrap(),
    );
    let mut presentation = UiPresentationRuntime::new(Arc::clone(&startup)).unwrap();
    let original = *presentation.font.glyph('A').unwrap();
    let alias = *presentation
        .font
        .font_named(ui::mod_panel::FONT_NAME)
        .glyph('A')
        .unwrap();
    assert_eq!(alias.page, 1);
    let textures = Arc::clone(&presentation.textures);
    let cell = assets::CellGlyph {
        codepoint: 'A',
        size: [1, 1],
        rgba8: vec![128; 4].into(),
        bearing: [0, 0],
        advance_64: 64,
        draw_size_64: [64, 64],
    };
    let sheets = Arc::new(SessionGlyphSheets::with_named(
        Vec::new(),
        std::collections::BTreeMap::from([(ui::mod_panel::FONT_NAME.into(), vec![cell])]),
    ));
    session_glyphs::observe(&mut presentation, Some(&sheets));
    assert_eq!(presentation.font.glyph('A'), Some(&original));
    assert_eq!(
        presentation
            .font
            .font_named(ui::mod_panel::FONT_NAME)
            .glyph('A'),
        Some(&alias)
    );
    assert_eq!(presentation.textures.pages()[1], textures.pages()[1]);
    let count = presentation.textures.pages().len();
    session_glyphs::observe(&mut presentation, Some(&sheets));
    assert_eq!(presentation.textures.pages().len(), count);
    session_glyphs::observe(&mut presentation, None);
    assert_eq!(presentation.font.glyph('A'), Some(&original));
    assert_eq!(
        presentation
            .font
            .font_named(ui::mod_panel::FONT_NAME)
            .glyph('A'),
        Some(&alias)
    );
    assert!(Arc::ptr_eq(&presentation.font, &startup));
    assert_eq!(presentation.textures.pages().len(), count);
}

/// Asserts `identifier` resolves to an icon whose UV rect is opaque on its uploaded page.
fn assert_icon_drawable(presentation: &UiPresentationRuntime, identifier: &str) {
    let icon = presentation
        .item_icon(identifier, 0)
        .unwrap_or_else(|| panic!("{identifier} has no icon"));
    let page = presentation
        .textures
        .pages()
        .get(usize::from(icon.page))
        .unwrap_or_else(|| panic!("{identifier} points past the uploaded pages"));
    let [width, height] = page.dimensions();
    assert!(u32::from(icon.uv[2]) <= width && u32::from(icon.uv[3]) <= height);
    let opaque = (icon.uv[1]..icon.uv[3]).any(|y| {
        (icon.uv[0]..icon.uv[2])
            .any(|x| page.pixels()[(usize::from(y) * width as usize + usize::from(x)) * 4 + 3] != 0)
    });
    assert!(opaque, "{identifier} points at blank texels");
}

// Hotbar icons stay drawable with and without server glyph sheets, a server UI pack and icons.
#[test]
fn item_icons_survive_session_glyphs_ui_pack_and_server_icons() {
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        fixture_font(),
        fixture_hud(),
        independent_icons(40, 16),
    )
    .unwrap();
    presentation
        .enable_json_ui(super::forms::tests::mini_carrier())
        .unwrap();
    assert_icon_drawable(&presentation, "minecraft:fixture_0007");

    let icons = Arc::new(SessionIcons {
        icons: vec![SessionIcon {
            identifier: "test:gem".into(),
            metadata: 0,
            width: 4,
            height: 4,
            rgba8: vec![255; 64].into(),
        }],
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    let mut rgba8 = vec![0u8; 128 * 128 * 4];
    rgba8[..8 * 128 * 4].fill(255);
    let sheets = Arc::new(SessionGlyphSheets {
        named: Default::default(),
        prepared: Default::default(),
        cells: assets::extract_cells(&assets::GlyphSheet {
            high_byte: 0xe0,
            width: 128,
            height: 128,
            rgba8: rgba8.into(),
        }),
    });
    session_glyphs::observe(&mut presentation, Some(&sheets));
    presentation.set_server_ui_pack(&super::forms::ServerUiPack::default());
    for identifier in [
        "minecraft:fixture_0007",
        "minecraft:fixture_0039",
        "test:gem",
    ] {
        assert_icon_drawable(&presentation, identifier);
    }
    session_icons::observe(&mut presentation, None);
    session_glyphs::observe(&mut presentation, None);
    assert_icon_drawable(&presentation, "minecraft:fixture_0007");
}

// Local-only: the production carriers plus session glyphs and a UI pack still upload every page.
#[test]
fn real_carriers_keep_icons_drawable_with_session_pages() {
    use super::forms::pack_harness;
    let local = |name: &str| {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("../.local/assets/compiled")
                .join(name),
        )
        .ok()
    };
    let (Some(hud), Some(icons), Some(carrier)) = (
        local("vanilla-v1.mcbehud"),
        local("vanilla-v1.mcbeico"),
        pack_harness::carrier(),
    ) else {
        eprintln!(
            "skipping real_carriers_keep_icons_drawable_with_session_pages: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let icons = Arc::new(RuntimeIconCatalog::decode(&icons).unwrap());
    let sample = icons.entries()[icons.entries().len() / 2]
        .identifier
        .to_string();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        pack_harness::font(),
        Arc::new(RuntimeHudCatalog::decode(&hud).unwrap()),
        icons,
    )
    .unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    eprintln!(
        "pages {} dynamic_start {} bytes {}",
        presentation.textures.pages().len(),
        presentation.textures.dynamic_start(),
        presentation.textures.plan().bytes()
    );
    assert_icon_drawable(&presentation, &sample);
    let sheets = Arc::new(SessionGlyphSheets {
        named: Default::default(),
        prepared: Default::default(),
        cells: assets::extract_cells(&assets::GlyphSheet {
            high_byte: 0xe0,
            width: 128,
            height: 128,
            rgba8: vec![255; 128 * 128 * 4].into(),
        }),
    });
    session_glyphs::observe(&mut presentation, Some(&sheets));
    assert!(presentation.font.glyph('\u{e005}').is_some());
    assert_icon_drawable(&presentation, &sample);
    let installed = presentation.textures.pages()[presentation.textures.dynamic_start() + 10]
        .pixels()
        .iter()
        .any(|byte| *byte != 0);
    assert!(installed, "the glyph page was not installed");
}

/// Mouse look with the preview hidden; run with `-- --ignored --nocapture`.
#[test]
#[ignore = "benchmark"]
fn frame_cost_bench_hidden_player_preview_while_turning() {
    let skin = vec![200; 64 * 64 * 4];
    let pose = |frame: u32| player_preview::PlayerPreviewPose::new(frame as f32, 0.0, 0.0, false);
    let time = |mut frame: Box<dyn FnMut(u32) + '_>| {
        let started = std::time::Instant::now();
        (0..200).for_each(&mut frame);
        started.elapsed().as_secs_f64() * 1e3 / 200.0
    };
    let mut presentation = UiPresentationRuntime::new(independent_font(&[256])).unwrap();
    let old = time(Box::new(|frame| {
        presentation.set_player_preview_skin(Some(&skin), pose(frame));
    }));
    let mut presentation = UiPresentationRuntime::new(independent_font(&[256])).unwrap();
    let new = time(Box::new(|frame| {
        presentation.sync_player_preview(Some(&skin), pose(frame), false, false, 0.0);
    }));
    eprintln!("FRAME_COST player_preview_turning_hidden: old={old:.3}ms new={new:.3}ms");
}

/// Coverage font pages add their own buckets beside the art and local-font slots and still fit.
#[test]
fn coverage_font_pages_fit_the_bucket_budget_with_every_reserved_slot() {
    let font = Arc::new(
        (*independent_font(&[1024, 2048, 2048]))
            .clone()
            .with_coverage_pages(),
    );
    let presentation =
        UiPresentationRuntime::with_hud(Arc::clone(&font), crate::test_support::fixture_hud())
            .unwrap();
    let plan = presentation.textures.plan();
    assert!(plan.buckets().len() <= render_model::MAX_UI_TEXTURE_BUCKETS);
    for (index, page) in presentation.textures.pages().iter().enumerate() {
        let fallback_start =
            presentation.textures.dynamic_start() + render_model::UI_FALLBACK_FONT_PAGE_OFFSET;
        let fallback_end = fallback_start + render_model::MAX_UI_FALLBACK_FONT_PAGES;
        let expected =
            if index < font.pages().len() || (fallback_start..fallback_end).contains(&index) {
                render_model::UiTextureFormat::Coverage
            } else {
                render_model::UiTextureFormat::Rgba8
            };
        assert_eq!(page.format(), expected, "page {index}");
    }
    let local = presentation.textures.dynamic_start() + render_model::UI_LOCAL_FONT_PAGE_OFFSET;
    let bucket = plan.buckets()[plan.locations()[local].bucket];
    assert_eq!(bucket.format, render_model::UiTextureFormat::Rgba8);
    assert_eq!(
        bucket.dimensions,
        [render_model::UI_LOCAL_FONT_PAGE_SIDE; 2]
    );
}

#[test]
fn multiple_coverage_families_use_their_actual_bytes_during_startup_admission() {
    let family = (*independent_font(&[2048; 3]))
        .clone()
        .with_coverage_pages();
    let font = family
        .with_named_font("body", &family)
        .unwrap()
        .with_named_font("heading", &family)
        .unwrap();
    let presentation =
        UiPresentationRuntime::with_hud(Arc::new(font), crate::test_support::fixture_hud())
            .unwrap();
    let plan = presentation.textures.plan();
    assert!(plan.bytes() < render_model::MAX_UI_TEXTURE_BYTES);
    assert!(plan.validate_device(4096, 256).is_ok());
    for index in 0..9 {
        let location = plan.locations()[index];
        assert_eq!(
            plan.buckets()[location.bucket].format,
            render_model::UiTextureFormat::Coverage
        );
    }
}
