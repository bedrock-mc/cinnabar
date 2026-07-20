use super::super::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::stream) struct PendingLight {
    pub(in crate::stream) revision: u64,
    pub(in crate::stream) queued_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) struct LightJobIdentity {
    pub(in crate::stream) revision: u64,
    pub(in crate::stream) block_generation: u64,
    pub(in crate::stream) previous_light_generation: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) struct LightOwnership {
    pub(in crate::stream) block_generation: u64,
    pub(in crate::stream) light_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) struct LightFailure {
    pub(in crate::stream) revision: u64,
    pub(in crate::stream) block_generation: u64,
    pub(in crate::stream) error: LightJobError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::stream) enum DirectSkyMask {
    Uniform(bool),
    Packed(Box<[u64; 64]>),
}

impl DirectSkyMask {
    pub(in crate::stream) fn from_output(output: &LightSolveOutput, key: SubChunkKey) -> Self {
        let mut words = Box::new([0_u64; 64]);
        let mut count = 0_usize;
        for x in 0_u8..16 {
            for z in 0_u8..16 {
                for y in 0_u8..16 {
                    let position = BlockPos::new(
                        key.x.saturating_mul(16).saturating_add(i32::from(x)),
                        key.y.saturating_mul(16).saturating_add(i32::from(y)),
                        key.z.saturating_mul(16).saturating_add(i32::from(z)),
                    );
                    if output.has_direct_sky_provenance(key.dimension, position) {
                        let index = light_local_index(x, y, z);
                        words[index / 64] |= 1_u64 << (index % 64);
                        count += 1;
                    }
                }
            }
        }
        match count {
            0 => Self::Uniform(false),
            4_096 => Self::Uniform(true),
            _ => Self::Packed(words),
        }
    }

