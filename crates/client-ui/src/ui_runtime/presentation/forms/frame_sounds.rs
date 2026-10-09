//! Sound feedback uses the rendered control's identity and existing dispatcher.

use std::sync::Mutex;

use crate::ui_runtime::forms::EngineFrame;
use json_ui::{ButtonInput, Dispatcher, InputMode, PointerInput, ScreenEvent, ViewState};
use ui::UiPoint;

#[derive(Default)]
pub(super) struct FrameSounds {
    frame: Option<EngineFrame>,
    input: Mutex<SoundInput>,
}

#[derive(Default)]
struct SoundInput {
    dispatcher: Dispatcher,
    view: ViewState,
    touches: [Option<(u64, String)>; crate::sound_requests::MAX_UI_TOUCHES],
}

impl FrameSounds {
    /// Retains the rendered regions without copying their sound declarations.
    pub(super) fn set_frame(&mut self, frame: EngineFrame) {
        self.frame = Some(frame);
    }

    /// Reads the current screen's regions and authored feedback.
    pub(super) fn frame(&self) -> Option<&EngineFrame> {
        self.frame.as_ref()
    }

    /// Clears stale geometry while preserving the current screen's replay history.
    pub(super) fn clear_frame(&mut self) {
        self.frame = None;
    }

    /// Starts input state afresh when a screen or modal changes ownership.
    pub(super) fn reset_input(&mut self) {
        *self
            .input
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = SoundInput::default();
    }

    /// Revokes held touches without forgetting sound replay deadlines.
    pub(super) fn cancel_touches(&self) {
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if input.touches.iter().any(Option::is_some) {
            input.touches.fill(None);
            if let Some(frame) = &self.frame {
                input.cancel_pointer(frame);
            }
        }
    }

    /// Dispatches a mouse press and its silent release through the rendered controls.
    pub(super) fn mouse(&self, point: UiPoint, now: f64, mut receive: impl FnMut(&str, f32, f32)) {
        let Some(frame) = &self.frame else {
            return;
        };
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let point = frame.to_virtual(point);
        input.pointer(frame, point, true, InputMode::Mouse, now, &mut receive);
        input.pointer(frame, point, false, InputMode::Mouse, now, &mut receive);
    }

    /// Sounds an accepted touch release; holds only inspect identity and allocate nothing.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn touch(
        &self,
        id: u64,
        point: Option<UiPoint>,
        pressed: bool,
        held: bool,
        now: f64,
        mut receive: impl FnMut(&str, f32, f32),
    ) {
        let Some(frame) = &self.frame else {
            return;
        };
        let point = point.map(|point| frame.to_virtual(point));
        let key = point
            .and_then(|point| json_ui::hit_test(&frame.hits, point))
            .map(|hit| hit.key.as_str());
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if pressed {
            if input
                .touches
                .iter()
                .flatten()
                .any(|(captured, _)| *captured == id)
            {
                return;
            }
            if let (Some(key), Some(slot)) =
                (key, input.touches.iter_mut().find(|slot| slot.is_none()))
            {
                *slot = Some((id, key.to_owned()));
            }
            if held {
                return;
            }
        }
        let Some(slot) = input
            .touches
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|(captured, _)| *captured == id))
        else {
            return;
        };
        if input.touches[slot]
            .as_ref()
            .is_none_or(|(_, captured)| Some(captured.as_str()) != key)
        {
            input.touches[slot] = None;
            input.cancel_pointer(frame);
            return;
        }
        if !held {
            input.touches[slot] = None;
            if let Some(point) = point {
                input.cancel_pointer(frame);
                input.pointer(frame, point, true, InputMode::Touch, now, &mut receive);
                input.pointer(frame, point, false, InputMode::Touch, now, &mut receive);
            }
        }
    }

    /// Sounds one keyboard or gamepad activation of the identified control.
    pub(super) fn activate(
        &self,
        key: &str,
        mode: InputMode,
        now: f64,
        mut receive: impl FnMut(&str, f32, f32),
    ) {
        let Some(frame) = &self.frame else {
            return;
        };
        let Some(region) = frame.hits.iter().find(|region| region.key == key) else {
            return;
        };
        let Some(mapping) = region.input.mappings.iter().find(|mapping| {
            Some(mapping.to.as_str()) == region.pressed.as_deref()
                && mapping.input_mode_condition.admits(mode)
        }) else {
            return;
        };
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let point = (mode != InputMode::Gamepad).then_some([
            region.rect.x + region.rect.w / 2.0,
            region.rect.y + region.rect.h / 2.0,
        ]);
        let SoundInput {
            dispatcher, view, ..
        } = &mut *input;
        dispatcher.pointer(
            &frame.hits,
            view,
            PointerInput {
                point,
                held: false,
                mode,
                now,
            },
        );
        view.focused = Some(key.to_owned());
        for down in [true, false] {
            forward(
                dispatcher.button(
                    &frame.hits,
                    view,
                    ButtonInput {
                        id: &mapping.from,
                        down,
                        point,
                        mode,
                        now,
                    },
                ),
                &mut receive,
            );
        }
        view.focused = None;
    }
}

