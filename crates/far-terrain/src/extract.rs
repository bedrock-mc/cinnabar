//! Finds each cell's visible top in already-decoded column data.

use std::{collections::BTreeMap, sync::Arc};

use assets::{
    BLOCK_VISUAL_VARIANT_MATERIAL_MASK, BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF,
    BLOCK_VISUAL_VARIANT_SEASONAL_LEAF, BLOCK_VISUAL_VARIANT_TOP_SNOW, BlockFace, BlockFlags,
    LinearBiomeTints, NetworkIdMode, ResolvedBiomeTints, RuntimeAssets,
    SEASONAL_LEAF_EXPOSED_OFFSET, VisualKind,
};
use world::{BLOCKS_PER_SUB_CHUNK, BiomeStorage, Chunk, SubChunk};

use crate::{
    ColumnSummary, SurfaceCell, SurfaceColours, SurfaceSlice,
    config::{MAX_WATER_SCAN_DEPTH, WATER_DEPTH_TRANSMITTANCE, WATER_SURFACE_OPACITY},
    summary::{COLUMN_CELLS, COLUMN_SIDE},
};

/// Block layers that can contribute to a surface, including water above a base layer.
pub(crate) const SURFACE_LAYERS: usize = 2;

/// Shared, immutable sections and biomes of one column, taken without copying block data.
#[derive(Clone, Debug)]
pub struct ColumnSnapshot {
    pub(crate) sections: Arc<BTreeMap<i32, Arc<SubChunk>>>,
    pub(crate) biomes: Vec<(i32, Arc<BiomeStorage>)>,
}

impl ColumnSnapshot {
    /// Sections keyed by absolute sub-chunk Y, with the biome storage of any of them.
    #[must_use]
    pub fn new(
        sections: Arc<BTreeMap<i32, Arc<SubChunk>>>,
        biomes: Vec<(i32, Arc<BiomeStorage>)>,
    ) -> Self {
        Self { sections, biomes }
    }

    /// Captures shared terrain and biome allocations without copying their storage.
    #[must_use]
    pub fn of(chunk: &Chunk) -> Self {
        let sections = chunk.shared_sub_chunks();
        let biomes = sections
            .keys()
            .filter_map(|&y| Some((y, chunk.biome_storage(y)?)))
            .collect();
        Self { sections, biomes }
    }

    /// Returns the captured biome storage for one absolute section.
    pub(crate) fn biome(&self, y: i32) -> Option<&BiomeStorage> {
        self.biomes
            .iter()
            .find(|(section, _)| *section == y)
            .map(|(_, storage)| storage.as_ref())
    }
}

/// Session registries a summary resolves blocks, materials and biomes against.
#[derive(Clone)]
pub struct SurfaceContext {
    /// The session's air runtime ID, which resolves as unknown in diagnostic assets.
    pub air: u32,
    pub assets: Arc<RuntimeAssets>,
    pub mode: NetworkIdMode,
    pub colours: Arc<SurfaceColours>,
    pub tints: Arc<ResolvedBiomeTints>,
    pub default_biome: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    /// Air, plants and thin decorations, seen through from above.
    Through,
    Solid(Option<u32>),
    Water(u32),
}

#[derive(Clone, Copy)]
enum Scan {
    Searching,
    Water { top: i16, rgb: [f32; 3], depth: u8 },
    Done,
}

