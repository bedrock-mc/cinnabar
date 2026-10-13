use super::fixtures::warp_tree;
use super::*;
use crate::ui_runtime::interaction::dispatch_chat_ui_action;
use protocol::chat_text_packet;
use ui::{PointerPhase, UiPoint};

#[test]
fn tab_cycles_whitespace_suggestions_as_complete_replacements() {
    let mut player_runtime = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("/tell A").unwrap();
    let request = runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .chat_autocomplete
        .apply(
            request,
            ui::ChatAutocompleteDelta {
                enum_name: Arc::from("players"),
                action: ui::ChatAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("Alice One"), Arc::from("Alice Two")]),
            },
        )
        .unwrap();
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/tell Alice One");
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/tell Alice Two");
}
