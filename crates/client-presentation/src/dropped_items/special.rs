//! Admits dedicated item geometry from the currently installed block-entity artwork.

use super::*;
use render_model::block_entity::PreparedHeldBlockModels;

/// Keeps admitted session artwork ahead of stock models and keys every model by its source.
pub(super) fn model<'a>(
    icons: &UiPresentationRuntime,
    source: Option<&'a PreparedHeldBlockModels>,
    identifier: &str,
    metadata: u32,
) -> Option<(ModelKey, DroppedItemShape, &'a DroppedItemBlock)> {
    if icons.session_item_icon(identifier, metadata).is_some() {
        return None;
    }
    let source = source?;
    let (rig, shape, model) = source.dropped_model(identifier)?;
    Some((
        ModelKey::HeldBlock {
            artwork: source.identity(),
            rig,
        },
        shape,
        model,
    ))
}