impl SurfaceContext {
    fn classify(&self, runtime_id: u32) -> Class {
        if runtime_id == self.air {
            return Class::Through;
        }
        let block = self.assets.resolve(self.mode, runtime_id);
        if !block.is_known() {
            return Class::Solid(None);
        }
        if block.flags().contains(BlockFlags::AIR) {
            return Class::Through;
        }
        let variant = block.variant();
        let top = if variant
            & (BLOCK_VISUAL_VARIANT_SEASONAL_LEAF | BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF)
            != 0
        {
            // A surface leaf has open sky above it, so seasonal ones take their exposed
            // material, as the terrain mesher picks for sky-exposed leaves.
            let exposed = if variant & BLOCK_VISUAL_VARIANT_SEASONAL_LEAF != 0 {
                SEASONAL_LEAF_EXPOSED_OFFSET
            } else {
                0
            };
            (variant & BLOCK_VISUAL_VARIANT_MATERIAL_MASK) + BlockFace::Up as u32 + exposed
        } else {
            block.face(BlockFace::Up).material_id()
        };
        match block.kind() {
            VisualKind::Invisible | VisualKind::Cross => Class::Through,
            VisualKind::Liquid if self.colours.is_water(top) => Class::Water(top),
            VisualKind::Model if variant & BLOCK_VISUAL_VARIANT_TOP_SNOW == 0 => {
                match block.model_template() {
                    Some(template) if self.colours.model_covers_top(template) => Class::Solid(
                        Some(self.colours.model_top_material(template).unwrap_or(top)),
                    ),
                    _ => Class::Through,
                }
            }
            _ => Class::Solid(Some(top)),
        }
    }

    fn tints(
        &self,
        biome: Option<&BiomeStorage>,
        x: u8,
        y: u8,
        z: u8,
    ) -> Option<&LinearBiomeTints> {
        let raw = biome
            .and_then(|storage| storage.biome_id(x, y, z))
            .unwrap_or(self.default_biome);
        self.tints.records.get(self.tints.dense_index(raw) as usize)
    }
}

/// Summarises the highest visible block of every cell, looking through water to its floor.
#[must_use]
pub fn summarise_column(context: &SurfaceContext, column: &ColumnSnapshot) -> ColumnSummary {
    let mut scan = SurfaceScan::default();
    for (&y, section) in column.sections.iter().rev() {
        if scan.is_complete() {
            break;
        }
        scan.section(context, y, section.storages(), column.biome(y));
    }
    scan.finish()
}

/// Resumable top-down extraction; water is finalised only after the last section.
pub struct SurfaceScan {
    summary: ColumnSummary,
    scans: [Scan; COLUMN_CELLS],
    remaining: usize,
    indices: [[u32; BLOCKS_PER_SUB_CHUNK]; SURFACE_LAYERS],
}

impl Default for SurfaceScan {
    fn default() -> Self {
        Self {
            summary: ColumnSummary::default(),
            scans: [Scan::Searching; COLUMN_CELLS],
            remaining: COLUMN_CELLS,
            indices: [[0; BLOCKS_PER_SUB_CHUNK]; SURFACE_LAYERS],
        }
    }
}

impl SurfaceScan {
    /// True when deeper sections cannot affect any cell.
    pub fn is_complete(&self) -> bool {
        self.remaining == 0
    }

    /// Applies a bounded, owned slice in descending section order.
    pub fn apply(&mut self, context: &SurfaceContext, slice: &SurfaceSlice) {
        for section in slice.sections.iter() {
            if self.is_complete() {
                break;
            }
            self.section(
                context,
                section.y,
                &section.storages,
                section.biome.as_ref(),
            );
        }
    }

