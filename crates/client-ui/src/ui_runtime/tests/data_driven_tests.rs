use protocol::ddui::{
    DataDrivenScreenCloseReason, DataDrivenUiEvent, DataPrimitive, DataStoreChange,
    DataStoreUpdate, DataValue, SCRIPT_STORE, ScriptedScreen, ShowDataDrivenScreen,
    data_driven_screen_closed_packet, data_store_update_packet,
};
use protocol::test_support::ddui::{custom_form_property, message_box_property, show_events};

use super::*;
use crate::ui_runtime::data_driven::DataDrivenAction;

fn sent(runtime: &mut UiRuntime) -> Vec<Vec<u8>> {
    std::iter::from_fn(|| runtime.take_client_packet())
        .map(|packet| protocol::encode(&packet).unwrap().to_vec())
        .collect()
}

fn closed(form_id: u32, reason: DataDrivenScreenCloseReason) -> Vec<u8> {
    protocol::encode(&data_driven_screen_closed_packet(form_id, reason))
        .unwrap()
        .to_vec()
}

fn update(property: &str, path: &str, value: DataPrimitive, path_count: u32) -> Vec<u8> {
    let update = DataStoreUpdate {
        store: SCRIPT_STORE.into(),
        property: property.into(),
        path: path.into(),
        value,
        property_update_count: 1,
        path_update_count: path_count,
    };
    protocol::encode(&data_store_update_packet(&update))
        .unwrap()
        .to_vec()
}

/// Applies UI events in order after the last sequence used.
struct Session {
    runtime: UiRuntime,
    player: player_state::PlayerState,
    sequence: u64,
}

impl Session {
    fn new() -> Self {
        Self {
            runtime: UiRuntime::new(1),
            player: player_state::PlayerState::new(1),
            sequence: 0,
        }
    }

    fn apply(&mut self, events: impl IntoIterator<Item = UiEvent>) {
        for event in events {
            self.sequence += 1;
            self.runtime
                .apply(&mut self.player, envelope(1, self.sequence, event))
                .unwrap();
        }
    }

    fn show_form(&mut self, form_id: u32) {
        self.apply(show_events(
            ScriptedScreen::CustomForm,
            form_id,
            7,
            custom_form_property(),
        ));
    }

    /// The projected item position of the first item `matches` accepts.
    fn item(&self, matches: impl Fn(&oreui::data_driven_screen::FormItem) -> bool) -> usize {
        self.runtime
            .data_driven_form()
            .unwrap()
            .items
            .iter()
            .position(matches)
            .unwrap()
    }
}

#[test]
fn one_screen_shows_and_later_requests_are_answered_busy() {
    let mut session = Session::new();
    session.show_form(4);
    assert_eq!(
        session
            .runtime
            .data_driven_ui()
            .active()
            .map(|active| active.form_id),
        Some(4)
    );
    assert!(sent(&mut session.runtime).is_empty());
    let form = session.runtime.data_driven_form().unwrap();
    assert_eq!(form.title, "Data-Driven Test");
    assert!(form.close);

    session.show_form(5);
    assert_eq!(
        sent(&mut session.runtime),
        [closed(5, DataDrivenScreenCloseReason::UserBusy)]
    );
    assert_eq!(
        session
            .runtime
            .data_driven_ui()
            .active()
            .map(|active| active.form_id),
        Some(4)
    );
}

#[test]
fn server_closes_end_only_the_named_form_and_are_confirmed() {
    let mut session = Session::new();
    session.show_form(4);
    session.apply([UiEvent::DataDrivenUi(DataDrivenUiEvent::Close {
        form_id: Some(9),
    })]);
    assert!(session.runtime.data_driven_ui().owns_input());
    assert!(sent(&mut session.runtime).is_empty());

    session.apply([UiEvent::DataDrivenUi(DataDrivenUiEvent::Close {
        form_id: Some(4),
    })]);
    assert!(!session.runtime.data_driven_ui().owns_input());
    assert_eq!(
        sent(&mut session.runtime),
        [closed(4, DataDrivenScreenCloseReason::ProgrammaticClose)]
    );

    session.show_form(6);
    session.apply([UiEvent::DataDrivenUi(DataDrivenUiEvent::Close {
        form_id: None,
    })]);
    assert_eq!(
        sent(&mut session.runtime),
        [closed(6, DataDrivenScreenCloseReason::ProgrammaticCloseAll)]
    );
}

#[test]
fn unknown_screens_are_closed_so_scripts_finish() {
    let mut session = Session::new();
    session.apply([UiEvent::DataDrivenUi(DataDrivenUiEvent::Show(
        ShowDataDrivenScreen {
            screen_id: "pack:custom_screen".into(),
            form_id: 3,
            data_instance_id: None,
        },
    ))]);
    assert!(!session.runtime.data_driven_ui().owns_input());
    assert_eq!(
        sent(&mut session.runtime),
        [closed(3, DataDrivenScreenCloseReason::ClientCanceled)]
    );
}

