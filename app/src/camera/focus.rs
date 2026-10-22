//! Desktop focus arbitration before input sampling and before OS cursor updates.
#[cfg(test)]
use bevy::prelude::{MessageWriter, Mut, Time, Update, default};
use bevy::{
    input::{
        InputSystems,
        mouse::{AccumulatedMouseMotion, MouseButtonInput},
    },
    prelude::{
        App, ButtonInput, DetectChangesMut, Entity, Gamepad, IntoScheduleConfigs, KeyCode, Local,
        MessageReader, MouseButton, PostUpdate, PreUpdate, Query, Res, ResMut, Single, Touches,
        Vec2, Window, With,
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowFocused, WindowOccluded},
};
use client_shell::physical_input::release_cursor;
use client_state::CursorFocus;

use {client_presentation::camera::AutoFly, client_state::DrivenInput};

#[cfg(any(windows, test))]
mod native;

/// Whether the platform can lock the OS cursor; winit's Android and UIKit backends reject every
/// grab.
const OS_CURSOR_GRAB: bool = !cfg!(any(target_os = "android", target_os = "ios"));

/// Installs focus tracking without opening or calling into a native window.
pub(super) fn install(app: &mut App) {
    #[cfg(all(windows, not(test)))]
    native::install(app);
    app.init_resource::<CursorFocus>()
        .add_message::<WindowFocused>()
        .add_message::<WindowOccluded>()
        .add_message::<MouseButtonInput>()
        .add_systems(PreUpdate, track_focus.after(InputSystems))
        .add_systems(PostUpdate, enforce_cursor_ownership);
}

/// Retains primary-window loss events before gameplay or screen input is sampled.
#[allow(clippy::too_many_arguments)]
fn track_focus(
    mut focus: ResMut<CursorFocus>,
    mut focused: MessageReader<WindowFocused>,
    mut occluded: MessageReader<WindowOccluded>,
    mut pointer_edges: MessageReader<MouseButtonInput>,
    window: Single<(Entity, &Window, &mut CursorOptions), With<PrimaryWindow>>,
    driven: Option<Res<DrivenInput>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    gamepads: Query<&Gamepad>,
    touches: Option<Res<Touches>>,
) {
    let (entity, window, mut cursor) = window.into_inner();
    focus.begin_frame(window.focused);
    for event in focused.read().filter(|event| event.window == entity) {
        focus.focus_changed(event.focused);
    }
    for event in occluded.read().filter(|event| event.window == entity) {
        focus.occlusion_changed(event.occluded);
    }
    let mut pointer_activated = false;
    for event in pointer_edges.read() {
        pointer_activated |= event.window == entity;
    }
    focus.record_activation(
        keys.get_just_pressed().next().is_some()
            || pointer_activated
            || buttons.get_just_pressed().next().is_some()
            || buttons.get_just_released().next().is_some()
            || touches
                .is_some_and(|touches| touches.any_just_pressed() || touches.any_just_released())
            || gamepads
                .iter()
                .any(|pad| pad.get_just_pressed().next().is_some()),
    );
    let available = focus.available();
    if driven.is_some() {
        return;
    }
    if !available {
        release_cursor(&mut cursor);
        keys.reset_all();
        buttons.reset_all();
        motion.delta = Vec2::ZERO;
    }
}

/// Includes controller ownership without requesting a physical OS cursor grab.
pub(crate) fn mouse_input_active(
    window: &Window,
    cursor: &CursorOptions,
    focus: Option<&CursorFocus>,
    driven: bool,
) -> bool {
    driven
        || if OS_CURSOR_GRAB {
            client_presentation::camera::input_is_active(window, cursor)
                && focus.is_none_or(|focus| focus.capture_allowed())
        } else {
            window.focused && focus.is_some_and(|focus| focus.grab_free_capture())
        }
}

/// Samples UI cursor authority immediately before presentation updates capture.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_cursor_capture(
    window: Single<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    auto_fly: ResMut<AutoFly>,
    screen_focus: Res<client_state::ScreenFocus>,
    consent: Option<Res<client_state::ConsentInput>>,
    driven: Option<Res<DrivenInput>>,
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    captured_last_frame: Local<bool>,
    #[cfg(all(windows, not(test)))] native: Res<native::NativeCaptureReady>,
) {
    let mut policy = client_presentation::observations::CursorPolicy {
        capture_allowed: true,
        driven: driven.is_some(),
        consent: consent.is_some_and(|consent| consent.0),
        absorbs_input: screen_focus.absorbs_input,
        steals_mouse: screen_focus.steals_mouse,
    };
    if let Some(focus) = focus.as_deref_mut() {
        // Without an OS cursor there is no desktop pointer to steal back, so no click is needed.
        policy.capture_allowed = focus.allow_capture(
            policy.absorbs_input,
            mouse_buttons.just_pressed(MouseButton::Left) || !OS_CURSOR_GRAB,
        );
        if !OS_CURSOR_GRAB {
            let captured = grab_free_capture(&policy, window.0.focused);
            focus.set_grab_free_capture(captured);
            if !captured && !policy.driven {
                clear_controller_input(&mut keys, &mut mouse_buttons, &mut mouse_motion);
            }
            return;
        }
    }
    #[cfg(all(windows, not(test)))]
    {
        policy.capture_allowed &= native.0;
    }
    let captured = apply_cursor_policy(
        policy,
        window,
        keys,
        mouse_buttons,
        mouse_motion,
        auto_fly,
        captured_last_frame,
    );
    if let Some(mut focus) = focus {
        focus.record_capture(captured);
    }
}

