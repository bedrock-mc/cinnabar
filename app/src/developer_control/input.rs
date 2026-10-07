//! Synthetic input: controls become Bevy keyboard and mouse messages, so the semantic router,
//! UI, menus and local mods see them exactly as real keys.

use std::collections::VecDeque;

use bevy::{
    ecs::message::MessageCursor,
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardFocusLost, KeyboardInput, NativeKey, NativeKeyCode},
        mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
    },
    prelude::*,
    window::{CursorMoved, CursorOptions, PrimaryWindow, WindowFocused},
};
use developer_control::{
    input::{Control, InputPlan, MouseButton as ControlButton, yaw_difference},
    protocol::{InputCommand, Look, Pointer, Wheel, WheelUnit},
};
use semantic_input::PhysicalControl;
use serde_json::{Value, json};

use crate::{
    camera::{DrivenInput, FlyCameraUpdateSet, PITCH_LIMIT},
    local_player::{LocalPlayerFrameSet, LocalViewPose},
    runtime::telemetry::bedrock_camera_rotation,
    semantic_controls::physical::{KEYBOARD_USAGES, mouse_button_code},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Physical {
    Key(KeyCode),
    Mouse(MouseButton),
}

#[derive(Debug, Clone, Copy)]
struct LookTween {
    from: [f32; 2],
    to: [f32; 2],
    frame: u32,
    frames: u32,
}

#[derive(Debug, Clone, Copy)]
enum InputEvent {
    Button(Physical, ButtonState),
    Pointer(Pointer),
    Wheel(Wheel),
}

#[derive(Resource, Default)]
pub(super) struct Driver {
    /// Held controls in press order; a tap carries the frames it has left.
    held: Vec<(Physical, Option<u32>)>,
    events: VecDeque<InputEvent>,
    look: Option<Look>,
    tween: Option<LookTween>,
    text: VecDeque<String>,
    /// The OS focus last reported, restored when control is handed back.
    real_focus: Option<bool>,
    focus_cursor: MessageCursor<WindowFocused>,
}

pub(super) fn configure(app: &mut App) {
    app.init_resource::<Driver>()
        .add_systems(PreUpdate, inject.before(InputSystems))
        .add_systems(
            Update,
            apply_look
                .after(FlyCameraUpdateSet)
                .before(LocalPlayerFrameSet::Physics),
        );
}

pub(super) fn apply(world: &mut World, command: &InputCommand) -> Result<Value, String> {
    let plan = InputPlan::from_command(command)?;
    let menu = world.get_resource::<crate::menu::MenuRuntime>();
    let resolve = |controls: &[Control]| -> Result<Vec<Physical>, String> {
        controls
            .iter()
            .map(|control| resolve(menu, control))
            .collect()
    };
    let (release, hold, tap) = (
        resolve(&plan.release)?,
        resolve(&plan.hold)?,
        resolve(&plan.tap)?,
    );
    let mut driver = world.resource_mut::<Driver>();
    if plan.release_all {
        let held: Vec<_> = driver.held.iter().map(|(physical, _)| *physical).collect();
        for physical in held {
            driver.release(physical);
        }
        driver.tween = None;
    }
    if let Some(pointer) = plan.pointer {
        driver.events.push_back(InputEvent::Pointer(pointer));
    }
    if let Some(wheel) = plan.wheel {
        driver.events.push_back(InputEvent::Wheel(wheel));
    }
    for physical in release {
        driver.release(physical);
    }
    for physical in hold {
        driver.press(physical, None);
    }
    for physical in tap {
        driver.press(physical, Some(plan.tap_frames));
    }
    if plan.look.is_some() {
        driver.look = plan.look;
    }
    if let Some(text) = plan.text {
        driver.text.push_back(text);
    }
    let held = driver
        .held
        .iter()
        .map(|(physical, _)| format!("{physical:?}"))
        .collect::<Vec<_>>();
    if plan.release_control {
        let mut driver = world.resource_mut::<Driver>();
        driver
            .events
            .retain(|event| matches!(event, InputEvent::Button(_, ButtonState::Released)));
        driver.text.clear();
        world.remove_resource::<DrivenInput>();
        if let Some(mut menu) = world.get_resource_mut::<crate::menu::MenuRuntime>() {
            menu.set_transient_toggles(false);
        }
        return Ok(json!({ "driven": false }));
    }
    world.init_resource::<DrivenInput>();
    if let Some(mut menu) = world.get_resource_mut::<crate::menu::MenuRuntime>() {
        menu.set_transient_toggles(true);
    }
    Ok(json!({ "driven": true, "held": held }))
}

impl Driver {
    fn press(&mut self, physical: Physical, frames: Option<u32>) {
        match self.held.iter_mut().find(|(held, _)| *held == physical) {
            Some((_, remaining)) => *remaining = frames,
            None => {
                self.held.push((physical, frames));
                self.events
                    .push_back(InputEvent::Button(physical, ButtonState::Pressed));
            }
        }
    }

    fn release(&mut self, physical: Physical) {
        let before = self.held.len();
        self.held.retain(|(held, _)| *held != physical);
        if self.held.len() != before {
            self.events
                .push_back(InputEvent::Button(physical, ButtonState::Released));
        }
    }
}

fn resolve(menu: Option<&crate::menu::MenuRuntime>, control: &Control) -> Result<Physical, String> {
    match control {
        Control::Key(name) => KEYBOARD_USAGES
            .iter()
            .find(|(key, _)| format!("{key:?}") == *name)
            .map(|(key, _)| Physical::Key(*key))
            .ok_or_else(|| format!("unknown key `{name}`")),
        Control::Mouse(button) => Ok(Physical::Mouse(match button {
            ControlButton::Left => MouseButton::Left,
            ControlButton::Right => MouseButton::Right,
            ControlButton::Middle => MouseButton::Middle,
            ControlButton::Back => MouseButton::Back,
            ControlButton::Forward => MouseButton::Forward,
        })),
        Control::Binding(name) => match crate::menu::settings_options::named_control(menu, name) {
            Some(PhysicalControl::KeyboardUsage(usage)) => KEYBOARD_USAGES
                .iter()
                .find(|(_, candidate)| *candidate == usage)
                .map(|(key, _)| Physical::Key(*key))
                .ok_or_else(|| format!("`{name}` is bound to an unmapped key")),
            Some(PhysicalControl::MouseButton(code)) => [
                MouseButton::Left,
                MouseButton::Right,
                MouseButton::Middle,
                MouseButton::Back,
                MouseButton::Forward,
                MouseButton::Other(u16::from(code.saturating_sub(1))),
            ]
            .into_iter()
            .find(|button| mouse_button_code(*button) == Some(code))
            .map(Physical::Mouse)
            .ok_or_else(|| format!("`{name}` is bound to an unknown mouse button")),
            Some(other) => Err(format!("`{name}` is bound to {other:?}, not a key")),
            None => Err(format!("unknown binding `{name}`")),
        },
    }
}

/// Emits queued edges and expires taps; while driven, the window counts as focused and
/// captured without the OS cursor ever being grabbed.
#[allow(clippy::too_many_arguments)]
fn inject(
    driven: Option<Res<DrivenInput>>,
    mut driver: ResMut<Driver>,
    focus: Res<Messages<WindowFocused>>,
    mut window: Query<(Entity, &mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut pointers: MessageWriter<CursorMoved>,
    mut wheels: MessageWriter<MouseWheel>,
    mut focus_lost: ResMut<Messages<KeyboardFocusLost>>,
    mut was_driven: Local<bool>,
) {
    let Ok((entity, mut window, mut cursor)) = window.single_mut() else {
        return;
    };
    let mut focus_cursor = std::mem::take(&mut driver.focus_cursor);
    if let Some(event) = focus_cursor.read(&focus).last() {
        driver.real_focus = Some(event.focused);
    }
    driver.focus_cursor = focus_cursor;
    let driving = driven.is_some();
    if driving {
        // OS focus loss would release every cached key, including the ones the controller holds.
        focus_lost.clear();
        let window = window.bypass_change_detection();
        driver.real_focus.get_or_insert(window.focused);
        window.focused = true;
        crate::camera::release_cursor(&mut cursor);
    } else if std::mem::take(&mut *was_driven) {
        window.bypass_change_detection().focused = driver.real_focus.unwrap_or(false);
        crate::camera::release_cursor(&mut cursor);
    }
    *was_driven = driving;
    let expired: Vec<_> = driver
        .held
        .iter()
        .filter(|(_, frames)| *frames == Some(0))
        .map(|(physical, _)| *physical)
        .collect();
    for physical in expired {
        driver.release(physical);
    }
    while let Some(event) = driver.events.pop_front() {
        match event {
            InputEvent::Pointer(pointer) => {
                let position = Vec2::new(pointer.x, pointer.y);
                let delta = window.cursor_position().map(|previous| position - previous);
                window
                    .bypass_change_detection()
                    .set_cursor_position(Some(position));
                pointers.write(CursorMoved {
                    window: entity,
                    position,
                    delta,
                });
            }
            InputEvent::Wheel(wheel) => {
                wheels.write(MouseWheel {
                    unit: match wheel.unit {
                        WheelUnit::Line => MouseScrollUnit::Line,
                        WheelUnit::Pixel => MouseScrollUnit::Pixel,
                    },
                    x: wheel.x,
                    y: wheel.y,
                    window: entity,
                });
            }
            InputEvent::Button(Physical::Key(key_code), state) => {
                keys.write(KeyboardInput {
                    key_code,
                    logical_key: Key::Unidentified(NativeKey::Unidentified),
                    state,
                    text: None,
                    repeat: false,
                    window: entity,
                });
            }
            InputEvent::Button(Physical::Mouse(button), state) => {
                buttons.write(MouseButtonInput {
                    button,
                    state,
                    window: entity,
                });
            }
        }
    }
    while let Some(text) = driver.text.pop_front() {
        keys.write(KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
            logical_key: Key::Character(text.clone().into()),
            state: ButtonState::Pressed,
            text: Some(text.into()),
            repeat: false,
            window: entity,
        });
    }
    for (_, frames) in &mut driver.held {
        if let Some(frames) = frames {
            *frames = frames.saturating_sub(1);
        }
    }
}

/// Bedrock yaw and pitch of the player's view, in degrees.
pub(super) fn view_angles(view: &LocalViewPose) -> [f32; 2] {
    let (yaw, pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
    [
        (180.0 - yaw.to_degrees()).rem_euclid(360.0),
        -pitch.to_degrees(),
    ]
}

/// Turns the player's own view, as mouse look would, so movement and the server follow it.
fn apply_look(mut driver: ResMut<Driver>, mut view: ResMut<LocalViewPose>) {
    if let Some(look) = driver.look.take() {
        let from = view_angles(&view);
        let to = if look.relative {
            [from[0] + look.yaw, from[1] + look.pitch]
        } else {
            [look.yaw, look.pitch]
        };
        let limit = PITCH_LIMIT.to_degrees();
        let to = [
            from[0] + yaw_difference(from[0], to[0]),
            to[1].clamp(-limit, limit),
        ];
        driver.tween = Some(LookTween {
            from,
            to,
            frame: 0,
            frames: look.frames,
        });
    }
    let Some(tween) = driver.tween.as_mut() else {
        return;
    };
    tween.frame = tween.frame.saturating_add(1);
    let t = if tween.frames == 0 {
        1.0
    } else {
        (tween.frame as f32 / tween.frames as f32).min(1.0)
    };
    let yaw = tween.from[0] + (tween.to[0] - tween.from[0]) * t;
    let pitch = tween.from[1] + (tween.to[1] - tween.from[1]) * t;
    view.set_rotation(bedrock_camera_rotation(yaw, pitch));
    if t >= 1.0 {
        driver.tween = None;
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        input::{InputPlugin, keyboard::KeyboardFocusLost},
        prelude::*,
        window::{CursorMoved, CursorOptions, PrimaryWindow, WindowFocused},
    };

    use super::{Driver, Physical, apply, inject};
    use crate::camera::DrivenInput;

    #[test]
    fn driven_input_releases_cursor_once_without_native_windowing() {
        #[derive(Resource, Default)]
        struct CursorWrites(usize);
        let mut app = App::new();
        app.add_plugins(InputPlugin)
            .add_message::<WindowFocused>()
            .add_message::<CursorMoved>()
            .init_resource::<Driver>()
            .init_resource::<DrivenInput>()
            .init_resource::<CursorWrites>()
            .add_systems(PreUpdate, inject.before(bevy::input::InputSystems))
            .add_systems(
                PostUpdate,
                |cursors: Query<(), Changed<CursorOptions>>, mut writes: ResMut<CursorWrites>| {
                    writes.0 += cursors.iter().count();
                },
            );
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: false,
                    ..default()
                },
                CursorOptions {
                    grab_mode: bevy::window::CursorGrabMode::Locked,
                    visible: false,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert_eq!(cursor.grab_mode, bevy::window::CursorGrabMode::None);
        assert!(cursor.visible);
        assert!(app.world().get::<Window>(window).unwrap().focused);
        assert_eq!(app.world().resource::<CursorWrites>().0, 1);
        app.update();
        assert_eq!(app.world().resource::<CursorWrites>().0, 1);
    }

    #[test]
    fn handing_back_control_discards_unconsumed_pointer_and_text() {
        let mut world = World::new();
        world.init_resource::<Driver>();
        world.init_resource::<DrivenInput>();
        let mut driver = world.resource_mut::<Driver>();
        driver.press(Physical::Key(KeyCode::KeyW), None);
        driver
            .events
            .push_back(super::InputEvent::Pointer(super::Pointer {
                x: 123.0,
                y: 234.0,
            }));
        driver
            .events
            .push_back(super::InputEvent::Wheel(super::Wheel::default()));
        driver.text.push_back("queued text".into());
        let command = serde_json::from_value(serde_json::json!({
            "release_control": true
        }))
        .unwrap();
        apply(&mut world, &command).unwrap();
        let driver = world.resource::<Driver>();
        assert_eq!(driver.events.len(), 1);
        assert!(matches!(
            driver.events.front(),
            Some(super::InputEvent::Button(
                Physical::Key(KeyCode::KeyW),
                bevy::input::ButtonState::Released
            ))
        ));
        assert!(driver.held.is_empty());
        assert!(driver.text.is_empty());
        assert!(!world.contains_resource::<DrivenInput>());
    }

    #[test]
    fn pointer_and_text_reach_the_window_and_keyboard_messages() {
        let mut app = App::new();
        app.add_plugins(InputPlugin)
            .add_message::<WindowFocused>()
            .add_message::<CursorMoved>()
            .init_resource::<Driver>()
            .init_resource::<DrivenInput>()
            .add_systems(PreUpdate, inject.before(bevy::input::InputSystems));
        let entity = app
            .world_mut()
            .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
            .id();
        apply(
            app.world_mut(),
            &super::InputCommand {
                cursor: Some([123.0, 234.0]),
                text: Some("azalea".into()),
                ..super::InputCommand::default()
            },
        )
        .unwrap();
        app.update();
        assert_eq!(
            app.world().get::<Window>(entity).unwrap().cursor_position(),
            Some(Vec2::new(123.0, 234.0))
        );
        let messages = app
            .world()
            .resource::<Messages<bevy::input::keyboard::KeyboardInput>>();
        let mut cursor = messages.get_cursor();
        let typed: Vec<_> = cursor
            .read(messages)
            .filter_map(|event| event.text.as_deref())
            .collect();
        assert_eq!(typed, ["azalea"]);
        app.update();
        assert!(app.world().resource::<Driver>().text.is_empty());
    }

    #[test]
    fn real_focus_loss_keeps_driven_keys_held() {
        let mut app = App::new();
        app.add_plugins(InputPlugin)
            .add_message::<WindowFocused>()
            .add_message::<CursorMoved>()
            .init_resource::<Driver>()
            .init_resource::<DrivenInput>()
            .add_systems(PreUpdate, inject.before(bevy::input::InputSystems));
        app.world_mut()
            .spawn((Window::default(), CursorOptions::default(), PrimaryWindow));
        app.world_mut()
            .resource_mut::<Driver>()
            .press(Physical::Key(KeyCode::KeyW), None);
        app.update();
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        app.world_mut().write_message(KeyboardFocusLost);
        app.update();
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW),
            "OS focus loss released a key the controller still holds"
        );
    }
    #[test]
    fn pointer_click_and_wheel_reach_bevy_once_in_the_same_frame() {
        use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
        use developer_control::protocol::{InputCommand, Pointer, Wheel, WheelUnit};
        let mut app = App::new();
        app.add_plugins(InputPlugin)
            .add_message::<WindowFocused>()
            .add_message::<CursorMoved>()
            .init_resource::<Driver>()
            .add_systems(PreUpdate, inject.before(bevy::input::InputSystems));
        let entity = app
            .world_mut()
            .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
            .id();
        super::apply(
            app.world_mut(),
            &InputCommand {
                cursor: Some([12.0, 34.0]),
                ..InputCommand::default()
            },
        )
        .unwrap();
        super::apply(
            app.world_mut(),
            &InputCommand {
                cursor: Some([90.0, 80.0]),
                pointer: Some(Pointer { x: 123.0, y: 45.0 }),
                wheel: Some(Wheel {
                    y: -2.0,
                    unit: WheelUnit::Pixel,
                    ..Wheel::default()
                }),
                press: vec!["MouseLeft".into()],
                ..InputCommand::default()
            },
        )
        .unwrap();
        app.update();
        assert_eq!(
            app.world().get::<Window>(entity).unwrap().cursor_position(),
            Some(Vec2::new(123.0, 45.0))
        );
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .just_pressed(MouseButton::Left)
        );
        let mut wheels = bevy::ecs::message::MessageCursor::<MouseWheel>::default();
        let messages = app.world().resource::<Messages<MouseWheel>>();
        let events = wheels.read(messages).collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].unit, MouseScrollUnit::Pixel);
        assert_eq!(events[0].y, -2.0);
        app.update();
        assert_eq!(
            app.world().get::<Window>(entity).unwrap().cursor_position(),
            Some(Vec2::new(123.0, 45.0))
        );
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .just_released(MouseButton::Left)
        );
        assert_eq!(
            wheels
                .read(app.world().resource::<Messages<MouseWheel>>())
                .count(),
            0
        );
    }
}
