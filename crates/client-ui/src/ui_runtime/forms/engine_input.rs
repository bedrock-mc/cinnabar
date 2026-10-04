//! Input for a form drawn by the JSON-UI engine. Raw pointer and keyboard
//! edges become vanilla input buttons (`button.menu_select`, `button.menu_ok`,
//! …) that the engine's dispatcher routes through the template's button
//! mappings to its components; this module is the form's screen controller,
//! turning the resulting screen events into form values and answers.

use bevy::input::{ButtonInput, keyboard::KeyCode, mouse::MouseScrollUnit};
use json_ui::{
    ButtonEvent, ButtonInput as EngineButton, Dispatch, HitKind, HitRegion, InputMode,
    PointerInput, ScreenEvent, hit_test,
};
use protocol::{CustomFormElement, MenuElement, ServerFormModel};
use ui::{ChatClipboard, UiPoint};

use super::engine_focus;
use super::engine_scroll;
use super::values::{EngineFrame, slider_value_at};
use super::{FormValue, LocalFormAction};
use crate::ui_runtime::{PlatformClipboard, UiRuntime};

/// Longest paste accepted into an edit box, before its own `max_length`.
const MAX_PASTE_BYTES: usize = 4096;
/// Animation end events one input frame relays at most.
const MAX_END_EVENTS: usize = 64;
/// The input button a primary pointer press is.
const SELECT: &str = "button.menu_select";

#[cfg(test)]
mod tests;

/// The primary pointer button's edges this frame and whether it is down.
#[derive(Clone, Copy, Debug, Default)]
pub struct PointerButtons {
    pub pressed: bool,
    pub released: bool,
    pub held: bool,
}

/// One frame of raw input for the engine path.
pub struct EngineInput<'a> {
    pub cursor: Option<UiPoint>,
    pub keys: &'a ButtonInput<KeyCode>,
    pub pointer: PointerButtons,
    pub pointer_edges: Vec<bool>,
    pub wheel: Vec<(f32, MouseScrollUnit)>,
    /// Pressed keys this frame with their produced text.
    /// Key presses with their text and whether the OS auto-repeated them.
    pub typed: Vec<(KeyCode, Option<String>, bool)>,
    /// Seconds on the app clock.
    pub now: f64,
    /// The form's animator: button events play and reset its animations.
    pub animator: Option<std::sync::MutexGuard<'a, json_ui::Animator>>,
}

