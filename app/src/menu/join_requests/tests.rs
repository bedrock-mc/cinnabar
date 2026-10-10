use std::{num::NonZeroU64, time::Duration};

use bevy::prelude::{App, Update};
use client_ui::ui_runtime::UiRuntime;
use launcher::menu::join_requests::{LIFETIME, respond_hint, title};
use semantic_input::{Action, ActionPhase, ActionSnapshot, InputMode};

use crate::semantic_controls::SemanticInputSnapshot;
use {
    super::super::MenuRuntime,
    launcher::menu::{MenuAction, MenuScreen},
};

fn host() -> MenuRuntime {
    MenuRuntime::new(true, 2, "Host".to_owned())
}

fn playing() -> MenuRuntime {
    let mut menu = host();
    menu.show_world();
    menu
}

fn millis(at: Duration) -> u64 {
    u64::try_from(at.as_millis()).unwrap()
}

/// A frame where the Open Notification key went down.
fn open_notification_pressed() -> SemanticInputSnapshot {
    let mut phases = [ActionPhase::default(); Action::COUNT];
    phases[Action::InteractWithToast as usize].pressed = true;
    SemanticInputSnapshot::from_finalized(ActionSnapshot {
        frame_sequence: 1,
        authority_generation: NonZeroU64::MIN,
        movement: [0.0; 2],
        raw_movement: [0.0; 2],
        analogue_movement: [0.0; 2],
        look_delta: [0.0; 2],
        input_mode: InputMode::KeyboardMouse,
        phases,
        release_reasons: [None; Action::COUNT],
        movement_buttons: Default::default(),
    })
}

#[test]
fn answers_go_out_in_order_and_the_next_request_takes_the_popup() {
    let mut menu = host();
    menu.push_join_request(1, "Alex".into(), Duration::ZERO);
    menu.push_join_request(2, "Sam".into(), Duration::ZERO);
    assert_eq!(menu.view().join_request_prompt(), Some("Alex"));
    assert_eq!(
        menu.focus_actions(),
        [
            MenuAction::JoinRequest(true),
            MenuAction::JoinRequest(false)
        ]
    );
    menu.activate(MenuAction::JoinRequest(true));
    assert_eq!(menu.view().join_request_prompt(), Some("Sam"));
    // Back on the popup declines, as vanilla's modal escape presses its second button.
    menu.go_back();
    assert_eq!(menu.view().join_request, None);
    assert_eq!(menu.take_join_reply(), Some((1, true)));
    assert_eq!(menu.take_join_reply(), Some((2, false)));
    assert_eq!(menu.take_join_reply(), None);
    assert_eq!(menu.screen(), MenuScreen::Home);
}

#[test]
fn the_open_notification_key_opens_the_pause_popup_only_while_a_request_waits() {
    let mut app = App::new();
    app.insert_resource(playing())
        .insert_resource(open_notification_pressed())
        .add_systems(Update, super::open_join_requests_from_key);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .push_join_request(1, "Alex".into(), Duration::ZERO);
    app.update();
    let menu = app.world().resource::<MenuRuntime>();
    assert!(menu.is_visible());
    assert_eq!(menu.screen(), MenuScreen::Pause);
    assert_eq!(menu.view().join_request_prompt(), Some("Alex"));
}

#[test]
fn the_join_toast_stands_while_its_request_is_open_and_leaves_with_it() {
    let mut menu = playing();
    let mut runtime = UiRuntime::new(1);
    let showing = |runtime: &UiRuntime, at: u64| runtime.hud().showing_toast(at).is_some();
    menu.push_join_request(1, "Alex".into(), Duration::ZERO);
    menu.sync_join_toast(&mut &mut runtime, Duration::ZERO);
    let toast = runtime.hud().standing_toast().unwrap();
    assert_eq!(&*toast.title, title("Alex"));
    assert_eq!(&*toast.message, respond_hint("N"));
    // It outlasts the notification duration and stays until Discord would close the request.
    assert!(showing(&runtime, millis(LIFETIME) - 1));
    // Opening the popup takes it down at once; answering leaves nothing to show.
    menu.open_join_requests();
    let opened = Duration::from_secs(5);
    menu.sync_join_toast(&mut &mut runtime, opened);
    assert!(!showing(
        &runtime,
        millis(opened) + ui::TOAST_SLIDE_OUT_MILLIS
    ));
    menu.activate(MenuAction::JoinRequest(true));
    menu.activate(MenuAction::PauseResume);
    menu.sync_join_toast(&mut &mut runtime, Duration::from_secs(6));
    assert!(!showing(&runtime, millis(Duration::from_secs(7))));
    // The next request stands until it lapses.
    let asked = Duration::from_secs(10);
    menu.push_join_request(2, "Sam".into(), asked);
    menu.sync_join_toast(&mut &mut runtime, asked);
    assert!(showing(&runtime, millis(asked + LIFETIME) - 1));
    menu.expire_join_requests(asked + LIFETIME);
    menu.sync_join_toast(&mut &mut runtime, asked + LIFETIME);
    assert!(!showing(
        &runtime,
        millis(asked + LIFETIME) + ui::TOAST_SLIDE_OUT_MILLIS
    ));
}

#[test]
fn turning_discord_off_drops_requests_and_unsent_answers() {
    let mut menu = host();
    menu.push_join_request(1, "Alex".into(), Duration::ZERO);
    menu.push_join_request(2, "Sam".into(), Duration::ZERO);
    menu.activate(MenuAction::JoinRequest(true));
    menu.clear_join_requests();
    assert_eq!(menu.view().join_request, None);
    assert_eq!(menu.take_join_reply(), None);
}
