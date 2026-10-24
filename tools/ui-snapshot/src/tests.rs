use crate::{carriers, fixtures, render};

#[test]
fn tiled_image_capture_covers_the_viewport_with_the_original_checker() {
    let state = fixtures::states()
        .find(|state| state.name == "jsonui-tiled-image")
        .unwrap();
    let png = render::png(state, std::path::Path::new("unused")).unwrap();
    let pixels = image::load_from_memory(&png).unwrap().to_rgba8();
    let colors = [*pixels.get_pixel(0, 0), *pixels.get_pixel(1, 0)];
    assert_ne!(colors[0], colors[1]);
    assert!(colors.iter().all(|pixel| pixel[3] == 255));
    for (x, y, pixel) in pixels.enumerate_pixels() {
        assert_eq!(*pixel, colors[((x + y) % 2) as usize], "checker at {x},{y}");
    }
}

#[test]
fn extreme_tiled_image_capture_covers_every_pixel() {
    let state = fixtures::states()
        .find(|state| state.name == "jsonui-tiled-image-extreme")
        .unwrap();
    let png = render::png(state, std::path::Path::new("unused")).unwrap();
    let pixels = image::load_from_memory(&png).unwrap().to_rgba8();
    assert!(pixels.pixels().all(|pixel| pixel[3] == 255));
}

#[test]
fn every_registered_state_renders_identical_png_bytes() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let mut names = std::collections::HashSet::new();
    for state in fixtures::states() {
        assert!(names.insert(state.name), "duplicate fixture {}", state.name);
        let first = match render::png(state, &dir) {
            Ok(png) => png,
            Err(carriers::Error::Missing(message)) => {
                eprintln!("skipping {}: missing fixture {message}", state.name);
                continue;
            }
            Err(error) => panic!("{}: {error:?}", state.name),
        };
        let second = render::png(state, &dir).unwrap();
        assert_eq!(first, second, "{} PNG bytes changed", state.name);
        let image = image::load_from_memory(&first).unwrap().to_rgba8();
        assert_eq!([image.width(), image.height()], state.size);
        assert!(
            image.pixels().any(|pixel| pixel[3] != 0),
            "{} is blank",
            state.name
        );
    }
}

#[test]
fn absent_and_broken_carriers_have_different_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        carriers::load(dir.path(), false),
        Err(carriers::Error::Missing(_))
    ));
    std::fs::write(dir.path().join(assets::carriers::FONT.output), b"invalid").unwrap();
    assert!(matches!(
        carriers::load(dir.path(), false),
        Err(carriers::Error::Invalid(_))
    ));
}

#[test]
fn oreui_artwork_requires_an_authenticated_pinned_carrier() {
    use assets::oreui_icons::{
        OREUI_ICONS_SOURCE_MANIFEST, OreUiIcon, OreUiIconFrame, encode_oreui_icons,
    };

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(assets::carriers::OREUI_ICONS.output);
    assert!(matches!(
        carriers::validate_oreui(dir.path()),
        Err(carriers::Error::Missing(_))
    ));
    let icon = OreUiIcon {
        key: "fixture".into(),
        width: 1,
        height: 1,
        frames: vec![OreUiIconFrame {
            millis: 0,
            rgba8: vec![255; 4].into(),
        }],
    };
    let manifest = assets::canonical_source_manifest_sha256(OREUI_ICONS_SOURCE_MANIFEST);
    let valid = encode_oreui_icons(manifest, std::slice::from_ref(&icon)).unwrap();
    std::fs::write(&path, &valid).unwrap();
    carriers::validate_oreui(dir.path()).unwrap();

    let mut damaged = valid;
    *damaged.last_mut().unwrap() ^= 1;
    let unpinned = encode_oreui_icons([0; 32], &[icon]).unwrap();
    for bytes in [b"invalid".to_vec(), damaged, unpinned] {
        std::fs::write(&path, bytes).unwrap();
        let Err(carriers::Error::Invalid(message)) = carriers::validate_oreui(dir.path()) else {
            panic!("broken artwork must fail before the optional loader can supply fallback art");
        };
        assert!(message.contains(assets::carriers::OREUI_ICONS.output));
        assert!(message.contains("make assets"));
    }
}

