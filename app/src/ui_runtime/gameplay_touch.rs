use std::sync::Once;

use bevy::{
    input::touch::Touches,
    prelude::{Res, ResMut},
};

use super::UiRuntime;
use crate::semantic_controls::SemanticTouchTargets;

/// Production has no version-matched native Bedrock touch geometry yet.
///
/// Keeping this false is intentional: assigning inferred rectangles here would
/// make the default touch bindings appear reachable without authoritative
/// layout, scale, or DPI evidence.
pub(crate) const PRODUCTION_TOUCH_LAYOUT_AVAILABLE: bool = false;

static TOUCH_LAYOUT_DIAGNOSTIC: Once = Once::new();

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GameplayTouchSample;

#[cfg(test)]
impl GameplayTouchSample {
    #[must_use]
    pub(crate) const fn new(_contact_id: u64, _position: [f32; 2], _delta: [f32; 2]) -> Self {
        Self
    }
}

#[cfg(test)]
pub(crate) fn reconcile_gameplay_touch_targets(
    targets: &mut SemanticTouchTargets,
    _samples: &[GameplayTouchSample],
) {
    targets.release_all();
}

pub(crate) fn drive_gameplay_touch_targets(
    touches: Res<Touches>,
    _ui: Res<UiRuntime>,
    mut targets: ResMut<SemanticTouchTargets>,
) {
    targets.release_all();
    if touches.iter().next().is_some() {
        TOUCH_LAYOUT_DIAGNOSTIC.call_once(|| {
            bevy::log::warn!(
                layout_available = PRODUCTION_TOUCH_LAYOUT_AVAILABLE,
                "gameplay touch controls are unavailable: native Bedrock layout/scale/DPI authority is not yet established",
            );
        });
    }
}
