use crate::chunk::*;

pub const MATERIAL_UV_ROTATE_90: u32 = 1;
pub const MATERIAL_UV_ROTATE_180: u32 = 2;
pub const MATERIAL_UV_ROTATE_270: u32 = 3;
pub const MATERIAL_UV_REFLECT_U: u32 = 1 << 2;
pub const MATERIAL_UV_REFLECT_V: u32 = 1 << 3;
pub(in crate::chunk) const MATERIAL_UV_ROTATION_MASK: u32 = 0b11;

/// Linear-space tint colours resolved from one live biome definition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiomeTint {
    pub grass: [f32; 3],
    pub foliage: [f32; 3],
    pub birch: [f32; 3],
    pub evergreen: [f32; 3],
    pub dry_foliage: [f32; 3],
    pub water: [f32; 3],
    pub flags: u32,
}

impl Default for BiomeTint {
    fn default() -> Self {
        Self {
            grass: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            foliage: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            birch: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            evergreen: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            dry_foliage: [0.191_201_69, 0.527_115_1, 0.102_241_73],
            water: [1.0; 3],
            flags: 0,
        }
    }
}

#[derive(Resource, Clone)]
pub struct ChunkBiomeTints {
    pub(in crate::chunk) entries: Arc<[BiomeTint]>,
    pub(in crate::chunk) identity: ChunkBiomeTintIdentity,
}

impl Default for ChunkBiomeTints {
    fn default() -> Self {
        Self {
            entries: Arc::from([BiomeTint::default()]),
            identity: ChunkBiomeTintIdentity::default(),
        }
    }
}

impl ChunkBiomeTints {
    #[must_use]
    pub fn from_resolved(resolved: &ResolvedBiomeTints, revision: u64) -> Self {
        Self::from_resolved_with_identity(resolved, ChunkBiomeTintIdentity::new(0, revision))
    }

    #[must_use]
    pub fn from_resolved_with_identity(
        resolved: &ResolvedBiomeTints,
        identity: ChunkBiomeTintIdentity,
    ) -> Self {
        let entries = resolved
            .records
            .iter()
            .map(|record| BiomeTint {
                grass: record.grass[..3].try_into().expect("three grass channels"),
                foliage: record.foliage[..3]
                    .try_into()
                    .expect("three foliage channels"),
                birch: record.birch[..3].try_into().expect("three birch channels"),
                evergreen: record.evergreen[..3]
                    .try_into()
                    .expect("three evergreen channels"),
                dry_foliage: record.dry_foliage[..3]
                    .try_into()
                    .expect("three dry foliage channels"),
                water: record.water[..3].try_into().expect("three water channels"),
                flags: record.flags,
            })
            .collect::<Vec<_>>();
        Self::with_identity(Arc::from(entries), identity)
    }

    /// Replaces tint colours while retaining the dense index contract used by
    /// queued [`PackedBiomeRecord`] palettes. Callers that change index
    /// assignments must enqueue replacement records with the same revision.
    #[must_use]
    pub fn with_revision(entries: Arc<[BiomeTint]>, revision: u64) -> Self {
        Self::with_identity(entries, ChunkBiomeTintIdentity::new(0, revision))
    }

    #[must_use]
    pub fn with_identity(entries: Arc<[BiomeTint]>, identity: ChunkBiomeTintIdentity) -> Self {
        let entries = if entries.is_empty() {
            Arc::from([BiomeTint::default()])
        } else {
            entries
        };
        Self { entries, identity }
    }

    #[must_use]
    pub fn entries(&self) -> &[BiomeTint] {
        &self.entries
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.identity.revision()
    }

    #[must_use]
    pub const fn table_identity(&self) -> ChunkBiomeTintIdentity {
        self.identity
    }

    pub(in crate::chunk) fn resource_identity(&self) -> ChunkBiomeTintResourceIdentity {
        ChunkBiomeTintResourceIdentity {
            pointer: Arc::as_ptr(&self.entries) as *const BiomeTint as usize,
            table: self.identity,
        }
    }
}

impl bevy::render::extract_resource::ExtractResource for ChunkBiomeTints {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct ChunkBiomeTintResourceIdentity {
    pub(in crate::chunk) pointer: usize,
    pub(in crate::chunk) table: ChunkBiomeTintIdentity,
}
