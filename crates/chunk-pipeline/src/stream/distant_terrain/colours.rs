//! Distant terrain colour tables from the runtime assets: the face colours of every store
//! material and the tints of every session biome.
//!
//! They are built here because this crate is the lowest owner of both inputs: the runtime
//! assets and the mapping from store material keys to session blocks
//! ([`SessionBlocks`]). `lod` defines the tables' layouts ([`lod::colour`]) and never
//! reads assets. A face's colour is the linear average of its texture's base mip: over
//! covered texels, weighted by alpha, so cutout gaps never darken leaves; for
//! overlay-masked faces such as grass sides, whose alpha weighs the tint and not
//! coverage, over every texel, with the tint weight the overlay's share of the colour.

use std::{collections::HashMap, sync::Arc};

use assets::{
    BlockFace, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_OVERLAY_MASK, MATERIAL_FLAG_TINT_MASK,
    MATERIAL_FLAG_WATER_TINT, ResolvedBiomeTints, ResolvedBlock, RuntimeAssets, TextureRef,
};
use lod::{
    colour::{BiomeTints, FaceColour, MaterialColours, TintClass, decode_srgb},
    geometry::Face,
    voxel::MaterialId,
};
use lod_store::store::SharedRegistry;

use super::blocks::SessionBlocks;

/// The asset face of a section face; both list faces as -X, +X, -Y, +Y, -Z, +Z.
pub(in crate::stream) const fn block_face(face: Face) -> BlockFace {
    match face {
        Face::NegX => BlockFace::West,
        Face::PosX => BlockFace::East,
        Face::NegY => BlockFace::Down,
        Face::PosY => BlockFace::Up,
        Face::NegZ => BlockFace::North,
        Face::PosZ => BlockFace::South,
    }
}

/// The tint class of an asset material's flags.
pub(in crate::stream) fn tint_class(flags: u32) -> TintClass {
    match flags & MATERIAL_FLAG_TINT_MASK {
        MATERIAL_FLAG_WATER_TINT => TintClass::Water,
        MATERIAL_FLAG_GRASS_TINT => TintClass::Grass,
        MATERIAL_FLAG_FOLIAGE_TINT => match flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK {
            MATERIAL_FLAG_BIRCH_FOLIAGE => TintClass::Birch,
            MATERIAL_FLAG_EVERGREEN_FOLIAGE => TintClass::Evergreen,
            MATERIAL_FLAG_DRY_FOLIAGE => TintClass::DryFoliage,
            _ => TintClass::Foliage,
        },
        _ => TintClass::None,
    }
}

/// Face colours of every store material under the session's assets, extended as the
/// registry grows and rebuilt when the session's identities change.
#[derive(Default)]
pub(in crate::stream) struct ColourTableBuilder {
    blocks: Option<Arc<SessionBlocks>>,
    table: Arc<MaterialColours>,
    /// Colours by asset material id, which many blocks share.
    by_asset_material: HashMap<u32, FaceColour>,
}

impl ColourTableBuilder {
    /// Colours every material `registry` assigned since the last refresh, starting over
    /// when `blocks` differ from the last ones; true when the table changed.
    pub(in crate::stream) fn refresh(
        &mut self,
        blocks: &Arc<SessionBlocks>,
        registry: &SharedRegistry,
    ) -> bool {
        let mut changed = false;
        if !self
            .blocks
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, blocks))
        {
            self.blocks = Some(Arc::clone(blocks));
            self.table = Arc::new(MaterialColours::new());
            self.by_asset_material.clear();
            changed = true;
        }
        let registry = registry
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let known = self.table.len() - MaterialId::FIRST_BLOCK.raw() as usize;
        if registry.len() <= known {
            return changed;
        }
        let table = Arc::make_mut(&mut self.table);
        for (material, key) in registry.iter().skip(known) {
            debug_assert_eq!(material.raw() as usize, table.len());
            let faces = match blocks.block_of_key(key) {
                Some(block) => block_colours(blocks.assets(), block, &mut self.by_asset_material),
                None => [FaceColour::UNKNOWN; 6],
            };
            if !table.push(faces) {
                break;
            }
        }
        true
    }

    /// The table as built so far.
    pub(in crate::stream) fn table(&self) -> &Arc<MaterialColours> {
        &self.table
    }
}

/// The face colours of `block`. A face without a material of its own (model blocks)
/// takes the average of the block's model quads facing that way, else of all its quads.
fn block_colours(
    assets: &RuntimeAssets,
    block: ResolvedBlock,
    cache: &mut HashMap<u32, FaceColour>,
) -> [FaceColour; 6] {
    let mut colour_of = |material: u32| {
        *cache
            .entry(material)
            .or_insert_with(|| material_colour(assets, material))
    };
    let model = model_materials(assets, block);
    Face::ALL.map(|face| {
        let own = block.face(block_face(face)).material_id();
        if own != DIAGNOSTIC_MATERIAL {
            return colour_of(own);
        }
        let facing: Vec<u32> = model
            .iter()
            .filter(|(quad_face, _)| *quad_face == Some(block_face(face)))
            .map(|(_, material)| *material)
            .collect();
        let materials = if facing.is_empty() {
            model.iter().map(|(_, material)| *material).collect()
        } else {
            facing
        };
        mean_colour(materials.into_iter().map(&mut colour_of))
    })
}

