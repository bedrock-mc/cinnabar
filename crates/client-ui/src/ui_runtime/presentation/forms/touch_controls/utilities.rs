//! Touch utility menu state and placement within the safe viewport.
use super::*;

impl UiPresentationRuntime {
    /// Opens or dismisses the utility dropdown without capturing gameplay fingers.
    pub fn toggle_touch_utilities(&mut self) {
        self.screens.touch.utilities_open = !self.screens.touch.utilities_open;
    }

    /// Dismisses the dropdown when gameplay loses input or a menu choice is selected.
    pub fn close_touch_utilities(&mut self) {
        self.screens.touch.utilities_open = false;
    }

    /// Finds the largest vertical roster region clear of placed utility controls.
    pub(in super::super) fn touch_roster_region(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<std::ops::Range<f32>, UiPresentationError> {
        if !self.screens.touch.enabled {
            return Ok(0. ..content[1]);
        }
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        if self.screens.touch.base_viewport != Some((content, px, self.safe_area)) {
            let rollback = (nodes.len(), *next);
            self.append_touch_controls(runtime, nodes, next, metrics, content)?;
            nodes.truncate(rollback.0);
            *next = rollback.1;
        }
        let state = &self.screens.touch;
        let mut view = ViewState::default();
        place_controls(state, &mut view, content, px);
        let mut occupied = Vec::new();
        if let Some(frame) = &state.base {
            for hit in &frame.hits {
                if !matches!(
                    control_id(hit.pressed.as_deref()),
                    Some(touch::UTILITIES | touch::CHAT | touch::MENU)
                ) {
                    continue;
                }
                let dy = view.drags.get(&hit.key).map_or(0., |shift| shift[1]);
                let top = ((hit.rect.y + dy) as f32 * frame.scale).clamp(0., content[1]);
                let bottom =
                    ((hit.rect.y + dy + hit.rect.h) as f32 * frame.scale).clamp(top, content[1]);
                occupied.push(top..bottom);
            }
        }
        occupied.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut clear = 0. ..0.;
        let mut cursor: f32 = 0.;
        for band in occupied
            .into_iter()
            .chain(std::iter::once(content[1]..content[1]))
        {
            if band.start - cursor > clear.end - clear.start {
                clear = cursor..band.start;
            }
            cursor = cursor.max(band.end);
        }
        Ok(clear)
    }

    /// Requests the same debug visibility toggle as the desktop binding.
    pub fn request_touch_debug_toggle(&mut self) {
        self.screens.touch.debug_toggle_pending ^= true;
    }

    /// Consumes pending touch requests once, preserving the parity of repeated toggles.
    pub fn take_touch_debug_toggle(&mut self) -> bool {
        std::mem::take(&mut self.screens.touch.debug_toggle_pending)
    }
}

/// Applies the same saved or draft control positions to painting and roster avoidance.
pub(super) fn place_controls(
    state: &TouchControls,
    view: &mut ViewState,
    content: [f32; 2],
    px: f32,
) {
    let saved = &state.saved;
    if let Some(base) = &state.base {
        for hit in base
            .hits
            .iter()
            .filter(|hit| control_id(hit.pressed.as_deref()).is_some_and(placeable_control))
        {
            let name = hit
                .pressed
                .as_deref()
                .unwrap()
                .strip_prefix("cinnabar.touch.")
                .unwrap();
            let desired = if let Some(editor) = &state.editor {
                editor
                    .rectangles()
                    .find(|(id, _)| *id == name)
                    .map(|(_, r)| [r[0], r[1]])
            } else {
                saved.position(name).map(|position| {
                    [
                        f64::from(position[0]) * (f64::from(content[0] / px) - hit.rect.w).max(0.),
                        f64::from(position[1]) * (f64::from(content[1] / px) - hit.rect.h).max(0.),
                    ]
                })
            };
            if let Some(at) = desired {
                view.drags
                    .insert(hit.key.clone(), [at[0] - hit.rect.x, at[1] - hit.rect.y]);
            }
        }
    }
}

/// Places the dropdown below its movable trigger, or above it when space is short.
pub(super) fn place_dropdown(
    base: Option<&EngineFrame>,
    view: &mut ViewState,
    content: [f32; 2],
    px: f32,
) {
    let Some(base) = base else {
        return;
    };
    let Some(trigger) = base
        .hits
        .iter()
        .find(|hit| control_id(hit.pressed.as_deref()) == Some(touch::UTILITIES))
    else {
        return;
    };
    let Some(first) = base
        .hits
        .iter()
        .find(|hit| control_id(hit.pressed.as_deref()) == Some(touch::DEBUG_OVERLAY))
    else {
        return;
    };
    let Some(last) = base
        .hits
        .iter()
        .find(|hit| control_id(hit.pressed.as_deref()) == Some(touch::EMOTE))
    else {
        return;
    };
    let shift = view.drags.get(&trigger.key).copied().unwrap_or_default();
    let viewport = content.map(|size| f64::from(size / px));
    let width = first.rect.w;
    let height = last.rect.y + last.rect.h - first.rect.y;
    let x = (trigger.rect.x + shift[0]).clamp(0., (viewport[0] - width).max(0.));
    let below = trigger.rect.y + shift[1] + trigger.rect.h + 2.;
    let y = if below + height <= viewport[1] {
        below
    } else {
        (trigger.rect.y + shift[1] - height - 2.).max(0.)
    };
    let Some(dropdown) = base
        .hits
        .iter()
        .find(|hit| hit.name == "utilities_dropdown")
    else {
        return;
    };
    // A stack's children are packed; translate the whole surface.
    view.drags
        .insert(dropdown.key.clone(), [x - first.rect.x, y - first.rect.y]);
}
