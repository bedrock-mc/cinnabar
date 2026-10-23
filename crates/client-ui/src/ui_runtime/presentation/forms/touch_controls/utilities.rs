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

    /// Measures the authored utility row before a roster's first paint or viewport change.
    pub(in super::super) fn touch_utility_row_bottom(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<f32, UiPresentationError> {
        if !self.screens.touch.enabled {
            return Ok(0.);
        }
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        if self.screens.touch.base_viewport != Some((content, px, self.safe_area)) {
            let rollback = (nodes.len(), *next);
            self.append_touch_controls(runtime, nodes, next, metrics, content)?;
            nodes.truncate(rollback.0);
            *next = rollback.1;
        }
        Ok(self.screens.touch.base.as_ref().map_or(0., |frame| {
            frame
                .hits
                .iter()
                .filter(|hit| {
                    matches!(
                        control_id(hit.pressed.as_deref()),
                        Some(touch::UTILITIES | touch::CHAT | touch::MENU)
                    )
                })
                .map(|hit| (hit.rect.y + hit.rect.h) as f32 * frame.scale)
                .fold(0., f32::max)
        }))
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