#[test]
fn player_input_writes_the_control_paths_the_server_listens_on() {
    use oreui::data_driven_screen::FormItem;
    let property = ScriptedScreen::CustomForm.property(Some(7));
    let mut session = Session::new();
    session.show_form(4);

    let button =
        session.item(|item| matches!(item, FormItem::Button { label, .. } if label == "Click me"));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Activate(button));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Activate(button));
    let toggle = session.item(|item| matches!(item, FormItem::Toggle { .. }));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Activate(toggle));
    let slider = session.item(|item| matches!(item, FormItem::Slider { .. }));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Slide(slider, 0.62));
    let dropdown = session.item(|item| matches!(item, FormItem::Dropdown { .. }));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Activate(dropdown));
    assert_eq!(
        session.runtime.data_driven_focus_items().open_dropdown,
        Some(dropdown)
    );
    session
        .runtime
        .data_driven_action(DataDrivenAction::Choose(2));
    let field = session.item(|item| matches!(item, FormItem::TextField { .. }));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Activate(field));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Type(" world".into()));
    session.runtime.data_driven_action(DataDrivenAction::Blur);
    let disabled =
        session.item(|item| matches!(item, FormItem::Button { label, .. } if label == "Disabled"));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Activate(disabled));

    assert_eq!(
        sent(&mut session.runtime),
        [
            update(
                &property,
                "layout[9].onClick",
                DataPrimitive::Number(1.0),
                1
            ),
            update(
                &property,
                "layout[9].onClick",
                DataPrimitive::Number(2.0),
                2
            ),
            update(
                &property,
                "layout[3].toggled",
                DataPrimitive::Bool(false),
                1
            ),
            // 0.62 of 0..10 snaps to the nearest step of 2.
            update(&property, "layout[4].value", DataPrimitive::Number(6.0), 1),
            update(&property, "layout[5].value", DataPrimitive::Number(2.0), 1),
            update(
                &property,
                "layout[6].text",
                DataPrimitive::String("world".into()),
                1
            ),
        ]
    );
    let form = session.runtime.data_driven_form().unwrap();
    assert!(
        matches!(&form.items[field], FormItem::TextField { text, .. } if text == "hello world")
    );
    assert!(matches!(
        &form.items[toggle],
        FormItem::Toggle { on: false, .. }
    ));

    session.runtime.data_driven_action(DataDrivenAction::Close);
    assert_eq!(
        sent(&mut session.runtime),
        [
            update(
                &property,
                "closeButton.onClick",
                DataPrimitive::Number(1.0),
                1
            ),
            closed(4, DataDrivenScreenCloseReason::ClientCanceled),
        ]
    );
    assert!(!session.runtime.data_driven_ui().owns_input());
}

#[test]
fn message_box_buttons_report_their_press_and_close_the_box() {
    use oreui::data_driven_screen::FormItem;
    let property = ScriptedScreen::MessageBox.property(Some(7));
    let mut session = Session::new();
    session.apply(show_events(
        ScriptedScreen::MessageBox,
        8,
        7,
        message_box_property(),
    ));
    let form = session.runtime.data_driven_form().unwrap();
    assert!(matches!(&form.items[0], FormItem::Label(body) if body == "Message body text"));
    let no = session.item(|item| matches!(item, FormItem::Button { label, .. } if label == "No"));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Activate(no));
    assert_eq!(
        sent(&mut session.runtime),
        [
            update(&property, "button2.onClick", DataPrimitive::Number(1.0), 1),
            closed(8, DataDrivenScreenCloseReason::ClientCanceled),
        ]
    );
}

#[test]
fn server_updates_redraw_the_form_and_raw_messages_resolve() {
    use oreui::data_driven_screen::FormItem;
    let mut session = Session::new();
    session.show_form(4);
    session.apply([UiEvent::DataDrivenUi(DataDrivenUiEvent::Store(vec![
        DataStoreChange::Update(DataStoreUpdate {
            store: SCRIPT_STORE.into(),
            property: ScriptedScreen::CustomForm.property(Some(7)).into(),
            path: "layout[1].text".into(),
            value: DataPrimitive::String("Clicks: 1".into()),
            property_update_count: 1,
            path_update_count: 1,
        }),
    ]))]);
    let form = session.runtime.data_driven_form().unwrap();
    assert!(
        form.items
            .iter()
            .any(|item| matches!(item, FormItem::Label(text) if text == "Clicks: 1"))
    );
    assert!(
        form.items
            .iter()
            .any(|item| matches!(item, FormItem::Label(text) if text == "Raw message text"))
    );
}

