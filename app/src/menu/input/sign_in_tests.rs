//! Sign-in prompts own input even when another route was being edited.

use super::*;
use crate::menu::{AuthState, MenuAction, MenuField, MenuScreen};

#[test]
fn device_prompt_releases_hidden_text_and_escape_cancels_first() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.screen = MenuScreen::AddServer;
    menu.name.set_text("Saved draft");
    menu.focus_field(MenuField::Name);
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    assert!(menu.field.is_none());
    menu.edit_text("hidden change");
    assert_eq!(menu.name.as_str(), "Saved draft");
    menu.go_back_from_input();
    assert_eq!(menu.current_auth().as_ref(), &AuthState::SignedOut);
    assert_eq!(menu.screen, MenuScreen::AddServer);
}

#[test]
fn device_prompt_back_precedes_hidden_inbox_controls() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.screen = MenuScreen::Inbox;
    menu.feeds.inbox_state.filters = true;
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    menu.go_back_from_input();
    assert_eq!(menu.current_auth().as_ref(), &AuthState::SignedOut);
    assert!(menu.feeds.inbox_state.filters);
}

#[test]
fn device_prompt_actions_precede_a_hidden_skin_editor() {
    use launcher::dressing_room::{SkinEditor, SkinEditorMode, SkinEditorTarget};
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.screen = MenuScreen::DressingRoom;
    std::sync::Arc::make_mut(&mut menu.dressing_room).editor = Some(SkinEditor {
        index: 0,
        mode: SkinEditorMode::Rename,
        target: SkinEditorTarget::Skin,
        draft: "Saved draft".into(),
    });
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    menu.activate(MenuAction::OpenSignInLink);
    assert_ne!(
        menu.sign_in_browser.state(menu.current_auth().as_ref()),
        launcher::menu::sign_in::BrowserState::Waiting
    );
    menu.go_back_from_input();
    assert_eq!(menu.current_auth().as_ref(), &AuthState::SignedOut);
    assert!(menu.dressing_room.editor.is_some());
}

#[test]
fn device_prompt_releases_pending_binding_capture() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.screen = MenuScreen::Settings;
    menu.key_remap = Some(0);
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    let view = menu.view();
    assert!(view.key_remap.is_none());
    assert_eq!(view.focused_action, Some(MenuAction::OpenSignInLink));
}

#[test]
fn a_device_code_keeps_confirmation_focus_until_the_dialog_closes() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.dialog = Some(crate::menu::MenuDialog::Exit);
    menu.focused = 1;
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    let view = menu.view();
    assert!(!view.sign_in_prompt_open());
    assert_eq!(view.focused_action, Some(MenuAction::DismissDialog));
    menu.activate(MenuAction::DismissDialog);
    let view = menu.view();
    assert!(view.sign_in_prompt_open());
    assert_eq!(view.focused_action, Some(MenuAction::OpenSignInLink));
}

#[test]
fn completed_add_account_prompt_keeps_cancel_above_a_hidden_editor() {
    use launcher::dressing_room::{SkinEditor, SkinEditorMode, SkinEditorTarget};
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.screen = MenuScreen::DressingRoom;
    std::sync::Arc::make_mut(&mut menu.dressing_room).editor = Some(SkinEditor {
        index: 0,
        mode: SkinEditorMode::Rename,
        target: SkinEditorTarget::Skin,
        draft: "Saved draft".into(),
    });
    menu.dialog = Some(crate::menu::MenuDialog::Accounts);
    menu.feeds.account_adding = true;
    menu.apply_control_auth(AuthState::Authenticated);
    assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
    menu.activate_focused();
    assert!(menu.dialog.is_none());
    assert!(menu.sign_in_cancelled);
    assert!(menu.dressing_room.editor.is_some());
}

#[test]
fn settings_refresh_keeps_sign_in_focus_before_geometry_arrives() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.screen = MenuScreen::Settings;
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    let actions = [
        MenuAction::CancelSignIn,
        MenuAction::OpenSignInLink,
        MenuAction::CancelSignIn,
    ];
    for _ in 0..3 {
        menu.refresh_settings_focus(actions);
        assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
    }
    menu.focused = 1;
    for _ in 0..3 {
        menu.refresh_settings_focus(actions);
        assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
    }
}

#[test]
fn pending_join_request_defers_input_until_sign_in_closes() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.push_join_request(1, "Alex".into(), std::time::Duration::ZERO);
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    assert!(!menu.join_request_prompted());
    assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
    menu.go_back_from_input();
    assert_eq!(menu.current_auth().as_ref(), &AuthState::SignedOut);
    assert!(menu.join_request_prompted());
    assert_eq!(menu.take_join_reply(), None);
}
