//! One finger drives the mouse pointer while the top screen is one a touchscreen can only reach
//! through the pointer (`Scene::reads_only_pointer`).
//!
//! Those screens already read the cursor and left button, so a touchscreen needs no screen
//! changes. Menus and chat read touches themselves and gameplay maps them to touch controls.

use std::collections::VecDeque;

use bevy::{
    ecs::message::MessageCursor,
    input::{
        ButtonState,
        mouse::{MouseButton, MouseButtonInput},
        touch::{TouchInput, TouchPhase},
    },
    math::Vec2,
    prelude::{
        DetectChangesMut, Entity, Local, MessageReader, Messages, Query, Res, ResMut, Resource,
        With,
    },
    window::{PrimaryWindow, Window},
};
use client_shell::{menu::MenuRuntime, ui_resources::UiRuntimeResource};
use client_ui::ui_runtime::{UiRuntime, presentation::forms::scene_policy::MenuScene};

/// Source of the pointer edge delivered to the active UI, including queued touch releases.
#[derive(Resource, Default)]
pub(crate) struct UiPointerSource(pub(crate) bool);

/// What one delivered step does to the emulated pointer's button.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerEdge {
    Press,
    /// A completed tap: the button lifts where the finger did.
    Release,
    /// An aborted gesture: the pointer leaves the screen before the button lifts, so nothing
    /// that fires on release sees it over a control.
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PointerEvent {
    Move(Vec2),
    Edge(PointerEdge, Option<Vec2>),
}

/// The finger that owns the emulated pointer and its undelivered events.
#[derive(Debug, Default)]
pub(crate) struct TouchPointer {
    owner: Option<u64>,
    /// A delivered press still waits for its release.
    held: bool,
    queue: VecDeque<PointerEvent>,
}

impl TouchPointer {
    /// Queues one touch event; only a finger that lands while a pointer screen owns input presses.
    pub(crate) fn queue(
        &mut self,
        pointer_screen: bool,
        id: u64,
        phase: TouchPhase,
        position: Vec2,
    ) {
        let event = match phase {
            TouchPhase::Started if pointer_screen && self.owner.is_none() => {
                self.owner = Some(id);
                PointerEvent::Edge(PointerEdge::Press, Some(position))
            }
            TouchPhase::Moved if self.owner == Some(id) => PointerEvent::Move(position),
            TouchPhase::Ended if self.owner == Some(id) => {
                self.owner = None;
                PointerEvent::Edge(PointerEdge::Release, Some(position))
            }
            TouchPhase::Canceled if self.owner == Some(id) => {
                self.owner = None;
                PointerEvent::Edge(PointerEdge::Cancel, None)
            }
            _ => return,
        };
        self.queue.push_back(event);
    }

    /// This frame's cursor change (`Some(None)` withdraws it) and at most one button edge, so
    /// screens hit-test each edge where it happened even when a gesture arrives in one batch.
    pub(crate) fn next_frame(&mut self) -> (Option<Option<Vec2>>, Option<PointerEdge>) {
        let mut cursor = None;
        while let Some(event) = self.queue.pop_front() {
            match event {
                PointerEvent::Move(position) => cursor = Some(Some(position)),
                PointerEvent::Edge(edge, at) => {
                    self.held = edge == PointerEdge::Press;
                    return (Some(at), Some(edge));
                }
            }
        }
        (cursor, None)
    }

    /// Drops undelivered events once the screen stops owning input, cancelling a held press.
    fn lose_screen(&mut self) -> Option<PointerEdge> {
        self.owner = None;
        self.queue.clear();
        std::mem::take(&mut self.held).then_some(PointerEdge::Cancel)
    }
}

/// Whether the top screen is one a touchscreen reaches only through the pointer.
fn takes_touch_pointer(
    ui: &UiRuntime,
    player: &player_state::PlayerState,
    menu: Option<&MenuRuntime>,
) -> bool {
    ui.top_scene_reads_only_pointer(player, menu.map(|menu| menu as &dyn MenuScene))
}