/// Dispatches one captured frame through the retained form controller.
pub fn drive(runtime: &mut UiRuntime, frame: &EngineFrame, mut input: EngineInput<'_>) {
    let Some(entry) = runtime.server_forms().active() else {
        return;
    };
    let identity = entry.identity;
    let model = entry.model.clone();
    // Controls `destroy_at_end` removed take no input.
    let survivors;
    let frame = match input.animator.as_ref() {
        Some(animator) if frame.hits.iter().any(|hit| animator.is_destroyed(&hit.key)) => {
            survivors = EngineFrame {
                hits: frame
                    .hits
                    .iter()
                    .filter(|hit| !animator.is_destroyed(&hit.key))
                    .cloned()
                    .collect(),
                ..frame.clone()
            };
            &survivors
        }
        _ => frame,
    };
    let point = input.cursor.map(|cursor| frame.to_virtual(cursor));
    let control = input.keys.pressed(KeyCode::ControlLeft)
        || input.keys.pressed(KeyCode::ControlRight)
        || input.keys.pressed(KeyCode::SuperLeft)
        || input.keys.pressed(KeyCode::SuperRight);
    let mut events = Vec::new();
    {
        let engine = runtime.server_forms_mut().engine_mut();
        let delta = (input.now - engine.clock).max(0.0);
        engine.clock = input.now;
        engine.dispatcher.tick(&frame.hits, &mut engine.view, delta);
        let pointer = PointerInput {
            point,
            held: input.pointer.held,
            mode: InputMode::Mouse,
            now: input.now,
        };
        events.extend(
            engine
                .dispatcher
                .pointer(&frame.hits, &mut engine.view, pointer)
                .events,
        );
        // Keyboard focus shows as hover while the pointer rests on nothing.
        if engine.view.hovered.is_none() {
            engine.view.hovered = engine.view.focused.clone();
        }
    }
    engine_scroll::step(runtime, frame);
    let edges = if input.pointer_edges.is_empty() {
        [
            input.pointer.pressed.then_some(true),
            input.pointer.released.then_some(false),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        std::mem::take(&mut input.pointer_edges)
    };
    // InputComponent sends pointer deltas to active components;
    // ScrollViewComponent consumes them while capture is still down.
    // Move the existing capture once before any release, even when the final state is up.
    if let Some(point) = point
        && (input.pointer.held || edges.contains(&false))
    {
        engine_scroll::drag(runtime, frame, point);
    }
    // Each release answers only for the control its press went down on.
    let mut releases = Vec::new();
    for down in edges {
        if down {
            if let Some(point) = point {
                let region = hit_test(&frame.hits, point);
                engine_scroll::press(runtime, frame, region, point);
                events.extend(button(runtime, frame, SELECT, true, Some(point), input.now).events);
            }
        } else {
            runtime.server_forms_mut().engine_mut().drag = None;
            // A touch pan past the tap slop presses nothing.
            let tapped = engine_scroll::release(runtime);
            let pressed = runtime
                .server_forms()
                .engine()
                .view
                .pressed
                .clone()
                .filter(|_| tapped);
            let up = button(runtime, frame, SELECT, false, point, input.now).events;
            releases.push((events.len()..events.len() + up.len(), pressed));
            events.extend(up);
        }
    }
    if let Some(point) = point
        && !input.wheel.is_empty()
    {
        engine_scroll::wheel(runtime, frame, point, &input.wheel);
    }
    for (key, text, repeat) in input.typed {
        // Held keys repeat text into an edit box, but never re-press a control.
        if repeat && !editing(runtime, frame) {
            continue;
        }
        events.extend(keyboard(
            runtime,
            frame,
            key,
            text.as_deref(),
            control,
            input.now,
        ));
    }
    if let Some(animator) = input.animator.as_mut() {
        animate(animator, &mut events);
    }
    let mut action = None;
    for (at, event) in events.iter().enumerate() {
        let pressed = releases
            .iter()
            .find(|(range, _)| range.contains(&at))
            .and_then(|(_, pressed)| pressed.as_deref());
        if let Some(found) = controller(runtime, frame, &model, event, pressed) {
            action = Some(found);
        }
    }
    if let Some(action) = action {
        let _ = runtime.respond_to_server_form(identity, action);
    }
}

/// Fire each button event into the animator; its `end_event`s come back as
/// button events the controller and other animations receive.
fn animate(animator: &mut json_ui::Animator, events: &mut Vec<ScreenEvent>) {
    // Ends that start animations ending at once must not feed back forever.
    let cap = events.len() + MAX_END_EVENTS;
    append_animation_ends(animator, events, cap);
    let mut at = 0;
    while at < events.len() {
        if let ScreenEvent::Button(button) = &events[at]
            && button.down
        {
            animator.fire(&button.id);
        }
        append_animation_ends(animator, events, cap);
        at += 1;
    }
}

/// Relays pending end events even when this frame has no physical input.
fn append_animation_ends(
    animator: &mut json_ui::Animator,
    events: &mut Vec<ScreenEvent>,
    cap: usize,
) {
    for ended in animator.take_events() {
        if let json_ui::AnimEvent::End(id) = ended
            && events.len() < cap
        {
            events.push(ScreenEvent::Button(ButtonEvent {
                id,
                from: String::new(),
                key: String::new(),
                collection_index: None,
                collection: None,
                down: true,
                interacted: true,
                scope: json_ui::MappingScope::Controller,
            }));
        }
    }
}

/// One input button edge through the dispatcher.
fn button(
    runtime: &mut UiRuntime,
    frame: &EngineFrame,
    id: &str,
    down: bool,
    point: Option<[f64; 2]>,
    now: f64,
) -> Dispatch {
    let engine = runtime.server_forms_mut().engine_mut();
    let input = EngineButton {
        id,
        down,
        point,
        mode: InputMode::Mouse,
        now,
    };
    engine
        .dispatcher
        .button(&frame.hits, &mut engine.view, input)
}

/// Whether the selected component is an edit box taking typed text.
fn editing(runtime: &UiRuntime, frame: &EngineFrame) -> bool {
    runtime
        .server_forms()
        .engine()
        .view
        .components
        .selected()
        .is_some_and(|key| edit_region(frame, key).is_some())
}

/// A key as the input buttons vanilla's keyboard mapping raises, or typed text
/// for the selected edit box.
fn keyboard(
    runtime: &mut UiRuntime,
    frame: &EngineFrame,
    key: KeyCode,
    text: Option<&str>,
    control: bool,
    now: f64,
) -> Vec<ScreenEvent> {
    let editing = editing(runtime, frame);
    let typed = match key {
        KeyCode::Backspace if editing => Some("\u{8}".to_owned()),
        KeyCode::Enter | KeyCode::NumpadEnter if editing => Some("\r".to_owned()),
        KeyCode::KeyV if control && editing => PlatformClipboard
            .read_text_bounded(MAX_PASTE_BYTES)
            .ok()
            .flatten()
            .map(|text| text.to_string()),
        _ if editing && !control => text
            .filter(|text| !text.chars().any(char::is_control))
            .map(str::to_owned),
        _ => None,
    };
    if let Some(typed) = typed {
        let engine = runtime.server_forms_mut().engine_mut();
        return engine
            .dispatcher
            .text(&frame.hits, &mut engine.view, &typed, None)
            .events;
    }
    let id = match key {
        KeyCode::Escape => "button.menu_cancel",
        KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => "button.menu_ok",
        KeyCode::ArrowUp => "button.menu_up",
        KeyCode::ArrowDown => "button.menu_down",
        KeyCode::ArrowLeft => "button.menu_left",
        KeyCode::ArrowRight => "button.menu_right",
        KeyCode::Tab => {
            engine_focus::tab(runtime, frame);
            return Vec::new();
        }
        _ => return Vec::new(),
    };
    let down = button(runtime, frame, id, true, None, now);
    let consumed = down.consumed;
    let mut events = down.events;
    events.extend(button(runtime, frame, id, false, None, now).events);
    // Content subtrees omit the screen's global cancel mapping.
    if !consumed && key == KeyCode::Escape
        && let Some(target) = &frame.cancel_target
        && !events.iter().any(|event| matches!(event, ScreenEvent::Button(button) if button.id == *target && button.down && button.interacted))
    {
        events.push(ScreenEvent::Button(ButtonEvent {
            id: target.clone(),
            from: id.to_owned(),
            key: String::new(),
            collection_index: None,
            collection: None,
            down: true,
            interacted: true,
            scope: json_ui::MappingScope::Global,
        }));
    }
    // An unconsumed direction moves focus.
    if !consumed && let Some(direction) = engine_focus::direction_of(key) {
        engine_focus::step(runtime, frame, direction);
    }
    events
}

/// The form's screen controller: what each screen event does to its values.
fn controller(
    runtime: &mut UiRuntime,
    frame: &EngineFrame,
    model: &ServerFormModel,
    event: &ScreenEvent,
    pressed: Option<&str>,
) -> Option<LocalFormAction> {
    match event {
        ScreenEvent::Button(button) => {
            // A pointer press answers on release over the control it went down on.
            let answers = if button.from == SELECT {
                !button.down && pressed == Some(button.key.as_str())
            } else {
                button.down && button.interacted
            };
            if button.id == "button.dropdown_exit" && button.down {
                close_dropdown(runtime, frame);
            }
            let action = answers.then(|| mapped_action(model, button)).flatten();
            if let Some(action) = &action {
                // Names the input that answered, for diagnosing unintended answers.
                bevy::log::info!(
                    target: "server_form",
                    id = %button.id,
                    from = %button.from,
                    key = %button.key,
                    index = ?button.collection_index,
                    ?action,
                    "form answered"
                );
            }
            action
        }
        ScreenEvent::Toggle {
            name,
            key,
            index,
            checked,
            ..
        } => {
            let index = (*index)?;
            let engine = runtime.server_forms_mut().engine_mut();
            let dropdown = frame
                .hits
                .iter()
                .any(|region| region.key == *key && region.kind == HitKind::Dropdown);
            if dropdown {
                engine.open_dropdown = checked.then_some(index);
            } else if name == "custom_dropdown_radio_toggle" {
                // Choosing an option answers the open dropdown and closes it.
                if *checked
                    && let Some(open) = engine.open_dropdown
                    && let Some(value) = engine.values.get_mut(open)
                {
                    *value = FormValue::Dropdown(index);
                    close_dropdown(runtime, frame);
                }
            } else if let Some(FormValue::Toggle(on)) = engine.values.get_mut(index) {
                *on = *checked;
            }
            None
        }
        ScreenEvent::Slider {
            index, value, step, ..
        } => {
            set_slider(runtime, model, (*index)?, *value, *step);
            None
        }
        ScreenEvent::TextEdit { index, text, .. } => {
            let engine = runtime.server_forms_mut().engine_mut();
            if let Some(FormValue::Text(value)) =
                index.and_then(|index| engine.values.get_mut(index))
            {
                value.clone_from(text);
            }
            None
        }
        ScreenEvent::Sound {
            name,
            volume,
            pitch,
        } => {
            crate::sound_requests::ui_sound(name, *volume, *pitch);
            None
        }
        ScreenEvent::TextEditSelected { .. } => None,
    }
}

/// A button id (from a press or a `global` mapping) as a form answer.
fn mapped_action(model: &ServerFormModel, button: &ButtonEvent) -> Option<LocalFormAction> {
    let index = button.collection_index;
    match button.id.as_str() {
        "button.form_button_click" => {
            let index = index?;
            let ordinal = match model {
                // Decorations share the collection; answers count buttons only.
                ServerFormModel::ElementMenu(menu) => menu.elements
                    [..index.min(menu.elements.len())]
                    .iter()
                    .filter(|element| matches!(element, MenuElement::Button { .. }))
                    .count(),
                _ => index,
            };
            Some(LocalFormAction::SubmitButton(ordinal as u32))
        }
        "button.submit_custom_form" => Some(LocalFormAction::CustomElements),
        // NPC dialogue: a student button answers its action, exiting closes.
        "button.student_button" => Some(LocalFormAction::SubmitButton(index? as u32)),
        "button.exit_student" => Some(LocalFormAction::Dismiss),
        "popup_dialog.left_button" => Some(LocalFormAction::SubmitButton(0)),
        "popup_dialog.rightcancel_button" => Some(LocalFormAction::SubmitButton(1)),
        "button.menu_exit" | "popup_dialog.escape" => Some(LocalFormAction::Dismiss),
        _ => None,
    }
}

/// `button.dropdown_exit`: the controller closes its dropdown and the dropdown
/// toggles drop what they wrote, so their bound state shows again.
fn close_dropdown(runtime: &mut UiRuntime, frame: &EngineFrame) {
    let engine = runtime.server_forms_mut().engine_mut();
    engine.open_dropdown = None;
    for region in frame
        .hits
        .iter()
        .filter(|region| region.kind == HitKind::Dropdown)
    {
        engine.view.components.forget(&region.key);
    }
}

fn edit_region<'a>(frame: &'a EngineFrame, key: &str) -> Option<&'a HitRegion> {
    frame
        .hits
        .iter()
        .find(|region| region.key == key && region.kind == HitKind::EditBox)
}

/// A slider event's `#slider_value` (a percentage, or a step index) as the element's value.
fn set_slider(
    runtime: &mut UiRuntime,
    model: &ServerFormModel,
    index: usize,
    value: f64,
    step: Option<usize>,
) {
    let ServerFormModel::Custom(form) = model else {
        return;
    };
    let value = match form.elements.get(index) {
        Some(CustomFormElement::Slider {
            min,
            max,
            step: size,
            ..
        }) => FormValue::Slider(slider_value_at(min.get(), max.get(), size.get(), value)),
        Some(CustomFormElement::StepSlider { steps, .. }) if !steps.is_empty() => {
            FormValue::Step(step.unwrap_or(value.max(0.0) as usize).min(steps.len() - 1))
        }
        _ => return,
    };
    if let Some(slot) = runtime
        .server_forms_mut()
        .engine_mut()
        .values
        .get_mut(index)
    {
        *slot = value;
    }
}
