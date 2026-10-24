//! Input for the showing data-driven form and the host state its requests read; the form's state
//! lives in client-ui.

use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::{
        ButtonState,
        mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel},
    },
    prelude::{
        ButtonInput, KeyCode, Local, MessageReader, MouseButton, Res, ResMut, Single, Window, With,
    },
    window::{CursorOptions, PrimaryWindow, WindowEvent},
};
use client_shell::{
    menu::MenuRuntime,
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_ui::ui_runtime::{
    data_driven::DataDrivenAction, presentation::forms::scene_policy::MenuScene,
    scene_stack::SceneHost,
};
use oreui::data_driven_screen::{FormHit, FormItem};
use protocol::ddui::DataDrivenScreenCloseReason;

use crate::ui_runtime::interaction::input_capture::{
    restore_gameplay_input_after_chat, suppress_gameplay_input_for_chat,
};

/// Publishes the host's menu screens, which screen requests arriving from UI events check for a
/// showing menu, and frees the input a form held through the frame it closed in.
pub(crate) fn publish_host_screens(
    menu: Option<Res<MenuRuntime>>,
    mut runtime: ResMut<UiRuntimeResource>,
) {
    // A form closed last frame held that frame's input; this frame's input is free again.
    if runtime.data_driven_ui().owns_input() && runtime.data_driven_ui().active().is_none() {
        runtime.release_closed_data_driven_input();
    }
    let host = SceneHost::of(menu.as_deref().map(|menu| menu as &dyn MenuScene), &runtime);
    runtime.observe_host_screens(host);
}

/// Logical px a wheel line scrolls the form's column.
const WHEEL_LINE: f32 = 40.0;

/// The pointer press in progress, the form instance it belongs to, and whether a form owned
/// input last frame.
#[derive(Default)]
pub(crate) struct FormGesture {
    held: Option<FormHit>,
    instance: Option<u64>,
    owned_last_frame: bool,
    /// Where the cursor ended last frame, which this frame's ordered moves start from.
    cursor: Option<ui::UiPoint>,
}

impl FormGesture {
    /// Follows the showing form instance: a press made on another form is dropped, so its release
    /// cannot act on the replacement.
    fn follow(&mut self, instance: Option<u64>) {
        if self.instance != instance {
            self.held = None;
            self.instance = instance;
        }
    }
}

/// Routes pointer, wheel and keyboard input to the showing data-driven form, and frees the
/// cursor while it shows.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_data_driven_input(
    player_runtime: Res<client_state::PlayerRuntime>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    window_events: Option<Res<Messages<WindowEvent>>>,
    mut event_cursor: Local<MessageCursor<WindowEvent>>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationResource>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut gesture: Local<FormGesture>,
    consent: Option<Res<client_state::ConsentInput>>,
) {
    let (window, mut cursor) = window.into_inner();
    let input_available = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    // Keys and left-button edges in arrival order, so text typed after a click reaches the
    // field that click focused.
    let inputs: Vec<FormInput> = window_events
        .as_deref()
        .map(|events| {
            event_cursor
                .read(events)
                .filter_map(FormInput::of)
                .collect()
        })
        .unwrap_or_default();
    if consent.is_some_and(|consent| consent.0) {
        // A consent or experience modal owns this frame; its events were read above and are
        // dropped, so the form never replays them. Their cursor moves are lost with them, so the
        // next edges start from the window's cursor.
        wheel.clear();
        gesture.held = None;
        gesture.cursor = None;
        return;
    }
    if runtime.credits().owns_input() || !runtime.data_driven_ui().owns_input() {
        wheel.clear();
        if gesture.owned_last_frame {
            presentation.set_data_driven_pointer(None, None);
            if !runtime.ui_focused(&player_runtime) && input_available {
                restore_gameplay_input_after_chat(&mut cursor, &mut keys, &mut mouse, &mut motion);
            }
        }
        *gesture = FormGesture::default();
        return;
    }
    if menu.as_ref().is_some_and(|menu| menu.is_visible()) {
        // The client was busy: a menu opened over the world as the form arrived.
        runtime.close_data_driven_screen(DataDrivenScreenCloseReason::UserBusy);
        wheel.clear();
        *gesture = FormGesture::default();
        return;
    }
    gesture.owned_last_frame = true;
    let instance = runtime
        .data_driven_ui()
        .active()
        .map(|active| active.instance);
    gesture.follow(instance);
    // Targets and slider tracks from a frame that painted another form instance are stale.
    let painted = presentation.data_driven_painted_form() == instance;
    if !input_available {
        wheel.clear();
        gesture.held = None;
        // Moves made meanwhile were not followed; the next edges start from the window's cursor.
        gesture.cursor = None;
        suppress_gameplay_input_for_chat(
            &player_runtime,
            &runtime,
            &mut cursor,
            &mut keys,
            &mut mouse,
            &mut motion,
        );
        return;
    }
    let form = runtime.data_driven_form();
    let pointer = window
        .cursor_position()
        .and_then(|point| ui::UiPoint::new(point.x, point.y).ok());

    for event in wheel.read() {
        let lines = match event.unit {
            MouseScrollUnit::Line => event.y * WHEEL_LINE,
            MouseScrollUnit::Pixel => event.y,
        };
        presentation.scroll_data_driven(pointer, -lines);
    }

    if mouse.just_pressed(MouseButton::Back) {
        runtime.data_driven_action(DataDrivenAction::Dismiss);
    }
    // Each button edge hits where the cursor was when it arrived.
    let mut at = gesture.cursor.or(pointer);
    gesture.cursor = pointer;
    for input in inputs {
        if runtime.data_driven_ui().active().is_none() {
            break;
        }
        match input {
            FormInput::Cursor(point) => at = Some(point),
            FormInput::Key(key_code, text, repeat) => {
                type_key(&mut runtime, key_code, text.as_deref(), repeat);
            }
            FormInput::Pointer(down) => {
                let hit = at
                    .filter(|_| painted)
                    .and_then(|point| presentation.hit_test_data_driven(point));
                if down {
                    gesture.held = hit;
                    press(&mut runtime, &presentation, form.as_ref(), hit, at);
                } else {
                    // A held slider takes the release position before the drag ends.
                    if let Some(FormHit::Item(key)) = gesture.held
                        && painted
                        && is_slider(form.as_ref(), key)
                    {
                        slide(&mut runtime, &presentation, key, at);
                    }
                    if let (Some(held), Some(released)) = (gesture.held, hit)
                        && held == released
                    {
                        release(&mut runtime, form.as_ref(), held);
                    }
                    gesture.held = None;
                }
            }
        }
    }
    if runtime.data_driven_ui().active().is_none() {
        // The player closed the form: the cursor may return to the game once the form's input
        // is released next frame, and the closing input must not act anywhere else this frame.
        if let Some(focus) = focus.as_deref_mut() {
            focus.authorize_screen_return();
        }
        *gesture = FormGesture {
            owned_last_frame: true,
            ..FormGesture::default()
        };
        presentation.set_data_driven_pointer(None, None);
        keys.reset_all();
        mouse.reset_all();
        return;
    }
    // A held slider follows the pointer along its track.
    if let Some(FormHit::Item(item)) = gesture.held
        && painted
        && is_slider(form.as_ref(), item)
    {
        slide(&mut runtime, &presentation, item, pointer);
    }
    presentation.set_data_driven_pointer(pointer, gesture.held);
    suppress_gameplay_input_for_chat(
        &player_runtime,
        &runtime,
        &mut cursor,
        &mut keys,
        &mut mouse,
        &mut motion,
    );
}

