//! Geometry primitives shared by the retained HUD builder.

use ui::{UiPoint, UiRect};

use super::UiPresentationError;

pub(super) fn rect(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> Result<UiRect, UiPresentationError> {
    UiRect::new(
        UiPoint::new(left, top).map_err(UiPresentationError::Geometry)?,
        UiPoint::new(right, bottom).map_err(UiPresentationError::Geometry)?,
    )
    .map_err(UiPresentationError::Geometry)
}
