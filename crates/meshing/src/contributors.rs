use assets::{
    BlockFace, BlockFlags, ContributorRole, DIAGNOSTIC_MATERIAL, NO_MODEL_TEMPLATE, NetworkIdMode,
    RuntimeAssets, VisualKind,
};
use world::{PalettedStorage, SubChunk};

use crate::{BlockClassifier, Face, SIDE};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedPaletteEntry {
    pub(crate) network_value: u32,
    pub(crate) contributor_role: ContributorRole,
    pub(crate) flags: BlockFlags,
    pub(crate) faces: [u32; Face::ALL.len()],
    pub(crate) kind: VisualKind,
    pub(crate) model_template: u32,
    pub(crate) variant: u32,
}

impl ResolvedPaletteEntry {
    pub(crate) const AIR: Self = Self {
        network_value: 0,
        contributor_role: ContributorRole::Air,
        flags: BlockFlags::AIR,
        faces: [DIAGNOSTIC_MATERIAL; Face::ALL.len()],
        kind: VisualKind::Invisible,
        model_template: NO_MODEL_TEMPLATE,
        variant: 0,
    };
    const DIAGNOSTIC: Self = Self {
        network_value: 0,
        contributor_role: ContributorRole::Primary,
        flags: BlockFlags::empty(),
        faces: [DIAGNOSTIC_MATERIAL; Face::ALL.len()],
        kind: VisualKind::Diagnostic,
        model_template: NO_MODEL_TEMPLATE,
        variant: 0,
    };

    const fn air(network_value: u32) -> Self {
        Self {
            network_value,
            ..Self::AIR
        }
    }

    const fn diagnostic(network_value: u32) -> Self {
        Self {
            network_value,
            ..Self::DIAGNOSTIC
        }
    }

    pub(crate) const fn emits_cube_geometry(self) -> bool {
        self.flags.contains(BlockFlags::CUBE_GEOMETRY)
            || matches!(self.kind, VisualKind::Diagnostic)
    }
}

/// Palette-native contributors resolved for one sub-chunk coordinate.
///
/// A diagnostic is mutually exclusive with real contributors. Liquid geometry
/// is produced in Phase 2.6 Task 12, but its exact network value is retained
/// here so layered plants/solids do not erase it in the meantime.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolvedContributors {
    primary: Option<ResolvedPaletteEntry>,
    liquid: Option<ResolvedPaletteEntry>,
    diagnostic: Option<ResolvedPaletteEntry>,
}

impl ResolvedContributors {
    pub(crate) const fn primary_entry(self) -> Option<ResolvedPaletteEntry> {
        self.primary
    }

    pub(crate) const fn liquid_entry(self) -> Option<ResolvedPaletteEntry> {
        self.liquid
    }

    #[must_use]
    pub const fn primary_network_value(self) -> Option<u32> {
        match self.primary {
            Some(entry) => Some(entry.network_value),
            None => None,
        }
    }

    #[must_use]
    pub const fn liquid_network_value(self) -> Option<u32> {
        match self.liquid {
            Some(entry) => Some(entry.network_value),
            None => None,
        }
    }

    #[must_use]
    pub const fn diagnostic_network_value(self) -> Option<u32> {
        match self.diagnostic {
            Some(entry) => Some(entry.network_value),
            None => None,
        }
    }

    const fn is_empty(self) -> bool {
        self.primary.is_none() && self.liquid.is_none() && self.diagnostic.is_none()
    }

    pub(crate) const fn geometry_entry(self) -> ResolvedPaletteEntry {
        match (self.diagnostic, self.primary, self.liquid) {
            (Some(entry), _, _) | (None, Some(entry), _) => entry,
            (None, None, Some(_)) | (None, None, None) => ResolvedPaletteEntry::AIR,
        }
    }

    fn push(&mut self, entry: ResolvedPaletteEntry) {
        if self.diagnostic.is_some() || entry.flags.contains(BlockFlags::AIR) {
            return;
        }
        match entry.contributor_role {
            ContributorRole::Primary => {
                if self.primary.is_some() {
                    self.fail_closed(entry.network_value);
                } else {
                    self.primary = Some(entry);
                }
            }
            ContributorRole::LiquidAdditional if matches!(entry.kind, VisualKind::Liquid) => {
                if self
                    .liquid
                    .is_some_and(|liquid| liquid.network_value != entry.network_value)
                {
                    self.fail_closed(entry.network_value);
                } else if self.liquid.is_none() {
                    self.liquid = Some(entry);
                }
            }
            ContributorRole::LiquidAdditional | ContributorRole::Air => {
                self.fail_closed(entry.network_value);
            }
        }
    }

