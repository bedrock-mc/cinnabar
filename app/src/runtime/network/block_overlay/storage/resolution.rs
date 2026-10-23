//! Reduces page resolution under byte pressure while keeping layer references stable.

use super::super::{MIN_TILE, textures::terrain};
use super::{Builder, Source, placement::FramePlacement};
use pack_runtime::pack_textures::DecodedTexture;

/// A complete frame run at a resolution that fits the retained and upload budgets.
pub(super) struct FrameResolution {
    pub(super) placements: Vec<FramePlacement>,
    pub(super) tiles: Vec<u32>,
    pub(super) side: u32,
}

impl Builder<'_> {
    /// Reduces incoming pixels only after a complete resolution plan succeeds.
    pub(super) fn fit_frames(
        &self,
        frames: &mut [DecodedTexture],
        grid: u8,
    ) -> Option<(Vec<FramePlacement>, Vec<u32>)> {
        let dimensions = frames
            .iter()
            .map(|frame| [frame.width, frame.height])
            .collect::<Vec<_>>();
        let plan = self.frame_resolution(&dimensions, grid)?;
        for frame in frames {
            terrain::reduce_texture(frame, plan.side);
        }
        Some((plan.placements, plan.tiles))
    }

    /// Plans complete frame dimensions without allocating their pixels or changing retained pages.
    pub(super) fn frame_resolution(
        &self,
        frames: &[[u32; 2]],
        grid: u8,
    ) -> Option<FrameResolution> {
        let own = frames
            .iter()
            .map(|frame| frame[0].max(frame[1]))
            .max()?
            .checked_next_power_of_two()?
            .max(MIN_TILE);
        let mut tiles = self.tiles.clone();
        loop {
            let mut side = own;
            loop {
                let dimensions = frames
                    .iter()
                    .map(|frame| terrain::reduced_dimensions(*frame, side))
                    .collect::<Vec<_>>();
                if dimensions.iter().any(|axes| {
                    axes.iter().any(|axis| {
                        axis.checked_shr(u32::from(grid))
                            .is_none_or(|size| size == 0)
                    })
                }) {
                    break;
                }
                let source_bytes = self
                    .sources
                    .iter()
                    .map(|(source, shard)| match source {
                        Source::Diagnostic => 0,
                        Source::Image(texture) | Source::GridImage(texture, _) => {
                            let [width, height] = terrain::reduced_dimensions(
                                [texture.width, texture.height],
                                tiles.get(*shard).copied().unwrap_or(MIN_TILE),
                            );
                            (width as usize) * (height as usize) * 4
                        }
                    })
                    .sum::<usize>()
                    + dimensions
                        .iter()
                        .map(|[width, height]| (*width as usize) * (*height as usize) * 4)
                        .sum::<usize>();
                let placements = self.place_frames_with_tiles(side, frames.len(), &tiles);
                if source_bytes <= self.texture_policy.storage_bytes()
                    && placements
                        .iter()
                        .map(|placement| placement.count)
                        .sum::<usize>()
                        == frames.len()
                {
                    return Some(FrameResolution {
                        placements,
                        tiles,
                        side,
                    });
                }
                if side == MIN_TILE {
                    break;
                }
                side = (side / 2).max(MIN_TILE);
            }
            let shard = tiles
                .iter()
                .enumerate()
                .filter(|(shard, tile)| {
                    **tile > MIN_TILE
                        && self
                            .sources
                            .iter()
                            .filter(|(_, assigned)| assigned == shard)
                            .all(|(source, _)| match source {
                                Source::GridImage(texture, grid) => terrain::reduced_dimensions(
                                    [texture.width, texture.height],
                                    **tile / 2,
                                )
                                .into_iter()
                                .all(|axis| {
                                    axis.checked_shr(u32::from(*grid))
                                        .is_some_and(|size| size > 0)
                                }),
                                _ => true,
                            })
                })
                .max_by_key(|(_, tile)| **tile)
                .map(|(shard, _)| shard)?;
            tiles[shard] /= 2;
        }
    }

    /// Applies a successful resolution plan without moving materials or animation references.
    pub(super) fn resize_pages(&mut self, tiles: &[u32]) {
        if self.tiles == tiles {
            return;
        }
        for (source, shard) in &mut self.sources {
            if let Source::Image(texture) | Source::GridImage(texture, _) = source {
                terrain::reduce_texture(texture, tiles.get(*shard).copied().unwrap_or(MIN_TILE));
            }
        }
        self.tiles = tiles.to_vec();
        self.recount_texture_bytes();
    }

    /// Recounts retained pixels and page mips after resizing or discarding unpublished layers.
    pub(in super::super) fn recount_texture_bytes(&mut self) {
        self.source_bytes = self
            .sources
            .iter()
            .map(|(source, _)| match source {
                Source::Diagnostic => 0,
                Source::Image(texture) | Source::GridImage(texture, _) => texture.rgba8.len(),
            })
            .sum();
        self.upload_bytes = (0..self.max_shards())
            .map(|shard| {
                self.texture_policy
                    .terrain_bytes(
                        self.tiles.get(shard).copied().unwrap_or(MIN_TILE),
                        self.shard_len(shard),
                    )
                    .expect("admitted page budget")
            })
            .sum();
    }
}
