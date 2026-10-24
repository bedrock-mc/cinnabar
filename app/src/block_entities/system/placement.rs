//! Builds fixed item placements with compatible frame visuals.

use std::sync::Arc;

use super::{HeldItem, StaticItemPlacement};

/// Retains a frame's compiled block identity alongside its artwork and fixed pose.
pub(super) fn held_placement(
    item: &HeldItem,
    world_from_item: [[f32; 4]; 3],
    light: Option<(u8, u8)>,
    frame_stream: Option<&chunk_pipeline::WorldStream>,
) -> StaticItemPlacement {
    StaticItemPlacement {
        identifier: Arc::clone(&item.identifier),
        metadata: item.metadata,
        potted_block: None,
        carried_block: frame_stream.and_then(|stream| {
            match stream
                .authority()
                .named_item_visual(&item.identifier, item.metadata)
            {
                assets::ItemVisualRoute::BlockItem(visual) => Some(visual),
                _ => None,
            }
        }),
        world_from_item,
        light,
    }
}
