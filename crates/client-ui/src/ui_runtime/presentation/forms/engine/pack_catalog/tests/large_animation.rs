use std::sync::Arc;

use crate::ui_runtime::{
    UiRuntime,
    presentation::forms::{ServerUiPack, snapshot},
};

const BLUE: [u8; 4] = [20, 80, 220, 255];
const GREEN: [u8; 4] = [30, 210, 70, 255];

/// Builds an original large sheet with two frame cells crossing a texture-page boundary.
fn animated_sheet_pack() -> ServerUiPack {
    let mut sheet = image::RgbaImage::new(4096, 4160);
    for y in 4064..4128 {
        for x in 0..128 {
            sheet.put_pixel(x, y, image::Rgba(if x < 64 { BLUE } else { GREEN }));
        }
    }
    let mut png = std::io::Cursor::new(Vec::new());
    sheet.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let files = vec![
        ("ui/_global_variables.json".into(), b"{}".to_vec()),
        (
            "ui/_ui_defs.json".into(),
            br#"{"ui_defs":["custom/animated.uidx"]}"#.to_vec(),
        ),
        (
            "custom/animated.uidx".into(),
            br#"{
                "namespace":"hud",
                "first":{"anim_type":"uv","duration":0.05,"from":[0,4064],"to":[0,4064],"next":"@hud.second"},
                "second":{"anim_type":"uv","duration":0.05,"from":[64,4064],"to":[64,4064],"next":"@hud.first"},
                "hud_screen":{"type":"screen","controls":[{"sheet":{"type":"image","texture":"textures/ui/original_sheet",
                    "anchor_from":"top_left","anchor_to":"top_left","offset":[16,16],
                    "size":[64,64],"uv_size":[64,64],"uv":"@hud.first"}}]}
            }"#.to_vec(),
        ),
    ];
    let catalog = json_ui::Catalog::from_files(
        files
            .iter()
            .map(|(path, bytes): &(String, Vec<u8>)| (path.as_str(), bytes.as_slice())),
    )
    .unwrap();
    ServerUiPack {
        ui_layers: vec![files],
        textures: vec![("textures/ui/original_sheet.png".into(), png.into_inner())],
        catalog: Some(Arc::new(catalog)),
        ..Default::default()
    }
}

/// Presents a caller-selected animation time without advancing the wall clock.
fn draw(
    presentation: &mut crate::ui_runtime::presentation::UiPresentationRuntime,
    runtime: &UiRuntime,
    millis: u64,
) -> render_model::UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            runtime,
            millis,
            [200, 128],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

/// Checks original frame colors on both sides of the source tile boundary.
fn assert_frame(input: &render_model::UiRenderInput, color: [u8; 4]) {
    let image = snapshot::rasterize(input);
    for y in [17, 45, 79] {
        for x in [17, 48, 79] {
            assert_eq!(image.get_pixel(x, y).0, color, "pixel {x},{y}");
        }
    }
}

#[test]
fn large_authored_uv_sheet_publishes_native_frame_pixels_and_loops() {
    let pack = animated_sheet_pack();
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_ui(Some(Arc::new(pack.clone())));
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.gui_scale_preference = Some(1);
    test_time::eventually("native pixels of a large authored animation sheet", || {
        snapshot::rasterize(&draw(&mut presentation, &runtime, 0))
            .get_pixel(48, 48)
            .0
            == BLUE
    });
    let first = draw(&mut presentation, &runtime, 0);
    assert_frame(&first, BLUE);
    snapshot::write(&first, "large-sheet-first");
    let identity = first.textures.identity();
    for (millis, color, name) in [
        (50, GREEN, "large-sheet-second"),
        (100, BLUE, "large-sheet-loop"),
    ] {
        let input = draw(&mut presentation, &runtime, millis);
        assert_frame(&input, color);
        snapshot::write(&input, name);
        assert_eq!(
            input.textures.identity(),
            identity,
            "UV changes must reuse immutable source pages"
        );
    }
    assert_frame(&draw(&mut presentation, &runtime, 150), GREEN);
    runtime.set_server_ui(None);
    let removed = draw(&mut presentation, &runtime, 160);
    let pixel = snapshot::rasterize(&removed).get_pixel(48, 48).0;
    assert_ne!(pixel, BLUE);
    assert_ne!(pixel, GREEN);
    assert!(
        removed.textures.plan().bytes() < first.textures.plan().bytes(),
        "deactivation must release source pages"
    );
    runtime.set_server_ui(Some(Arc::new(pack)));
    test_time::eventually("reloaded sheet starts at its authored first frame", || {
        snapshot::rasterize(&draw(&mut presentation, &runtime, 180))
            .get_pixel(48, 48)
            .0
            == BLUE
    });
    assert_frame(&draw(&mut presentation, &runtime, 230), GREEN);
}

