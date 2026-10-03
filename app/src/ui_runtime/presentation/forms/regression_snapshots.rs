//! Offline witnesses for the captured Zeqa shop and populated hotbar.

use super::pack_harness;
use ui::DpiScale;

#[test]
#[ignore = "requires installed local carriers (make assets)"]
fn populated_hotbar_snapshot() {
    use super::super::{IconRef, tests::engine_hud_tests};
    let mut presentation = engine_hud_tests::engine_presentation()
        .expect("required offline fixture; see the ignore reason");
    if let Some(pack) = pack_harness::env_pack() {
        presentation.set_server_ui_pack(&pack);
    }
    let mut runtime = crate::ui_runtime::UiRuntime::new(1);
    runtime.publish_player_game_mode(protocol::PlayerGameMode::Survival);
    runtime.set_local_selected_slot(0);
    let icon = IconRef {
        page: presentation.solid_texture_page,
        uv: [0, 0, 1, 1],
        glint: false,
    };
    for index in 0..presentation.hud_frame.hotbar_stacks.len() {
        presentation.hud_frame.hotbar_stacks[index] = Some(protocol::NetworkItemStack {
            network_id: 1,
            count: 1,
            ..protocol::NetworkItemStack::empty()
        });
        presentation.hud_frame.hotbar_icons[index] = Some(icon);
    }
    let input = presentation
        .build(&runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
        .unwrap();
    super::snapshot::write(&input, "hotbar");
    let image = super::snapshot::rasterize(&input);
    assert!(image.pixels().filter(|pixel| pixel.0 == [255; 4]).count() > 100);
}

#[test]
#[ignore = "requires installed UI carrier, CINNABAR_LOBBY_CAPTURE and CINNABAR_FORM_PACK_DIR"]
fn spirit_bundle_snapshot() {
    let runtime = captured_form().expect("required offline fixture; see the ignore reason");
    let mut presentation = pack_harness::engine_presentation().expect("installed UI carrier");
    let pack = pack_harness::env_pack().expect("captured server UI pack");
    presentation.set_server_ui_pack(&pack);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        presentation
            .build(&runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
            .unwrap();
        let engine = presentation.form_presentation.engine.as_ref().unwrap();
        if engine.drawn_sprites().1.is_empty() || std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    presentation.finish_menu_artwork();
    let input = presentation
        .build(&runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
        .unwrap();
    super::snapshot::write(&input, "spirit-bundle");
    eprintln!(
        "spirit: vertices={} batches={}",
        input.vertices.len(),
        input.batches.len()
    );
    let engine = presentation.form_presentation.engine.as_ref().unwrap();
    let (drawn, missing) = engine.drawn_sprites();
    assert!(missing.is_empty(), "unresolved form images: {missing:?}");
    for texture in [
        "textures/ads/SpiritBundle",
        "textures/ui/common/dark_field",
        "textures/ui/common/buttons/green/default",
    ] {
        assert!(drawn.iter().any(|key| key == texture), "{texture} absent");
    }
    let passes = engine.passes;
    for _ in 0..5 {
        presentation
            .build(&runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
            .unwrap();
    }
    assert_eq!(
        presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .passes,
        passes
    );
    let image = super::snapshot::rasterize(&input);
    assert!(
        image
            .enumerate_pixels()
            .filter(|(x, y, pixel)| {
                (435..843).contains(x)
                    && (205..440).contains(y)
                    && pixel[2] > pixel[0]
                    && pixel[0] > 60
            })
            .count()
            > 1_000,
        "the purple bundle artwork must reach the published texture pages"
    );
}

#[test]
#[ignore = "requires installed UI carrier, CINNABAR_LOBBY_CAPTURE and CINNABAR_FORM_PACK_DIR"]
fn spirit_bundle_before_pack_has_stable_resident_pages_after_install() {
    let runtime = captured_form().expect("required offline fixture; see the ignore reason");
    let mut presentation = pack_harness::engine_presentation().expect("installed UI carrier");
    let dpi = DpiScale::new(1.0).unwrap();
    let cold = presentation.build(&runtime, 0, [1280, 720], dpi).unwrap();
    super::snapshot::write(&cold, "spirit-bundle-before-pack");
    let pack = pack_harness::env_pack().expect("captured server UI pack");
    presentation.set_server_ui_pack(&pack);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        presentation.build(&runtime, 0, [1280, 720], dpi).unwrap();
        let engine = presentation.form_presentation.engine.as_ref().unwrap();
        let atlas = engine.textures.lock();
        let resident = [
            "textures/ui/common/dark_field",
            "textures/ui/common/buttons/green/default",
        ]
        .iter()
        .all(|key| atlas.placement(key).is_some());
        drop(atlas);
        if resident {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "button textures never became resident"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    presentation.finish_menu_artwork();
    let settled = presentation.build(&runtime, 0, [1280, 720], dpi).unwrap();
    super::snapshot::write(&settled, "spirit-bundle-after-pack");
    let pixels = super::snapshot::rasterize(&settled);
    assert!(
        pixels
            .enumerate_pixels()
            .filter(|(x, y, pixel)| (435..843).contains(x)
                && (205..440).contains(y)
                && pixel[2] > pixel[0]
                && pixel[0] > 60)
            .count()
            > 1_000,
        "bundle art did not reach the published pages"
    );
    let passes = presentation
        .form_presentation
        .engine
        .as_ref()
        .unwrap()
        .passes;
    let pages: Vec<_> = settled
        .textures
        .pages()
        .iter()
        .map(render::UiTexturePage::identity)
        .collect();
    for now in 1..6 {
        let frame = presentation.build(&runtime, now, [1280, 720], dpi).unwrap();
        assert_eq!(
            presentation
                .form_presentation
                .engine
                .as_ref()
                .unwrap()
                .passes,
            passes
        );
        assert_eq!(
            frame
                .textures
                .pages()
                .iter()
                .map(render::UiTexturePage::identity)
                .collect::<Vec<_>>(),
            pages
        );
    }
}

/// Replays the captured ModalFormRequest through the packet decoder and UI state.
fn captured_form() -> Option<crate::ui_runtime::UiRuntime> {
    let bytes = std::fs::read(std::env::var_os("CINNABAR_LOBBY_CAPTURE")?).unwrap();
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let id = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let length = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &bytes[at + 8..at + 8 + length];
        at += 8 + length;
        if id != 100 {
            continue;
        }
        let mut batch = vec![0xfe];
        varint(&mut batch, body.len() as u64 + 1);
        batch.push(id as u8);
        batch.extend(body);
        let session = protocol::BedrockSession { shield_item_id: 0 };
        let packet = protocol::decode_batch(batch.into(), &session)
            .unwrap()
            .remove(0);
        let Some(protocol::WorldEvent::Ui(event)) = protocol::into_world_event(packet, 0).unwrap()
        else {
            panic!("captured form did not decode as a UI event");
        };
        let mut runtime = crate::ui_runtime::UiRuntime::new(1);
        runtime
            .apply(crate::ui_runtime::SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event,
            })
            .unwrap();
        return Some(runtime);
    }
    panic!("capture contains no server form");
}

/// Writes the unsigned length prefix used by packet batches.
fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 0x80 });
        if value == 0 {
            break;
        }
    }
}
