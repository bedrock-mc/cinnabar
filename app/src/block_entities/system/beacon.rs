//! Beacon beams: colour sections from the compiled tints of the blocks above each beacon.

use client_interaction::movement::PhysicsCollisionRegistries;
use render_model::BeaconModel;

use super::{BlockEntityRuntime, block_info};

/// Builds an admitted beacon's sections for caching in its owning column scan.
#[allow(clippy::too_many_arguments)]
pub(super) fn model(
    runtime: &mut BlockEntityRuntime,
    collisions: &PhysicsCollisionRegistries,
    assets: &assets::RuntimeAssets,
    store: &world::ChunkStore,
    dimension: i32,
    mode: assets::NetworkIdMode,
    position: [i32; 3],
    top: i32,
) -> BeaconModel {
    let sections = assets::beacon::beacon_sections(position[1] + 1, top, |y| {
        let id = store
            .sub_chunk(world::SubChunkKey::new(
                dimension,
                position[0].div_euclid(16),
                y.div_euclid(16),
                position[2].div_euclid(16),
            ))
            .and_then(|sub| {
                sub.runtime_id(
                    0,
                    position[0].rem_euclid(16) as u8,
                    y.rem_euclid(16) as u8,
                    position[2].rem_euclid(16) as u8,
                )
            });
        let Some(id) = id else {
            return assets::beacon::BeaconMedium::Clear;
        };
        let tint = runtime.beacon_tints.get(mode, id);
        let info = block_info(assets, runtime, collisions, mode, id);
        assets::beacon::beacon_medium(
            info.as_ref().map_or("", |info| &info.name),
            tint,
            assets.resolve(mode, id).light_properties().filter(),
        )
    });
    BeaconModel {
        sections: sections.into(),
    }
}
