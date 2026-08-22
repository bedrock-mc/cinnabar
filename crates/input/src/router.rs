use core::num::NonZeroU64;

use crate::{
    Action, ActionPhase, ActionSnapshot, AxisDirection, BindingError, ControlSettings,
    ControllerFrame, DeviceFrame, FrameError, InputChord, InputContext, InputMode,
    MAX_CONTROLLER_BUTTONS, MAX_CONTROLLERS, MAX_KEYBOARD_KEYS, MAX_MOUSE_BUTTONS,
    MAX_TOUCH_CONTACTS, PhysicalControl, ReleaseReason, TouchControlLayout,
    axes::{
        axis_is_positive, clamp_vector, directional_axis, merged_touch_movement, mouse_axis_value,
        radial_deadzone, scale_look_axis, synthesize_directions, touch_control_strength,
    },
};

/// Maximum Euclidean magnitude accepted for a semantic look delta.
pub const MAX_LOOK_DELTA_PER_FRAME: f32 = 2048.0;
const CONTROLLER_AXES: usize = 8;
const MAX_QUARANTINED_CONTROLS: usize = 2
    * (MAX_KEYBOARD_KEYS
        + MAX_MOUSE_BUTTONS
        + MAX_CONTROLLERS * (MAX_CONTROLLER_BUTTONS + CONTROLLER_AXES)
        + MAX_TOUCH_CONTACTS);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouterError {
    InvalidFrame(FrameError),
    PendingFrameAlreadyRouted,
    MissingPendingFrame,
    GameplayActionPreview(Action),
    FrameSequenceExhausted,
    NonMonotonicActivitySequence { previous: u64, actual: u64 },
}

#[derive(Debug)]
pub struct SemanticInputRouter {
    settings: ControlSettings,
    context: InputContext,
    authority_generation: NonZeroU64,
    pending_authority: Option<NonZeroU64>,
    pending: Option<DeviceFrame>,
    pending_context: Option<InputContext>,
    pending_releases: [Option<ReleaseReason>; Action::COUNT],
    physical_down: [bool; Action::COUNT],
    frame_sequence: u64,
    input_mode: InputMode,
    input_activity_sequence: u64,
    touch_layout: TouchControlLayout,
    activity_watermark: u64,
    previous_frame: DeviceFrame,
    quarantined_controls: Vec<QuarantinedControl>,
    quarantined_controllers: Vec<u32>,
    controller_activity_baselines: Vec<ControllerActivityBaseline>,
    pending_release_barrier: bool,
}

impl Default for SemanticInputRouter {
    fn default() -> Self {
        Self {
            settings: ControlSettings::default(),
            context: InputContext::Gameplay,
            authority_generation: NonZeroU64::MIN,
            pending_authority: None,
            pending: None,
            pending_context: None,
            pending_releases: [None; Action::COUNT],
            physical_down: [false; Action::COUNT],
            frame_sequence: 0,
            input_mode: InputMode::KeyboardMouse,
            input_activity_sequence: 0,
            touch_layout: TouchControlLayout::default(),
            activity_watermark: 0,
            previous_frame: DeviceFrame::default(),
            quarantined_controls: Vec::new(),
            quarantined_controllers: Vec::new(),
            controller_activity_baselines: Vec::new(),
            pending_release_barrier: false,
        }
    }
}

impl SemanticInputRouter {
    pub fn with_settings_and_touch_layout(
        settings: ControlSettings,
        touch_layout: TouchControlLayout,
    ) -> Result<Self, BindingError> {
        settings.validate(&touch_layout)?;
        Ok(Self {
            settings,
            touch_layout,
            ..Self::default()
        })
    }

    /// Reports whether the merged, deadzone-adjusted controller state changed.
    pub fn controller_activity_changed<'a, 'b>(
        &self,
        previous: impl IntoIterator<Item = &'a ControllerFrame>,
        current: impl IntoIterator<Item = &'b ControllerFrame>,
    ) -> bool {
        evaluate_controller_state(previous, &self.settings)
            != evaluate_controller_state(current, &self.settings)
    }