    fn fail_closed(&mut self, network_value: u32) {
        self.primary = None;
        self.liquid = None;
        self.diagnostic = Some(ResolvedPaletteEntry::diagnostic(network_value));
    }
}

pub(crate) struct StoragePaletteFacts<'a> {
    storage: &'a PalettedStorage,
    entries: Box<[ResolvedPaletteEntry]>,
}

pub(crate) enum PaletteSource<'a> {
    Air,
    Uniform(ResolvedContributors),
    Mixed(Box<[StoragePaletteFacts<'a>]>),
}

/// Block flags and six-face materials parallel to storage palettes, never to
/// the 4,096 voxel positions.
pub(crate) struct PaletteFacts<'a> {
    pub(crate) source: PaletteSource<'a>,
}

impl<'a> PaletteFacts<'a> {
    pub(crate) fn new(
        classifier: BlockClassifier,
        visuals: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
        sub_chunk: &'a SubChunk,
    ) -> Self {
        let mut contributors = ResolvedContributors::default();
        for storage in sub_chunk.storages() {
            match storage.uniform_runtime_id() {
                Some(network_value) => contributors.push(resolve_palette_entry(
                    classifier,
                    visuals,
                    network_id_mode,
                    network_value,
                )),
                None => return Self::mixed(classifier, visuals, network_id_mode, sub_chunk),
            }
        }

        if contributors.is_empty() {
            Self {
                source: PaletteSource::Air,
            }
        } else {
            Self {
                source: PaletteSource::Uniform(contributors),
            }
        }
    }

