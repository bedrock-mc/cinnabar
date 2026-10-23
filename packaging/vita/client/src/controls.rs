//! Physical Vita controls translated into Cinnabar's existing semantic router.
//! Legacy Console defaults, rather than the modern DualShock/L3/R3 layout.
//! Exact retail timing and menu/touch parity still require device comparison.

use semantic_input::{
    Action, ActionBinding, ActionSnapshot, AxisDirection, ControlSettings, ControllerFrame,
    DeviceFrame, InputChord, InputContext, ModifierChord, PhysicalControl, RouterError,
    SemanticInputRouter, TouchControlLayout,
};

// One shared table; the C adapter checks its values against the SDK enums.
include!(concat!(env!("OUT_DIR"), "/pad_masks.rs"));
const SPRINT: u32 = 1 << 16;
const SNEAK: u32 = 1 << 17;
const PHYSICAL_MASK: u32 =
    SELECT | START | UP | RIGHT | DOWN | LEFT | L | R | TRIANGLE | CIRCLE | CROSS | SQUARE;

/// Provisional double-forward window; qualify against Vita Edition before parity claims.
const SPRINT_TAP_US: u64 = 350_000;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadSample {
    pub buttons: u32,
    pub left_x: u8,
    pub left_y: u8,
    pub right_x: u8,
    pub right_y: u8,
}

const _: () = assert!(size_of::<PadSample>() == 8);

impl Default for PadSample {
    fn default() -> Self {
        Self {
            buttons: 0,
            left_x: 128,
            left_y: 128,
            right_x: 128,
            right_y: 128,
        }
    }
}

pub struct InputFrame {
    pub actions: ActionSnapshot,
    // These are client-shell requests, not movement actions in semantic-input.
    pub inventory: bool,
    pub crafting: bool,
    pub drop_item: bool,
}

pub struct VitaInput {
    router: SemanticInputRouter,
    previous: PadSample,
    sequence: u64,
    previous_mask: u32,
    last_forward: Option<u64>,
    forward_down: bool,
    sprinting: bool,
    sneaking: bool,
    context: InputContext,
}

impl Default for VitaInput {
    fn default() -> Self {
        Self {
            router: SemanticInputRouter::with_settings_and_touch_layout(
                settings(),
                TouchControlLayout::default(),
            )
            .expect("Vita built-in controls are valid"),
            previous: PadSample::default(),
            sequence: 0,
            previous_mask: 0,
            last_forward: None,
            forward_down: false,
            sprinting: false,
            sneaking: false,
            context: InputContext::Gameplay,
        }
    }
}

impl VitaInput {
    /// `now_us` is supplied by the platform clock. Gameplay and UI bindings are
    /// separate; entering a menu releases/quarantines gameplay through the router.
    pub fn sample(
        &mut self,
        sample: PadSample,
        now_us: u64,
        context: InputContext,
    ) -> Result<InputFrame, RouterError> {
        let context_changed = context != self.context;
        if context_changed {
            self.router.set_context(context);
            self.context = context;
            self.last_forward = None;
            self.sprinting = false;
            self.sneaking = false;
        }
        let buttons = sample.buttons & PHYSICAL_MASK;
        let pressed = buttons & !self.previous.buttons;
        let forward = -axis(sample.left_y);
        if context == InputContext::Gameplay && !context_changed {
            if pressed & DOWN != 0 {
                self.sneaking = !self.sneaking;
            }
            if forward > 0.7 && !self.forward_down {
                self.sprinting = self.last_forward.is_some_and(|last| {
                    now_us
                        .checked_sub(last)
                        .is_some_and(|elapsed| elapsed <= SPRINT_TAP_US)
                });
                self.last_forward = Some(now_us);
            }
        }
        if forward < 0.2 || self.sneaking || context != InputContext::Gameplay {
            self.sprinting = false;
        }
        if forward > 0.7 {
            self.forward_down = true;
        } else if forward < 0.2 {
            self.forward_down = false;
        }
        let mask = buttons
            | if self.sprinting { SPRINT } else { 0 }
            | if self.sneaking { SNEAK } else { 0 };
        if self.sequence == 0 || self.previous != sample || self.previous_mask != mask {
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or(RouterError::FrameSequenceExhausted)?;
        }
        let controller = ControllerFrame {
            device_id: 0,
            activity_sequence: self.sequence,
            axes: [
                axis(sample.left_x),
                forward,
                axis(sample.right_x),
                -axis(sample.right_y),
                0.,
                0.,
                0.,
                0.,
            ],
            buttons: (0..32).filter(|bit| mask & (1 << bit) != 0).collect(),
            ..ControllerFrame::default()
        };
        self.router.route(DeviceFrame {
            controllers: vec![controller],
            ..DeviceFrame::default()
        })?;
        let actions = self.router.finalize()?;
        self.previous = sample;
        self.previous_mask = mask;
        let gameplay = context == InputContext::Gameplay && !context_changed;
        Ok(InputFrame {
            actions,
            inventory: gameplay && pressed & TRIANGLE != 0,
            crafting: gameplay && pressed & SQUARE != 0,
            drop_item: gameplay && pressed & CIRCLE != 0,
        })
    }
}

fn axis(value: u8) -> f32 {
    let centered = f32::from(value) - 128.;
    centered / if centered < 0. { 128. } else { 127. }
}

