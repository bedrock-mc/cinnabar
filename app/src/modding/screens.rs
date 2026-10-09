//! A player mod package's screens beside the container screens: the session data it reads, the
//! layout it lays out against, and the pointer, wheel, edit box and declared-key events its
//! callbacks receive. Input reaches the mod only while a container screen (or its view) is up.
//! Every loaded package reads the session; one, the screen owner, draws and gets the input.

use std::sync::Arc;

use bevy::{
    ecs::{message::MessageReader, system::SystemParam},
    input::{
        keyboard::KeyboardInput,
        mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
    },
    prelude::*,
};
use client_ui::ui_runtime::{
    UiRuntime,
    presentation::{ModScreensInput, UiPresentationRuntime, publish::hovered_stack},
    session_data::{SessionDataCache, session_data, session_stack},
};
use mod_host::{KeyDecl, KeyModifiers, ModEvent, ModHost, Modifier};
use server_experience::screen::ScreenLayout;

use super::ModRuntime;

/// What the adapter keeps between frames.
#[derive(Default)]
pub(super) struct ScreenState {
    session: SessionDataCache,
    /// The mod that owned the screens last frame.
    owner: Option<usize>,
    /// The layout last delivered to the owner as `screen-changed`.
    layout: Option<ScreenLayout>,
    /// The owner's session notification, delivered with its current layout.
    data_change: Option<ModEvent>,
}

#[derive(SystemParam)]
pub(super) struct ScreenInput<'w, 's> {
    keyboard: MessageReader<'w, 's, KeyboardInput>,
    buttons: MessageReader<'w, 's, MouseButtonInput>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    presses: Local<'s, Vec<KeyPress>>,
    modifier_state: Local<'s, ButtonInput<KeyCode>>,
    time: Option<Res<'w, Time<Real>>>,
}

/// A physical press and the modifiers held when it arrived.
struct KeyPress {
    key: KeyCode,
    text: Option<String>,
    modifiers: KeyModifiers,
}

/// Reads both physical sides of each modifier from the retained state.
fn held_modifiers(held: &ButtonInput<KeyCode>) -> KeyModifiers {
    let pressed = |left, right| held.pressed(left) || held.pressed(right);
    KeyModifiers {
        ctrl: pressed(KeyCode::ControlLeft, KeyCode::ControlRight),
        shift: pressed(KeyCode::ShiftLeft, KeyCode::ShiftRight),
        alt: pressed(KeyCode::AltLeft, KeyCode::AltRight),
    }
}

impl ScreenInput<'_, '_> {
    /// Retains modifiers and each ordered press's state; focus loss clears both.
    pub(super) fn modifiers(&mut self, focused: bool) -> KeyModifiers {
        self.presses.clear();
        if !focused {
            self.modifier_state.reset_all();
            self.keyboard.clear();
            return KeyModifiers::default();
        }
        let events: Vec<_> = self.keyboard.read().cloned().collect();
        crate::ui_runtime::interaction::chat_modifiers::capture_before_events(
            &mut self.modifier_state,
            &self.keys,
            &events,
        );
        for event in events {
            crate::ui_runtime::interaction::chat_modifiers::track(&mut self.modifier_state, &event);
            if event.state.is_pressed() {
                let modifiers = held_modifiers(&self.modifier_state);
                self.presses.push(KeyPress {
                    key: event.key_code,
                    text: event.text.as_deref().map(str::to_owned),
                    modifiers,
                });
            }
        }
        held_modifiers(&self.modifier_state)
    }
}