/// Whether gameplay holds keyboard and mouse input on a platform without an OS cursor grab:
/// wherever the desktop policy would lock the cursor.
fn grab_free_capture(
    policy: &client_presentation::observations::CursorPolicy,
    focused: bool,
) -> bool {
    !policy.driven
        && focused
        && policy.capture_allowed
        && !policy.consent
        && !policy.absorbs_input
        && policy.steals_mouse != Some(false)
}

/// Requests the captured gameplay pointer state.
fn capture_cursor(cursor: &mut CursorOptions) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

/// Drops held pointer and key input when gameplay capture is released.
fn clear_controller_input(
    keys: &mut ButtonInput<KeyCode>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    keys.reset_all();
    mouse_buttons.reset_all();
    mouse_motion.delta = Vec2::ZERO;
}

/// Applies input policy and reports the resulting gameplay capture request.
#[allow(clippy::too_many_arguments)]
fn apply_cursor_policy(
    policy: client_presentation::observations::CursorPolicy,
    window: Single<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    mut auto_fly: ResMut<AutoFly>,
    mut captured_last_frame: Local<bool>,
) -> bool {
    let was_captured = std::mem::take(&mut *captured_last_frame);
    if policy.driven {
        return false;
    }
    let (mut window, mut cursor) = window.into_inner();

    // Focus loss has priority over every capture request, including auto-fly.
    // The trusted consent popup needs a pointer whatever settings the scene behind it declares.
    if !window.focused || !policy.capture_allowed || policy.consent {
        release_cursor(&mut cursor);
        clear_controller_input(&mut keys, &mut mouse_buttons, &mut mouse_motion);
        auto_fly.clear_capture_request();
        return false;
    }

    let steals = policy.steals_mouse;
    if policy.absorbs_input || steals == Some(false) {
        release_cursor(&mut cursor);
        clear_controller_input(&mut keys, &mut mouse_buttons, &mut mouse_motion);
        auto_fly.clear_capture_request();
        return false;
    }

    // Escape also wins if it arrives in the same frame as a left click.
    if keys.just_pressed(KeyCode::Escape) {
        release_cursor(&mut cursor);
        clear_controller_input(&mut keys, &mut mouse_buttons, &mut mouse_motion);
        auto_fly.clear_capture_request();
        return false;
    }

    let active = client_presentation::camera::input_is_active(&window, &cursor);
    let recapture_click = !active && mouse_buttons.just_pressed(MouseButton::Left);
    if recapture_click || (steals == Some(true) && !active) || auto_fly.capture_pending() {
        capture_cursor(&mut cursor);
        if recapture_click {
            // Recapture consumes the click; a later physical press must rearm attack.
            mouse_buttons.release(MouseButton::Left);
            mouse_motion.delta = Vec2::ZERO;
        }
        auto_fly.clear_capture_request();
    }
    let captured = client_presentation::camera::input_is_active(&window, &cursor);
    if captured && (!was_captured || !active) {
        let centre = window.size() * 0.5;
        window.set_cursor_position(Some(centre));
        // The backend applies position before grab. Warping can release the native lock,
        // so submit it again even when a screen adapter already requested capture.
        cursor.set_changed();
        mouse_motion.delta = Vec2::ZERO;
    }
    *captured_last_frame = captured;
    captured
}

/// Prevents any screen or developer adapter from submitting an unauthorized OS grab.
fn enforce_cursor_ownership(
    focus: Res<CursorFocus>,
    driven: Option<Res<DrivenInput>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    #[cfg(all(windows, not(test)))] native: Res<native::NativeCaptureReady>,
) {
    let allowed = focus.capture_allowed();
    #[cfg(all(windows, not(test)))]
    let allowed = allowed && native.0;
    if driven.is_some() || !allowed {
        for mut cursor in &mut cursors {
            release_cursor(&mut cursor);
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod high_rate_tests;

#[cfg(test)]
mod recapture_tests;

#[cfg(test)]
mod regain_tests;
