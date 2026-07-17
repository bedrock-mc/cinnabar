use core::num::NonZeroU64;

use crate::{
    Action, ActionPhase, ActionSnapshot, AxisDirection, BindingError, ControlSettings, DeviceFrame,
    FrameError, InputChord, InputContext, InputMode, MouseAxis, PhysicalControl, ReleaseReason,
    TouchAxis, TouchControlKind, TouchControlLayout,
};

/// Maximum Euclidean magnitude accepted for a semantic look delta.
pub const MAX_LOOK_DELTA_PER_FRAME: f32 = 2048.0;

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
        let input_mode = self.selected_input_mode(frame).0;
        let sample = self.sample(frame, input_mode);
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
        self.queue_held_releases(ReleaseReason::BindingChanged);
        self.settings = settings;
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
        let frame = self
            .pending
            .take()
            .ok_or(RouterError::MissingPendingFrame)?;

        if frame.window_focus_lost {
            self.queue_held_releases(ReleaseReason::WindowFocusLost);
        }
        if !frame.disconnected_controllers.is_empty() && self.input_mode == InputMode::GamePad {
            self.queue_held_releases(ReleaseReason::ControllerDisconnected);
        }

        let (input_mode, activity_sequence) = self.selected_input_mode(&frame);
        let sample = self.sample(&frame, input_mode);
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
                    is_down && (!was_down || authority_release)
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
        self.pending_context = None;
        self.frame_sequence = next_sequence;
        self.input_mode = input_mode;
        self.input_activity_sequence = activity_sequence;
        self.activity_watermark = self.activity_watermark.max(frame_activity_max(&frame));
        self.previous_frame = frame;
        if let Some(generation) = self.pending_authority.take() {
            self.authority_generation = generation;
        }

        Ok(ActionSnapshot {
            frame_sequence: self.frame_sequence,
            authority_generation: self.authority_generation,
            movement: sample.movement,
            look_delta: sample.look_delta,
            input_mode: self.input_mode,
            phases,
            release_reasons,
        })
    }

    fn queue_held_releases(&mut self, reason: ReleaseReason) {
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
        let controller_axes = merged_controller_axes(frame, &self.settings);
        let touch_movement = merged_touch_movement(frame);
        let previous_controller_axes = merged_controller_axes(&self.previous_frame, &self.settings);
        let mut strengths = [0.0_f32; Action::COUNT];
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
            strengths[binding.action as usize] = strengths[binding.action as usize].max(strength);
            if strength > 0.0
                && physical_strength(
                    binding.chord,
                    &self.previous_frame,
                    previous_controller_axes,
                    &self.touch_layout,
                ) == 0.0
                && !self.edge_claimed_by_routed_context(binding.chord, frame, controller_axes)
            {
                pressed[binding.action as usize] = true;
            }
        }

        let mut movement = [
            strengths[Action::MoveRight as usize] - strengths[Action::MoveLeft as usize],
            strengths[Action::MoveForward as usize] - strengths[Action::MoveBackward as usize],
        ];
        if input_mode == InputMode::Touch && self.context == InputContext::Gameplay {
            movement = touch_movement;
            synthesize_directions(
                &mut strengths,
                movement,
                Action::MoveLeft,
                Action::MoveRight,
                Action::MoveBackward,
                Action::MoveForward,
            );
        }
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