/// One frame of the packages' screens: deliver the session data to every package and this
/// frame's events to the screen owner, then publish what the owner committed. A refused
/// template quarantines the owner.
#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
pub(super) fn drive(
    runtime: &mut ModRuntime,
    player_runtime: &player_state::PlayerState,
    ui: &UiRuntime,
    presentation: &mut UiPresentationRuntime,
    menu: Option<&crate::menu::MenuRuntime>,
    cursor: Option<[f32; 2]>,
    focused: bool,
    input: &mut ScreenInput,
) {
    let owner = runtime.screen_owner();
    let mut changes = Vec::new();
    let readers: Vec<usize> = (0..runtime.host_count())
        .filter(|&index| {
            let host = runtime.host(index);
            let grants = host.grants();
            host.package().is_some() && host.is_active() && (grants.items || grants.recipes)
        })
        .collect();
    if !readers.is_empty() {
        let session = session_data(&mut runtime.screens.session, player_runtime, ui);
        for index in readers {
            if let Some(event) = runtime.host_mut(index).update_session(Arc::clone(&session)) {
                changes.push((index, event));
            }
        }
    }
    set_owner(runtime, owner);
    for (index, event) in changes {
        if Some(index) == owner {
            runtime.screens.data_change = Some(event);
        } else if let Err(error) = runtime.host_mut(index).dispatch(vec![event]) {
            eprintln!("Cinnabar mod data-changed failed: {error:#}");
        }
    }
    let owner = runtime.screen_owner();
    set_owner(runtime, owner);
    let Some(owner) = owner else {
        presentation.set_mod_screens(None);
        input.modifiers(false);
        input.buttons.clear();
        input.wheel.clear();
        return;
    };
    let mut state = std::mem::take(&mut runtime.screens);
    drive_owner(
        runtime.host_mut(owner),
        &mut state,
        player_runtime,
        ui,
        presentation,
        menu,
        cursor,
        focused,
        input,
    );
    runtime.screens = state;
}

/// Closes the departing owner's layout before any pending session notification.
fn set_owner(runtime: &mut ModRuntime, owner: Option<usize>) {
    if owner == runtime.screens.owner {
        return;
    }
    if let Some(previous) = runtime.screens.owner {
        let mut events = vec![ModEvent::ScreenChanged(None)];
        events.extend(runtime.screens.data_change.take());
        if let Err(error) = runtime.host_mut(previous).dispatch(events) {
            eprintln!("Cinnabar departing mod screen closure failed: {error:#}");
        }
    }
    runtime.screens.owner = owner;
    runtime.screens.layout = None;
}

/// The screen owner's frame: its layout, input events and committed screens.
#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
fn drive_owner(
    host: &mut ModHost,
    state: &mut ScreenState,
    player_runtime: &player_state::PlayerState,
    ui: &UiRuntime,
    presentation: &mut UiPresentationRuntime,
    menu: Option<&crate::menu::MenuRuntime>,
    cursor: Option<[f32; 2]>,
    focused: bool,
    input: &mut ScreenInput,
) {
    let Some(package) = host.package() else {
        return;
    };
    let keys = package.keys.clone();
    if !presentation.mod_screens_match(&package.id, &package.files) {
        publish(host, presentation);
    }
    if let Some(reason) = presentation.mod_screens_failure() {
        eprintln!("Cinnabar mod screens refused, mod quarantined: {reason}");
        host.quarantine();
    }
    let mut events: Vec<ModEvent> = state.data_change.take().into_iter().collect();
    if host.is_active() {
        let layout = presentation.mod_screen_layout().cloned();
        if layout != state.layout {
            state.layout.clone_from(&layout);
            events.push(ModEvent::ScreenChanged(layout));
        }
    }
    let up = state.layout.is_some();
    if up && focused {
        let modifiers = input.modifiers(true);
        let pressed = pointer_events(presentation, cursor, modifiers, input, &mut events);
        let frame = KeyFrame {
            keys: &keys,
            menu,
            cursor,
            pressed,
        };
        key_events(
            host,
            player_runtime,
            ui,
            presentation,
            frame,
            input,
            &mut events,
        );
    } else {
        input.modifiers(false);
        input.buttons.clear();
        input.wheel.clear();
    }
    if !events.is_empty()
        && let Err(error) = host.dispatch(events)
    {
        eprintln!("Cinnabar mod event refused: {error:#}");
    }
    publish(host, presentation);
}

