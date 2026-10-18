//! Applies optional terrain mip chains without changing geometry or texture references.

use super::*;
use crate::queue_writes::ProfiledQueue as _;
use bevy::{
    prelude::Local,
    tasks::{AsyncComputeTaskPool, Task, block_on},
};
use render_scene::ChunkTextureAssetIdentity;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(in crate::chunk) struct MipState {
    applied: Option<(ChunkTextureAssetIdentity, bool)>,
    failed: Option<(ChunkTextureAssetIdentity, bool)>,
    pending: Option<Task<PreparedMips>>,
}

struct PreparedMips {
    key: (ChunkTextureAssetIdentity, bool),
    pages: Option<Vec<assets::TextureArray>>,
}

/// Coalesces style changes into one worker and publishes only the current asset generation.
pub(in crate::chunk) fn prepare_java_terrain_mips(
    source: Res<ChunkTextureAssets>,
    java: Option<Res<render_scene::JavaWorldVisuals>>,
    gpu: Res<ChunkGpuTextureAssets>,
    queue: Res<RenderQueue>,
    mut state: Local<MipState>,
) {
    let Some(prepared) = &gpu.prepared else {
        return;
    };
    if prepared.identity.carrier != source.identity() {
        return;
    }
    let wanted = (
        source.identity(),
        java.as_ref().is_some_and(|java| java.enabled()),
    );
    if state.pending.as_ref().is_some_and(Task::is_finished) {
        let finished = block_on(state.pending.take().expect("completed mip task"));
        if finished.key == wanted {
            let Some(pages) = finished.pages else {
                state.failed = Some(wanted);
                bevy::log::warn!(
                    "Could not prepare terrain mip levels for the selected world visuals"
                );
                return;
            };
            for (page, texture) in pages.iter().zip(&prepared._textures) {
                for (level, mip) in page.mips.iter().enumerate() {
                    queue.profiled_write_texture(
                        TexelCopyTextureInfo {
                            texture,
                            mip_level: level as u32,
                            origin: Origin3d::default(),
                            aspect: Default::default(),
                        },
                        &mip.rgba8,
                        TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(mip.size * 4),
                            rows_per_image: Some(mip.size),
                        },
                        Extent3d {
                            width: mip.size,
                            height: mip.size,
                            depth_or_array_layers: page.layers,
                        },
                    );
                }
            }
            state.applied = Some(wanted);
            state.failed = None;
        }
    }
    if !wanted.1 && state.applied.is_none_or(|applied| applied.0 != wanted.0) {
        state.applied = Some(wanted);
    }
    if state.applied == Some(wanted) || state.failed == Some(wanted) || state.pending.is_some() {
        return;
    }
    let assets = Arc::clone(source.assets());
    state.pending = Some(AsyncComputeTaskPool::get().spawn(async move {
        PreparedMips {
            key: wanted,
            pages: prepare(&assets, wanted.1),
        }
    }));
}

/// Derives cutout layers from material and flipbook references, then rebuilds each admitted page.
fn prepare(assets: &assets::RuntimeAssets, java: bool) -> Option<Vec<assets::TextureArray>> {
    let mut cutouts = BTreeSet::new();
    let mut dark = BTreeSet::new();
    for material in assets.materials() {
        if material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT == 0 {
            continue;
        }
        let references = std::iter::once(material.texture).chain(
            assets
                .animations()
                .get(material.animation as usize)
                .into_iter()
                .flat_map(|animation| {
                    assets
                        .animation_frames()
                        .get(
                            animation.frame_start as usize
                                ..(animation.frame_start + animation.frame_count) as usize,
                        )
                        .unwrap_or(&[])
                        .iter()
                        .copied()
                }),
        );
        for reference in references {
            let key = (reference.page(), reference.layer());
            cutouts.insert(key);
            if material.flags & assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR != 0 {
                dark.insert(key);
            }
        }
    }
    assets
        .texture_pages()
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let texture = assets.terrain_texture_page(index).ok()?;
            if !java {
                return Some(texture.into_owned());
            }
            let base = texture.mips.first()?;
            let size = base.size;
            let mut levels: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
            for (layer, pixels) in base
                .rgba8
                .chunks_exact((size * size * 4) as usize)
                .enumerate()
            {
                let key = (index as u32, layer as u32);
                let mips = assets::java_world::terrain_layer(
                    pixels,
                    size,
                    texture.mips.len(),
                    cutouts.contains(&key),
                    dark.contains(&key),
                )?;
                for (level, mip) in mips.iter().enumerate() {
                    levels
                        .entry(level)
                        .or_default()
                        .extend_from_slice(&mip.rgba8);
                }
            }
            Some(assets::TextureArray {
                layers: texture.layers,
                mips: levels
                    .into_iter()
                    .map(|(level, rgba8)| assets::TextureMip {
                        size: size >> level,
                        rgba8: rgba8.into_boxed_slice(),
                    })
                    .collect(),
            })
        })
        .collect()
}