#[test]
fn death_and_session_end_dismiss_the_form() {
    let mut session = Session::new();
    session.show_form(4);
    session.runtime.publish_local_player_alive(true);
    session.runtime.publish_local_player_alive(false);
    assert!(!session.runtime.data_driven_ui().owns_input());
    assert_eq!(
        sent(&mut session.runtime),
        [closed(4, DataDrivenScreenCloseReason::ClientCanceled)]
    );

    session.show_form(5);
    session.player.begin_session(2);
    session.runtime.begin_session(2);
    assert!(!session.runtime.data_driven_ui().owns_input());
    assert!(
        session
            .runtime
            .data_driven_ui()
            .stores()
            .property(SCRIPT_STORE, &ScriptedScreen::CustomForm.property(Some(7)))
            .is_none()
    );
}

#[test]
fn screen_answers_wait_out_a_full_client_queue() {
    let mut session = Session::new();
    let filler = closed(1, DataDrivenScreenCloseReason::UserBusy);
    let mut queued = 0;
    while session
        .runtime
        .queue_client_packet(data_driven_screen_closed_packet(
            1,
            DataDrivenScreenCloseReason::UserBusy,
        ))
    {
        queued += 1;
    }
    session.show_form(4);
    session.show_form(5);
    let mut expected = vec![filler; queued];
    expected.push(closed(5, DataDrivenScreenCloseReason::UserBusy));
    assert_eq!(sent(&mut session.runtime), expected);
}

/// Applies a server path update to the shown custom form (instance 7).
fn server_write(session: &mut Session, path: &str, value: DataPrimitive, path_count: u32) {
    session.apply([UiEvent::DataDrivenUi(DataDrivenUiEvent::Store(vec![
        DataStoreChange::Update(DataStoreUpdate {
            store: SCRIPT_STORE.into(),
            property: ScriptedScreen::CustomForm.property(Some(7)).into(),
            path: path.into(),
            value,
            property_update_count: 1,
            path_update_count: path_count,
        }),
    ]))]);
}

#[test]
fn typing_follows_the_focused_field_when_earlier_controls_hide() {
    use oreui::data_driven_screen::FormItem;
    let property = ScriptedScreen::CustomForm.property(Some(7));
    let mut session = Session::new();
    session.show_form(4);
    let field = session.item(|item| matches!(item, FormItem::TextField { .. }));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Activate(field));
    // The server hides the toggle above the field, moving the field up one item.
    server_write(
        &mut session,
        "layout[3].visible",
        DataPrimitive::Bool(false),
        1,
    );
    assert_eq!(
        session.runtime.data_driven_focus_items().editing,
        Some(field - 1)
    );
    session
        .runtime
        .data_driven_action(DataDrivenAction::Type("!".into()));
    session
        .runtime
        .data_driven_action(DataDrivenAction::Backspace);
    assert_eq!(
        sent(&mut session.runtime),
        [
            update(
                &property,
                "layout[6].text",
                DataPrimitive::String("hello!".into()),
                1
            ),
            update(
                &property,
                "layout[6].text",
                DataPrimitive::String("hello".into()),
                2
            ),
        ]
    );

    // A field that stops showing takes no more text.
    server_write(
        &mut session,
        "layout[6].visible",
        DataPrimitive::Bool(false),
        1,
    );
    session
        .runtime
        .data_driven_action(DataDrivenAction::Type("x".into()));
    assert!(sent(&mut session.runtime).is_empty());
    assert_eq!(session.runtime.data_driven_focus_items().editing, None);
}

#[test]
fn an_open_sign_editor_answers_screen_requests_busy() {
    let mut session = Session::new();
    session
        .runtime
        .sign_editor_mut()
        .open(crate::ui_runtime::sign_editor::SignEdit::new(
            [0, 64, 0],
            true,
            Default::default(),
        ));
    session.show_form(4);
    assert!(!session.runtime.data_driven_ui().owns_input());
    assert_eq!(
        sent(&mut session.runtime),
        [closed(4, DataDrivenScreenCloseReason::UserBusy)]
    );
}

#[test]
fn a_disabled_close_button_ignores_presses_but_escape_still_dismisses() {
    let mut session = Session::new();
    let mut form = custom_form_property();
    let DataValue::Map(entries) = &mut form else {
        panic!("the form property is an object");
    };
    let Some(DataValue::Map(close)) = entries.get_mut("closeButton") else {
        panic!("the form has a close button");
    };
    close.insert("disabled".into(), DataValue::Bool(true));
    session.apply(show_events(ScriptedScreen::CustomForm, 4, 7, form));
    assert_eq!(
        session.runtime.data_driven_form().unwrap().close,
        Some(false)
    );
    session.runtime.data_driven_action(DataDrivenAction::Close);
    assert!(session.runtime.data_driven_ui().owns_input());
    assert!(sent(&mut session.runtime).is_empty());

    session
        .runtime
        .data_driven_action(DataDrivenAction::Dismiss);
    assert!(!session.runtime.data_driven_ui().owns_input());
    assert_eq!(
        sent(&mut session.runtime),
        [closed(4, DataDrivenScreenCloseReason::ClientCanceled)]
    );
}
