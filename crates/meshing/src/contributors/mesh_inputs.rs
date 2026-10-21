//! Proof that a committed block change leaves every terrain mesh unchanged.
//!
//! The mesher reads a block only through its resolved [`ResolvedPaletteEntry`]: flags, faces,
//! kind, model template, variant, contributor role and, through [`RuntimeAssets::resolve`],
//! light emission. Raw network values are compared only by diagnostic geometry, liquid
//! contributor merging, transparent-cube self culling, water-filled neighbours and air
//! classification. A change must preserve those inputs as well as resolved visuals.

use assets::{
    BLOCK_VISUAL_VARIANT_COVERED_GRASS, BLOCK_VISUAL_VARIANT_MATERIAL_MASK,
    BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF, BLOCK_VISUAL_VARIANT_SEASONAL_LEAF, DIAGNOSTIC_MATERIAL,
    MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE, NetworkIdMode, RuntimeAssets, VisualKind,
};
use world::{BLOCKS_PER_SUB_CHUNK, SubChunk};

use super::{ResolvedContributors, ResolvedPaletteEntry, palette_entry_of_block};
use crate::{BlockClassifier, chunk::models::model_template_flags};

// A leaf or covered-grass face material is its non-diagnostic base plus non-negative offsets.
const _: () = assert!(DIAGNOSTIC_MATERIAL == 0);

/// Whether replacing `previous` with `replacement` leaves the mesh of that section, and of every
/// section that samples it, exactly as it is.
///
/// Every cell whose value differs in some layer must swap two registered, non-air values that
/// resolve to the identical [`assets::ResolvedBlock`] and that no mesher path compares by raw
/// value, and the cell's layers must still merge without a diagnostic. The whole sections are
/// compared, so cells changed outside `hint` are still found; `hint` (the cells an update set)
/// is checked first so an ordinary visible change is rejected without decoding a layer.
/// `water_filled` is the sorted list used by the liquid mesher. Membership must be unchanged.
/// Unregistered values answer `false` without counting a registry miss.
pub fn block_changes_preserve_meshes(
    classifier: BlockClassifier,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    previous: &SubChunk,
    replacement: &SubChunk,
    hint: impl IntoIterator<Item = [u8; 3]>,
    water_filled: &[u32],
) -> bool {
    let layers = previous.storages().len();
    if layers != replacement.storages().len() {
        return false;
    }
    let shares_inputs = |old: u32, new: u32| {
        old == new
            || (water_filled.binary_search(&old).is_ok()
                == water_filled.binary_search(&new).is_ok()
                && values_share_mesh_inputs(classifier, assets, mode, old, new))
    };
    for [x, y, z] in hint {
        for layer in 0..layers {
            match (
                previous.runtime_id(layer, x, y, z),
                replacement.runtime_id(layer, x, y, z),
            ) {
                (Some(old), Some(new)) if shares_inputs(old, new) => {}
                _ => return false,
            }
        }
    }

    let mut differing = [0_u64; BLOCKS_PER_SUB_CHUNK / 64];
    let mut old_indices = [0_u32; BLOCKS_PER_SUB_CHUNK];
    let mut new_indices = [0_u32; BLOCKS_PER_SUB_CHUNK];
    let mut proven = None;
    for (old_storage, new_storage) in previous.storages().iter().zip(replacement.storages()) {
        if old_storage == new_storage {
            continue;
        }
        old_storage.palette_indices(&mut old_indices);
        new_storage.palette_indices(&mut new_indices);
        let old_palette = old_storage.palette().values();
        let new_palette = new_storage.palette().values();
        for (linear, (&old, &new)) in old_indices.iter().zip(&new_indices).enumerate() {
            let (Some(&old), Some(&new)) =
                (old_palette.get(old as usize), new_palette.get(new as usize))
            else {
                return false;
            };
            if old == new {
                continue;
            }
            if proven != Some((old, new)) {
                if !shares_inputs(old, new) {
                    return false;
                }
                proven = Some((old, new));
            }
            differing[linear / 64] |= 1 << (linear % 64);
        }
    }

    for (word_index, &word) in differing.iter().enumerate() {
        let mut bits = word;
        while bits != 0 {
            let linear = word_index * 64 + bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let cell = [linear >> 8, linear & 15, (linear >> 4) & 15].map(|axis| axis as u8);
            if !merges_without_diagnostic(classifier, assets, mode, replacement, cell) {
                return false;
            }
        }
    }
    true
}

/// Whether two distinct values feed every mesher input identically: both registered and non-air,
/// with equal resolved visuals and light, and none of the value-compared kinds (liquid,
/// diagnostic, transparent cube) or a diagnostic cube face material.
fn values_share_mesh_inputs(
    classifier: BlockClassifier,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    old: u32,
    new: u32,
) -> bool {
    if classifier.is_air(old) || classifier.is_air(new) {
        return false;
    }
    let (Some(old_block), Some(new_block)) = (
        assets.resolve_known(mode, old),
        assets.resolve_known(mode, new),
    ) else {
        return false;
    };
    if old_block != new_block {
        return false;
    }
    let entry = palette_entry_of_block(assets, mode, new, new_block);
    !matches!(entry.kind, VisualKind::Liquid | VisualKind::Diagnostic)
        && model_template_flags(assets, entry) & MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE == 0
        && cube_materials_are_registered(entry)
}

/// Whether no cube face of `entry` can select the diagnostic material, whose greedy merge and
/// geometry summary compare raw values.
fn cube_materials_are_registered(entry: ResolvedPaletteEntry) -> bool {
    if !entry.emits_cube_geometry() {
        return true;
    }
    let selects_base = entry.variant
        & (BLOCK_VISUAL_VARIANT_SEASONAL_LEAF
            | BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF
            | BLOCK_VISUAL_VARIANT_COVERED_GRASS)
        != 0;
    entry.faces.iter().all(|&face| face != DIAGNOSTIC_MATERIAL)
        && (!selects_base
            || entry.variant & BLOCK_VISUAL_VARIANT_MATERIAL_MASK != DIAGNOSTIC_MATERIAL)
}

/// Whether the layers of `cell` merge into contributors without a diagnostic, which would carry
/// a raw value into the mesh. Unregistered layer values answer `false`.
fn merges_without_diagnostic(
    classifier: BlockClassifier,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    section: &SubChunk,
    [x, y, z]: [u8; 3],
) -> bool {
    let mut contributors = ResolvedContributors::default();
    for layer in 0..section.storages().len() {
        let Some(value) = section.runtime_id(layer, x, y, z) else {
            return false;
        };
        let entry = if classifier.is_air(value) {
            ResolvedPaletteEntry::air(value)
        } else {
            let Some(block) = assets.resolve_known(mode, value) else {
                return false;
            };
            palette_entry_of_block(assets, mode, value, block)
        };
        contributors.push(entry);
    }
    contributors.diagnostic.is_none()
}
