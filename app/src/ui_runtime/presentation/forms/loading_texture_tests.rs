//! Loading-screen texture residency across artwork replacement and pack reloads.

use std::io::Cursor;

use ui::DpiScale;

use super::super::{LoadingStage, UiPresentationRuntime, menu_artwork};
use super::{pack_harness, snapshot};
use crate::ui_runtime::UiRuntime;

/// Encodes a test texture with a distinctive opaque color.
fn png(size: [u32; 2], color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(size[0], size[1], image::Rgba(color))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Builds an overworld loading frame with a fixed animation clock.
fn frame(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    presentation: &mut UiPresentationRuntime,
) -> render::UiRenderInput {
    presentation
        .build(
            player_runtime,
            &UiRuntime::new(1),
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

#[test]
fn loading_screen_keeps_artwork_uvs_with_the_pixels_they_address() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let title = menu_artwork::TITLE_KEY;
    presentation.set_server_ui_pack(&super::ServerUiPack {
        textures: vec![(format!("{title}.png"), png([900, 300], [220, 20, 30, 255]))],
        ..Default::default()
    });
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    frame(&player_runtime, &mut presentation);
    presentation.finish_menu_artwork();
    let before = snapshot::rasterize(&frame(&player_runtime, &mut presentation));

    // A completed worker atlas is installed while the next frame is being built.
    let path = std::env::temp_dir().join(format!("loading-repack-{}.png", std::process::id()));
    std::fs::write(&path, png([512, 512], [20, 220, 30, 255])).unwrap();
    let mut set = presentation.menu_artwork_set.clone();
    set.paths.push((path.to_string_lossy().into_owned(), 512));
    presentation.menu_artwork_set = set.clone();
    presentation.menu_artwork_loader.request(set);
    presentation.menu_artwork_loader.wait();
    let transition = frame(&player_runtime, &mut presentation);
    snapshot::write(&transition, "artwork-transition");
    let after = snapshot::rasterize(&transition);
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        before.get_pixel(640, 170),
        after.get_pixel(640, 170),
        "the title sampled another artwork's region"
    );
    presentation.set_server_ui_pack(&super::ServerUiPack {
        textures: vec![(format!("{title}.png"), png([900, 300], [20, 30, 220, 255]))],
        ..Default::default()
    });
    let replaced = snapshot::rasterize(&settled(&player_runtime, &mut presentation));
    assert_eq!(
        *replaced.get_pixel(640, 170),
        image::Rgba([20, 30, 220, 255]),
        "same-key pack replacement kept old artwork"
    );
}

/// Waits for the loading textures, then installs their complete artwork atlas.
fn settled(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    presentation: &mut UiPresentationRuntime,
) -> render::UiRenderInput {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        frame(player_runtime, presentation);
        let atlas = presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .textures
            .lock();
        let ready = atlas.placement("textures/blocks/dirt").is_some()
            && atlas.placement("textures/ui/loading_bar").is_some();
        drop(atlas);
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "loading textures never became resident"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    presentation.finish_menu_artwork();
    frame(player_runtime, presentation)
}

/// Reads the pinned vanilla texture used as the pixel reference.
fn vanilla_image(path: &str) -> image::RgbaImage {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local")
        .join(crate::install_layout::vanilla_pack_relative());
    image::open(root.join(format!("{path}.png")))
        .unwrap()
        .into_rgba8()
}

#[test]
fn zeqa_loading_pixels_survive_artwork_repacking_and_pack_reload() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(pack) = pack_harness::env_pack() else {
        return;
    };
    let mut presentation = pack_harness::engine_presentation().expect("real UI carrier required");
    assert!(
        pack.textures
            .iter()
            .any(|(key, _)| key == &format!("{}.png", menu_artwork::TITLE_KEY))
    );
    presentation.set_server_ui_pack(&pack);
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    let cold = frame(&player_runtime, &mut presentation);
    let cold_pixels = snapshot::rasterize(&cold);
    assert!(
        cold_pixels.get_pixel(0, 0)[0] > 0,
        "cold frame lost its dirt backdrop"
    );
    let before = settled(&player_runtime, &mut presentation);
    snapshot::write(&before, "zeqa-warm");
    let pixels = snapshot::rasterize(&before);
    let dirt = vanilla_image("textures/blocks/dirt");
    // progress_screen.json:1215-1228: 2x dirt and black alpha 0.5 -> 0.7.
    for (x, y) in [(0, 0), (16, 0), (64, 0), (0, 640)] {
        let texel = dirt.get_pixel((x % 64) / 4, (y % 64) / 4);
        let shade = 0.5 - 0.2 * (y as f32 + 0.5) / 720.0;
        for channel in 0..3 {
            let expected = (f32::from(texel[channel]) * shade).round() as u8;
            assert!(
                pixels.get_pixel(x, y)[channel].abs_diff(expected) <= 1,
                "dirt/gradient pixel ({x},{y}): {:?}, expected {expected}",
                pixels.get_pixel(x, y)
            );
        }
    }
    let bar_bytes = &pack
        .textures
        .iter()
        .find(|(key, _)| key == "textures/ui/loading_bar.png")
        .unwrap()
        .1;
    let bar = image::load_from_memory(bar_bytes).unwrap().into_rgba8();
    assert!(
        bar.pixels().all(|pixel| pixel[3] == 0),
        "Zeqa hides its loading bar"
    );
    let key = format!("{}textures/ui/loading_bar", menu_artwork::SERVER_ART_PREFIX);
    let reference = presentation.menu_artwork.refs[&key];
    let page = &before.textures.pages()[usize::from(reference.page)];
    for y in reference.uv[1]..reference.uv[3] {
        for x in reference.uv[0]..reference.uv[2] {
            let at = (u32::from(y) * page.dimensions()[0] + u32::from(x)) as usize * 4;
            assert_eq!(
                page.pixels()[at + 3],
                0,
                "Zeqa's bar sampled unrelated atlas pixels"
            );
        }
    }
    let title = presentation
        .form_presentation
        .engine
        .as_ref()
        .unwrap()
        .textures
        .oversized();
    assert!(title.iter().any(|(key, _)| key == menu_artwork::TITLE_KEY));
    let path = std::env::temp_dir().join(format!("zeqa-repack-{}.png", std::process::id()));
    std::fs::write(&path, png([512, 512], [20, 220, 30, 255])).unwrap();
    let mut set = presentation.menu_artwork_set.clone();
    set.paths.push((path.to_string_lossy().into_owned(), 512));
    presentation.menu_artwork_set = set.clone();
    presentation.menu_artwork_loader.request(set);
    presentation.menu_artwork_loader.wait();
    let after = frame(&player_runtime, &mut presentation);
    snapshot::write(&after, "zeqa-after");
    assert!(
        pixels == snapshot::rasterize(&after),
        "repacking changed loading-screen pixels"
    );
    presentation.set_server_ui_pack(&pack);
    let reloaded = settled(&player_runtime, &mut presentation);
    assert!(
        pixels == snapshot::rasterize(&reloaded),
        "same pack reload changed loading-screen pixels"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn zeqa_loading_screen_animates_the_vanilla_bar_when_not_overridden() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut pack) = pack_harness::env_pack() else {
        return;
    };
    // This offline variant removes only Zeqa's intentionally transparent image.
    pack.textures
        .retain(|(key, _)| key != "textures/ui/loading_bar.png");
    let mut presentation = pack_harness::engine_presentation().expect("real UI carrier required");
    presentation.set_server_ui_pack(&pack);
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    let first = settled(&player_runtime, &mut presentation);
    let pixels = snapshot::rasterize(&first);
    let bar = vanilla_image("textures/ui/loading_bar");
    let mut checked = 0;
    // progress_screen.json:531-553: 64x8 cells, tint 0.7, ten frames per second.
    for y in 0..8 {
        for x in 0..64 {
            let texel = bar.get_pixel(x, y);
            if texel[3] != 255 {
                continue;
            }
            for channel in 0..3 {
                let expected = (u16::from(texel[channel]) * 179 / 255) as u8;
                assert!(
                    pixels.get_pixel(576 + x * 2, 406 + y * 2)[channel].abs_diff(expected) <= 1,
                    "loading bar sampled another region at ({x},{y})"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 0);
    let animated = presentation
        .build(
            &player_runtime,
            &UiRuntime::new(1),
            100,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let animated_pixels = snapshot::rasterize(&animated);
    assert_ne!(
        image::imageops::crop_imm(&pixels, 576, 406, 128, 16).to_image(),
        image::imageops::crop_imm(&animated_pixels, 576, 406, 128, 16).to_image(),
        "bar did not advance at 10 fps"
    );
    snapshot::write(&animated, "zeqa-vanilla-bar-after");
}
