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

/// The pointer press in progress, and whether a form owned input last frame.
#[derive(Default)]
pub(crate) struct FormGesture {
    held: Option<FormHit>,
    owned_last_frame: bool,
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
        presentation.scroll_data_driven(pointer, -lines);
    }

    let focused = runtime.data_driven_focus_items();
    let mut dismiss = mouse.just_pressed(MouseButton::Back);
    for event in keyboard
        .read()
        .filter(|event| event.state == ButtonState::Pressed)
    {
        let editing = focused.editing.is_some();
        match event.key_code {
            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter if editing => {
                runtime.data_driven_action(DataDrivenAction::Blur);
            }
            KeyCode::Escape if focused.open_dropdown.is_some() => {
                runtime.data_driven_action(DataDrivenAction::Blur);
            }
            KeyCode::Escape => dismiss = true,
            KeyCode::Backspace if editing => {
                runtime.data_driven_action(DataDrivenAction::Backspace);
            }
            _ if editing => {
                let typed: String = event
                    .text
                    .iter()
                    .flat_map(|text| text.chars())
                    .filter(|character| !character.is_control())
                    .collect();
                if !typed.is_empty() {
                    runtime.data_driven_action(DataDrivenAction::Type(typed));
                }
            }
            _ => {}
        }
    }
    if dismiss {
        runtime.data_driven_action(DataDrivenAction::Dismiss);
    }

    for down in edges {
        if !runtime.data_driven_ui().owns_input() {
            break;
        }
        let hit = pointer.and_then(|point| presentation.hit_test_data_driven(point));
        if down {
            gesture.held = hit;
            press(&mut runtime, &presentation, form.as_ref(), hit, pointer);
        } else {
            if let (Some(held), Some(released)) = (gesture.held, hit)
                && held == released
            {
                release(&mut runtime, form.as_ref(), held);
            }
            gesture.held = None;
        }
    }
    if !runtime.data_driven_ui().owns_input() {
        // The player closed the form: the cursor may return to the game, and the closing input
        // must not also act there or open the pause menu this frame.
        if let Some(focus) = focus.as_deref_mut() {
            focus.authorize_screen_return();
        }
        *gesture = FormGesture::default();
        presentation.set_data_driven_pointer(None, None);
        keys.reset_all();
        mouse.reset_all();
        if !runtime.ui_focused(&player_runtime) {
            restore_gameplay_input_after_chat(&mut cursor, &mut keys, &mut mouse, &mut motion);
        }
        return;
    }
    // A held slider follows the pointer along its track.
    if let Some(FormHit::Item(item)) = gesture.held
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
    let FormHit::Item(item) = hit else {
        return None;
    };
    form?.items.get(item)
}

fn is_slider(form: Option<&oreui::data_driven_screen::DataDrivenForm>, item: usize) -> bool {
    matches!(
        item_of(form, FormHit::Item(item)),
        Some(FormItem::Slider { .. })
    )
}

/// Moves slider `item` to where the pointer lies along its thumb's travel.
fn slide(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    presentation: &client_ui::ui_runtime::presentation::UiPresentationRuntime,
    item: usize,
    pointer: Option<ui::UiPoint>,
) {
    if let Some(fraction) =
        pointer.and_then(|pointer| presentation.data_driven_slider_fraction(item, pointer.x()))
    {
        runtime.data_driven_action(DataDrivenAction::Slide(item, fraction));
    }
}