/// One input the form reads in arrival order.
enum FormInput {
    /// A key press, the text it typed, and whether it is an OS repeat of a held key.
    Key(KeyCode, Option<String>, bool),
    /// A left-button edge; `true` when pressed.
    Pointer(bool),
    /// The cursor moved here, in window-logical px.
    Cursor(ui::UiPoint),
}

impl FormInput {
    fn of(event: &WindowEvent) -> Option<Self> {
        match event {
            WindowEvent::KeyboardInput(input) if input.state == ButtonState::Pressed => {
                Some(Self::Key(
                    input.key_code,
                    input.text.as_ref().map(|text| text.to_string()),
                    input.repeat,
                ))
            }
            WindowEvent::MouseButtonInput(input) if input.button == MouseButton::Left => {
                Some(Self::Pointer(input.state == ButtonState::Pressed))
            }
            WindowEvent::CursorMoved(moved) => ui::UiPoint::new(moved.position.x, moved.position.y)
                .ok()
                .map(Self::Cursor),
            _ => None,
        }
    }
}

/// One key press: Escape and Enter end text entry, Escape closes an open option list or else
/// dismisses the form, and the focused text field takes typed text and Backspace. A held Escape
/// or Enter acts once; its repeats are ignored, while typing and Backspace repeat.
fn type_key(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    key_code: KeyCode,
    text: Option<&str>,
    repeat: bool,
) {
    if repeat
        && matches!(
            key_code,
            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter
        )
    {
        return;
    }
    let focused = runtime.data_driven_focus_items();
    let editing = focused.editing.is_some();
    match key_code {
        KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter if editing => {
            runtime.data_driven_action(DataDrivenAction::Blur);
        }
        KeyCode::Escape if focused.open_dropdown.is_some() => {
            runtime.data_driven_action(DataDrivenAction::Blur);
        }
        KeyCode::Escape => runtime.data_driven_action(DataDrivenAction::Dismiss),
        KeyCode::Backspace if editing => runtime.data_driven_action(DataDrivenAction::Backspace),
        _ if editing => {
            let typed: String = text
                .unwrap_or_default()
                .chars()
                .filter(|character| !character.is_control())
                .collect();
            if !typed.is_empty() {
                runtime.data_driven_action(DataDrivenAction::Type(typed));
            }
        }
        _ => {}
    }
}

