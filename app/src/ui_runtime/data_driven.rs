//! Pointer and keyboard input for the showing data-driven form; its state lives in client-ui.

use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::{AccumulatedMouseMotion, MouseButtonInput, MouseScrollUnit, MouseWheel},
    },
    prelude::{
        ButtonInput, KeyCode, Local, MessageReader, MouseButton, Res, ResMut, Single, Window, With,
    },
    window::{CursorOptions, PrimaryWindow},
};
use client_shell::{
    menu::MenuRuntime,
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_ui::ui_runtime::data_driven::DataDrivenAction;
use oreui::data_driven_screen::{FormHit, FormItem};
use protocol::ddui::DataDrivenScreenCloseReason;

use crate::ui_runtime::interaction::input_capture::{
    restore_gameplay_input_after_chat, suppress_gameplay_input_for_chat,
};

/// Logical px a wheel line scrolls the form's column.
const WHEEL_LINE: f32 = 40.0;

/// The pointer press in progress and the text being typed into a field.
#[derive(Default)]
pub(crate) struct FormGesture {
    held: Option<FormHit>,
    owned_last_frame: bool,
    /// The edited field's item position and its text so far.
    editing: Option<(usize, String)>,
}

/// Routes pointer, wheel and keyboard input to the showing data-driven form, and frees the
/// cursor while it shows.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_data_driven_input(
    player_runtime: Res<client_state::PlayerRuntime>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    focus: Option<Res<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut keyboard: MessageReader<KeyboardInput>,
    button_messages: Option<Res<Messages<MouseButtonInput>>>,
    mut button_cursor: Local<MessageCursor<MouseButtonInput>>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationResource>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut gesture: Local<FormGesture>,
) {
    let (window, mut cursor) = window.into_inner();
    let input_available = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    let edges: Vec<bool> = button_messages
        .as_deref()
        .map(|messages| {
            button_cursor
                .read(messages)
                .filter(|input| input.button == MouseButton::Left)
                .map(|input| input.state == ButtonState::Pressed)
                .collect()
        })
        .unwrap_or_default();
    if runtime.credits().owns_input() || !runtime.data_driven_ui().owns_input() {
        wheel.clear();
        keyboard.clear();
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
        keyboard.clear();
        *gesture = FormGesture::default();
        return;
    }
    gesture.owned_last_frame = true;
    if !input_available {
        wheel.clear();
        keyboard.clear();
        gesture.held = None;
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
        presentation.scroll_data_driven(-lines);
    }

    let mut dismiss = mouse.just_pressed(MouseButton::Back);
    for event in keyboard
        .read()
        .filter(|event| event.state == ButtonState::Pressed)
    {
        match (event.key_code, gesture.editing.as_mut()) {
            (KeyCode::Escape, Some(_)) | (KeyCode::Enter | KeyCode::NumpadEnter, Some(_)) => {
                gesture.editing = None;
            }
            (KeyCode::Escape, None) if presentation.data_driven_open_dropdown().is_some() => {
                presentation.set_data_driven_dropdown(None);
            }
            (KeyCode::Escape, None) => dismiss = true,
            (KeyCode::Backspace, Some((item, text))) => {
                if text.pop().is_some() {
                    runtime.data_driven_action(DataDrivenAction::Text(*item, text.clone()));
                }
            }
            (_, Some((item, text))) => {
                let typed: String = event
                    .text
                    .iter()
                    .flat_map(|text| text.chars())
                    .filter(|character| !character.is_control())
                    .collect();
                if !typed.is_empty() {
                    text.push_str(&typed);
                    runtime.data_driven_action(DataDrivenAction::Text(*item, text.clone()));
                }
            }
            _ => {}
        }
    }
    if dismiss {
        runtime.data_driven_action(DataDrivenAction::Close);
        *gesture = FormGesture::default();
        // The dismissing key must not also open the pause menu this frame.
        keys.reset_all();
        mouse.reset_all();
        return;
    }

    for down in edges {
        let hit = pointer.and_then(|point| presentation.hit_test_data_driven(point));
        if down {
            gesture.held = hit;
            press(
                &mut runtime,
                &mut presentation,
                &mut gesture,
                form.as_ref(),
                hit,
                pointer,
            );
        } else {
            if let (Some(held), Some(released)) = (gesture.held, hit)
                && held == released
            {
                release(&mut runtime, &mut presentation, form.as_ref(), held);
            }
            gesture.held = None;
        }
    }
    // A held slider follows the pointer along its track.
    if let Some(FormHit::Item(item)) = gesture.held
        && is_slider(form.as_ref(), item)
    {
        slide(&mut runtime, &presentation, item, pointer);
    }
    presentation.set_data_driven_pointer(pointer, gesture.held);
    presentation.set_data_driven_editing(gesture.editing.as_ref().map(|(item, _)| *item));
    suppress_gameplay_input_for_chat(
        &player_runtime,
        &runtime,
        &mut cursor,
        &mut keys,
        &mut mouse,
        &mut motion,
    );
}