impl SoundInput {
    /// Clears abandoned button edges without sounding a touch release.
    fn cancel_pointer(&mut self, frame: &EngineFrame) {
        self.view.focused = None;
        self.view.pressed = None;
        self.dispatcher.pointer(
            &frame.hits,
            &mut self.view,
            PointerInput {
                point: None,
                held: false,
                mode: InputMode::Mouse,
                now: 0.0,
            },
        );
        self.dispatcher.button(
            &frame.hits,
            &mut self.view,
            ButtonInput {
                id: "button.menu_select",
                down: false,
                point: None,
                mode: InputMode::Mouse,
                now: 0.0,
            },
        );
    }

    /// Routes a pointer edge while discarding host actions already handled elsewhere.
    fn pointer(
        &mut self,
        frame: &EngineFrame,
        point: [f64; 2],
        down: bool,
        mode: InputMode,
        now: f64,
        receive: &mut impl FnMut(&str, f32, f32),
    ) {
        forward(
            self.dispatcher.pointer(
                &frame.hits,
                &mut self.view,
                PointerInput {
                    point: Some(point),
                    held: down,
                    mode,
                    now,
                },
            ),
            receive,
        );
        forward(
            self.dispatcher.button(
                &frame.hits,
                &mut self.view,
                ButtonInput {
                    id: "button.menu_select",
                    down,
                    point: Some(point),
                    mode,
                    now,
                },
            ),
            receive,
        );
    }
}

/// Forwards only sound events to the existing pack-aware queue.
fn forward(dispatch: json_ui::Dispatch, receive: &mut impl FnMut(&str, f32, f32)) {
    for event in dispatch.events {
        if let ScreenEvent::Sound {
            name,
            volume,
            pitch,
        } = event
        {
            receive(&name, volume, pitch);
        }
    }
}

#[cfg(test)]
pub(super) mod fixtures {
    use crate::ui_runtime::forms::EngineFrame;
    use json_ui::{LayoutEnv, ResolvedControl, TextMeasure, TextureMeta, TextureSource, ViewState};
    use serde_json::{Value, json};

    struct Text;
    impl TextMeasure for Text {
        /// Supplies empty text metrics for sound-only fixtures.
        fn extent(&self, _: &str) -> [f64; 2] {
            [0.0; 2]
        }
    }
    struct Textures;
    impl TextureSource for Textures {
        /// These controls do not use textures.
        fn texture(&self, _: &str) -> Option<TextureMeta> {
            None
        }
    }