    pub fn route(&mut self, frame: DeviceFrame) -> Result<(), RouterError> {
        if self.pending.is_some() {
            return Err(RouterError::PendingFrameAlreadyRouted);
        }
        frame
            .validate(&self.touch_layout)
            .map_err(RouterError::InvalidFrame)?;
        self.validate_activity_sequences(&frame)?;
        self.pending = Some(frame);
        self.pending_context = Some(self.context);
        Ok(())
    }

    pub fn preview_ui_phase(&self, action: Action) -> Result<ActionPhase, RouterError> {
        if !action.is_ui_preview() {
            return Err(RouterError::GameplayActionPreview(action));
        }
        let frame = self
            .pending
            .as_ref()
            .ok_or(RouterError::MissingPendingFrame)?;
        let quarantined_controllers = self.controller_quarantine_for_frame(frame);
        let filtered = without_quarantined_controls(
            frame,
            &self.quarantined_controls,
            &quarantined_controllers,
            &self.controller_activity_baselines,
            &self.settings,
        );
        let input_mode = self.selected_input_mode(&filtered).0;
        let sample = if self.pending_release_barrier {
            Sample::default()
        } else {
            self.sample(&filtered, input_mode)
        };
        let index = action as usize;
        let active = sample.active[index];
        Ok(ActionPhase {
            pressed: sample.pressed[index],
            held: active && !action.is_one_shot(),
            released: !active && self.physical_down[index] && !action.is_one_shot(),
        })
    }

    pub fn set_context(&mut self, context: InputContext) {
        if self.context != context {
            self.queue_held_releases(ReleaseReason::UiFocusTaken);
            self.context = context;
        }
    }

    pub fn replace_authority(&mut self, generation: NonZeroU64) {
        let current = self.pending_authority.unwrap_or(self.authority_generation);
        if current != generation {
            self.queue_held_releases(ReleaseReason::AuthorityChanged);
            self.pending_authority = Some(generation);
        }
    }

    pub fn replace_bindings(&mut self, settings: ControlSettings) -> Result<(), BindingError> {
        settings.validate(&self.touch_layout)?;
        self.settings = settings;
        self.queue_held_releases(ReleaseReason::BindingChanged);
        Ok(())
    }

    pub fn release_all(&mut self, reason: ReleaseReason) {
        self.queue_held_releases(reason);
    }

    pub fn finalize(&mut self) -> Result<ActionSnapshot, RouterError> {
        let next_sequence = self
            .frame_sequence
            .checked_add(1)
            .ok_or(RouterError::FrameSequenceExhausted)?;
        if self
            .pending
            .as_ref()
            .is_some_and(|frame| frame.window_focus_lost)
        {
            self.queue_held_releases(ReleaseReason::WindowFocusLost);
        }
        let frame = self
            .pending
            .take()
            .ok_or(RouterError::MissingPendingFrame)?;

        if !frame.disconnected_controllers.is_empty() && self.input_mode == InputMode::GamePad {
            self.queue_held_releases(ReleaseReason::ControllerDisconnected);
        }
        if frame.window_focus_lost {
            let mut currently_active = Vec::new();
            quarantine_active_controls(&frame, &mut currently_active, &self.settings);
            if !currently_active.is_empty() {
                // A validated frame carrying active devices gives us a newer
                // bounded physical truth than older focus-loss samples. Keep
                // only that truth; app-produced focus-loss frames are empty,
                // so they continue preserving the last focused controls.
                self.quarantined_controls = currently_active;
            }
        }

        self.quarantined_controllers = self.controller_quarantine_for_frame(&frame);
        let filtered = without_quarantined_controls(
            &frame,
            &self.quarantined_controls,
            &self.quarantined_controllers,
            &self.controller_activity_baselines,
            &self.settings,
        );
        let (input_mode, activity_sequence) = self.selected_input_mode(&filtered);
        let sample = if self.pending_release_barrier {
            Sample::default()
        } else {
            self.sample(&filtered, input_mode)
        };
        let mut phases = [ActionPhase::default(); Action::COUNT];
        let mut release_reasons = [None; Action::COUNT];

        for action in Action::ALL {
            let index = action as usize;
            let was_down = self.physical_down[index];
            let is_down = sample.active[index];
            let queued_reason = self.pending_releases[index];
            let persistent = !action.is_one_shot();
            let authority_release = persistent && was_down && queued_reason.is_some();
            phases[index] = ActionPhase {
                pressed: if action.is_one_shot() {
                    sample.pressed[index]
                } else {
                    is_down && (sample.pressed[index] || authority_release)
                },
                held: is_down && persistent,
                released: persistent && was_down && (!is_down || authority_release),
            };
            if authority_release {
                release_reasons[index] = queued_reason;
            }
        }

        self.physical_down = sample.active;
        self.pending_releases = [None; Action::COUNT];
        self.pending_release_barrier = false;
        self.pending_context = None;
        self.frame_sequence = next_sequence;
        self.input_mode = input_mode;
        self.input_activity_sequence = activity_sequence;
        self.activity_watermark = self.activity_watermark.max(frame_activity_max(&frame));
        self.update_controller_activity_baselines(&frame);
        let quarantined_state = evaluate_controller_state(
            frame
                .controllers
                .iter()
                .filter(|controller| self.quarantined_controllers.contains(&controller.device_id)),
            &self.settings,
        );
        self.quarantined_controllers.retain(|device_id| {
            frame
                .controllers
                .iter()
                .any(|controller| controller.device_id == *device_id)
                && !quarantined_state.is_neutral()
        });
        if !frame.window_focus_lost {
            let mut active_controls = Vec::new();
            quarantine_active_controls(&frame, &mut active_controls, &self.settings);
            self.quarantined_controls
                .retain(|control| active_controls.contains(control));
        }
        self.previous_frame = frame;
        if let Some(generation) = self.pending_authority.take() {
            self.authority_generation = generation;
        }

        Ok(ActionSnapshot {
            frame_sequence: self.frame_sequence,
            authority_generation: self.authority_generation,
            movement: sample.movement,
            raw_movement: sample.raw,
            analogue_movement: sample.analogue,
            look_delta: sample.look_delta,
            input_mode: self.input_mode,
            phases,
            release_reasons,
        })
    }