/// Runs before Bevy's input systems, so the emitted button edge reaches `ButtonInput` this frame.
pub(crate) fn emulate_ui_pointer(
    mut touch_input: MessageReader<TouchInput>,
    mut buttons: ResMut<Messages<MouseButtonInput>>,
    mut mouse_cursor: Local<MessageCursor<MouseButtonInput>>,
    mut source: Option<ResMut<UiPointerSource>>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    ui: Option<Res<UiRuntimeResource>>,
    player: Option<Res<client_state::PlayerRuntime>>,
    menu: Option<Res<MenuRuntime>>,
    mut pointer: Local<TouchPointer>,
) {
    if mouse_cursor.read(&buttons).next().is_some()
        && let Some(source) = source.as_deref_mut()
    {
        source.0 = false;
    }
    let Ok((entity, mut window)) = windows.single_mut() else {
        return;
    };
    let pointer_screen = ui
        .zip(player)
        .is_some_and(|(ui, player)| takes_touch_pointer(&ui, &player, menu.as_deref()));
    for input in touch_input.read() {
        pointer.queue(pointer_screen, input.id, input.phase, input.position);
    }
    let (cursor, edge) = if pointer_screen {
        pointer.next_frame()
    } else {
        let edge = pointer.lose_screen();
        (edge.map(|_| None), edge)
    };
    if (cursor.is_some() || edge.is_some())
        && let Some(source) = source.as_deref_mut()
    {
        source.0 = true;
    }
    if let Some(cursor) = cursor {
        // Bypassed so the window backend is not asked to warp a cursor touchscreens lack.
        window.bypass_change_detection().set_cursor_position(cursor);
    }
    if let Some(edge) = edge {
        buttons.write(MouseButtonInput {
            button: MouseButton::Left,
            state: if edge == PointerEdge::Press {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            window: entity,
        });
    }
    mouse_cursor.clear(&buttons);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        ecs::message::MessageCursor,
        prelude::{App, Messages, Update},
    };
    use client_ui::ui_runtime::SequencedUiEvent;

    /// Creates a retained form whose ownership the pointer adapter must follow.
    fn form_runtime(settings: bool) -> UiRuntime {
        let mut runtime = UiRuntime::new(1);
        runtime.server_forms_mut().select_settings_form(settings);
        runtime
            .apply(
                &mut player_state::PlayerState::new(1),
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 1,
                    local_millis: 0,
                    server_tick: None,
                    event: protocol::UiEvent::Form(protocol::FormRequestEvent {
                        form_id: 7,
                        kind: if settings {
                            protocol::FormKind::ServerSettings
                        } else {
                            protocol::FormKind::Menu
                        },
                        title: None,
                        json: "{}".into(),
                        model: protocol::ServerFormModel::TextMenu(protocol::TextMenuForm {
                            title: "Test".into(),
                            content: String::new().into(),
                            buttons: vec!["Choose".into()].into(),
                            button_images: [].into(),
                            omitted_images: 0,
                        }),
                    }),
                },
            )
            .unwrap();
        runtime
    }

    #[test]
    fn settings_form_over_the_menu_emits_touch_press_and_release() {
        for (settings, visible, expected) in [
            (true, true, true),
            (false, true, false),
            (false, false, true),
        ] {
            let mut menu = MenuRuntime::new(true, 2, "Test".into());
            menu.set_visible(visible);
            let mut app = App::new();
            app.add_message::<TouchInput>()
                .add_message::<MouseButtonInput>()
                .insert_resource(UiRuntimeResource(form_runtime(settings)))
                .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
                    1,
                )))
                .insert_resource(menu)
                .add_systems(Update, emulate_ui_pointer);
            let entity = app
                .world_mut()
                .spawn((Window::default(), PrimaryWindow))
                .id();
            let position = Vec2::new(40.0, 50.0);
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                app.world_mut().write_message(TouchInput {
                    phase,
                    position,
                    window: entity,
                    force: None,
                    id: 1,
                });
            }
            // One button edge per frame: the press, then the release.
            app.update();
            app.update();
            let mut cursor = MessageCursor::<MouseButtonInput>::default();
            let edges = cursor
                .read(app.world().resource::<Messages<MouseButtonInput>>())
                .map(|event| (event.button, event.state, event.window))
                .collect::<Vec<_>>();
            assert_eq!(
                edges,
                if expected {
                    vec![
                        (MouseButton::Left, ButtonState::Pressed, entity),
                        (MouseButton::Left, ButtonState::Released, entity),
                    ]
                } else {
                    Vec::new()
                },
                "settings={settings}, visible={visible}"
            );
            if expected {
                assert_eq!(
                    app.world().get::<Window>(entity).unwrap().cursor_position(),
                    Some(position)
                );
            }
        }
    }

    /// Fingers that land outside a pointer screen, or a second finger, never press.
    #[test]
    fn only_the_first_finger_on_a_pointer_screen_presses() {
        let mut pointer = TouchPointer::default();
        let at = Vec2::new(10.0, 20.0);
        pointer.queue(false, 1, TouchPhase::Started, at);
        pointer.queue(true, 1, TouchPhase::Ended, at);
        assert_eq!(pointer.next_frame(), (None, None));
        pointer.queue(true, 2, TouchPhase::Started, at);
        pointer.queue(true, 3, TouchPhase::Started, at);
        pointer.queue(true, 3, TouchPhase::Ended, at);
        assert_eq!(
            pointer.next_frame(),
            (Some(Some(at)), Some(PointerEdge::Press))
        );
        assert_eq!(pointer.next_frame(), (None, None));
    }

    /// A batched press and lift are delivered on separate frames, each at its own position.
    #[test]
    fn each_edge_keeps_the_position_it_happened_at() {
        let mut pointer = TouchPointer::default();
        let (down, moved, up) = (Vec2::ZERO, Vec2::new(5.0, 5.0), Vec2::new(9.0, 9.0));
        pointer.queue(true, 1, TouchPhase::Started, down);
        pointer.queue(true, 1, TouchPhase::Moved, moved);
        pointer.queue(true, 1, TouchPhase::Ended, up);
        assert_eq!(
            pointer.next_frame(),
            (Some(Some(down)), Some(PointerEdge::Press))
        );
        assert_eq!(
            pointer.next_frame(),
            (Some(Some(up)), Some(PointerEdge::Release))
        );
        assert_eq!(pointer.next_frame(), (None, None));
    }

    /// The bed screen's Leave Bed and Open Chat read only the pointer, so touch drives it.
    #[test]
    fn the_bed_screen_takes_the_touch_pointer() {
        let player = player_state::PlayerState::new(1);
        let mut ui = UiRuntime::new(1);
        assert!(!takes_touch_pointer(&ui, &player, None));
        ui.set_local_sleeping(true);
        assert!(takes_touch_pointer(&ui, &player, None));
        assert!(!takes_touch_pointer(&ui, &player, Some(&visible_menu())));
    }

    /// The sign editor's exit reads only the pointer; without a keyboard touch is the way out.
    #[test]
    fn the_sign_editor_takes_the_touch_pointer() {
        let player = player_state::PlayerState::new(1);
        let mut ui = UiRuntime::new(1);
        ui.sign_editor_mut()
            .open(client_ui::ui_runtime::sign_editor::SignEdit::new(
                [0; 3],
                true,
                world::NbtCompound::default(),
            ));
        assert!(takes_touch_pointer(&ui, &player, None));
        assert!(!takes_touch_pointer(&ui, &player, Some(&visible_menu())));
    }

    /// The End credits reveal and activate Skip through the pointer alone.
    #[test]
    fn the_credits_take_the_touch_pointer() {
        let player = player_state::PlayerState::new(1);
        let mut ui = UiRuntime::new(1);
        assert!(ui.credits_mut().open(5, 1, 0));
        assert!(takes_touch_pointer(&ui, &player, None));
    }

    /// A cancelled gesture, or one whose screen closes, must not complete as a tap.
    #[test]
    fn cancelled_gestures_cancel_instead_of_releasing() {
        let mut pointer = TouchPointer::default();
        let at = Vec2::ONE;
        pointer.queue(true, 1, TouchPhase::Started, at);
        pointer.queue(true, 1, TouchPhase::Canceled, at);
        assert_eq!(pointer.next_frame().1, Some(PointerEdge::Press));
        assert_eq!(
            pointer.next_frame(),
            (Some(None), Some(PointerEdge::Cancel))
        );
        pointer.queue(true, 2, TouchPhase::Started, at);
        pointer.next_frame();
        assert_eq!(pointer.lose_screen(), Some(PointerEdge::Cancel));
        pointer.queue(true, 2, TouchPhase::Ended, at);
        assert_eq!(pointer.next_frame(), (None, None));
        assert_eq!(pointer.lose_screen(), None);
    }

    fn visible_menu() -> MenuRuntime {
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.set_visible(true);
        menu
    }
}
