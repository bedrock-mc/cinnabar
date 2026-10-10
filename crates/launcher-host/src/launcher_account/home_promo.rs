//! Offline home-feed mapping and rendered start-screen evidence.

use {super::*, launcher::menu::auth::AuthState};

#[test]
fn home_feed_preserves_promo_and_fallback_label() {
    let mut home = Home::default();
    home.live_events.push(bridge::LiveEvent {
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