    /// Applies one section using the same classification and scan order as full extraction.
    fn section(
        &mut self,
        context: &SurfaceContext,
        section_y: i32,
        storages: &[world::PalettedStorage],
        biome: Option<&BiomeStorage>,
    ) {
        let classes: [Vec<Class>; SURFACE_LAYERS] = std::array::from_fn(|layer| {
            storages.get(layer).map_or_else(Vec::new, |storage| {
                storage
                    .palette()
                    .values()
                    .iter()
                    .map(|&id| context.classify(id))
                    .collect()
            })
        });
        let sees_through = |layer: &Vec<Class>| layer.iter().all(|class| *class == Class::Through);
        if sees_through(&classes[0]) && sees_through(&classes[1]) {
            return;
        }
        for (layer, storage) in storages.iter().take(SURFACE_LAYERS).enumerate() {
            storage.palette_indices(&mut self.indices[layer]);
        }
        let class_at = |layer: usize, linear: usize| {
            classes[layer]
                .get(self.indices[layer][linear] as usize)
                .copied()
                .unwrap_or(Class::Through)
        };
        for (index, scan) in self.scans.iter_mut().enumerate() {
            if matches!(scan, Scan::Done) {
                continue;
            }
            let (x, z) = (index % COLUMN_SIDE, index / COLUMN_SIDE);
            for local_y in (0..16_u8).rev() {
                let linear = (x << 8) | (z << 4) | usize::from(local_y);
                // A plant or other see-through block on layer 0 can hold water or snow below it.
                let class = match class_at(0, linear) {
                    Class::Through => class_at(1, linear),
                    class => class,
                };
                let top = (section_y * 16 + i32::from(local_y) + 1) as i16;
                let tints = || context.tints(biome, x as u8, local_y, z as u8);
                let finished = match (*scan, class) {
                    (_, Class::Through) | (Scan::Done, _) => None,
                    (Scan::Searching, Class::Solid(material)) => Some(SurfaceCell {
                        height: top,
                        rgb: to_srgb8(solid_rgb(context, material, tints())),
                        water_depth: 0,
                    }),
                    (Scan::Searching, Class::Water(material)) => {
                        *scan = Scan::Water {
                            top,
                            rgb: context.colours.tinted(material, tints()),
                            depth: 1,
                        };
                        None
                    }
                    (Scan::Water { top, rgb, depth }, Class::Water(_)) => {
                        if depth >= MAX_WATER_SCAN_DEPTH {
                            Some(open_water(top, rgb, depth))
                        } else {
                            *scan = Scan::Water {
                                top,
                                rgb,
                                depth: depth + 1,
                            };
                            None
                        }
                    }
                    (Scan::Water { top, rgb, depth }, Class::Solid(material)) => {
                        let floor = solid_rgb(context, material, tints());
                        let opacity = water_opacity(depth);
                        Some(SurfaceCell {
                            height: top,
                            rgb: to_srgb8(std::array::from_fn(|channel| {
                                rgb[channel] * opacity + floor[channel] * (1.0 - opacity)
                            })),
                            water_depth: depth,
                        })
                    }
                };
                if let Some(cell) = finished {
                    self.summary.cells_mut()[index] = cell;
                    *scan = Scan::Done;
                    self.remaining -= 1;
                    break;
                }
            }
        }
    }

    /// Finishes water with no visible floor after the source has been exhausted.
    pub fn finish(mut self) -> ColumnSummary {
        for (index, scan) in self.scans.into_iter().enumerate() {
            if let Scan::Water { top, rgb, depth } = scan {
                self.summary.cells_mut()[index] = open_water(top, rgb, depth);
            }
        }
        self.summary
    }
}

fn solid_rgb(
    context: &SurfaceContext,
    material: Option<u32>,
    tints: Option<&LinearBiomeTints>,
) -> [f32; 3] {
    match material {
        Some(material) => context.colours.tinted(material, tints),
        None => crate::config::UNKNOWN_SURFACE_RGB.map(|value| f32::from(value) / 255.0),
    }
}

fn open_water(top: i16, rgb: [f32; 3], depth: u8) -> SurfaceCell {
    SurfaceCell {
        height: top,
        rgb: to_srgb8(rgb),
        water_depth: depth,
    }
}

/// Share of the water colour over the floor after `depth` blocks.
fn water_opacity(depth: u8) -> f32 {
    1.0 - (1.0 - WATER_SURFACE_OPACITY)
        * WATER_DEPTH_TRANSMITTANCE.powi(i32::from(depth.saturating_sub(1)))
}

fn to_srgb8(rgb: [f32; 3]) -> [u8; 3] {
    rgb.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}