    fn queue_held_releases(&mut self, reason: ReleaseReason) {
        let previous_quarantine_len = self.quarantined_controls.len();
        quarantine_active_controls(
            &self.previous_frame,
            &mut self.quarantined_controls,
            &self.settings,
        );
        if let Some(frame) = self.pending.as_ref() {
            quarantine_active_controls(frame, &mut self.quarantined_controls, &self.settings);
        }
        let pending_mouse_motion = self
            .pending
            .as_ref()
            .and_then(|frame| frame.keyboard_mouse.as_ref())
            .is_some_and(|keyboard| keyboard.mouse_motion != [0.0, 0.0]);
        self.pending_release_barrier |= self.physical_down.iter().any(|down| *down)
            || self.quarantined_controls.len() > previous_quarantine_len
            || pending_mouse_motion;
        for action in Action::ALL {
            let index = action as usize;
            if self.physical_down[index] && !action.is_one_shot() {
                let replace = self.pending_releases[index]
                    .is_none_or(|current| reason.priority() > current.priority());
                if replace {
                    self.pending_releases[index] = Some(reason);
                }
            }
        }
    }

    fn selected_input_mode(&self, frame: &DeviceFrame) -> (InputMode, u64) {
        let keyboard = frame
            .keyboard_mouse
            .as_ref()
            .map(|sample| (InputMode::KeyboardMouse, sample.activity_sequence));
        let gamepad = frame
            .controllers
            .iter()
            .map(|sample| sample.activity_sequence)
            .max()
            .map(|sequence| (InputMode::GamePad, sequence));
        let touch = frame
            .touches
            .iter()
            .map(|sample| sample.activity_sequence)
            .max()
            .map(|sequence| (InputMode::Touch, sequence));
        let candidates = [keyboard, gamepad, touch];
        let mut selected = candidates
            .iter()
            .flatten()
            .copied()
            .find(|candidate| candidate.0 == self.input_mode)
            .or_else(|| candidates.iter().flatten().copied().next())
            .unwrap_or((self.input_mode, self.input_activity_sequence));
        // Equal global stamps retain the active mode. If it is absent, fixed
        // KeyboardMouse -> GamePad -> Touch candidate order breaks the tie.
        for candidate in candidates.into_iter().flatten() {
            if candidate.1 > selected.1 {
                selected = candidate;
            }
        }
        selected
    }