/// The overlay's and view's hover, wheel, presses and secondary presses; returns whether the
/// primary button went down, which the edit boxes also follow.
fn pointer_events(
    presentation: &mut UiPresentationRuntime,
    cursor: Option<[f32; 2]>,
    modifiers: KeyModifiers,
    input: &mut ScreenInput,
    events: &mut Vec<ModEvent>,
) -> bool {
    presentation.hover_mod_screens(cursor);
    let notches: f64 = input
        .wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => -f64::from(event.y),
            MouseScrollUnit::Pixel => -f64::from(event.y) / 16.0,
        })
        .sum();
    if let Some(point) = cursor
        && let Some([x, y]) = presentation.scroll_mod_screens(point, notches)
    {
        events.push(ModEvent::Scrolled {
            delta: notches,
            x,
            y,
            modifiers,
        });
    }
    let (mut pressed, mut released) = (false, false);
    let (mut secondary_pressed, mut secondary_released) = (false, false);
    for event in input.buttons.read() {
        match event.button {
            MouseButton::Left => {
                pressed |= event.state.is_pressed();
                released |= !event.state.is_pressed();
            }
            MouseButton::Right => {
                secondary_pressed |= event.state.is_pressed();
                secondary_released |= !event.state.is_pressed();
            }
            _ => {}
        }
    }
    if let Some((id, index)) = presentation.press_mod_screens(cursor, pressed, released) {
        events.push(ModEvent::Action {
            id,
            index: index.and_then(|index| u32::try_from(index).ok()),
        });
    }
    if let Some((id, index)) =
        presentation.secondary_press_mod_screens(cursor, secondary_pressed, secondary_released)
    {
        events.push(ModEvent::SecondaryAction {
            id,
            index: index.and_then(|index| u32::try_from(index).ok()),
        });
    }
    pressed
}

/// What the key events of one frame read besides the input.
struct KeyFrame<'a> {
    keys: &'a [KeyDecl],
    menu: Option<&'a crate::menu::MenuRuntime>,
    cursor: Option<[f32; 2]>,
    /// The primary button went down this frame.
    pressed: bool,
}

/// Typing into the mod's edit boxes, Escape (which deselects a box, else returns from the
/// view), and the declared keys with the hovered slot's stack.
fn key_events(
    host: &mut ModHost,
    player_runtime: &player_state::PlayerState,
    ui: &UiRuntime,
    presentation: &mut UiPresentationRuntime,
    frame: KeyFrame<'_>,
    input: &mut ScreenInput,
    events: &mut Vec<ModEvent>,
) {
    let KeyFrame {
        keys,
        menu,
        cursor,
        pressed,
    } = frame;
    let view = presentation.mod_view_shown();
    let presses: Vec<_> = input.presses.drain(..).collect();
    let typed: Vec<String> = presses
        .iter()
        .filter_map(|press| {
            crate::ui_runtime::typed_text(press.key, press.text.as_deref(), press.modifiers.ctrl)
        })
        .collect();
    let escape = presses.iter().any(|press| press.key == KeyCode::Escape);
    let now = input
        .time
        .as_ref()
        .map_or(0.0, |time| time.elapsed_secs_f64());
    let edits = presentation.edit_mod_screens(cursor, pressed, &typed, escape, now);
    let text_focused = presentation.mod_text_focused();
    for (control, text) in edits.edits {
        events.push(ModEvent::TextChanged { control, text });
    }
    if escape
        && !edits.escape_consumed
        && presentation.mod_view_shown()
        && let Err(error) = host.close_view()
    {
        eprintln!("Cinnabar mod view-closed failed: {error:#}");
    }
    if text_focused || edits.escape_consumed || (!view && ui.screen_state().text_focused()) {
        return;
    }
    for press in &presses {
        let key = &press.key;
        if crate::ui_runtime::interaction::inventory_consumes_key(menu, *key, view) {
            continue;
        }
        let Some(declared) = keys.iter().find(|declared| {
            key_code(&declared.key) == Some(*key) && matches(press.modifiers, declared)
        }) else {
            continue;
        };
        let registry = ui
            .inventory_ledger(player_runtime)
            .negotiated_item_registry();
        let hovered = hovered_stack(player_runtime, ui)
            .and_then(|(stack, _)| session_stack(registry, &stack));
        let row = cursor.and_then(|point| presentation.mod_screens_row(point));
        events.push(ModEvent::Key {
            id: declared.id.clone(),
            hovered,
            row,
        });
    }
}

