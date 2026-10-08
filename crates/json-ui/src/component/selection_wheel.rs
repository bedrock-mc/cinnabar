//! Selection wheel: pointer sectors and component-managed state children, matching
//! vanilla 26.30 and the pinned UI definitions.

use std::collections::VecDeque;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::text_of;
use crate::tree::ResolvedControl;
use crate::widgets::bound_number;

/// Geometry and event names authored on a `selection_wheel` component.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionWheelMeta {
    pub slice_count: usize,
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub hovered_slice: Option<usize>,
    pub select_button: Option<String>,
    pub hover_button: Option<String>,
    pub analog_button: Option<String>,
}

impl SelectionWheelMeta {
    pub(crate) fn read(control: &ResolvedControl) -> Self {
        let slice_count = bound_number(control, "slice_count")
            .filter(|count| *count >= 1.0 && *count <= f64::from(u32::MAX) && count.fract() == 0.0)
            .map_or(1, |count| count as usize);
        let hovered_slice = bound_number(control, "#hover_slice")
            .filter(|slice| *slice >= 0.0 && *slice < slice_count as f64 && slice.fract() == 0.0)
            .map(|slice| slice as usize);
        Self {
            slice_count,
            inner_radius: bound_number(control, "inner_radius").unwrap_or(0.0),
            outer_radius: bound_number(control, "outer_radius").unwrap_or(1.0),
            hovered_slice,
            select_button: text_of(control, "select_button_name"),
            hover_button: text_of(control, "hover_button_name"),
            analog_button: text_of(control, "analog_button_name"),
        }
    }

    /// Slice at a normalized offset from the centre, with positive Y pointing
    /// down. Slice zero is centred above the wheel and subsequent slices run
    /// clockwise. The inner boundary is excluded and the outer one included.
    pub fn slice_at(&self, offset: [f64; 2]) -> Option<usize> {
        let [x, y] = offset;
        if self.slice_count == 0
            || !x.is_finite()
            || !y.is_finite()
            || !self.inner_radius.is_finite()
            || !self.outer_radius.is_finite()
            || self.inner_radius < 0.0
            || self.outer_radius <= self.inner_radius
        {
            return None;
        }
        let radius_squared = x * x + y * y;
        if radius_squared <= self.inner_radius * self.inner_radius
            || radius_squared > self.outer_radius * self.outer_radius
        {
            return None;
        }
        let width = TAU / self.slice_count as f64;
        let angle = (y.atan2(x) + FRAC_PI_2 + PI / self.slice_count as f64).rem_euclid(TAU);
        Some(((angle / width) as usize).min(self.slice_count - 1))
    }

    /// Pointer coordinates on the laid-out wheel. Native uses an inscribed
    /// circle with radius half the smaller side, even for a rectangular control.
    pub fn slice_at_rect(&self, rect: [f64; 4], point: [f64; 2]) -> Option<usize> {
        let [x, y, width, height] = rect;
        let radius = width.min(height) * 0.5;
        if !rect.into_iter().all(f64::is_finite) || radius <= 0.0 {
            return None;
        }
        self.slice_at([
            (point[0] - (x + width * 0.5)) / radius,
            (point[1] - (y + height * 0.5)) / radius,
        ])
    }
}

/// Native shows only `state_controls[hover + 1]`: a missing slice selects
/// entry zero, the default. The last visibility write wins for repeated targets.
pub(crate) fn visibility(control: &ResolvedControl) -> Vec<(&ResolvedControl, bool)> {
    let selected = SelectionWheelMeta::read(control)
        .hovered_slice
        .map_or(0, |slice| slice + 1);
    let Some(states) = control
        .properties
        .get("state_controls")
        .and_then(|value| value.as_array())
    else {
        return Vec::new();
    };
    let mut writes: Vec<(&ResolvedControl, bool)> = Vec::new();
    for (index, state) in states.iter().enumerate() {
        let Some(name) = state.get("control_name").and_then(|name| name.as_str()) else {
            continue;
        };
        let mut descendants: VecDeque<_> = control.children.iter().collect();
        let target = std::iter::from_fn(|| {
            let next = descendants.pop_front()?;
            descendants.extend(next.children.iter());
            Some(next)
        })
        .find(|child| child.name == name);
        let Some(target) = target else { continue };
        if let Some((_, shown)) = writes
            .iter_mut()
            .find(|(known, _)| std::ptr::eq(*known, target))
        {
            *shown = index == selected;
        } else {
            writes.push((target, index == selected));
        }
    }
    writes
}