    fn controller_quarantine_for_frame(&self, frame: &DeviceFrame) -> Vec<u32> {
        let mut quarantined = self.quarantined_controllers.clone();
        quarantined.retain(|device_id| {
            frame
                .controllers
                .iter()
                .any(|controller| controller.device_id == *device_id)
        });
        if self.frame_sequence != 0 {
            for controller in &frame.controllers {
                let was_observed = self
                    .previous_frame
                    .controllers
                    .iter()
                    .any(|previous| previous.device_id == controller.device_id);
                if !was_observed && !quarantined.contains(&controller.device_id) {
                    quarantined.push(controller.device_id);
                }
            }
        }
        quarantined
    }

    fn update_controller_activity_baselines(&mut self, frame: &DeviceFrame) {
        self.controller_activity_baselines.retain(|baseline| {
            frame
                .controllers
                .iter()
                .any(|controller| controller.device_id == baseline.device_id)
        });
        let quarantined_state = evaluate_controller_state(
            frame
                .controllers
                .iter()
                .filter(|controller| self.quarantined_controllers.contains(&controller.device_id)),
            &self.settings,
        );
        if !quarantined_state.is_neutral() {
            return;
        }
        for device_id in &self.quarantined_controllers {
            let Some(controller) = frame
                .controllers
                .iter()
                .find(|controller| controller.device_id == *device_id)
            else {
                continue;
            };
            if let Some(baseline) = self
                .controller_activity_baselines
                .iter_mut()
                .find(|baseline| baseline.device_id == *device_id)
            {
                baseline.activity_sequence = controller.activity_sequence;
            } else {
                self.controller_activity_baselines
                    .push(ControllerActivityBaseline {
                        device_id: *device_id,
                        activity_sequence: controller.activity_sequence,
                    });
            }
        }
    }

    fn validate_activity_sequences(&self, frame: &DeviceFrame) -> Result<(), RouterError> {
        if let Some(keyboard) = &frame.keyboard_mouse {
            let previous = self
                .previous_frame
                .keyboard_mouse
                .as_ref()
                .map(|sample| sample.activity_sequence);
            validate_activity(
                previous,
                self.activity_watermark,
                keyboard.activity_sequence,
            )?;
        }
        for controller in &frame.controllers {
            let previous = self
                .previous_frame
                .controllers
                .iter()
                .find(|sample| sample.device_id == controller.device_id)
                .map(|sample| sample.activity_sequence);
            validate_activity(
                previous,
                self.activity_watermark,
                controller.activity_sequence,
            )?;
        }
        for contact in &frame.touches {
            let previous = self
                .previous_frame
                .touches
                .iter()
                .find(|sample| sample.contact_id == contact.contact_id)
                .map(|sample| sample.activity_sequence);
            validate_activity(previous, self.activity_watermark, contact.activity_sequence)?;
        }
        Ok(())
    }

