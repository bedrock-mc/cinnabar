use super::*;

/// Encodes a deterministic PNG fixture.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([1, 2, 3, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

// Prepared HUD textures read on the worker; an unprepared pack reads them on first draw.
#[test]
fn prepared_hud_textures_read_off_the_frame() {
    const HOTBAR: &str = "textures/ui/hotbar_0";
    let files = vec![(format!("{HOTBAR}.png"), png(182, 22))];
    for prepared in [true, false] {
        let Some(mut presentation) = crate::test_support::engine_presentation() else {
            eprintln!(
                "skipping prepared_hud_textures_read_off_the_frame: missing local UI carrier (make assets)"
            );
            return;
        };
        let base = presentation.pack_catalog_base().unwrap();
        let pack = ServerUiPack {
            view: Some(resource_pack::test_support::pack_view(&files)),
            ..ServerUiPack::default()
        };
        let pack = if prepared {
            (*pack.prepare_catalog(&base)).clone()
        } else {
            ServerUiPack {
                catalog: Some(Arc::new(
                    super::super::screen_registry::registry()
                        .catalog()
                        .layer(&base, &[])
                        .pack,
                )),
                ..pack
            }
        };
        ui_engine::atlas::PACK_READS.with(|reads| reads.borrow_mut().clear());
        presentation.set_server_ui_pack(&pack);
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        presentation
            .build(
                &player,
                &crate::ui_runtime::UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let read = ui_engine::atlas::PACK_READS
            .with(|reads| reads.borrow().iter().any(|key| key == HOTBAR));
        assert_eq!(
            read, !prepared,
            "prepared={prepared}: the frame read the hotbar from the pack"
        );
    }
}
