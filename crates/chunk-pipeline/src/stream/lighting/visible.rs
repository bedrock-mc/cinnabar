//! Whether a section's first solved light reaches anything its provisional meshes drew.

use super::super::*;
use world::{LightChannel, SubChunkLight};

/// Where a solved light differs from the light provisional meshes read, counting only cells a
/// mesh can sample: `interior` for the section's own mesh, `faces` (-X, +X, -Y, +Y, -Z, +Z)
/// for the neighbours that sample its boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) struct VisibleLightChange {
    pub(in crate::stream) interior: bool,
    pub(in crate::stream) faces: [bool; 6],
}

impl WorldStream {
    /// Compares a section's first solved light with the dimension default its provisional
    /// meshes read in its place. Solid-render cells that emit nothing are skipped: their solved
    /// light is dark, and a vertex reads their light only as one of four samples whose maximum
    /// an unchanged open neighbour already sets at the default's full sky.
    pub(in crate::stream) fn first_light_visible_change(
        &self,
        key: SubChunkKey,
        solved: &SubChunkLight,
    ) -> VisibleLightChange {
        let [default_block, default_sky] =
            super::super::light_queries::dimension_default_light(key.dimension);
        let mut change = VisibleLightChange {
            interior: false,
            faces: [false; 6],
        };
        let sub_chunk = self.authority.terrain().sub_chunk(key);
        let assets = self.authority.runtime_assets();
        let mode = self.authority.network_id_mode();
        // Palettes are small, so each runtime id resolves once to (solid-render, emits).
        let mut resolved: Vec<(u32, bool, bool)> = Vec::new();
        let mut cell_unread = |x: u8, y: u8, z: u8| {
            let Some(sub_chunk) = sub_chunk.as_deref() else {
                return false;
            };
            let mut solid = false;
            for layer in 0..sub_chunk.storages().len() {
                let Some(id) = sub_chunk.runtime_id(layer, x, y, z) else {
                    continue;
                };
                let (layer_solid, emits) = match resolved.iter().find(|entry| entry.0 == id) {
                    Some(&(_, layer_solid, emits)) => (layer_solid, emits),
                    None => {
                        let visual = assets.resolve(mode, id);
                        let layer_solid = visual.variant() != assets::BLOCK_VISUAL_VARIANT_TOP_SNOW
                            && visual
                                .flags()
                                .contains(assets::BlockFlags::OCCLUDES_FULL_FACE);
                        let emits = visual.light_properties().emission() > 0;
                        resolved.push((id, layer_solid, emits));
                        (layer_solid, emits)
                    }
                };
                if emits {
                    return false;
                }
                solid |= layer_solid;
            }
            solid
        };
        for y in 0..16_u8 {
            for z in 0..16_u8 {
                for x in 0..16_u8 {
                    let block = solved.get(LightChannel::Block, x, y, z).unwrap_or(0);
                    let sky = solved.get(LightChannel::Sky, x, y, z).unwrap_or(0);
                    if (block, sky) == (default_block, default_sky) || cell_unread(x, y, z) {
                        continue;
                    }
                    change.interior = true;
                    change.faces[0] |= x == 0;
                    change.faces[1] |= x == 15;
                    change.faces[2] |= y == 0;
                    change.faces[3] |= y == 15;
                    change.faces[4] |= z == 0;
                    change.faces[5] |= z == 15;
                    if change.faces.iter().all(|&face| face) {
                        return change;
                    }
                }
            }
        }
        change
    }
}
