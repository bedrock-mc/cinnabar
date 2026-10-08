use world::{
    BLOCKS_PER_SUB_CHUNK, BlockPos, BoundaryLightSample, DimensionLightProfile, LightBlockAccess,
    LightBlockSample, LightBounds, LightChannel, LightProperties, LightReadAccess,
    LightSolveOutput, SUB_CHUNK_SIDE, SolverLimits,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    SkyTop,
    SkyAll,
    Solid,
    Terrain,
    Sparse,
    Dense,
    Removal,
    Halo,
}

impl Scene {
    pub const ALL: [Self; 8] = [
        Self::SkyTop,
        Self::SkyAll,
        Self::Solid,
        Self::Terrain,
        Self::Sparse,
        Self::Dense,
        Self::Removal,
        Self::Halo,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::SkyTop => "sky_top",
            Self::SkyAll => "sky_all_seeded",
            Self::Solid => "solid_dark",
            Self::Terrain => "terrain_caves_filters",
            Self::Sparse => "sparse_emitters",
            Self::Dense => "dense_emitters",
            Self::Removal => "removed_emitters",
            Self::Halo => "trusted_halo",
        }
    }
}

pub struct Fixture {
    pub scene: Scene,
    pub bounds: LightBounds,
    pub height: usize,
    pub samples: Vec<LightBlockSample>,
}

impl Fixture {
    /// Synthetic block facts are prepared before timing; no terrain generation is measured.
    pub fn new(scene: Scene, sections: usize) -> Self {
        let side = SUB_CHUNK_SIDE as i32;
        let height = sections * SUB_CHUNK_SIDE;
        let bounds = LightBounds::new(
            0,
            BlockPos::new(0, 0, 0),
            BlockPos::new(side - 1, height as i32 - 1, side - 1),
        )
        .unwrap();
        let mut samples = Vec::with_capacity(sections * BLOCKS_PER_SUB_CHUNK);
        for x in 0..side {
            for y in 0..height as i32 {
                for z in 0..side {
                    samples.push(sample(scene, x, y, z, height as i32));
                }
            }
        }
        Self {
            scene,
            bounds,
            height,
            samples,
        }
    }

    pub fn profile(&self) -> DimensionLightProfile {
        match self.scene {
            Scene::SkyTop | Scene::SkyAll | Scene::Terrain | Scene::Halo => {
                DimensionLightProfile::Overworld {
                    direct_sky_down: true,
                }
            }
            _ => DimensionLightProfile::Nether,
        }
    }

    pub fn limits(&self) -> SolverLimits {
        SolverLimits::new(self.samples.len(), self.samples.len() * 64)
    }

    pub fn positions(&self) -> impl Iterator<Item = BlockPos> + '_ {
        (0..SUB_CHUNK_SIDE as i32).flat_map(move |x| {
            (0..self.height as i32)
                .flat_map(move |y| (0..SUB_CHUNK_SIDE as i32).map(move |z| BlockPos::new(x, y, z)))
        })
    }
}

fn resident(emission: u8, filter: u8) -> LightBlockSample {
    LightBlockSample::Resident(LightProperties::new(emission, filter).unwrap())
}

fn sample(scene: Scene, x: i32, y: i32, z: i32, height: i32) -> LightBlockSample {
    let side = SUB_CHUNK_SIDE as i32;
    match scene {
        Scene::Solid => resident(0, 15),
        Scene::Sparse | Scene::Removal
            if x == side / 2 && z == side / 2 && y % (side * 4) == side / 2 =>
        {
            resident(15, 0)
        }
        Scene::Dense => match ((x * 3) ^ y ^ (z * 5)) & 31 {
            0 => resident(9, 2),
            1 => resident(15, 0),
            _ => LightBlockSample::KnownAir,
        },
        Scene::Terrain => {
            let ground = (height / 2 + (x * 7 + z * 11) % 7 - 3).max(1);
            if x == 0 && z == 0 && y < ground {
                return LightBlockSample::Unknown;
            }
            let cave = (side / 4..side / 2).contains(&(y % side)) && (x + z + y / side) % 5 != 0;
            if (y < ground && !cave) || (y == ground + 3 && x < side / 2) {
                resident(0, 15)
            } else if y == ground && z < side / 3 {
                resident(0, 3)
            } else if cave && x == side / 2 && z == side / 2 {
                resident(13, 0)
            } else {
                LightBlockSample::KnownAir
            }
        }
        _ => LightBlockSample::KnownAir,
    }
}

impl LightBlockAccess for Fixture {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        if self.bounds.contains(position) {
            let index = (position.x as usize * self.height + position.y as usize) * SUB_CHUNK_SIDE
                + position.z as usize;
            self.samples[index]
        } else if self.scene == Scene::Halo {
            LightBlockSample::KnownAir
        } else {
            LightBlockSample::Unknown
        }
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        match self.scene {
            Scene::SkyAll => 15,
            Scene::SkyTop | Scene::Terrain if position.y == self.bounds.max().y => 15,
            _ => 0,
        }
    }
}

pub struct Prior<'a> {
    pub output: Option<&'a LightSolveOutput>,
    pub fixture: &'a Fixture,
}

impl LightReadAccess for Prior<'_> {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        self.output
            .map_or(0, |output| output.read_light(dimension, position, channel))
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        self.output
            .is_some_and(|output| output.has_direct_sky_provenance(dimension, position))
    }

    fn boundary_light(
        &self,
        _dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        if self.fixture.scene != Scene::Halo {
            return BoundaryLightSample::unknown();
        }
        if channel == LightChannel::Sky && position.y == self.fixture.height as i32 {
            return BoundaryLightSample::trusted(15, true).unwrap();
        }
        if channel == LightChannel::Block && position.x == -1 {
            let distance = (position.y - self.fixture.height as i32 / 2).unsigned_abs()
                + (position.z - SUB_CHUNK_SIDE as i32 / 2).unsigned_abs();
            return BoundaryLightSample::trusted(15_u32.saturating_sub(distance) as u8, false)
                .unwrap();
        }
        BoundaryLightSample::unknown()
    }
}