    pub(in crate::stream) fn get(&self, x: u8, y: u8, z: u8) -> bool {
        match self {
            Self::Uniform(value) => *value,
            Self::Packed(words) => {
                let index = light_local_index(x, y, z);
                words[index / 64] & (1_u64 << (index % 64)) != 0
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::stream) struct StoredDirectSky {
    pub(in crate::stream) light_revision: u64,
    pub(in crate::stream) mask: Arc<DirectSkyMask>,
}

#[derive(Debug, Clone)]
pub(in crate::stream) enum SnapshotBlock {
    KnownAir,
    Resident(Arc<SubChunk>),
}

#[derive(Clone)]
pub(in crate::stream) struct LightBlockSnapshot {
    pub(in crate::stream) dimension: i32,
    pub(in crate::stream) blocks: BTreeMap<SubChunkKey, SnapshotBlock>,
    pub(in crate::stream) classifier: BlockClassifier,
    pub(in crate::stream) network_id_mode: NetworkIdMode,
    pub(in crate::stream) runtime_assets: Arc<RuntimeAssets>,
    pub(in crate::stream) resolved_light: HashMap<u32, SolverLightProperties>,
    pub(in crate::stream) profile: DimensionLightProfile,
    pub(in crate::stream) overworld_top_y: Option<i32>,
}

impl LightBlockSnapshot {
    pub(in crate::stream) fn resolve_palette_light(&mut self) {
        for block in self.blocks.values() {
            let SnapshotBlock::Resident(sub_chunk) = block else {
                continue;
            };
            for storage in sub_chunk.storages() {
                for &runtime_id in storage.palette().values() {
                    if self.classifier.is_air(runtime_id) {
                        continue;
                    }
                    self.resolved_light.entry(runtime_id).or_insert_with(|| {
                        let properties = self
                            .runtime_assets
                            .resolve(self.network_id_mode, runtime_id)
                            .light_properties();
                        SolverLightProperties::new(properties.emission(), properties.filter())
                            .expect("MCBEAS05 light nibbles are validated")
                    });
                }
            }
        }
    }
}

impl LightBlockAccess for LightBlockSnapshot {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        let (key, [x, y, z]) = split_light_position(self.dimension, position);
        match self.blocks.get(&key) {
            None => LightBlockSample::Unknown,
            Some(SnapshotBlock::KnownAir) => LightBlockSample::KnownAir,
            Some(SnapshotBlock::Resident(sub_chunk)) => {
                let mut emission = 0_u8;
                let mut filter = 0_u8;
                let mut found = false;
                for layer in 0..sub_chunk.storages().len() {
                    let Some(runtime_id) = sub_chunk.runtime_id(layer, x, y, z) else {
                        continue;
                    };
                    if self.classifier.is_air(runtime_id) {
                        continue;
                    }
                    let properties = self
                        .resolved_light
                        .get(&runtime_id)
                        .copied()
                        .unwrap_or_else(|| {
                            SolverLightProperties::new(0, 15).expect("constant nibbles are valid")
                        });
                    emission = emission.max(properties.emission());
                    filter = filter.max(properties.filter());
                    found = true;
                }
                if found {
                    LightBlockSample::Resident(
                        SolverLightProperties::new(emission, filter)
                            .expect("MCBEAS05 light nibbles are validated"),
                    )
                } else {
                    LightBlockSample::KnownAir
                }
            }
        }
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        if self.overworld_top_y == Some(position.y)
            && matches!(self.profile, DimensionLightProfile::Overworld { .. })
            && self.sample(position) == LightBlockSample::KnownAir
        {
            15
        } else {
            0
        }
    }
}

#[derive(Clone)]
pub(in crate::stream) struct LightPriorSnapshot {
    pub(in crate::stream) light: LightStoreSnapshot,
    pub(in crate::stream) direct_sky: BTreeMap<SubChunkKey, StoredDirectSky>,
    pub(in crate::stream) trusted_boundaries: BTreeSet<SubChunkKey>,
}

impl LightReadAccess for LightPriorSnapshot {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        let (key, [x, y, z]) = split_light_position(dimension, position);
        self.light
            .light(key)
            .and_then(|light| light.get(channel, x, y, z))
            .unwrap_or(0)
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        let (key, [x, y, z]) = split_light_position(dimension, position);
        let Some(light) = self.light.light(key) else {
            return false;
        };
        self.direct_sky.get(&key).is_some_and(|direct| {
            direct.light_revision == light.generation() && direct.mask.get(x, y, z)
        })
    }

    fn boundary_light(
        &self,
        dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        let (key, [x, y, z]) = split_light_position(dimension, position);
        if !self.trusted_boundaries.contains(&key) {
            return if self.light.kind(key) == LightSubChunkKind::Unknown {
                BoundaryLightSample::unknown()
            } else {
                BoundaryLightSample::untrusted()
            };
        }
        let Some(light) = self.light.light(key) else {
            return BoundaryLightSample::unknown();
        };
        BoundaryLightSample::trusted(
            light.get(channel, x, y, z).unwrap_or(0),
            channel == LightChannel::Sky
                && self.direct_sky.get(&key).is_some_and(|direct| {
                    direct.light_revision == light.generation() && direct.mask.get(x, y, z)
                }),
        )
        .expect("stored light is nibble-bounded")
    }
}

#[derive(Debug)]
pub(in crate::stream) struct LightCompletion {
    pub(in crate::stream) key: SubChunkKey,
    pub(in crate::stream) identity: LightJobIdentity,
    pub(in crate::stream) result: Result<SolvedLightJob, LightJobError>,
    pub(in crate::stream) queue_wait: Duration,
    pub(in crate::stream) duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) enum LightJobError {
    Solve(LightSolveError),
    MissingTargetOutput,
}

#[derive(Debug)]
pub(in crate::stream) struct SolvedLightJob {
    pub(in crate::stream) replacement: SubChunkLight,
    pub(in crate::stream) direct_sky: Arc<DirectSkyMask>,
    pub(in crate::stream) used_uniform_fast_path: bool,
    pub(in crate::stream) light_levels_changed: bool,
    pub(in crate::stream) direct_sky_changed: bool,
    pub(in crate::stream) changed_faces: [bool; 6],
}

pub(in crate::stream) struct PreparedLightJob {
    pub(in crate::stream) key: SubChunkKey,
    pub(in crate::stream) identity: LightJobIdentity,
    pub(in crate::stream) blocks: LightBlockSnapshot,
    pub(in crate::stream) prior: LightPriorSnapshot,
    pub(in crate::stream) bounds: LightBounds,
    pub(in crate::stream) queued_at: Instant,
}

pub(in crate::stream) fn solve_prepared_light_job(
    mut job: PreparedLightJob,
) -> Result<SolvedLightJob, LightJobError> {
    let old_light = job.prior.light.light(job.key).cloned();
    let old_direct = job
        .prior
        .direct_sky
        .get(&job.key)
        .cloned()
        .filter(|direct| {
            old_light
                .as_deref()
                .is_some_and(|light| direct.light_revision == light.generation())
        });
    let (replacement, direct_sky, used_uniform_fast_path) =
        if let Some((light, direct)) = uniform_known_air_light(&job) {
            (light, Arc::new(direct), true)
        } else {
            job.blocks.resolve_palette_light();
            let profile = job.blocks.profile;
            let output = solve_light(
                &job.blocks,
                &job.prior,
                job.bounds,
                job.identity.revision,
                profile,
                LIGHT_SOLVE_LIMITS,
            )
            .map_err(LightJobError::Solve)?;
            let light = output
                .sub_chunks()
                .get(&job.key)
                .map(|light| light.as_ref().clone())
                .ok_or(LightJobError::MissingTargetOutput)?;
            let direct = Arc::new(DirectSkyMask::from_output(&output, job.key));
            (light, direct, false)
        };
    let light_levels_changed = old_light
        .as_deref()
        .is_none_or(|previous| !light_levels_equal(previous, &replacement));
    let direct_sky_changed = old_direct
        .as_ref()
        .is_none_or(|previous| previous.mask.as_ref() != direct_sky.as_ref());
    let changed_faces = std::array::from_fn(|index| {
        light_face_changed(
            old_light.as_deref(),
            &replacement,
            old_direct.as_ref().map(|direct| direct.mask.as_ref()),
            direct_sky.as_ref(),
            LIGHT_NEIGHBOUR_OFFSETS[index],
        )
    });
    Ok(SolvedLightJob {
        replacement,
        direct_sky,
        used_uniform_fast_path,
        light_levels_changed,
        direct_sky_changed,
        changed_faces,
    })
}

pub(in crate::stream) fn uniform_known_air_light(
    job: &PreparedLightJob,
) -> Option<(SubChunkLight, DirectSkyMask)> {
    if !matches!(
        job.blocks.blocks.get(&job.key),
        Some(SnapshotBlock::KnownAir)
    ) {
        return None;
    }
    let trusted_zero = BoundaryLightSample::trusted(0, false).ok()?;
    for offset in LIGHT_NEIGHBOUR_OFFSETS {
        let neighbour = offset_sub_chunk_key(job.key, offset)?;
        if !job.prior.trusted_boundaries.contains(&neighbour) {
            continue;
        }
        for a in 0_u8..16 {
            for b in 0_u8..16 {
                let position = light_boundary_position(job.key, offset, a, b)?;
                let sample =
                    job.prior
                        .boundary_light(job.key.dimension, position, LightChannel::Block);
                if sample != BoundaryLightSample::unknown()
                    && sample != BoundaryLightSample::untrusted()
                    && sample != trusted_zero
                {
                    return None;
                }
            }
        }
    }

    let (sky, direct) = match job.blocks.profile {
        DimensionLightProfile::Nether | DimensionLightProfile::End => (0, false),
        DimensionLightProfile::Overworld { .. }
            if job.blocks.overworld_top_y == job.key.y.checked_mul(16)?.checked_add(15) =>
        {
            (15, true)
        }
        DimensionLightProfile::Overworld { .. } => {
            let direct_above = (0_u8..16).all(|x| {
                (0_u8..16).all(|z| {
                    let Some(position) = light_boundary_position(job.key, [0, 1, 0], x, z) else {
                        return false;
                    };
                    job.prior
                        .boundary_light(job.key.dimension, position, LightChannel::Sky)
                        == BoundaryLightSample::trusted(15, true)
                            .expect("constant sky nibble is valid")
                })
            });
            if direct_above {
                (15, true)
            } else {
                for offset in LIGHT_NEIGHBOUR_OFFSETS {
                    let neighbour = offset_sub_chunk_key(job.key, offset)?;
                    if !job.prior.trusted_boundaries.contains(&neighbour) {
                        continue;
                    }
                    for a in 0_u8..16 {
                        for b in 0_u8..16 {
                            let position = light_boundary_position(job.key, offset, a, b)?;
                            let sample = job.prior.boundary_light(
                                job.key.dimension,
                                position,
                                LightChannel::Sky,
                            );
                            if sample != BoundaryLightSample::unknown()
                                && sample != BoundaryLightSample::untrusted()
                                && sample != trusted_zero
                            {
                                return None;
                            }
                        }
                    }
                }
                (0, false)
            }
        }
    };
    Some((
        SubChunkLight::uniform(0, sky, job.identity.revision)
            .expect("constant light nibbles are valid"),
        DirectSkyMask::Uniform(direct),
    ))
}

pub(in crate::stream) fn light_boundary_position(
    key: SubChunkKey,
    offset: [i32; 3],
    a: u8,
    b: u8,
) -> Option<BlockPos> {
    let base_x = key.x.checked_mul(16)?;
    let base_y = key.y.checked_mul(16)?;
    let base_z = key.z.checked_mul(16)?;
    let a = i32::from(a);
    let b = i32::from(b);
    let position = match offset {
        [-1, 0, 0] => [
            base_x.checked_sub(1)?,
            base_y.checked_add(a)?,
            base_z.checked_add(b)?,
        ],
        [1, 0, 0] => [
            base_x.checked_add(16)?,
            base_y.checked_add(a)?,
            base_z.checked_add(b)?,
        ],
        [0, -1, 0] => [
            base_x.checked_add(a)?,
            base_y.checked_sub(1)?,
            base_z.checked_add(b)?,
        ],
        [0, 1, 0] => [
            base_x.checked_add(a)?,
            base_y.checked_add(16)?,
            base_z.checked_add(b)?,
        ],
        [0, 0, -1] => [
            base_x.checked_add(a)?,
            base_y.checked_add(b)?,
            base_z.checked_sub(1)?,
        ],
        [0, 0, 1] => [
            base_x.checked_add(a)?,
            base_y.checked_add(b)?,
            base_z.checked_add(16)?,
        ],
        _ => return None,
    };
    Some(BlockPos::new(position[0], position[1], position[2]))
}

pub(in crate::stream) const LIGHT_NEIGHBOUR_OFFSETS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

pub(in crate::stream) fn light_local_index(x: u8, y: u8, z: u8) -> usize {
    (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y)
}

pub(in crate::stream) fn split_light_position(
    dimension: i32,
    position: BlockPos,
) -> (SubChunkKey, [u8; 3]) {
    (
        SubChunkKey::new(
            dimension,
            position.x.div_euclid(16),
            position.y.div_euclid(16),
            position.z.div_euclid(16),
        ),
        [
            position.x.rem_euclid(16) as u8,
            position.y.rem_euclid(16) as u8,
            position.z.rem_euclid(16) as u8,
        ],
    )
}
