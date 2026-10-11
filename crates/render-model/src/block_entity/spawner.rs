//! Spawner cage contents: the configured mob, small and slowly spinning.
//!
//! Scale, height and spin rate are provisional and need native measurement.

use std::sync::Arc;

use glam::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder, WHITE, model_matrix},
    mob::MobModels,
    scene::SceneClock,
};

const MOB_SCALE: f32 = 0.4;
const MOB_FLOOR_PIXELS: f32 = 3.0;
const SPIN_DEGREES_PER_TICK: f64 = 4.5;

#[derive(Clone, Debug, PartialEq)]
pub struct SpawnerModel {
    /// The spawned entity identifier, for example `minecraft:zombie`.
    pub mob: Arc<str>,
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    mobs: &MobModels,
    block: [i32; 3],
    model: &SpawnerModel,
    clock: SceneClock,
) {
    let Some(mob) = mobs.get(&model.mob) else {
        return;
    };
    let Some(texture) = atlas.texture(&mob.texture, mob.model.texture) else {
        return;
    };
    let spin = ((clock.ticks * SPIN_DEGREES_PER_TICK) % 360.0) as f32;
    let matrix = model_matrix(block, [0.5, 0.0, 0.5], spin)
        * Mat4::from_translation(Vec3::new(0.0, MOB_FLOOR_PIXELS, 0.0))
        * Mat4::from_scale(Vec3::splat(MOB_SCALE));
    for head_box in &mob.model.boxes {
        builder.cuboid(
            Layer::Solid,
            &texture,
            matrix * head_box.matrix,
            head_box.spec,
            WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_mobs_draw_nothing() {
        let mut builder = MeshBuilder::new([16, 16]);
        let atlas_assets =
            assets::encode_block_entity_catalog(b"{}", 4, 4, &[0u8; 4 * 4 * 4], &[]).unwrap();
        let atlas = BlockEntityAtlas::from_assets(
            &assets::RuntimeBlockEntityAssets::decode(&atlas_assets).unwrap(),
        );
        emit(
            &mut builder,
            &atlas,
            &MobModels::default(),
            [0; 3],
            &SpawnerModel {
                mob: Arc::from("minecraft:zombie"),
            },
            SceneClock::default(),
        );
        assert!(builder.solid.is_empty());
    }
}
