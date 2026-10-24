//! A captured chat finger scrolls history or completes one tap, including cancellation.

use bevy::input::touch::Touches;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use ui::UiPoint;

#[derive(Default)]
pub(crate) struct ChatTouch {
    finger: Option<(u64, UiPoint, UiPoint)>,
    dragged: bool,
    history: bool,
    slider: Option<u16>,
    last_action: Option<launcher::menu::MenuAction>,
}

/// A completed ordinary tap or one changed value from the finger's captured slider.
pub(super) enum ChatTouchAction {
    Tap(UiPoint, UiPoint),
    Slider(launcher::menu::MenuAction),
}

impl ChatTouch {
    /// Drops a finger when chat loses input ownership.
    pub(super) fn cancel(&mut self) {
        *self = Self::default();
    }

    /// Returns a completed tap or moves the history, retaining the first finger's ownership.
    pub(super) fn drive(
        &mut self,
        touches: &Touches,
        presentation: &mut UiPresentationRuntime,
    ) -> Option<ChatTouchAction> {
        if self.finger.is_none()
            && let Some(touch) = touches.iter_just_pressed().next()
            && let Ok(point) = UiPoint::new(touch.position().x, touch.position().y)
        {
            self.finger = Some((touch.id(), point, point));
            self.dragged = false;
            self.history = presentation.chat_history_contains(point);
            self.slider = presentation.chat_slider_at(point);
            self.last_action = None;
        }
        let (id, start, last) = self.finger?;
        let touch = touches.get_pressed(id).or_else(|| touches.get_released(id));
        let Some(point) =
            touch.and_then(|touch| UiPoint::new(touch.position().x, touch.position().y).ok())
        else {
            self.cancel();
            return None;
        };
        if let Some(index) = self.slider {
            let action = presentation.chat_slider_action(index, point);
            let changed = action.filter(|action| self.last_action != Some(*action));
            self.last_action = action;
            if action.is_none() || touches.get_pressed(id).is_none() {
                self.cancel();
            }
            return changed.map(ChatTouchAction::Slider);
        }
        let distance = (point.x() - start.x()).hypot(point.y() - start.y());
        self.dragged |= distance > ui::TOUCH_DRAG_DISTANCE;
        if self.dragged && self.history {
            presentation.scroll_chat(point.y() - last.y(), true);
        }
        self.finger = Some((id, start, if self.dragged { point } else { last }));
        if touches.get_pressed(id).is_some() {
            None
        } else {
            let tap = (!self.dragged).then_some(ChatTouchAction::Tap(start, point));
            self.cancel();
            tap
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::touch::{TouchInput, TouchPhase, touch_screen_input_system},
        prelude::*,
    };

    #[derive(Resource)]
    struct Fixture {
        gesture: ChatTouch,
        presentation: UiPresentationRuntime,
        taps: Vec<(UiPoint, UiPoint)>,
    }

    /// Runs the production touch adapter after Bevy has collected this frame's touch changes.
    fn drive(touches: Res<Touches>, mut fixture: ResMut<Fixture>) {
        let Fixture {
            gesture,
            presentation,
            taps,
        } = &mut *fixture;
        if let Some(ChatTouchAction::Tap(start, end)) = gesture.drive(&touches, presentation) {
            taps.push((start, end));
        }
    }

    /// Sends one edge through the platform-independent touch collector.
    fn step(app: &mut App, id: u64, phase: TouchPhase, y: f32) {
        app.world_mut().write_message(TouchInput {
            window: Entity::PLACEHOLDER,
            id,
            phase,
            position: Vec2::new(30.0, y),
            force: None,
        });
        app.update();
    }

    #[test]
    fn swipes_cancellation_and_second_fingers_do_not_activate_chat_links() {
        let mut app = App::new();
        app.add_message::<TouchInput>()
            .init_resource::<Touches>()
            .insert_resource(Fixture {
                gesture: ChatTouch::default(),
                taps: Vec::new(),
                presentation: UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
                    .unwrap(),
            })
            .add_systems(PreUpdate, touch_screen_input_system)
            .add_systems(Update, drive);
        step(&mut app, 1, TouchPhase::Started, 30.0);
        step(&mut app, 2, TouchPhase::Started, 60.0);
        step(&mut app, 2, TouchPhase::Ended, 60.0);
        assert!(app.world().resource::<Fixture>().taps.is_empty());
        step(&mut app, 1, TouchPhase::Moved, 90.0);
        step(&mut app, 1, TouchPhase::Ended, 90.0);
        step(&mut app, 3, TouchPhase::Started, 30.0);
        step(&mut app, 3, TouchPhase::Canceled, 30.0);
        assert!(app.world().resource::<Fixture>().taps.is_empty());
        step(&mut app, 4, TouchPhase::Started, 30.0);
        step(&mut app, 4, TouchPhase::Ended, 30.0);
        assert_eq!(app.world().resource::<Fixture>().taps.len(), 1);
    }
}
