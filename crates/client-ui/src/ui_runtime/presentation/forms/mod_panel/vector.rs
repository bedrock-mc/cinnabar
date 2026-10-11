//! Generic two-axis pointer input backed by two existing bounded slider controls.
use super::*;
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
            .then(|| {
                frame.hits.iter().rev().find(|r| {
                    r.enabled
                        && r.contains(point)
                        && r.pressed.as_deref().is_some_and(|a| {
                            a.starts_with("mod.vector:") || a.starts_with("mod.axis:")
                        })
                })
            })
            .flatten()
            .and_then(|r| r.pressed.clone());
        if pressed {
            self.vector_drag = selected;
        }
        let action = self.vector_drag.clone()?;
        let Some(region) = frame
            .hits
            .iter()
            .find(|r| r.pressed.as_deref() == Some(&action))
        else {
            self.vector_drag = None;
            return Some(Vec::new());
        };
        let indices: [Option<usize>; 2] = if let Some(vector) = action.strip_prefix("mod.vector:") {
            let (x, y) = vector.split_once(':')?;
            [Some(x.parse().ok()?), Some(y.parse().ok()?)]
        } else if let Some(axis) = action.strip_prefix("mod.axis:") {
            let (index, axis) = axis.split_once(':')?;
            let index = Some(index.parse().ok()?);
            if axis == "y" {
                [None, index]
            } else {
                [index, None]
            }
        } else {
            self.vector_drag = None;
            return Some(Vec::new());
        };
        let fractions = [
            region.fraction_at(point[0]),
            if region.rect.h > 0. {
                ((point[1] - region.rect.y) / region.rect.h).clamp(0., 1.)
            } else {
                0.
            },
        ];
        let mut events = Vec::new();
        for (index, fraction) in indices.into_iter().zip(fractions) {
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
            let raw = *min + fraction as f32 * (*max - *min);
            let next = (*min + ((raw - *min) / *step).round() * *step).clamp(*min, *max);
            if *value != next {
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

#[cfg(test)]
mod tests {
    use super::super::super::tests::mini_engine_presentation;
    use super::super::tests::{frame, panel, point};
    use super::*;
    use serde_json::json;
    #[test]
    fn two_axis_capture_clamps_and_commits_release_without_resuming() {
        let mut spec = panel();
        spec.controls.push(Control::Slider {
            id: "vertical".into(),
            label: "Vertical".into(),
            value: 0.,
            min: 0.,
            max: 100.,
            step: 1.,
        });
        spec.surface = Some(ui::mod_panel::Surface {screen:"vector_test.root".into(),bindings:Default::default(),document:json!({"namespace":"vector_test","root":{"type":"panel","size":[200,150],"controls":[{"area":{"type":"button","size":[100,100],"button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.vector:1:4","mapping_type":"pressed"}]}}]}}).to_string()});
        let mut presentation = mini_engine_presentation();
        presentation.set_mod_panel(Some(&spec)).unwrap();
        presentation.set_mod_panel_open(true);
        frame(&mut presentation, [1280, 720]);
        let at = point(&presentation, "mod.vector:1:4", 0.25);
        let events = presentation.mod_panel_events(at, true, true);
        assert_eq!(
            events,
            vec![
                Event {
                    id: "strength".into(),
                    value: 25.
                },
                Event {
                    id: "vertical".into(),
                    value: 50.
                }
            ]
        );
        let events = presentation.mod_panel_events([20000., 20000.], false, false);
        assert_eq!(
            events,
            vec![
                Event {
                    id: "strength".into(),
                    value: 100.
                },
                Event {
                    id: "vertical".into(),
                    value: 100.
                }
            ]
        );
        assert!(presentation.mod_panel_events(at, false, true).is_empty());
    }
}