    fn sample(&self, frame: &DeviceFrame, input_mode: InputMode) -> Sample {
        let controller_axes =
            evaluate_controller_state(frame.controllers.iter(), &self.settings).axes;
        let touch_movement = merged_touch_movement(frame);
        let previous_controller_axes =
            evaluate_controller_state(self.previous_frame.controllers.iter(), &self.settings).axes;
        let mut strengths = [0.0_f32; Action::COUNT];
        let mut axis_strengths = [0.0_f32; Action::COUNT];
        let mut pressed = [false; Action::COUNT];
        for binding in self.settings.bindings() {
            if binding.context != self.context
                || !control_matches_mode(binding.chord.control, input_mode)
            {
                continue;
            }
            if self.has_more_specific_chord(binding.chord, frame) {
                continue;
            }
            let strength =
                physical_strength(binding.chord, frame, controller_axes, &self.touch_layout);
            let action_index = binding.action as usize;
            strengths[action_index] = strengths[action_index].max(strength);
            if matches!(binding.chord.control, PhysicalControl::GamepadAxis { .. }) {
                axis_strengths[action_index] = axis_strengths[action_index].max(strength);
            }
            if strength > 0.0
                && physical_strength(
                    binding.chord,
                    &self.previous_frame,
                    previous_controller_axes,
                    &self.touch_layout,
                ) == 0.0
                && !self.edge_claimed_by_routed_context(binding.chord, frame, controller_axes)
            {
                pressed[action_index] = true;
            }
        }

        let mut movement = [
            strengths[Action::MoveRight as usize] - strengths[Action::MoveLeft as usize],
            strengths[Action::MoveForward as usize] - strengths[Action::MoveBackward as usize],
        ];
        let mut analogue = match input_mode {
            InputMode::GamePad => [
                axis_strengths[Action::MoveRight as usize]
                    - axis_strengths[Action::MoveLeft as usize],
                axis_strengths[Action::MoveForward as usize]
                    - axis_strengths[Action::MoveBackward as usize],
            ],
            InputMode::KeyboardMouse | InputMode::Touch => movement,
        };
        if input_mode == InputMode::Touch && self.context == InputContext::Gameplay {
            movement = touch_movement;
            analogue = touch_movement;
            synthesize_directions(
                &mut strengths,
                movement,
                Action::MoveLeft,
                Action::MoveRight,
                Action::MoveBackward,
                Action::MoveForward,
            );
        }
        let raw = movement;
        movement = clamp_vector(movement, 1.0);

        let mut raw_look = [
            strengths[Action::LookRight as usize] - strengths[Action::LookLeft as usize],
            strengths[Action::LookDown as usize] - strengths[Action::LookUp as usize],
        ];
        let (sensitivity, invert_y) = match input_mode {
            InputMode::KeyboardMouse => (
                self.settings.mouse_sensitivity,
                self.settings.invert_mouse_y,
            ),
            InputMode::GamePad => (
                self.settings.gamepad_look_sensitivity,
                self.settings.invert_gamepad_y,
            ),
            InputMode::Touch => (self.settings.touch_look_sensitivity, false),
        };
        if invert_y {
            raw_look[1] = -raw_look[1];
            strengths.swap(Action::LookUp as usize, Action::LookDown as usize);
        }
        let mut look_delta = [
            scale_look_axis(raw_look[0], sensitivity),
            scale_look_axis(raw_look[1], sensitivity),
        ];
        look_delta = clamp_vector(look_delta, MAX_LOOK_DELTA_PER_FRAME);
        let active = strengths.map(|strength| strength > 0.0);
        Sample {
            movement,
            raw,
            analogue,
            look_delta,
            active,
            pressed,
        }
    }

    fn has_more_specific_chord(&self, chord: InputChord, frame: &DeviceFrame) -> bool {
        let Some(keyboard) = frame.keyboard_mouse.as_ref() else {
            return false;
        };
        self.settings.bindings().iter().any(|candidate| {
            candidate.context == self.context
                && candidate.chord.control == chord.control
                && candidate.chord.modifiers.specificity() > chord.modifiers.specificity()
                && candidate
                    .chord
                    .modifiers
                    .is_satisfied_by(keyboard.modifiers)
        })
    }

