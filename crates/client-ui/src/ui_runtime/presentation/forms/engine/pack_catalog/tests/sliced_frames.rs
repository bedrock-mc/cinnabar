use std::sync::Arc;

use crate::ui_runtime::{
    UiRuntime,
    presentation::forms::{ServerUiPack, snapshot},
};

/// Builds an original tiny frame whose source slices overlap across its face.
fn disabled_frame_pack(inset: u8) -> ServerUiPack {
    let mut pixels = vec![24; 3 * 3 * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    pixels[4 * 4..5 * 4].copy_from_slice(&[120, 120, 120, 255]);
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &pixels,
        3,
        3,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    let mut pack = ServerUiPack {
        ui_layers: vec![vec![
            ("ui/_global_variables.json".into(), b"{}".to_vec()),
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".into(),
                br#"{
            "namespace":"hud",
            "hud_screen":{"type":"screen","controls":[{
                "frame":{"type":"image","texture":"textures/ui/tiny_frame",
                    "size":[100,30],"anchor_from":"center","anchor_to":"center"}
            }]}
        }"#
                .to_vec(),
            ),
        ]],
        textures: vec![
            ("textures/ui/tiny_frame.png".into(), png),
            (
                "textures/ui/tiny_frame.json".into(),
                serde_json::json!({"nineslice_size":inset,"base_size":[3,3]})
                    .to_string()
                    .into_bytes(),
            ),
        ],
        ..Default::default()
    };
    pack.catalog = Some(Arc::new(
        json_ui::Catalog::from_files(
            pack.ui_layers[0]
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        )
        .unwrap(),
    ));
    pack
}

#[test]
fn tiny_disabled_frame_keeps_one_texel_borders_through_the_atlas() {
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_ui(Some(Arc::new(disabled_frame_pack(2))));
    for scale in [3, 6] {
        let mut presentation = crate::test_support::mini_engine_presentation();
        presentation.gui_scale_preference = Some(scale);
        let viewport = [400 * u32::from(scale), 260 * u32::from(scale)];
        let input = presentation
            .build(
                &player,
                &runtime,
                0,
                viewport,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        snapshot::write(&input, &format!("tiny-disabled-frame-{scale}"));
        let image = snapshot::rasterize(&input);
        let k = u32::from(scale);
        let [left, top] = [150 * k, 115 * k];
        // Check both sides of every one-texel border, including its first face pixel.
        for (x, y, expected) in [
            (left + k - 1, top + 15 * k, 24),
            (left + k, top + 15 * k, 120),
            (left + 99 * k - 1, top + 15 * k, 120),
            (left + 99 * k, top + 15 * k, 24),
            (left + 50 * k, top + k - 1, 24),
            (left + 50 * k, top + k, 120),
            (left + 50 * k, top + 29 * k - 1, 120),
            (left + 50 * k, top + 29 * k, 24),
            (left + 50 * k, top + 15 * k, 120),
        ] {
            assert_eq!(
                image.get_pixel(x, y).0,
                [expected, expected, expected, 255],
                "frame pixel ({x}, {y}) at GUI scale {scale}"
            );
        }
    }
}

#[test]
fn oversized_disabled_frame_insets_do_not_sample_atlas_gutters() {
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_ui(Some(Arc::new(disabled_frame_pack(5))));
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.gui_scale_preference = Some(3);
    let input = presentation
        .build(
            &player,
            &runtime,
            0,
            [1200, 780],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&input, "oversized-disabled-frame");
    let image = snapshot::rasterize(&input);
    for y in 345..435 {
        for x in 450..750 {
            let color = image.get_pixel(x, y).0;
            assert!(
                color == [24, 24, 24, 255] || color == [120, 120, 120, 255],
                "({x}, {y}) sampled outside the opaque frame: {color:?}"
            );
        }
    }
}
