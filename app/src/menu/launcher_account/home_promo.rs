//! Offline home-feed mapping and rendered start-screen evidence.

use super::*;

#[test]
fn home_feed_preserves_promo_and_fallback_label() {
    let mut home = Home::default();
    home.live_events
        .push(protocol::launcher_control::LiveEvent {
            caption_text: "Live now".into(),
            route_to_servers: true,
            ..Default::default()
        });
    let card = menu_home(&home, 0).live_event.unwrap();
    assert_eq!(card.button_text, "gathering.button.liveEventFallback");
    assert_eq!(card.caption, "Live now");
    assert!(card.route_to_servers);
    home.live_events[0].button_text = "Learn More".into();
    assert_eq!(
        menu_home(&home, 0).live_event.unwrap().button_text,
        "Learn More"
    );
}

#[test]
fn snapshot_core_home_promo() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        crate::ui_runtime::presentation::forms::pack_harness::engine_presentation()
    else {
        assert!(
            std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR").is_none(),
            "requested snapshot requires the real UI carrier"
        );
        eprintln!("home promo render skipped: real UI carrier unavailable");
        return;
    };
    let mut home = if let Ok(path) = std::env::var("CINNABAR_HOME_PROMO_FIXTURE") {
        serde_json::from_slice::<Home>(&std::fs::read(path).unwrap()).unwrap()
    } else {
        Home {
            live_events: vec![protocol::launcher_control::LiveEvent {
                button_text: "Learn More".into(),
                caption_text: "Live now".into(),
                route_to_servers: true,
                ..Default::default()
            }],
            ..Default::default()
        }
    };
    assert!(!home.live_events.is_empty(), "fixture has no promo tile");
    let scratch = std::env::temp_dir().join(format!("cinnabar-home-promo-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let badge = scratch.join("offline-promo-badge.png");
    image::RgbaImage::from_pixel(256, 128, image::Rgba([40, 90, 200, 255]))
        .save(&badge)
        .unwrap();
    home.live_events[0].badge.path = badge.to_string_lossy().into_owned();
    let mut view = crate::menu::MenuRuntime::new(true, 2, "Steve".into()).view();
    view.auth_state = AuthState::Authenticated;
    view.catalog_loading = false;
    presentation.sync_menu_artwork(vec![(badge.to_string_lossy().into_owned(), 256)]);
    presentation.finish_menu_artwork();
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    let dpi = ui::DpiScale::new(2.0).unwrap();
    presentation.set_menu_view(Some(view.clone()));
    let before = presentation
        .build(&player_runtime, &runtime, 0, [2560, 1440], dpi)
        .unwrap();
    let before_frame = crate::ui_runtime::presentation::forms::snapshot::rasterize(&before);
    assert!(
        !before_frame
            .pixels()
            .any(|pixel| pixel.0 == [40, 90, 200, 255])
    );
    crate::ui_runtime::presentation::forms::snapshot::write(&before, "home-promo-before");
    view.feeds.home = menu_home(&home, 0);
    for _ in 0..3 {
        presentation.set_menu_view(Some(view.clone()));
        presentation
            .build(&player_runtime, &runtime, 0, [2560, 1440], dpi)
            .unwrap();
    }
    presentation.set_menu_view(Some(view));
    let input = presentation
        .build(&player_runtime, &runtime, 0, [2560, 1440], dpi)
        .unwrap();
    let frame = crate::ui_runtime::presentation::forms::snapshot::rasterize(&input);
    assert!(
        frame
            .pixels()
            .filter(|pixel| pixel.0 == [40, 90, 200, 255])
            .count()
            > 1_000,
        "promo badge was not rendered"
    );
    crate::ui_runtime::presentation::forms::snapshot::write(&input, "home-promo");
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn home_feed_keeps_inbox_identity_dates_counts_and_marketplace_ribbon() {
    let home: Home = serde_json::from_value(serde_json::json!({
        "inbox": {"unread":30,"categories":[{"type":"News","unread":30}]},
        "messages": [
            {"surface":"InboxMessage","instance_id":"instance","report_id":"report","received":"2026-10-03T10:00:00Z","sender":"Minecraft","category":"News","status":"Unread"},
            {"surface":"MarketplaceButton","banner":"Add-ons!","colors":{"BannerTextColor":[255,200,40]},"images":[{"id":"defaultBackground","path":"/offline/art.png"},{"id":"banner","path":"/offline/ribbon.png"}]}
        ]
    })).unwrap();
    let mapped = menu_home(&home, 0);
    assert_eq!(mapped.inbox_counts.get(&0), Some(&30));
    assert_eq!(mapped.inbox[0].instance_id, "instance");
    assert_eq!(mapped.inbox[0].report_id, "report");
    assert_eq!(mapped.inbox[0].received, "2026-10-03T10:00:00Z");
    assert_eq!(mapped.inbox[0].source, "Minecraft");
    let art = mapped.store_art.unwrap();
    assert_eq!(art.banner, "Add-ons!");
    assert_eq!(art.banner_texture, "/offline/ribbon.png");
    assert_eq!(art.default_background, "/offline/art.png");
    assert_eq!(art.colors.get("BannerTextColor"), Some(&[255, 200, 40]));
}