/// A press focuses or opens what it lands on; buttons and toggles act on release.
fn press(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    presentation: &mut client_ui::ui_runtime::presentation::UiPresentationRuntime,
    gesture: &mut FormGesture,
    form: Option<&oreui::data_driven_screen::DataDrivenForm>,
    hit: Option<FormHit>,
    pointer: Option<ui::UiPoint>,
) {
    let open = presentation.data_driven_open_dropdown();
    if !matches!(hit, Some(FormHit::Option(_))) {
        presentation.set_data_driven_dropdown(None);
    }
    let Some(FormHit::Item(item)) = hit else {
        gesture.editing = None;
        return;
    };
    match form.and_then(|form| form.items.get(item)) {
        Some(FormItem::TextField { text, .. }) => {
            gesture.editing = Some((item, text.clone()));
        }
        Some(FormItem::Dropdown { .. }) => {
            gesture.editing = None;
            presentation.set_data_driven_dropdown((open != Some(item)).then_some(item));
        }
        Some(FormItem::Slider { .. }) => {
            gesture.editing = None;
            slide(runtime, presentation, item, pointer);
        }
        _ => gesture.editing = None,
    }
}

fn release(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    presentation: &mut client_ui::ui_runtime::presentation::UiPresentationRuntime,
    form: Option<&oreui::data_driven_screen::DataDrivenForm>,
    hit: FormHit,
) {
    match hit {
        FormHit::Close => runtime.data_driven_action(DataDrivenAction::Close),
        FormHit::Option(choice) => {
            if let Some(item) = presentation.data_driven_open_dropdown() {
                runtime.data_driven_action(DataDrivenAction::Choose(item, choice));
            }
            presentation.set_data_driven_dropdown(None);
        }
        FormHit::Item(item) => {
            if matches!(
                form.and_then(|form| form.items.get(item)),
                Some(FormItem::Button { .. } | FormItem::Toggle { .. })
            ) {
                runtime.data_driven_action(DataDrivenAction::Activate(item));
            }
        }
    }
}

fn is_slider(form: Option<&oreui::data_driven_screen::DataDrivenForm>, item: usize) -> bool {
    matches!(
        form.and_then(|form| form.items.get(item)),
        Some(FormItem::Slider { .. })
    )
}

/// Moves slider `item` to where the pointer lies along its last drawn track.
fn slide(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    presentation: &client_ui::ui_runtime::presentation::UiPresentationRuntime,
    item: usize,
    pointer: Option<ui::UiPoint>,
) {
    let (Some(pointer), Some(track)) = (
        pointer,
        presentation.data_driven_bounds(FormHit::Item(item)),
    ) else {
        return;
    };
    let width = track.max().x() - track.min().x();
    if width <= 0.0 {
        return;
    }
    let fraction = (pointer.x() - track.min().x()) / width;
    runtime.data_driven_action(DataDrivenAction::Slide(item, fraction));
}