/// Samples an optional locally installed inventory animation through the full carrier.
#[test]
fn retained_inventory_animation_advances_and_repeats_two_loops() {
    use crate::ui_runtime::presentation::forms::pack_harness;
    let Some(period) = std::env::var("CINNABAR_INVENTORY_ANIMATION_LOOP_MS").ok() else {
        eprintln!(
            "skipping retained_inventory_animation_advances_and_repeats_two_loops: missing CINNABAR_INVENTORY_ANIMATION_LOOP_MS fixture"
        );
        return;
    };
    let period: f64 = period
        .parse()
        .expect("positive authored loop duration in milliseconds");
    assert!(period.is_finite() && period > 0.0 && period <= 60_000.0);
    let Some(pack) = pack_harness::env_pack() else {
        eprintln!(
            "skipping retained_inventory_animation_advances_and_repeats_two_loops: missing CINNABAR_FORM_PACK_DIR fixture"
        );
        return;
    };
    let Some(mut presentation) =
        crate::test_support::engine_presentation_with(pack_harness::font())
    else {
        eprintln!(
            "skipping retained_inventory_animation_advances_and_repeats_two_loops: missing installed UI carrier"
        );
        return;
    };
    presentation.gui_scale_preference = Some(2);
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.set_server_ui(Some(Arc::new(pack)));
    runtime.toggle_inventory(&mut player);
    let build = |presentation: &mut crate::ui_runtime::presentation::UiPresentationRuntime, now| {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    };
    test_time::eventually("original inventory animation source ready", || {
        build(&mut presentation, 0);
        !presentation.server_ui_sheet_pages().is_empty()
    });
    let first = build(&mut presentation, 0);
    first.validate().unwrap();
    snapshot::write(&first, "retained-inventory-first");
    let first_pixels = snapshot::rasterize(&first);
    let identity = first.textures.identity();
    let phase = period / 4.0;
    let mut last = 0;
    let mut phase_pixels = None;
    for (target, name, is_loop) in [
        (phase, "retained-inventory-phase", false),
        (period, "retained-inventory-loop-one", true),
        (period + phase, "retained-inventory-phase-two", false),
        (period * 2.0, "retained-inventory-loop-two", true),
    ] {
        let target = target.ceil() as u64;
        while last + 16 < target {
            last += 16;
            build(&mut presentation, last);
        }
        let input = build(&mut presentation, target);
        last = target;
        input.validate().unwrap();
        snapshot::write(&input, name);
        let pixels = snapshot::rasterize(&input);
        assert_eq!(
            input.textures.identity(),
            identity,
            "authored frames reuse their resident sheet"
        );
        if is_loop {
            assert!(
                pixels == first_pixels,
                "inventory returns to the authored starting frame"
            );
        } else {
            assert!(
                pixels != first_pixels,
                "inventory animation must visibly advance"
            );
            if let Some(expected) = &phase_pixels {
                assert!(
                    &pixels == expected,
                    "the same phase repeats on the next loop"
                );
            } else {
                phase_pixels = Some(pixels);
            }
        }
    }
}
