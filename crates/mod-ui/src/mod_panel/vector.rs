//! Generic two-axis pointer input backed by two existing bounded slider controls.
use super::*;

/// Captured axis routes retain their initial values for relative dragging.
#[derive(Clone)]
pub(super) struct Drag {
    key: String,
    action: String,
    indices: [Option<usize>; 2],
    origin: [f64; 2],
    values: Option<[f32; 2]>,
    extent: [f64; 2],
}

impl ModPanel {
    /// Captures authored vector rectangles and commits both stepped axes, including release.
    pub(super) fn vector_pointer(
        &mut self,
        point: [f64; 2],
        pressed: bool,
        held: bool,
    ) -> Option<Vec<Event>> {
        let frame = self.frame.as_ref()?;
        let selected = pressed
            .then(|| json_ui::hit_test(&frame.hits, point))
            .flatten()
            .filter(|region| region.pressed.as_deref().and_then(axes).is_some());
        if pressed {
            self.vector_drag = selected.and_then(|region| {
                let action = region.pressed.clone()?;
                let (indices, relative, override_extent) = axes(&action)?;
                let values = if relative {
                    let mut values = [0.0; 2];
                    for (axis, index) in indices.into_iter().enumerate() {
                        if let Some(index) = index {
                            let Some(Control::Slider { value, .. }) =
                                self.panel.controls.get(index)
                            else {
                                return None;
                            };
                            values[axis] = *value;
                        }
                    }
                    Some(values)
                } else {
                    None
                };
                let mut extent = self
                    .panel
                    .reference_size
                    .map(|size| size.map(f64::from))
                    .unwrap_or(self.viewport);
                if let Some(override_extent) = override_extent {
                    let override_extent =
                        override_extent.resolve(&self.panel.surface.as_ref()?.bindings)?;
                    extent = [override_extent; 2];
                }
                if action.starts_with("mod.drag:") {
                    let rect = json_ui::Rect::new(
                        region.rect.x,
                        region.rect.y,
                        region.rect.w,
                        region.rect.h,
                    );
                    let bounds = region
                        .transform
                        .map_or(rect, |transform| transform.bounds(rect));
                    extent = [extent[0] - bounds.w, extent[1] - bounds.h]
                        .map(|available| available.max(0.0));
                }
                Some(Drag {
                    key: region.key.clone(),
                    action,
                    indices,
                    origin: point,
                    values,
                    extent,
                })
            });
        }
        let drag = self.vector_drag.clone()?;
        let action = &drag.action;
        let Some(region) = frame.hits.iter().find(|region| {
            region.key == drag.key && region.pressed.as_deref() == Some(action.as_str())
        }) else {
            self.vector_drag = None;
            return Some(Vec::new());
        };
        let local = region.local(point);
        let fractions = [
            region.fraction_at(local[0]),
            if region.rect.h > 0. {
                ((local[1] - region.rect.y) / region.rect.h).clamp(0., 1.)
            } else {
                0.
            },
        ];
        let mut events = Vec::new();
        for (axis, (index, fraction)) in drag.indices.into_iter().zip(fractions).enumerate() {
            let Some(index) = index else {
                continue;
            };
            let Some(Control::Slider {
                id,
                value,
                min,
                max,
                step,
                ..
            }) = self.panel.controls.get_mut(index)
            else {
                continue;
            };
            let raw = if let Some(initial) = drag.values {
                let delta = if drag.extent[axis] > 0.0 {
                    ((point[axis] - drag.origin[axis]) / drag.extent[axis]) as f32
                } else {
                    0.0
                };
                initial[axis] + delta * (*max - *min)
            } else {
                *min + fraction as f32 * (*max - *min)
            };
            let next = if pressed && drag.values.is_some() {
                *value
            } else {
                (*min + ((raw - *min) / *step).round() * *step).clamp(*min, *max)
            };
            if *value != next || pressed && drag.values.is_some() {
                *value = next;
                events.push(Event {
                    id: id.clone(),
                    value: next,
                });
            }
        }
        if !held {
            self.vector_drag = None;
        }
        self.refresh_data();
        Some(events)
    }
}

/// Decodes the shared absolute/relative paired and single-axis pointer routes.
fn axes(
    action: &str,
) -> Option<(
    [Option<usize>; 2],
    bool,
    Option<ui::mod_panel::AxisExtent<'_>>,
)> {
    if let Some(route) = action
        .strip_prefix("mod.vector:")
        .or_else(|| action.strip_prefix("mod.drag:"))
    {
        let (x, y) = route.split_once(':')?;
        Some((
            [Some(x.parse().ok()?), Some(y.parse().ok()?)],
            action.starts_with("mod.drag:"),
            None,
        ))
    } else {
        let axis = ui::mod_panel::AxisRoute::parse(action)?;
        let mut indices = [None; 2];
        indices[axis.axis] = Some(axis.control);
        Some((indices, axis.relative, axis.extent))
    }
}