/// A press focuses a text field, opens or closes a dropdown or moves a slider; buttons and
/// toggles act on release. A press elsewhere ends text entry and closes an open option list.
fn press(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    presentation: &client_ui::ui_runtime::presentation::UiPresentationRuntime,
    form: Option<&oreui::data_driven_screen::DataDrivenForm>,
    hit: Option<FormHit>,
    pointer: Option<ui::UiPoint>,
) {
    match (hit, hit.and_then(|hit| item_of(form, hit))) {
        (Some(FormHit::Option(_)), _) => {}
        (
            Some(FormHit::Item(item)),
            Some(FormItem::TextField { .. } | FormItem::Dropdown { .. }),
        ) => {
            runtime.data_driven_action(DataDrivenAction::Activate(item));
        }
        (Some(FormHit::Item(item)), Some(FormItem::Slider { .. })) => {
            slide(runtime, presentation, item, pointer);
        }
        _ => runtime.data_driven_action(DataDrivenAction::Blur),
    }
}

fn release(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    form: Option<&oreui::data_driven_screen::DataDrivenForm>,
    hit: FormHit,
) {
    match (hit, item_of(form, hit)) {
        (FormHit::Close, _) => runtime.data_driven_action(DataDrivenAction::Close),
        (FormHit::Option(choice), _) => {
            runtime.data_driven_action(DataDrivenAction::Choose(choice))
        }
        (FormHit::Item(item), Some(FormItem::Button { .. } | FormItem::Toggle { .. })) => {
            runtime.data_driven_action(DataDrivenAction::Activate(item));
        }
        _ => {}
    }
}

fn item_of(
    form: Option<&oreui::data_driven_screen::DataDrivenForm>,
    hit: FormHit,
) -> Option<&FormItem> {
    let FormHit::Item(key) = hit else {
        return None;
    };
    form?
        .items
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| &entry.item)
}

fn is_slider(form: Option<&oreui::data_driven_screen::DataDrivenForm>, item: usize) -> bool {
    matches!(
        item_of(form, FormHit::Item(item)),
        Some(FormItem::Slider { .. })
    )
}

/// Moves the slider with `key` to where the pointer lies along its thumb's travel.
fn slide(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    presentation: &client_ui::ui_runtime::presentation::UiPresentationRuntime,
    key: usize,
    pointer: Option<ui::UiPoint>,
) {
    if let Some(fraction) =
        pointer.and_then(|pointer| presentation.data_driven_slider_fraction(key, pointer.x()))
    {
        runtime.data_driven_action(DataDrivenAction::Slide(key, fraction));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_escape_leaves_the_form_after_ending_text_entry() {
        use client_ui::ui_runtime::{SequencedUiEvent, UiRuntime};
        use protocol::{ddui::ScriptedScreen, test_support::ddui};
        let mut runtime = UiRuntime::new(1);
        let mut player = player_state::PlayerState::new(1);
        let events = ddui::show_events(
            ScriptedScreen::CustomForm,
            4,
            7,
            ddui::custom_form_property(),
        );
        for (sequence, event) in (1..).zip(events) {
            runtime
                .apply(
                    &mut player,
                    SequencedUiEvent {
                        session_id: 1,
                        fifo_sequence: sequence,
                        local_millis: sequence * 10,
                        server_tick: None,
                        event,
                    },
                )
                .unwrap();
        }
        // Layout entry 6 is the fixture's text field.
        runtime.data_driven_action(DataDrivenAction::Activate(6));
        type_key(&mut runtime, KeyCode::Escape, None, false);
        assert_eq!(runtime.data_driven_focus_items().editing, None);
        type_key(&mut runtime, KeyCode::Escape, None, true);
        assert!(runtime.data_driven_ui().active().is_some());
        type_key(&mut runtime, KeyCode::Escape, None, false);
        assert!(runtime.data_driven_ui().active().is_none());
        // The closing frame keeps its input from the screens that update after the form.
        assert!(runtime.data_driven_ui().owns_input());
        runtime.release_closed_data_driven_input();
        assert!(!runtime.data_driven_ui().owns_input());
    }

    #[test]
    fn a_press_does_not_carry_over_to_a_replacement_form() {
        let mut gesture = FormGesture::default();
        gesture.follow(Some(4));
        gesture.held = Some(FormHit::Item(9));
        gesture.follow(Some(4));
        assert_eq!(gesture.held, Some(FormHit::Item(9)));
        gesture.follow(Some(5));
        assert_eq!(gesture.held, None);
    }
}
