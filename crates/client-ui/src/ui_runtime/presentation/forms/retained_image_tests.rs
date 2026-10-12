use super::super::UiPresentationRuntime;
use crate::ui_runtime::UiRuntime;
use launcher::menu::{MenuScreen, MenuView};
use std::sync::Arc;
use ui::DpiScale;

#[test]
fn retained_menu_publishes_requested_original_image_pixels() {
    use super::ServerUiPack;
    use ui_engine::textures::TITLE_KEY;
    let color = [123, 47, 201, 255];
    let size = [1992, 669];
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(size[0], size[1], image::Rgba(color))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let mut runtime = UiRuntime::new(0);
    runtime.set_server_ui(Some(Arc::new(ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/original_title.json"]}"#.to_vec(),
            ),
            (
                "ui/original_title.json".into(),
                serde_json::to_vec(&serde_json::json!({
                    "namespace":"common_art",
                    "title_image":{"type":"image", "texture":TITLE_KEY}
                }))
                .unwrap(),
            ),
        ]],
        textures: vec![(format!("{TITLE_KEY}.png"), bytes.into_inner())],
        ..Default::default()
    })));
    let player = player_state::PlayerState::new(0);
    let mut presentation = crate::test_support::mini_engine_presentation();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Settings;
    let animations = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "screen_animations")
        .unwrap();
    Arc::make_mut(&mut view.settings_options).set(animations, 0);
    presentation.set_menu_view(Some(view));
    let build = |presentation: &mut UiPresentationRuntime, now| {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    };
    build(&mut presentation, 0);
    build(&mut presentation, 1000);
    presentation.title_icon(&presentation.menu_artwork.refs);
    test_time::eventually(
        "requested image publishes while the launcher menu stays unchanged",
        || {
            let input = build(&mut presentation, 1000);
            input.validate().unwrap();
            input
                .textures
                .pages()
                .iter()
                .any(|page| page.dimensions() == size && page.pixels()[..4] == color)
        },
    );
    let ready = build(&mut presentation, 1000);
    let (_, allocations) = test_alloc::count(|| {
        let unchanged = build(&mut presentation, 1000);
        assert_eq!(unchanged.revision, ready.revision);
        assert!(Arc::ptr_eq(&unchanged.vertices, &ready.vertices));
        assert!(Arc::ptr_eq(&unchanged.textures, &ready.textures));
    });
    assert_eq!(allocations, 0);
}
