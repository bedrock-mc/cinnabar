//! Normalizers for block-side packets: map pixel updates, sign editor requests and block events.

use bedrock_protocol_1_26_51::packets::{BlockEvent, ClientboundMapItemData, OpenSign};

use super::events::{BlockEventEvent, MAP_IMAGE_SIDE, MapDataEvent, OpenSignEvent};

/// The pixel rectangle of a map update; `None` for a partial packet (no pixels) or a rectangle
/// outside the fixed image or with the wrong pixel count, which are semantic skips.
pub(super) fn normalize_map_data(packet: &ClientboundMapItemData) -> Option<MapDataEvent> {
    let (Some(width), Some(height), Some(start_x), Some(start_y), Some(pixels)) = (
        packet.width.map(|value| value.0),
        packet.height.map(|value| value.0),
        packet.start_x.map(|value| value.0),
        packet.start_y.map(|value| value.0),
        packet.pixels.as_deref(),
    ) else {
        return None;
    };
    let (Ok(width), Ok(height), Ok(start_x), Ok(start_y)) = (
        u32::try_from(width),
        u32::try_from(height),
        u32::try_from(start_x),
        u32::try_from(start_y),
    ) else {
        return None;
    };
    let fits = width != 0
        && height != 0
        && width.saturating_add(start_x) <= MAP_IMAGE_SIDE
        && height.saturating_add(start_y) <= MAP_IMAGE_SIDE
        && pixels.len() as u64 == u64::from(width) * u64::from(height);
    fits.then(|| MapDataEvent {
        map_id: packet.map_id.0,
        start_x,
        start_y,
        width,
        height,
        pixels: pixels
            .iter()
            .map(|pixel| {
                let [blue, green, red, alpha] = pixel.0.to_le_bytes();
                u32::from_le_bytes([red, green, blue, alpha])
            })
            .collect(),
    })
}

pub(super) fn normalize_open_sign(packet: &OpenSign, dimension: i32) -> OpenSignEvent {
    OpenSignEvent {
        dimension,
        position: [packet.pos.x.0, packet.pos.y.0, packet.pos.z.0],
        front: packet.is_front_side,
    }
}

pub(super) fn normalize_block_event(packet: &BlockEvent, dimension: i32) -> BlockEventEvent {
    BlockEventEvent {
        dimension,
        position: [
            packet.block_position.x.0,
            packet.block_position.y.0,
            packet.block_position.z.0,
        ],
        event_type: packet.event_type.0,
        event_value: packet.event_value.0,
    }
}
