use assets::{BlockFlags, SeasonalFoliageBlock, seasonal_foliage_cell_shelters};
use chunk_pipeline::WorldStream;
use particles::{Fluid, ParticleWorld};
use sim::{Aabb, BlockPhysicsFlags, CollisionRegistry, CollisionWorld, PaletteWorld, Vec3};
use world::SubChunkKey;

/// Live-session world access for particle collision, lighting and fluid checks.
pub(super) struct StreamParticleWorld<'a> {
    stream: &'a WorldStream,
    registry: &'a CollisionRegistry,
}

impl<'a> StreamParticleWorld<'a> {
    pub(super) fn new(stream: &'a WorldStream, registry: &'a CollisionRegistry) -> Self {
        Self { stream, registry }
    }

    fn palette(&self) -> PaletteWorld<'a> {
        PaletteWorld::new(
            self.stream.collision_store(),
            self.registry,
            self.stream.current_dimension(),
        )
    }

    /// The network runtime id at a block cell, when that chunk is loaded.
    pub(super) fn block_runtime_id(&self, block: [i32; 3]) -> Option<u32> {
        self.palette().primary_runtime_id(block).ok()
    }

    /// Native colour shelter uses the un-reordered main/extra storage pair,
    /// not collision contributors or the below-block material admission.
    pub(super) fn seasonal_cell_shelters(&self, [x, y, z]: [i32; 3]) -> bool {
        let store = self.stream.collision_store();
        let key = SubChunkKey::new(self.stream.current_dimension(), x >> 4, y >> 4, z >> 4);
        if !store.is_sub_chunk_loaded(key) {
            return seasonal_foliage_cell_shelters(None, None);
        }
        let Some(sub_chunk) = store.sub_chunk(key) else {
            return seasonal_foliage_cell_shelters(Some(SeasonalFoliageBlock::AIR), None);
        };
        let mode = self.stream.network_id_mode();
        let assets = self.stream.runtime_assets();
        let at = |layer| {
            sub_chunk.runtime_id(
                layer,
                x.rem_euclid(16) as u8,
                y.rem_euclid(16) as u8,
                z.rem_euclid(16) as u8,
            )
        };
        // Native has one extra layer. Additional non-air server layers remain
        // conservative rather than becoming an implicit exposed palette route.
        if (2..sub_chunk.storages().len()).any(|layer| {
            at(layer).is_none_or(|id| !assets.resolve(mode, id).flags().contains(BlockFlags::AIR))
        }) {
            return true;
        }
        let main = at(0).map_or(SeasonalFoliageBlock::AIR, |id| {
            assets.resolve(mode, id).into()
        });
        let extra = at(1).map(|id| assets.resolve(mode, id).into());
        seasonal_foliage_cell_shelters(Some(main), extra)
    }
}

impl ParticleWorld for StreamParticleWorld<'_> {
    fn solid_boxes(&self, min: [f32; 3], max: [f32; 3], out: &mut Vec<[f32; 6]>) {
        let query = Aabb::new(
            Vec3::new(f64::from(min[0]), f64::from(min[1]), f64::from(min[2])),
            Vec3::new(f64::from(max[0]), f64::from(max[1]), f64::from(max[2])),
        );
        if let Ok(found) = self.palette().collision_boxes_camera_lenient(query) {
            out.extend(found.value.iter().map(|shape| {
                [
                    shape.min.x as f32,
                    shape.min.y as f32,
                    shape.min.z as f32,
                    shape.max.x as f32,
                    shape.max.y as f32,
                    shape.max.z as f32,
                ]
            }));
        }
    }

    fn light(&self, block: [i32; 3]) -> (u8, u8) {
        self.stream.light_level_at(block.map(|c| c as f32 + 0.5))
    }

    fn fluid(&self, block: [i32; 3]) -> Fluid {
        match self.palette().block_physics(block) {
            Ok(sample) => {
                let flags = sample.primary().flags;
                if flags.contains(BlockPhysicsFlags::WATER) {
                    Fluid::Water
                } else if flags.contains(BlockPhysicsFlags::LAVA) {
                    Fluid::Lava
                } else {
                    Fluid::None
                }
            }
            Err(_) => Fluid::None,
        }
    }
}
