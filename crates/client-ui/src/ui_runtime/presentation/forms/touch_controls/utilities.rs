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
