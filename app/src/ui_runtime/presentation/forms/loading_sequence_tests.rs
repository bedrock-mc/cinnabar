//! Runtime pack observation while artwork arrives, is superseded and is cancelled.
use super::super::{LoadingStage, UiPresentationRuntime};
use super::{ServerUiPack, pack_harness, snapshot};
use client_ui::ui_runtime::UiRuntime;
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

/// Mounts fixture textures as an archive so the production lazy reader supplies them.
pub(super) fn lazy(mut pack: ServerUiPack) -> ServerUiPack {
    let id = "11111111-2222-3333-4444-555555555555".parse().unwrap();
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"name":"offline","description":"offline","uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources","uuid":"{}","version":[1,0,0]}}]}}"#,
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in std::iter::once(("manifest.json".into(), manifest.into_bytes()))
        .chain(std::mem::take(&mut pack.textures))
    {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id,
        "1.0.0".into(),
        String::new(),
        zip.finish().unwrap().into_inner(),
    );
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
            archive,
        ]));
    assert!(stack.rejections().is_empty(), "{:?}", stack.rejections());
    pack.view = Some(resource_pack::LayeredPackView::new(stack));
    pack
}

/// Publishes the same runtime across menu, join, reload and cancellation frames.
fn frame(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
) -> render_model::UiRenderInput {
    presentation
        .build(
            player_runtime,
            runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

#[test]
fn zeqa_lazy_pages_survive_menu_join_reload_and_cancellation() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(pack) = client_ui::test_support::pack_harness::env_pack() else {
        eprintln!(
            "skipping zeqa_lazy_pages_survive_menu_join_reload_and_cancellation: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut presentation = pack_harness::startup_presentation().expect("installed carriers");
    let dir = client_ui::test_support::pack_harness::scratch_dir("loading-sequence");
    let view = client_ui::test_support::fixture_view(&dir);
    let paths = super::super::menu_artwork::view_paths(&view);
    let mut runtime = UiRuntime::new(1);
    presentation.set_menu_view(Some(view.clone()));
    presentation.sync_menu_artwork(paths.clone());
    frame(&player_runtime, &mut presentation, &runtime);
    presentation.finish_menu_artwork();
    frame(&player_runtime, &mut presentation, &runtime);
    presentation.set_menu_view(None);
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    crate::session::begin_session(&mut runtime, &mut player_runtime, 2);
    runtime.set_server_ui(Some(Arc::new(lazy(pack.clone()))));
    let glyphs = client_ui::test_support::pack_harness::env_glyphs();
    runtime.set_session_glyphs(glyphs.clone());
    let mut warm = None;
    for phase in 0..4 {
        for index in 0..40 {
            if index < 12 {
                let subset = paths
                    .iter()
                    .take(index % (paths.len() + 1))
                    .cloned()
                    .collect();
                presentation.sync_menu_artwork(subset);
            }
            let input = frame(&player_runtime, &mut presentation, &runtime);
            input.validate().unwrap();
            let pixels = snapshot::rasterize(&input);
            let dirt = image::open(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../.local")
                    .join(launcher::install_layout::vanilla_pack_relative())
                    .join("textures/blocks/dirt.png"),
            )
            .unwrap()
            .into_rgba8();
            for (x, y) in [(0, 0), (64, 0), (0, 640)] {
                let texel = dirt.get_pixel((x % 64) / 4, (y % 64) / 4);
                let shade = 0.5 - 0.2 * (y as f32 + 0.5) / 720.0;
                let source = std::array::from_fn(|channel| {
                    if channel == 3 {
                        255
                    } else {
                        (f32::from(texel[channel]) * shade).round() as u8
                    }
                });
                let expected = snapshot::loading_backdrop_texel(&presentation, source, [x, y]);
                for (channel, expected) in expected.iter().enumerate().take(3) {
                    assert!(
                        pixels.get_pixel(x, y)[channel].abs_diff(*expected) <= 1,
                        "phase {phase} frame {index} dirt sampled another page at {x},{y}"
                    );
                }
            }
            if index == 39 {
                presentation.finish_menu_artwork();
                let input = frame(&player_runtime, &mut presentation, &runtime);
                let pixels = snapshot::rasterize(&input);
                if let Some(warm) = &warm {
                    assert_eq!(warm, &pixels, "settled reload changed Zeqa pixels");
                }
                warm = Some(pixels);
                snapshot::write(&input, &format!("zeqa-live-order-{phase}"));
            }
        }
        runtime.set_server_ui(None);
        frame(&player_runtime, &mut presentation, &runtime);
        crate::session::begin_session(&mut runtime, &mut player_runtime, phase + 3);
        runtime.set_server_ui(Some(Arc::new(lazy(pack.clone()))));
        runtime.set_session_glyphs(glyphs.clone());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn vanilla_loading_before_pack_arrival_survives_static_page_insertion() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = client_ui::test_support::pack_harness::engine_presentation()
    else {
        eprintln!(
            "skipping vanilla_loading_before_pack_arrival_survives_static_page_insertion: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let side = client_ui::ui_runtime::oreui_assets::OREUI_PAGE_SIDE as usize;
    presentation
        .enable_oreui_originals(client_ui::ui_runtime::oreui_assets::OreUiImages {
            pages: vec![client_ui::ui_runtime::oreui_assets::OreUiPage {
                dimensions: [side as u32; 2],
                pixels: vec![255; side * side * 4].into(),
            }],
            sprites: Default::default(),
            loading_frames: Default::default(),
            animations: Default::default(),
            source: None,
        })
        .unwrap();
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    let runtime = client_ui::test_support::pack_harness::menu_runtime();
    let input = frame(&player_runtime, &mut presentation, &runtime);
    snapshot::write(&input, "loading-before-pack");
    let pixels = snapshot::rasterize(&input);
    let dirt = image::open(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.local")
            .join(launcher::install_layout::vanilla_pack_relative())
            .join("textures/blocks/dirt.png"),
    )
    .unwrap()
    .into_rgba8();
    let source = std::array::from_fn(|channel| {
        if channel == 3 {
            255
        } else {
            (f32::from(dirt.get_pixel(0, 0)[channel]) * 0.5).round() as u8
        }
    });
    let expected = snapshot::loading_backdrop_texel(&presentation, source, [0, 0]);
    for (channel, expected) in expected.iter().enumerate().take(3) {
        assert!(
            pixels.get_pixel(0, 0)[channel].abs_diff(*expected) <= 1,
            "before the server pack arrives, dirt must not address the preceding glyph page"
        );
    }
}