fn settings() -> ControlSettings {
    use Action::*;
    use InputContext::{Gameplay, UiFocused};
    let mut bindings = Vec::new();
    for (context, action, mask) in [
        (Gameplay, Jump, CROSS),
        (Gameplay, Attack, R),
        (Gameplay, Use, L),
        (Gameplay, Sneak, SNEAK),
        (Gameplay, Sprint, SPRINT),
        (Gameplay, HotbarPrevious, LEFT),
        (Gameplay, HotbarNext, RIGHT),
        (Gameplay, CyclePerspective, UP),
        (Gameplay, Menu, START),
        (Gameplay, PlayerList, SELECT),
        (UiFocused, UiUp, UP),
        (UiFocused, UiDown, DOWN),
        (UiFocused, UiLeft, LEFT),
        (UiFocused, UiRight, RIGHT),
        (UiFocused, UiAccept, CROSS),
        (UiFocused, UiCancel, CIRCLE),
        (UiFocused, UiTabPrevious, L),
        (UiFocused, UiTabNext, R),
        (UiFocused, Back, START),
    ] {
        bindings.push(ActionBinding {
            action,
            context,
            chord: InputChord {
                control: PhysicalControl::GamepadButton(mask.trailing_zeros() as u8),
                modifiers: ModifierChord::default(),
            },
        });
    }
    for (action, axis, direction) in [
        (MoveRight, 0, AxisDirection::Positive),
        (MoveLeft, 0, AxisDirection::Negative),
        (MoveForward, 1, AxisDirection::Positive),
        (MoveBackward, 1, AxisDirection::Negative),
        (LookRight, 2, AxisDirection::Positive),
        (LookLeft, 2, AxisDirection::Negative),
        (LookUp, 3, AxisDirection::Positive),
        (LookDown, 3, AxisDirection::Negative),
    ] {
        bindings.push(ActionBinding {
            action,
            context: Gameplay,
            chord: InputChord {
                control: PhysicalControl::GamepadAxis { axis, direction },
                modifiers: ModifierChord::default(),
            },
        });
    }
    ControlSettings::new(bindings, 0.5, 1., 1., false, false, 0.15, 0.15)
        .expect("Vita bindings are valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(input: &mut VitaInput, buttons: u32, now: u64) -> InputFrame {
        input
            .sample(
                PadSample {
                    buttons,
                    ..PadSample::default()
                },
                now,
                InputContext::Gameplay,
            )
            .unwrap()
    }

    #[test]
    fn attack_edges_release_and_item_drop_is_not_sneak() {
        let mut input = VitaInput::default();
        let press = sample(&mut input, R | CIRCLE, 1);
        assert!(press.actions.phases[Action::Attack as usize].pressed);
        assert!(press.drop_item);
        assert!(!press.actions.phases[Action::Sneak as usize].held);
        let held = sample(&mut input, R | CIRCLE, 2);
        assert!(!held.drop_item);
        assert!(!held.actions.phases[Action::Attack as usize].pressed);
        assert!(sample(&mut input, 0, 3).actions.phases[Action::Attack as usize].released);
    }

    #[test]
    fn dpad_sneak_toggles_and_hotbar_does_not_use_shoulders() {
        let mut input = VitaInput::default();
        assert!(sample(&mut input, DOWN, 1).actions.phases[Action::Sneak as usize].held);
        assert!(sample(&mut input, 0, 2).actions.phases[Action::Sneak as usize].held);
        assert!(!sample(&mut input, DOWN, 3).actions.phases[Action::Sneak as usize].held);
        let next = sample(&mut input, RIGHT | L, 4);
        assert!(next.actions.phases[Action::HotbarNext as usize].pressed);
        assert!(next.actions.phases[Action::Use as usize].held);
        assert!(!next.actions.phases[Action::HotbarPrevious as usize].pressed);
    }

    #[test]
    fn forward_double_tap_sprints_and_neutral_stops() {
        let mut input = VitaInput::default();
        let forward = PadSample {
            left_y: 0,
            ..PadSample::default()
        };
        let a = input.sample(forward, 0, InputContext::Gameplay).unwrap();
        assert!(!a.actions.phases[Action::Sprint as usize].held);
        sample(&mut input, 0, 100_000);
        let b = input
            .sample(forward, 200_000, InputContext::Gameplay)
            .unwrap();
        assert!(b.actions.phases[Action::Sprint as usize].held);
        assert!(sample(&mut input, 0, 300_000).actions.phases[Action::Sprint as usize].released);
    }

    #[test]
    fn menu_releases_attack_and_requires_a_fresh_accept_press() {
        let mut input = VitaInput::default();
        sample(&mut input, R | CROSS, 1);
        let menu = input
            .sample(
                PadSample {
                    buttons: R | CROSS,
                    ..PadSample::default()
                },
                2,
                InputContext::UiFocused,
            )
            .unwrap();
        assert!(!menu.actions.phases[Action::Attack as usize].held);
        assert!(!menu.actions.phases[Action::UiAccept as usize].pressed);
        input
            .sample(PadSample::default(), 3, InputContext::UiFocused)
            .unwrap();
        let accept = input
            .sample(
                PadSample {
                    buttons: CROSS,
                    ..PadSample::default()
                },
                4,
                InputContext::UiFocused,
            )
            .unwrap();
        assert!(accept.actions.phases[Action::UiAccept as usize].pressed);
    }

    #[test]
    fn stick_jitter_and_expired_second_tap_do_not_start_sprinting() {
        let mut input = VitaInput::default();
        for (time, y) in [
            (0, 0),
            (50_000, 50),
            (100_000, 0),
            (150_000, 128),
            (900_000, 0),
        ] {
            let frame = input
                .sample(
                    PadSample {
                        left_y: y,
                        ..PadSample::default()
                    },
                    time,
                    InputContext::Gameplay,
                )
                .unwrap();
            assert!(!frame.actions.phases[Action::Sprint as usize].held);
        }
    }
}