/// A declared key fires only with exactly its modifiers held.
fn matches(held: KeyModifiers, declared: &KeyDecl) -> bool {
    let wants = |modifier| declared.modifiers.contains(&modifier);
    held.ctrl == wants(Modifier::Ctrl)
        && held.shift == wants(Modifier::Shift)
        && held.alt == wants(Modifier::Alt)
}

/// The key a declaration's name binds: letters, digits, F1 to F12, Backspace, Page Up and
/// Page Down.
fn key_code(name: &str) -> Option<KeyCode> {
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    const FUNCTIONS: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    match name {
        "backspace" => return Some(KeyCode::Backspace),
        "page_up" => return Some(KeyCode::PageUp),
        "page_down" => return Some(KeyCode::PageDown),
        _ => {}
    }
    let mut chars = name.chars();
    match (chars.next()?, chars.as_str()) {
        (letter @ 'a'..='z', "") => LETTERS.get(usize::from(letter as u8 - b'a')).copied(),
        (digit @ '0'..='9', "") => DIGITS.get(usize::from(digit as u8 - b'0')).copied(),
        ('f', number) => FUNCTIONS
            .get(number.parse::<usize>().ok()?.checked_sub(1)?)
            .copied(),
        _ => None,
    }
}

/// Hands the host's committed screens to the presentation; a stopped mod draws nothing.
fn publish(host: &ModHost, presentation: &mut UiPresentationRuntime) {
    let (Some(package), true) = (host.package(), host.is_active()) else {
        presentation.set_mod_screens(None);
        return;
    };
    let screens = host.screens();
    let files: &Arc<_> = &package.files;
    presentation.set_mod_screens(Some(ModScreensInput {
        id: &package.id,
        files,
        overlay: screens.overlay.as_ref(),
        view: screens.view.as_ref(),
        focus: screens.focus.as_ref(),
        data: &screens.data,
    }));
}

#[cfg(test)]
#[path = "screens/input_tests.rs"]
mod input_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_key_names_map_to_their_keys() {
        assert_eq!(key_code("r"), Some(KeyCode::KeyR));
        assert_eq!(key_code("u"), Some(KeyCode::KeyU));
        assert_eq!(key_code("0"), Some(KeyCode::Digit0));
        assert_eq!(key_code("f1"), Some(KeyCode::F1));
        assert_eq!(key_code("f12"), Some(KeyCode::F12));
        assert_eq!(key_code("f13"), None);
        assert_eq!(key_code("f0"), None);
        assert_eq!(key_code("space"), None);
        assert_eq!(key_code("backspace"), Some(KeyCode::Backspace));
        assert_eq!(key_code("page_up"), Some(KeyCode::PageUp));
        assert_eq!(key_code("page_down"), Some(KeyCode::PageDown));
        for name in mod_host::KEY_NAMES {
            assert!(key_code(name).is_some(), "{name}");
        }
    }

    #[test]
    fn declared_modifiers_must_match_exactly() {
        let declared = |modifiers: Vec<Modifier>| KeyDecl {
            id: "bei.toggle".into(),
            key: "o".into(),
            modifiers,
            label: "k".into(),
        };
        let ctrl = KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        };
        assert!(matches(ctrl, &declared(vec![Modifier::Ctrl])));
        assert!(!matches(ctrl, &declared(vec![])));
        assert!(!matches(
            ctrl,
            &declared(vec![Modifier::Ctrl, Modifier::Shift])
        ));
    }
}