#[test]
fn snapshot_settings_painter_samples_the_attached_original_artwork() {
    use oreui::menu_screen::{MenuGeometry, MenuInput};
    use ui_engine::engine::screen::{PaintFrame, Screen};

    let font = ui_engine::test_support::fixture_font();
    let solid_page = font.pages().len() as u16;
    let first = solid_page + 1;
    let mut screen = render::oreui_screen(first, oreui::test_support::settings_icon_images());
    let view = fixtures::settings("video_forced_index");
    let size = [1280.0, 720.0];
    let artwork = Default::default();
    let mut nodes = Vec::new();
    let mut layouts = ui::TextLayoutCache::new(4096, 32 * 1024 * 1024);
    let mut scrolls = ui_engine::menu_scroll::MenuScrolls::default();
    for now in [0.0, render::CAPTURE_SECONDS] {
        nodes.clear();
        let mut next = 1;
        let mut frame = PaintFrame {
            layouts: &mut layouts,
            font: &font,
            metrics: ui_engine::text_metrics::TextMetrics::for_viewport(
                size.map(|value| value as u32),
                ui::DpiScale::new(1.0).unwrap(),
                Some(2),
            ),
            solid_page,
            safe_area: ui::SafeArea::ZERO,
            content: size,
            translate: &|_| None,
            language: [0; 3],
            now,
            nodes: &mut nodes,
            next: &mut next,
            renderers: &mut [],
        };
        screen
            .paint(
                MenuInput {
                    view: &view,
                    size,
                    portrait: None,
                    translate: frame.translate,
                    artwork: &artwork,
                    title_artwork: None,
                    scrolls: &mut scrolls,
                    geometry: MenuGeometry::default(),
                },
                &mut frame,
            )
            .unwrap();
        screen
            .take_paint()
            .unwrap()
            .apply_entrance(frame.nodes, size)
            .unwrap();
    }
    let mut tree = ui::UiTree::new(nodes).unwrap();
    tree.layout(
        ui_engine::engine::rect(0.0, 0.0, size[0], size[1]).unwrap(),
        ui::UiScale::default(),
        ui::SafeArea::ZERO,
    )
    .unwrap();
    let list = tree.build_draw_list().unwrap();
    assert!(
        list.batches.iter().any(|batch| batch.texture_page == first),
        "settings icons must sample their registered artwork page"
    );
}

/// Optional bundle failures must remain visible when the standalone tool falls back.
#[test]
fn missing_artwork_carrier_warning_reaches_stderr() {
    const CHILD: &str = "CINNABAR_SNAPSHOT_LOG_TEST";
    if let Some(dir) = std::env::var_os(CHILD) {
        super::init_logging();
        oreui::images::load_optional_oreui_images(std::path::Path::new(&dir));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::missing_artwork_carrier_warning_reaches_stderr",
            "--nocapture",
        ])
        .env(CHILD, dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "child test failed: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("shipped artwork unavailable"),
        "the missing carrier warning was lost: {output:?}"
    );
}

#[test]
fn world_tab_snapshots_cover_the_shared_category_catalog() {
    use crate::fixtures::Fixture;
    use launcher::local_worlds::Tab;
    for &tab in Tab::ALL {
        let name = format!(
            "world-create-{}",
            tab.label().to_ascii_lowercase().replace(' ', "-")
        );
        let state = crate::fixtures::states()
            .find(|state| state.name == name)
            .expect("world tab fixture");
        let Fixture::Menu(view) = (state.fixture)() else {
            panic!("world tab menu");
        };
        assert_eq!(view.local.tab, tab);
    }
}
