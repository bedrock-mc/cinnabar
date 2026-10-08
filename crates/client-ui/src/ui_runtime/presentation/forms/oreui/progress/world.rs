use super::{Bounds, Canvas, LoadingStage, UiPresentationError, theme, widgets};
use crate::ui_runtime::oreui_assets::{OVERWORLD_BLOCK_IMAGE, dimensions};

const TERRAIN_TINT: [[u8; 4]; 2] = [[18, 20, 24, 224], [25, 27, 32, 224]];
const DIMENSION_TINT: [[u8; 4]; 2] = [[19, 20, 21, 246], [26, 27, 28, 244]];
const SCENERY_TINT: [[u8; 4]; 2] = [[12, 14, 18, 160], [10, 12, 15, 182]];

/// Pack backdrops remain visible under a quiet tint; connecting keeps its panorama treatment.
pub(super) fn backdrop(
    canvas: &mut Canvas<'_>,
    size: [f32; 2],
    stage: LoadingStage,
    destination: Option<i32>,
) -> Result<(), UiPresentationError> {
    if stage == LoadingStage::ChangingDimension
        && let Some(art) = destination.and_then(dimensions::destination)
        && canvas.cover_sprite(art.background, [0.0, 0.0, size[0], size[1]])?
    {
        return canvas.gradient(
            [0.0, 0.0, size[0], size[1]],
            SCENERY_TINT.map(|color| canvas.appearance.backdrop(color)),
        );
    }
    let colors = match stage {
        LoadingStage::Connecting => return Ok(()),
        LoadingStage::BuildingTerrain => TERRAIN_TINT,
        LoadingStage::ChangingDimension => DIMENSION_TINT,
    };
    canvas.gradient(
        [0.0, 0.0, size[0], size[1]],
        colors.map(|color| canvas.appearance.backdrop(color)),
    )
}

pub(super) fn panel(
    canvas: &mut Canvas<'_>,
    bounds: Bounds,
    stage: LoadingStage,
) -> Result<(), UiPresentationError> {
    if stage == LoadingStage::Connecting {
        return widgets::panel(canvas, bounds);
    }
    let shadow = canvas.r(0.4);
    canvas.fill(bounds.map(|v| v + shadow), [0, 0, 0, 70])?;
    widgets::panel(canvas, bounds)?;
    canvas.frame(bounds, theme::EDGE, [64, 66, 70, 255])
}

pub(super) fn has_destination_art(canvas: &Canvas<'_>, dimension: i32) -> bool {
    canvas.destination_icon.is_some()
        || dimension == 0
            && canvas
                .originals
                .is_some_and(|originals| originals.sprites.contains_key(OVERWORLD_BLOCK_IMAGE))
}

/// The native grass image and existing item atlas supply matching block silhouettes.
pub(super) fn destination(
    canvas: &mut Canvas<'_>,
    bounds: Bounds,
    dimension: i32,
) -> Result<(), UiPresentationError> {
    if dimension == 0 && canvas.fitted_sprite(OVERWORLD_BLOCK_IMAGE, bounds)? {
        return Ok(());
    }
    if let Some(icon) = canvas.destination_icon {
        let side = (bounds[2] - bounds[0]).min(bounds[3] - bounds[1]);
        let x = (bounds[0] + bounds[2] - side) * 0.5;
        let y = (bounds[1] + bounds[3] - side) * 0.5;
        canvas.icon_ref(icon, [x, y, x + side, y + side])?;
    }
    Ok(())
}