    fn mixed(
        classifier: BlockClassifier,
        visuals: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
        sub_chunk: &'a SubChunk,
    ) -> Self {
        let storages = sub_chunk
            .storages()
            .iter()
            .map(|storage| StoragePaletteFacts {
                storage,
                entries: storage
                    .palette()
                    .values()
                    .iter()
                    .copied()
                    .map(|network_value| {
                        resolve_palette_entry(classifier, visuals, network_id_mode, network_value)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            source: PaletteSource::Mixed(storages),
        }
    }

    pub(crate) const fn is_air(&self) -> bool {
        matches!(self.source, PaletteSource::Air)
    }

    fn contributors_at(&self, x: usize, y: usize, z: usize) -> ResolvedContributors {
        match &self.source {
            PaletteSource::Air => ResolvedContributors::default(),
            PaletteSource::Uniform(contributors) => *contributors,
            PaletteSource::Mixed(storages) => {
                let mut contributors = ResolvedContributors::default();
                for storage in storages {
                    let Some(index) = packed_palette_index(storage.storage, x, y, z) else {
                        contributors.fail_closed(0);
                        return contributors;
                    };
                    let Some(&entry) = storage.entries.get(index) else {
                        contributors.fail_closed(0);
                        return contributors;
                    };
                    contributors.push(entry);
                }
                contributors
            }
        }
    }

    pub(crate) fn at(&self, x: usize, y: usize, z: usize) -> ResolvedPaletteEntry {
        self.contributors_at(x, y, z).geometry_entry()
    }
}

/// Resolves all bounded storage layers without expanding them into a flat
/// 4,096-entry block array.
pub struct ContributorResolver<'a> {
    facts: PaletteFacts<'a>,
    palette_entry_count: usize,
}

impl<'a> ContributorResolver<'a> {
    #[must_use]
    pub fn new(
        classifier: BlockClassifier,
        visuals: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
        sub_chunk: &'a SubChunk,
    ) -> Self {
        Self {
            facts: PaletteFacts::new(classifier, visuals, network_id_mode, sub_chunk),
            palette_entry_count: sub_chunk
                .storages()
                .iter()
                .map(|storage| storage.palette().values().len())
                .sum(),
        }
    }

    /// Number of palette facts retained by this resolver. This is bounded by
    /// storage palette cardinality, never the 4,096 voxel coordinates.
    #[must_use]
    pub const fn palette_entry_count(&self) -> usize {
        self.palette_entry_count
    }

    #[must_use]
    pub fn resolve(&self, coordinate: [u8; 3]) -> ResolvedContributors {
        if coordinate
            .into_iter()
            .any(|coordinate| usize::from(coordinate) >= SIDE)
        {
            let mut contributors = ResolvedContributors::default();
            contributors.fail_closed(0);
            return contributors;
        }
        self.facts.contributors_at(
            usize::from(coordinate[0]),
            usize::from(coordinate[1]),
            usize::from(coordinate[2]),
        )
    }

    /// Resolves one coordinate directly from packed storage palettes without
    /// constructing the palette-fact cache. This path performs no heap
    /// allocation and is intended for sparse, per-frame queries such as the
    /// camera eye medium; full sub-chunk meshing should keep using `new`.
    #[must_use]
    pub fn resolve_direct(
        classifier: BlockClassifier,
        visuals: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
        sub_chunk: &SubChunk,
        coordinate: [u8; 3],
    ) -> ResolvedContributors {
        if coordinate
            .into_iter()
            .any(|coordinate| usize::from(coordinate) >= SIDE)
        {
            let mut contributors = ResolvedContributors::default();
            contributors.fail_closed(0);
            return contributors;
        }

        let mut contributors = ResolvedContributors::default();
        for layer in 0..sub_chunk.storages().len() {
            let Some(network_value) =
                sub_chunk.runtime_id(layer, coordinate[0], coordinate[1], coordinate[2])
            else {
                contributors.fail_closed(0);
                return contributors;
            };
            contributors.push(resolve_palette_entry(
                classifier,
                visuals,
                network_id_mode,
                network_value,
            ));
        }
        contributors
    }
}

fn resolve_palette_entry(
    classifier: BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    network_value: u32,
) -> ResolvedPaletteEntry {
    if classifier.is_air(network_value) {
        return ResolvedPaletteEntry::air(network_value);
    }

    let block = visuals.resolve(network_id_mode, network_value);
    let mut flags = block.flags();
    flags.remove(BlockFlags::AIR);
    let faces = if flags.contains(BlockFlags::CUBE_GEOMETRY)
        || matches!(block.kind(), VisualKind::Liquid | VisualKind::Model)
    {
        Face::ALL.map(|face| block.face(block_face(face)).material_id())
    } else {
        flags.remove(BlockFlags::LEAF_MODEL);
        if block.kind() != VisualKind::Model {
            flags.remove(BlockFlags::OCCLUDES_FULL_FACE);
        }
        [DIAGNOSTIC_MATERIAL; Face::ALL.len()]
    };
    ResolvedPaletteEntry {
        network_value,
        contributor_role: block.contributor_role(),
        flags,
        faces,
        kind: block.kind(),
        model_template: block.model_template().unwrap_or(NO_MODEL_TEMPLATE),
        variant: block.variant(),
    }
}

pub(crate) const fn pack_model_transform(local: [u8; 3], transform: u32) -> u32 {
    (local[0] as u32)
        | ((local[1] as u32) << 4)
        | ((local[2] as u32) << 8)
        | ((transform & 0x000f_ffff) << 12)
}

fn packed_palette_index(storage: &PalettedStorage, x: usize, y: usize, z: usize) -> Option<usize> {
    if x >= SIDE || y >= SIDE || z >= SIDE {
        return None;
    }
    if storage.bits_per_index() == 0 {
        return Some(0);
    }

    let linear = (x << 8) | (z << 4) | y;
    let bits = usize::from(storage.bits_per_index());
    let values_per_word = 32 / bits;
    let word = *storage.packed_words().get(linear / values_per_word)?;
    let shift = (linear % values_per_word) * bits;
    let mask = (1_u32 << storage.bits_per_index()) - 1;
    Some(((word >> shift) & mask) as usize)
}

const fn block_face(face: Face) -> BlockFace {
    match face {
        Face::NegativeX => BlockFace::West,
        Face::PositiveX => BlockFace::East,
        Face::NegativeY => BlockFace::Down,
        Face::PositiveY => BlockFace::Up,
        Face::NegativeZ => BlockFace::North,
        Face::PositiveZ => BlockFace::South,
    }
}
