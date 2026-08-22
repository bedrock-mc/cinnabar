use crate::{
    Action, DeviceFrame, MAX_LOOK_DELTA_PER_FRAME, MouseAxis, TouchAxis, TouchControlKind,
    TouchControlLayout,
};

pub(crate) fn touch_control_strength(
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

pub(crate) fn mouse_axis_value(motion: [f32; 2], axis: MouseAxis) -> f32 {
    match axis {
        MouseAxis::XPositive | MouseAxis::XNegative => motion[0],
        MouseAxis::YPositive | MouseAxis::YNegative => motion[1],
    }
}

pub(crate) fn axis_is_positive(axis: MouseAxis) -> bool {
    matches!(axis, MouseAxis::XPositive | MouseAxis::YPositive)
}

pub(crate) fn directional_axis(value: f32, positive: bool) -> f32 {
    if positive {
        value.max(0.0)
    } else {
        (-value).max(0.0)
    }
}

pub(crate) fn radial_deadzone(value: [f32; 2], deadzone: f32) -> [f32; 2] {
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

pub(crate) fn merged_touch_movement(frame: &DeviceFrame) -> [f32; 2] {
    let mut movement = [0.0_f32; 2];
    for contact in frame
        .touches
        .iter()
        .filter(|contact| contact.hit_id.is_none())
    {
        if contact.position[0] <= 0.5 && contact.position[1] >= 0.5 {
            let candidate = [
                (contact.position[0] - 0.25) * 4.0,
                (0.75 - contact.position[1]) * 4.0,
            ];
            if candidate[0].hypot(candidate[1]) > movement[0].hypot(movement[1]) {
                movement = candidate;
            }
        }
    }
    clamp_vector(movement, 1.0)
}

pub(crate) fn synthesize_directions(
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

pub(crate) fn clamp_vector(value: [f32; 2], maximum: f32) -> [f32; 2] {
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

pub(crate) fn scale_look_axis(value: f32, sensitivity: f32) -> f32 {
    value.clamp(
        -MAX_LOOK_DELTA_PER_FRAME / sensitivity,
        MAX_LOOK_DELTA_PER_FRAME / sensitivity,
    ) * sensitivity
}