    /// Builds a synthetic control without reading a game pack.
    fn control(
        name: &str,
        kind: &str,
        properties: Value,
        children: Vec<ResolvedControl>,
    ) -> ResolvedControl {
        ResolvedControl {
            name: name.into(),
            control_type: Some(kind.into()),
            base: None,
            unresolved_base: None,
            properties: properties
                .as_object()
                .unwrap()
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into(),
            children,
            factory: None,
        }
    }

    /// Two buttons share one action ID but declare distinct sound feedback.
    pub(in crate::ui_runtime::presentation::forms) fn frame() -> EngineFrame {
        let button = |name, offset, sound, volume| {
            control(
                name,
                "button",
                json!({
                    "anchor_from":"top_left", "anchor_to":"top_left", "size":[40,20], "offset":[offset,0], "focus_enabled":true,
                    "sound_name":sound, "sound_volume":volume, "sound_pitch":1.25,
                    "sounds":[{"event_type":"button_event", "sound_name":"extra", "sound_volume":0.25, "sound_pitch":0.75, "min_seconds_between_plays":1}],
                    "button_mappings":[{"from_button_id":"button.menu_select", "to_button_id":"button.shared", "mapping_type":"pressed"}]
                }),
                vec![],
            )
        };
        let root = control(
            "screen",
            "panel",
            json!({"anchor_from":"top_left", "anchor_to":"top_left", "size":[100,100]}),
            vec![
                button("left", 0, "left.click", 1.0),
                button("right", 50, "right.click", 0.5),
            ],
        );
        let env = LayoutEnv {
            text: &Text,
            textures: &Textures,
        };
        let (laid, _) = json_ui::layout_with(&root, [100.0; 2], &env, &ViewState::default());
        EngineFrame {
            identity: None,
            hits: json_ui::hit_regions(&laid).into(),
            report: Default::default(),
            cancel_target: None,
            origin: [10.0, 20.0],
            scale: 2.0,
            panel: None,
            edit_texts: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Installs two original controls sharing one action ID.
    fn sounds() -> FrameSounds {
        let mut sounds = FrameSounds::default();
        sounds.set_frame(fixtures::frame());
        sounds
    }

    /// Returns physical points accounting for the fixture's origin and scale.
    fn point(right: bool) -> UiPoint {
        UiPoint::new(if right { 120.0 } else { 20.0 }, 30.0).unwrap()
    }

    #[test]
    fn buttons_sharing_an_action_do_not_share_feedback() {
        let sounds = sounds();
        let mut emitted = Vec::new();
        sounds.mouse(point(true), 1.0, |name, volume, pitch| {
            emitted.push((name.to_owned(), volume, pitch))
        });
        assert_eq!(
            emitted,
            [
                ("right.click".into(), 0.5, 1.25),
                ("extra".into(), 0.25, 0.75)
            ]
        );
    }

    #[test]
    fn replay_deadlines_belong_to_controls_and_survive_redraws() {
        let mut sounds = sounds();
        let mut emitted = Vec::new();
        sounds.mouse(point(false), 1.0, |name, _, _| {
            emitted.push(name.to_owned())
        });
        sounds.set_frame(fixtures::frame());
        sounds.mouse(point(true), 1.5, |name, _, _| emitted.push(name.to_owned()));
        sounds.mouse(point(false), 2.0, |name, _, _| {
            emitted.push(name.to_owned())
        });
        sounds.mouse(point(false), 2.01, |name, _, _| {
            emitted.push(name.to_owned())
        });
        assert_eq!(
            emitted,
            [
                "left.click",
                "extra",
                "right.click",
                "extra",
                "left.click",
                "left.click",
                "extra"
            ]
        );
        sounds.reset_input();
        emitted.clear();
        sounds.mouse(point(false), 2.02, |name, _, _| {
            emitted.push(name.to_owned())
        });
        assert_eq!(emitted, ["left.click", "extra"]);
    }

    #[test]
    fn shorthand_is_untimed_but_zero_interval_array_entries_require_time_to_advance() {
        let mut sounds = sounds();
        let frame = sounds.frame.as_mut().unwrap();
        for region in std::sync::Arc::make_mut(&mut frame.hits) {
            if let Some(meta) = region.widget.sounds.as_mut() {
                meta.entries[0].min_seconds_between_plays = 0.0;
            }
        }
        let mut emitted = Vec::new();
        for now in [1.0, 1.0, 1.01] {
            sounds.mouse(point(true), now, |name, _, _| emitted.push(name.to_owned()));
        }
        assert_eq!(
            emitted,
            [
                "right.click",
                "extra",
                "right.click",
                "right.click",
                "extra"
            ]
        );
    }

    #[test]
    fn touch_sounds_once_on_release_and_holds_allocate_nothing() {
        let sounds = sounds();
        let mut emitted = Vec::new();
        sounds.touch(1, Some(point(true)), true, true, 1.0, |name, _, _| {
            emitted.push(name.to_owned())
        });
        assert!(emitted.is_empty());
        let (_, allocations) = crate::allocation_count::count(|| {
            for _ in 0..100 {
                sounds.touch(1, Some(point(true)), false, true, 1.1, |_, _, _| {
                    panic!("a held touch is silent")
                });
            }
        });
        assert_eq!(allocations, 0);
        for _ in 0..2 {
            sounds.touch(1, Some(point(true)), false, false, 1.2, |name, _, _| {
                emitted.push(name.to_owned())
            });
        }
        assert_eq!(emitted, ["right.click", "extra"]);
    }

    #[test]
    fn independent_touches_share_only_the_controls_replay_deadline() {
        let sounds = sounds();
        let mut emitted = Vec::new();
        for id in [1, 2] {
            sounds.touch(id, Some(point(true)), true, true, 1.0, |_, _, _| {
                panic!("press is silent")
            });
        }
        for (id, now) in [(1, 1.1), (2, 1.2)] {
            sounds.touch(id, Some(point(true)), false, false, now, |name, _, _| {
                emitted.push(name.to_owned())
            });
        }
        assert_eq!(emitted, ["right.click", "extra", "right.click"]);
    }

    #[test]
    fn simultaneous_touches_keep_each_controls_feedback() {
        let sounds = sounds();
        let mut emitted = Vec::new();
        for (id, right) in [(1, false), (2, true)] {
            sounds.touch(id, Some(point(right)), true, true, 1.0, |_, _, _| {
                panic!("press is silent")
            });
        }
        for (id, right) in [(1, false), (2, true)] {
            sounds.touch(id, Some(point(right)), false, false, 1.1, |name, _, _| {
                emitted.push(name.to_owned())
            });
        }
        assert_eq!(emitted, ["left.click", "extra", "right.click", "extra"]);
    }

    #[test]
    fn another_control_with_the_same_action_cannot_accept_a_touch_release() {
        let sounds = sounds();
        sounds.touch(1, Some(point(false)), true, true, 1.0, |_, _, _| {
            panic!("press is silent")
        });
        sounds.touch(1, Some(point(true)), false, true, 1.1, |_, _, _| {
            panic!("movement is silent")
        });
        sounds.touch(1, Some(point(false)), false, false, 1.2, |_, _, _| {
            panic!("cancelled release is silent")
        });
        sounds.touch(2, Some(point(true)), false, false, 1.2, |_, _, _| {
            panic!("uncaptured release is silent")
        });
    }

    #[test]
    fn ownership_changes_revoke_touch_feedback() {
        let mut sounds = sounds();
        sounds.touch(1, Some(point(true)), true, true, 1.0, |_, _, _| {
            panic!("press is silent")
        });
        sounds.reset_input();
        sounds.touch(1, Some(point(true)), false, false, 1.1, |_, _, _| {
            panic!("old ownership cannot sound")
        });
        sounds.touch(2, Some(point(true)), true, true, 2.0, |_, _, _| {
            panic!("press is silent")
        });
        sounds.cancel_touches();
        sounds.touch(2, Some(point(true)), false, false, 2.1, |_, _, _| {
            panic!("cancelled touch cannot sound")
        });
    }

    #[test]
    fn cancelled_touch_does_not_suppress_the_next_mouse_press() {
        let sounds = sounds();
        sounds.touch(1, Some(point(true)), true, true, 1.0, |_, _, _| {
            panic!("press is silent")
        });
        sounds.cancel_touches();
        let mut emitted = Vec::new();
        sounds.mouse(point(true), 1.1, |name, _, _| emitted.push(name.to_owned()));
        assert_eq!(emitted, ["right.click", "extra"]);
    }

    #[test]
    fn gamepad_activation_uses_the_focused_control_and_does_not_sound_on_release() {
        let sounds = sounds();
        let key = sounds
            .frame()
            .unwrap()
            .hits
            .iter()
            .find(|hit| hit.name == "right")
            .unwrap()
            .key
            .clone();
        let mut emitted = Vec::new();
        sounds.activate(&key, InputMode::Gamepad, 1.0, |name, volume, pitch| {
            emitted.push((name.to_owned(), volume, pitch))
        });
        assert_eq!(
            emitted,
            [
                ("right.click".into(), 0.5, 1.25),
                ("extra".into(), 0.25, 0.75)
            ]
        );
    }

    #[test]
    fn keyboard_activation_honors_non_gamepad_mappings() {
        let mut sounds = sounds();
        let frame = sounds.frame.as_mut().unwrap();
        let right = std::sync::Arc::make_mut(&mut frame.hits)
            .iter_mut()
            .find(|hit| hit.name == "right")
            .unwrap();
        right.input.mappings[0].input_mode_condition = json_ui::InputModeCondition::NotGamepad;
        let key = right.key.clone();
        let mut emitted = Vec::new();
        sounds.activate(&key, InputMode::Mouse, 1.0, |name, _, _| {
            emitted.push(name.to_owned())
        });
        assert_eq!(emitted, ["right.click", "extra"]);
    }

    #[test]
    fn a_touch_pressed_and_released_in_one_frame_sounds_once() {
        let sounds = sounds();
        let mut emitted = Vec::new();
        sounds.touch(1, Some(point(true)), true, false, 1.0, |name, _, _| {
            emitted.push(name.to_owned())
        });
        assert_eq!(emitted, ["right.click", "extra"]);
        sounds.touch(1, Some(point(true)), false, false, 1.1, |name, _, _| {
            emitted.push(name.to_owned())
        });
        assert_eq!(emitted, ["right.click", "extra"]);
    }

    #[test]
    fn disabled_controls_are_silent_and_zero_volume_is_preserved() {
        let mut sounds = sounds();
        let frame = sounds.frame.as_mut().unwrap();
        let right = std::sync::Arc::make_mut(&mut frame.hits)
            .iter_mut()
            .find(|hit| hit.name == "right")
            .unwrap();
        right.enabled = false;
        sounds.mouse(point(true), 1.0, |_, _, _| {
            panic!("disabled control is silent")
        });
        let frame = sounds.frame.as_mut().unwrap();
        let right = std::sync::Arc::make_mut(&mut frame.hits)
            .iter_mut()
            .find(|hit| hit.name == "right")
            .unwrap();
        right.enabled = true;
        right
            .widget
            .sounds
            .as_mut()
            .unwrap()
            .shorthand
            .as_mut()
            .unwrap()
            .volume = 0.0;
        let mut emitted = Vec::new();
        sounds.mouse(point(true), 1.1, |name, volume, pitch| {
            emitted.push((name.to_owned(), volume, pitch))
        });
        assert_eq!(
            emitted,
            [
                ("right.click".into(), 0.0, 1.25),
                ("extra".into(), 0.25, 0.75)
            ]
        );
    }
}
