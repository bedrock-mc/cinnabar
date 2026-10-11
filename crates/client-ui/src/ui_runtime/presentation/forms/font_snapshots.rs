//! Optional installed-carrier snapshots of mixed-script menu and chat text.

use std::sync::Arc;

use assets::RuntimeFontCatalog;
use {
    crate::ui_runtime::presentation::UiPresentationRuntime,
    launcher::menu::{MenuScreen, MenuView},
};

#[test]
fn compact_font_snapshots() {
    let Ok(path) = std::env::var("CINNABAR_FONT_SNAPSHOT_CARRIER") else {
        eprintln!(
            "skipping compact_font_snapshots: missing CINNABAR_FONT_SNAPSHOT_CARRIER fixture"
        );
        return;
    };
    let Some(carrier) = super::pack_harness::carrier() else {
        return;
    };
    let manifest = assets::canonical_source_manifest_sha256(include_bytes!(
        "../../../../../../assets/cinnangles-sans-source.json"
    ));
    let started = std::time::Instant::now();
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "skipping compact_font_snapshots: font carrier fixture unavailable at {path} ({error})"
            );
            return;
        }
    };
    let font = RuntimeFontCatalog::decode(&bytes, manifest)
        .unwrap()
        .with_coverage_pages();
    let pixel_bytes: usize = font.pages().iter().map(|p| p.pixels.bytes().len()).sum();
    let mut presentation = UiPresentationRuntime::new(Arc::new(font)).unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = super::pack_harness::menu_runtime();
    let text = "Hello 世界 Привет";
    let mut view = MenuView::new(true, text.into());
    let dpi = ui::DpiScale::new(1.0).unwrap();
    for (screen, name) in [
        (MenuScreen::Home, "font-home"),
        (MenuScreen::Settings, "font-settings"),
    ] {
        view.screen = screen;
        if screen == MenuScreen::Settings {
            view.settings_section = super::menu_screens::SETTINGS_SECTIONS
                .iter()
                .find_map(|(name, index)| (*name == "language_forced_index").then_some(*index))
                .unwrap();
            view.language_choices = vec![
                ("en_US".into(), "English".into()),
                ("zh_CN".into(), "中文".into()),
                ("ru_RU".into(), "Русский".into()),
            ]
            .into();
        }
        presentation.set_menu_view(Some(view.clone()));
        let input = presentation
            .build(&player, &runtime, 0, [1280, 720], dpi)
            .unwrap();
        if screen == MenuScreen::Home {
            eprintln!(
                "font fixture: decoded_pixels={pixel_bytes}, texture_plan={}, first_menu_ms={:.3}",
                input.textures.plan().bytes(),
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        assert!(!input.vertices.is_empty());
        resident_snapshot(&input, name);
    }
    presentation.set_menu_view(None);
    runtime.open_chat(&mut player);
    runtime.insert_chat_text(text).unwrap();
    let input = presentation
        .build(&player, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    assert!(!input.vertices.is_empty());
    resident_snapshot(&input, "font-chat");
}

/// Exercises the renderer's atlas planner and samples the resulting resident pages offline.
fn resident_snapshot(input: &render_model::UiRenderInput, name: &str) {
    use render_model::{FontAtlasFrame, UiTextureCatalog, UiTextureFormat, UiTexturePage};
    let mut pixels: Vec<_> = input
        .textures
        .pages()
        .iter()
        .map(|page| {
            page.font_atlas_side()
                .map(|side| vec![0; (side * side) as usize * page.format().bytes_per_texel()])
        })
        .collect();
    let mut atlas = FontAtlasFrame::default();
    let mut uploaded = 0;
    atlas
        .prepare(input, |index, origin, size, bytes| {
            uploaded += bytes.len();
            let page = &input.textures.pages()[index];
            let side = page.font_atlas_side().unwrap();
            let stride = page.format().bytes_per_texel();
            let target = pixels[index].as_mut().unwrap();
            for row in 0..size[1] {
                let start = ((origin[1] + row) * side + origin[0]) as usize * stride;
                let from = (row * size[0]) as usize * stride;
                let length = size[0] as usize * stride;
                target[start..start + length].copy_from_slice(&bytes[from..from + length]);
            }
        })
        .unwrap();
    let pages = input
        .textures
        .pages()
        .iter()
        .zip(pixels)
        .map(|(page, pixels)| match pixels {
            None => page.clone(),
            Some(pixels) => match page.format() {
                UiTextureFormat::Coverage => {
                    UiTexturePage::coverage(page.resident_dimensions(), pixels.into()).unwrap()
                }
                UiTextureFormat::Rgba8 => {
                    UiTexturePage::owned(page.resident_dimensions(), pixels.into()).unwrap()
                }
            },
        })
        .collect();
    let resident = render_model::UiRenderInput {
        textures: Arc::new(UiTextureCatalog::new(pages, input.textures.dynamic_start()).unwrap()),
        ..input.clone()
    };
    let rendered = super::snapshot::rasterize_resident(&resident, &atlas.vertices);
    assert!(
        super::snapshot::rasterize(input) == rendered,
        "resident glyphs changed the rendered frame"
    );
    eprintln!("font fixture: uploaded_font_bytes={uploaded}");
    super::snapshot::write_image(&rendered, name);
}