    fn edge_claimed_by_routed_context(
        &self,
        chord: InputChord,
        frame: &DeviceFrame,
        controller_axes: [f32; 8],
    ) -> bool {
        let routed_context = self.pending_context.unwrap_or(self.context);
        routed_context != self.context
            && self.settings.bindings().iter().any(|candidate| {
                candidate.context == routed_context
                    && candidate.chord == chord
                    && physical_strength(
                        candidate.chord,
                        frame,
                        controller_axes,
                        &self.touch_layout,
                    ) > 0.0
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuarantinedControl {
    KeyboardUsage(u16),
    MouseButton(u8),
    ControllerButton { device_id: u32, button: u8 },
    ControllerAxis { device_id: u32, axis: u8 },
    TouchContact(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ControllerActivityBaseline {
    device_id: u32,
    activity_sequence: u64,
}

#[derive(Clone, Copy)]
struct Sample {
    movement: [f32; 2],
    raw: [f32; 2],
    analogue: [f32; 2],
    look_delta: [f32; 2],
    active: [bool; Action::COUNT],
    pressed: [bool; Action::COUNT],
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            movement: [0.0; 2],
            raw: [0.0; 2],
            analogue: [0.0; 2],
            look_delta: [0.0; 2],
            active: [false; Action::COUNT],
            pressed: [false; Action::COUNT],
        }
    }
}

fn quarantine_active_controls(
    frame: &DeviceFrame,
    output: &mut Vec<QuarantinedControl>,
    settings: &ControlSettings,
) {
    if let Some(keyboard) = frame.keyboard_mouse.as_ref() {
        for code in &keyboard.keys {
            push_quarantine(output, QuarantinedControl::KeyboardUsage(*code));
        }
        for button in &keyboard.mouse_buttons {
            push_quarantine(output, QuarantinedControl::MouseButton(*button));
        }
    }
    let controller_state = evaluate_controller_state(frame.controllers.iter(), settings);
    for controller in &frame.controllers {
        for button in &controller.buttons {
            push_quarantine(
                output,
                QuarantinedControl::ControllerButton {
                    device_id: controller.device_id,
                    button: *button,
                },
            );
        }
        for axis in 0..controller.axes.len() {
            if controller.axes[axis] == 0.0 || !controller_state.axis_family_is_active(axis) {
                continue;
            }
            push_quarantine(
                output,
                QuarantinedControl::ControllerAxis {
                    device_id: controller.device_id,
                    axis: axis as u8,
                },
            );
        }
    }
    for contact in &frame.touches {
        push_quarantine(output, QuarantinedControl::TouchContact(contact.contact_id));
    }
}

fn push_quarantine(output: &mut Vec<QuarantinedControl>, control: QuarantinedControl) {
    if !output.contains(&control) {
        if output.len() == MAX_QUARANTINED_CONTROLS {
            output.remove(0);
        }
        output.push(control);
    }
}

fn without_quarantined_controls(
    frame: &DeviceFrame,
    quarantine: &[QuarantinedControl],
    quarantined_controllers: &[u32],
    controller_activity_baselines: &[ControllerActivityBaseline],
    settings: &ControlSettings,
) -> DeviceFrame {
    let mut filtered = frame.clone();
    if let Some(keyboard) = filtered.keyboard_mouse.as_mut() {
        keyboard
            .keys
            .retain(|code| !quarantine.contains(&QuarantinedControl::KeyboardUsage(*code)));
        keyboard
            .mouse_buttons
            .retain(|button| !quarantine.contains(&QuarantinedControl::MouseButton(*button)));
    }
    let controller_state = evaluate_controller_state(filtered.controllers.iter(), settings);
    filtered.controllers.retain_mut(|controller| {
        if quarantined_controllers.contains(&controller.device_id)
            || controller_activity_baselines.iter().any(|baseline| {
                baseline.device_id == controller.device_id
                    && (controller.activity_sequence <= baseline.activity_sequence
                        || controller_state.is_neutral())
            })
        {
            return false;
        }
        controller.buttons.retain(|button| {
            !quarantine.contains(&QuarantinedControl::ControllerButton {
                device_id: controller.device_id,
                button: *button,
            })
        });
        for (axis, value) in controller.axes.iter_mut().enumerate() {
            if quarantine.contains(&QuarantinedControl::ControllerAxis {
                device_id: controller.device_id,
                axis: axis as u8,
            }) {
                *value = 0.0;
            }
        }
        true
    });
    // A neutral controller family cannot compete with another present device
    // family. Preserve the sole-controller mode fallback without allowing it
    // to suppress semantically active keyboard or touch input.
    if evaluate_controller_state(filtered.controllers.iter(), settings).is_neutral()
        && (filtered.keyboard_mouse.is_some() || !filtered.touches.is_empty())
    {
        filtered.controllers.clear();
    }
    filtered.touches.retain(|contact| {
        !quarantine.contains(&QuarantinedControl::TouchContact(contact.contact_id))
    });
    filtered
}

fn control_matches_mode(control: PhysicalControl, mode: InputMode) -> bool {
    matches!(
        (control, mode),
        (
            PhysicalControl::KeyboardUsage(_)
                | PhysicalControl::MouseButton(_)
                | PhysicalControl::MouseAxis(_),
            InputMode::KeyboardMouse
        ) | (
            PhysicalControl::GamepadButton(_) | PhysicalControl::GamepadAxis { .. },
            InputMode::GamePad
        ) | (PhysicalControl::TouchControl(_), InputMode::Touch)
    )
}

fn validate_activity(
    source_previous: Option<u64>,
    global_watermark: u64,
    actual: u64,
) -> Result<(), RouterError> {
    if let Some(previous) = source_previous {
        if actual < previous {
            return Err(RouterError::NonMonotonicActivitySequence { previous, actual });
        }
        if actual == previous {
            return Ok(());
        }
    }
    if actual <= global_watermark {
        return Err(RouterError::NonMonotonicActivitySequence {
            previous: global_watermark,
            actual,
        });
    }
    Ok(())
}

fn frame_activity_max(frame: &DeviceFrame) -> u64 {
    frame
        .keyboard_mouse
        .iter()
        .map(|sample| sample.activity_sequence)
        .chain(
            frame
                .controllers
                .iter()
                .map(|sample| sample.activity_sequence),
        )
        .chain(frame.touches.iter().map(|sample| sample.activity_sequence))
        .max()
        .unwrap_or(0)
}

fn physical_strength(
    chord: InputChord,
    frame: &DeviceFrame,
    controller_axes: [f32; 8],
    touch_layout: &TouchControlLayout,
) -> f32 {
    match chord.control {
        PhysicalControl::KeyboardUsage(code) => {
            frame.keyboard_mouse.as_ref().map_or(0.0, |sample| {
                (chord.modifiers.is_satisfied_by(sample.modifiers) && sample.keys.contains(&code))
                    as u8 as f32
            })
        }
        PhysicalControl::MouseButton(button) => {
            frame.keyboard_mouse.as_ref().map_or(0.0, |sample| {
                (chord.modifiers.is_satisfied_by(sample.modifiers)
                    && sample.mouse_buttons.contains(&button)) as u8 as f32
            })
        }
        PhysicalControl::MouseAxis(axis) => frame.keyboard_mouse.as_ref().map_or(0.0, |sample| {
            if chord.modifiers.is_satisfied_by(sample.modifiers) {
                directional_axis(
                    mouse_axis_value(sample.mouse_motion, axis),
                    axis_is_positive(axis),
                )
            } else {
                0.0
            }
        }),
        PhysicalControl::GamepadButton(button) => frame
            .controllers
            .iter()
            .any(|sample| sample.buttons.contains(&button))
            as u8 as f32,
        PhysicalControl::GamepadAxis { axis, direction } => directional_axis(
            controller_axes[axis as usize],
            direction == AxisDirection::Positive,
        ),
        PhysicalControl::TouchControl(hit_id) => {
            touch_control_strength(hit_id, frame, touch_layout)
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct EvaluatedControllerState {
    axes: [f32; 8],
    buttons: u32,
}

impl EvaluatedControllerState {
    fn is_neutral(self) -> bool {
        self.buttons == 0 && self.axes.iter().all(|axis| *axis == 0.0)
    }

    fn axis_family_is_active(self, axis: usize) -> bool {
        match axis {
            0 | 1 => self.axes[0] != 0.0 || self.axes[1] != 0.0,
            2 | 3 => self.axes[2] != 0.0 || self.axes[3] != 0.0,
            _ => self.axes[axis] != 0.0,
        }
    }
}

fn evaluate_controller_state<'a>(
    controllers: impl IntoIterator<Item = &'a crate::ControllerFrame>,
    settings: &ControlSettings,
) -> EvaluatedControllerState {
    // Sampling, mode eligibility, quarantine, and reconnect rearming all
    // consume this exact component-wise merge and radial-deadzone result.
    let mut axes = [0.0_f32; 8];
    let mut buttons = 0_u32;
    for controller in controllers {
        for button in &controller.buttons {
            if let Some(button) = 1_u32.checked_shl(u32::from(*button)) {
                buttons |= button;
            }
        }
        for (output, input) in axes.iter_mut().zip(controller.axes) {
            if input.abs() > output.abs() {
                *output = input;
            }
        }
    }
    let movement = radial_deadzone([axes[0], axes[1]], settings.gamepad_move_deadzone);
    let look = radial_deadzone([axes[2], axes[3]], settings.gamepad_look_deadzone);
    axes[0..2].copy_from_slice(&movement);
    axes[2..4].copy_from_slice(&look);
    for axis in &mut axes[4..] {
        *axis = axis.clamp(-1.0, 1.0);
    }
    EvaluatedControllerState { axes, buttons }
}
