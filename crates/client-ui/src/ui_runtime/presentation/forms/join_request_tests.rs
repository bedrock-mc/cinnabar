//! A Discord join request asks the host through vanilla's modal popup over the launcher and
//! pause screens; only its Accept and Decline buttons take presses.

use std::sync::Arc;

use super::{pack_harness, test_support::draw_menu_actions};
use crate::menu::{MenuAction, MenuDialog, MenuScreen, MenuView, join_requests};

fn asking(screen: MenuScreen) -> MenuView {
    let mut view = MenuView::new(true, "Host".into());
    view.screen = screen;
    view.over_world = screen == MenuScreen::Pause;
    view.join_request = Some("Alex".into());
    view
}

#[test]
fn join_request_popup_answers_with_accept_and_decline() {
    for screen in [MenuScreen::Home, MenuScreen::Pause] {
        let Some(mut presentation) = pack_harness::engine_presentation() else {
            eprintln!(
                "skipping join_request_popup_answers_with_accept_and_decline: missing local UI carrier; make assets"
            );
            return;
        };
        let player_runtime = player_state::PlayerState::new(1);
        let actions = draw_menu_actions(&player_runtime, &mut presentation, &asking(screen));
        let texts = pack_harness::drawn_texts(pack_harness::menu_nodes(&presentation)).join(" ");
        for expected in [join_requests::title("Alex").as_str(), "Accept", "Decline"] {
            assert!(
                texts.contains(expected),
                "{screen:?}: missing {expected:?} in {texts:?}"
            );
        }
        for answer in [true, false] {
            assert!(
                actions.contains(&MenuAction::JoinRequest(answer)),
                "{screen:?}: {actions:?}"
            );
        }
        assert!(
            actions
                .iter()
                .all(|action| matches!(action, MenuAction::JoinRequest(_))),
            "{screen:?}: {actions:?}"
        );
    }
}

#[test]
fn an_open_launcher_dialog_keeps_its_buttons_over_a_join_request() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        eprintln!(
            "skipping an_open_launcher_dialog_keeps_its_buttons_over_a_join_request: missing local UI carrier; make assets"
        );
        return;
    };
    let player_runtime = player_state::PlayerState::new(1);
    let mut view = asking(MenuScreen::Home);
    view.dialog = Some(MenuDialog::Exit);
    let actions = draw_menu_actions(&player_runtime, &mut presentation, &view);
    assert!(
        actions.contains(&MenuAction::ConfirmExit)
            && !actions
                .iter()
                .any(|action| matches!(action, MenuAction::JoinRequest(_))),
        "{actions:?}"
    );
}

#[test]
fn join_request_buttons_read_the_active_language() {
    let translate = |key: &str| match key {
        "gui.accept" => Some(Arc::<str>::from("Annehmen")),
        "gui.decline" => Some(Arc::<str>::from("Ablehnen")),
        _ => None,
    };
    let json_ui::FormModel::Modal(modal) =
        super::menu_screens::join_request_model("Alex", &translate)
    else {
        panic!("a join request is a modal popup");
    };
    assert_eq!(
        (
            modal.title.as_str(),
            modal.button1.as_str(),
            modal.button2.as_str()
        ),
        (
            join_requests::title("Alex").as_str(),
            "Annehmen",
            "Ablehnen"
        )
    );
}