#[derive(Clone, Copy)]
struct Sample {
    movement: [f32; 2],
    look_delta: [f32; 2],
    active: [bool; Action::COUNT],
    pressed: [bool; Action::COUNT],
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

fn touch_control_strength(
    hit_id: u16,
    frame: &DeviceFrame,
    touch_layout: &TouchControlLayout,
) -> f32 {
    let Some(control) = touch_layout.control(hit_id) else {
        return 0.0;
    };
    match control.kind {
        TouchControlKind::Button => frame
            .touches
            .iter()
            .any(|contact| contact.hit_id == Some(hit_id)) as u8
            as f32,
        TouchControlKind::LookAxis(axis) => frame
            .touches
            .iter()
            .filter(|contact| contact.hit_id == Some(hit_id))
            .map(|contact| touch_axis_strength(contact.delta, axis))
            .sum::<f32>()
            .clamp(0.0, MAX_LOOK_DELTA_PER_FRAME),
    }
}

fn touch_axis_strength(delta: [f32; 2], axis: TouchAxis) -> f32 {
    let value = match axis {
        TouchAxis::XPositive | TouchAxis::XNegative => delta[0],
        TouchAxis::YPositive | TouchAxis::YNegative => delta[1],
    }
    .clamp(-1.0, 1.0)
        * MAX_LOOK_DELTA_PER_FRAME;
    directional_axis(
        value,
        matches!(axis, TouchAxis::XPositive | TouchAxis::YPositive),
    )
}

fn mouse_axis_value(motion: [f32; 2], axis: MouseAxis) -> f32 {
    match axis {
        MouseAxis::XPositive | MouseAxis::XNegative => motion[0],
        MouseAxis::YPositive | MouseAxis::YNegative => motion[1],
    }
}

fn axis_is_positive(axis: MouseAxis) -> bool {
    matches!(axis, MouseAxis::XPositive | MouseAxis::YPositive)
}

fn directional_axis(value: f32, positive: bool) -> f32 {
    if positive {
        value.max(0.0)
    } else {
        (-value).max(0.0)
    }
}

fn merged_controller_axes(frame: &DeviceFrame, settings: &ControlSettings) -> [f32; 8] {
    let mut axes = [0.0_f32; 8];
    for controller in &frame.controllers {
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
    axes
}

fn radial_deadzone(value: [f32; 2], deadzone: f32) -> [f32; 2] {
    let value = [value[0].clamp(-1.0, 1.0), value[1].clamp(-1.0, 1.0)];
    let magnitude = value[0].hypot(value[1]);
    if magnitude <= deadzone || magnitude == 0.0 {
        return [0.0, 0.0];
    }
    let clamped = magnitude.min(1.0);
    let remapped = (clamped - deadzone) / (1.0 - deadzone);
    [
        value[0] / magnitude * remapped,
        value[1] / magnitude * remapped,
    ]
}

fn merged_touch_movement(frame: &DeviceFrame) -> [f32; 2] {
    let mut movement = [0.0_f32; 2];
    for contact in frame
        .touches
        .iter()
        .filter(|contact| contact.hit_id.is_none())
    {
        if contact.position[0] <= 0.5 && contact.position[1] <= 0.5 {
            let candidate = [
                (contact.position[0] - 0.25) * 4.0,
                (contact.position[1] - 0.25) * 4.0,
            ];
            if candidate[0].hypot(candidate[1]) > movement[0].hypot(movement[1]) {
                movement = candidate;
            }
        }
    }
    clamp_vector(movement, 1.0)
}

fn synthesize_directions(
    strengths: &mut [f32; Action::COUNT],
    value: [f32; 2],
    negative_x: Action,
    positive_x: Action,
    negative_y: Action,
    positive_y: Action,
) {
    strengths[negative_x as usize] = strengths[negative_x as usize].max((-value[0]).max(0.0));
    strengths[positive_x as usize] = strengths[positive_x as usize].max(value[0].max(0.0));
    strengths[negative_y as usize] = strengths[negative_y as usize].max((-value[1]).max(0.0));
    strengths[positive_y as usize] = strengths[positive_y as usize].max(value[1].max(0.0));
}

fn clamp_vector(value: [f32; 2], maximum: f32) -> [f32; 2] {
    let magnitude = value[0].hypot(value[1]);
    if magnitude > maximum {
        [
            value[0] / magnitude * maximum,
            value[1] / magnitude * maximum,
        ]
    } else {
        value
    }
}

fn scale_look_axis(value: f32, sensitivity: f32) -> f32 {
    value.clamp(
        -MAX_LOOK_DELTA_PER_FRAME / sensitivity,
        MAX_LOOK_DELTA_PER_FRAME / sensitivity,
    ) * sensitivity
}