/// The materials of `block`'s model quads with the face each quad lies on, if any.
fn model_materials(assets: &RuntimeAssets, block: ResolvedBlock) -> Vec<(Option<BlockFace>, u32)> {
    let Some(parts) = block
        .model_template()
        .and_then(|first| assets::model_template_parts(assets.model_templates(), first))
    else {
        return Vec::new();
    };
    let quads = assets.model_quads();
    parts
        .iter()
        .filter_map(|part| {
            let start = part.quad_start as usize;
            quads.get(start..start.saturating_add(part.quad_count as usize))
        })
        .flatten()
        .filter(|quad| quad.material != DIAGNOSTIC_MATERIAL)
        .map(|quad| (BlockFace::of_model_quad_flags(quad.flags), quad.material))
        .collect()
}

/// The mean of `colours` in linear light, taking the first tinted one's class and the
/// mean weight; [`FaceColour::UNKNOWN`] when there are none.
fn mean_colour(colours: impl Iterator<Item = FaceColour>) -> FaceColour {
    let mut sum = [0.0_f32; 3];
    let mut weight = 0.0;
    let mut tint = TintClass::None;
    let mut count = 0;
    for colour in colours {
        let linear = colour.linear();
        for channel in 0..3 {
            sum[channel] += linear[channel];
        }
        weight += colour.tint_weight();
        if tint == TintClass::None {
            tint = colour.tint();
        }
        count += 1;
    }
    if count == 0 {
        return FaceColour::UNKNOWN;
    }
    let count = count as f32;
    FaceColour::from_linear(sum.map(|value| value / count), tint, weight / count)
}

/// The flat colour of asset material `id`: see the module documentation.
pub(in crate::stream) fn material_colour(assets: &RuntimeAssets, id: u32) -> FaceColour {
    let material = assets.material(id);
    if material.texture == TextureRef::DIAGNOSTIC {
        return FaceColour::UNKNOWN;
    }
    let Some(texels) = base_texels(assets, material.texture) else {
        return FaceColour::UNKNOWN;
    };
    let tint = tint_class(material.flags);
    let overlay = material.flags & MATERIAL_FLAG_OVERLAY_MASK != 0;
    let mut colour = [0.0_f64; 3];
    let mut weights = 0.0_f64;
    let mut overlaid = 0.0_f64;
    let mut brightness = 0.0_f64;
    for texel in texels.as_chunks::<4>().0 {
        let linear = [texel[0], texel[1], texel[2]].map(|byte| f64::from(decode_srgb(byte)));
        let alpha = f64::from(texel[3]) / 255.0;
        let weight = if overlay { 1.0 } else { alpha };
        for channel in 0..3 {
            colour[channel] += linear[channel] * weight;
        }
        weights += weight;
        let luminance = 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
        brightness += luminance;
        overlaid += luminance * alpha;
    }
    if weights <= 0.0 {
        return FaceColour::UNKNOWN;
    }
    let linear = colour.map(|value| (value / weights) as f32);
    let tint_weight = if overlay && brightness > 0.0 {
        (overlaid / brightness) as f32
    } else {
        1.0
    };
    FaceColour::from_linear(linear, tint, tint_weight)
}

/// The RGBA8 texels of `texture`'s layer in its page's base mip.
fn base_texels(assets: &RuntimeAssets, texture: TextureRef) -> Option<&[u8]> {
    let mip = assets
        .texture_pages()
        .get(texture.page() as usize)?
        .texture
        .mips
        .first()?;
    let layer_bytes = mip.size as usize * mip.size as usize * 4;
    let start = texture.layer() as usize * layer_bytes;
    mip.rgba8.get(start..start + layer_bytes)
}

/// The tints of every session biome by tint class, from their linear records.
pub(in crate::stream) fn biome_tints(resolved: &ResolvedBiomeTints) -> BiomeTints {
    let rgb = |colour: [f32; 4]| [colour[0], colour[1], colour[2]];
    let mut records: Vec<[[f32; 3]; TintClass::COUNT]> = resolved
        .records
        .iter()
        .map(|record| {
            TintClass::ALL.map(|class| match class {
                TintClass::None => [1.0; 3],
                TintClass::Grass => rgb(record.grass),
                TintClass::Foliage => rgb(record.foliage),
                TintClass::Birch => rgb(record.birch),
                TintClass::Evergreen => rgb(record.evergreen),
                TintClass::DryFoliage => rgb(record.dry_foliage),
                TintClass::Water => rgb(record.water),
            })
        })
        .collect();
    if records.is_empty() {
        records.push([[1.0; 3]; TintClass::COUNT]);
    }
    let count = records.len() as u32;
    let by_raw_id = resolved
        .raw_id_to_dense
        .iter()
        .map(|&record| if record < count { record } else { 0 })
        .collect();
    BiomeTints::new(by_raw_id, records.into_boxed_slice())
}

#[cfg(test)]
mod tests;
