//! Destination decode probes and local presentation readiness during transfers.

use super::{WorldStream, floor_to_i32, vanilla_dimension_range};
use world::{ChunkKey, SUB_CHUNK_SIDE as SIDE, SubChunkKey};

// Vanilla loading offsets remain available for transfer diagnostics.
const TRANSFER_RADIUS_BLOCKS: f32 = 16.0;
const END_SPAWN_Y: f32 = 50.0;

// Current client offset initializer 02ddc3a0 copies these 57 eight-byte
// ChunkPos entries into a 0x1c8-byte vector. Level slot 0x830 (01313e50)
// returns this list independently of the server's simulation radius.
pub(super) const CLIENT_TICKING_OFFSETS: &[[i32; 2]] = &[
    [-1, -4],
    [0, -4],
    [1, -4],
    [-2, -3],
    [-1, -3],
    [0, -3],
    [1, -3],
    [2, -3],
    [-3, -2],
    [-2, -2],
    [-1, -2],
    [0, -2],
    [1, -2],
    [2, -2],
    [3, -2],
    [-4, -1],
    [-3, -1],
    [-2, -1],
    [-1, -1],
    [0, -1],
    [1, -1],
    [2, -1],
    [3, -1],
    [4, -1],
    [-4, 0],
    [-3, 0],
    [-2, 0],
    [-1, 0],
    [0, 0],
    [1, 0],
    [2, 0],
    [3, 0],
    [4, 0],
    [-4, 1],
    [-3, 1],
    [-2, 1],
    [-1, 1],
    [0, 1],
    [1, 1],
    [2, 1],
    [3, 1],
    [4, 1],
    [-3, 2],
    [-2, 2],
    [-1, 2],
    [0, 2],
    [1, 2],
    [2, 2],
    [3, 2],
    [-2, 3],
    [-1, 3],
    [0, 3],
    [1, 3],
    [2, 3],
    [-1, 4],
    [0, 4],
    [1, 4],
];

impl WorldStream {
    /// The native loading counter gives each fully loaded client ticking
    /// column two points, with a threshold of twice this list's length. It
    /// therefore opens only when every selected column has decoded data.
    /// Lighting, mesh and GPU readiness are checked separately by presentation.
    #[must_use]
    pub fn dimension_loading_columns_ready(&self) -> bool {
        let position = self.authority.resolved_server_position().position;
        if position.iter().any(|component| !component.is_finite()) {
            return false;
        }
        let dimension = self.authority.current_dimension();
        let center_x = floor_to_i32(position[0]).div_euclid(SIDE as i32);
        let center_z = floor_to_i32(position[2]).div_euclid(SIDE as i32);
        CLIENT_TICKING_OFFSETS.iter().all(|offset| {
            self.loaded_columns.contains(&ChunkKey::new(
                dimension,
                center_x + offset[0],
                center_z + offset[1],
            ))
        })
    }

    /// Missing native loading columns, bounded by the fixed client offset list.
    /// Transfer diagnostics use this to distinguish unavailable destination data
    /// from lighting, presentation or acknowledgement waits.
    #[must_use]
    pub fn dimension_loading_missing_columns(&self) -> Vec<ChunkKey> {
        let position = self.authority.resolved_server_position().position;
        let dimension = self.authority.current_dimension();
        let center_x = floor_to_i32(position[0]).div_euclid(SIDE as i32);
        let center_z = floor_to_i32(position[2]).div_euclid(SIDE as i32);
        CLIENT_TICKING_OFFSETS
            .iter()
            .map(|offset| ChunkKey::new(dimension, center_x + offset[0], center_z + offset[1]))
            .filter(|column| !self.loaded_columns.contains(column))
            .collect()
    }

    /// Whether every destination subchunk in the native ±16 block probe has
    /// decoded data. Our local presentation gate retains this collision probe;
    /// the client acknowledges its committed dimension flush independently.
    #[must_use]
    pub fn dimension_transfer_ready(&self, position: [f32; 3]) -> bool {
        let Some(position) = self.transfer_probe_position(position) else {
            return false;
        };
        let dimension = self.authority.current_dimension();
        let range = vanilla_dimension_range(dimension);
        let side = SIDE as i32;
        let minimum =
            position.map(|value| floor_to_i32(value - TRANSFER_RADIUS_BLOCKS).div_euclid(side));
        let maximum =
            position.map(|value| floor_to_i32(value + TRANSFER_RADIUS_BLOCKS).div_euclid(side));
        for x in minimum[0]..=maximum[0] {
            for z in minimum[2]..=maximum[2] {
                for y in minimum[1]..=maximum[1] {
                    if range.is_some_and(|range| {
                        y < range.base_sub_chunk_y
                            || y >= range.base_sub_chunk_y + range.sub_chunk_count as i32
                    }) {
                        continue;
                    }
                    let key = SubChunkKey::new(dimension, x, y, z);
                    if !self.known_air.contains(&key)
                        && !self.authority.terrain().is_sub_chunk_loaded(key)
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Collision data must be decoded around the destination, but presentation
    /// waits only for the player's cell and footing. Other column heights and
    /// neighbors can continue lighting and meshing after the loading screen.
    #[must_use]
    pub fn dimension_transfer_presentable(&self, position: [f32; 3]) -> bool {
        if !self.dimension_transfer_ready(position) {
            return false;
        }
        let keys = self
            .transfer_mesh_keys(position)
            .expect("decoded transfer probe has a finite position");
        let dimension = self.authority.current_dimension();
        let range = vanilla_dimension_range(dimension);
        keys.into_iter().all(|key| {
            if range.is_some_and(|range| {
                key.y < range.base_sub_chunk_y
                    || key.y >= range.base_sub_chunk_y + range.sub_chunk_count as i32
            }) {
                return true;
            }
            self.known_air.contains(&key) || (self.light_is_current(key) && self.is_mesh_clean(key))
        })
    }

    pub(super) fn transfer_probe_position(&self, mut position: [f32; 3]) -> Option<[f32; 3]> {
        if position.iter().any(|component| !component.is_finite()) {
            return None;
        }
        let dimension = self.authority.current_dimension();
        let range = vanilla_dimension_range(dimension);
        let side = SIDE as i32;
        if let Some(range) = range {
            let minimum_y = range.base_sub_chunk_y * side;
            let maximum_y = minimum_y + range.sub_chunk_count as i32 * side;
            // Native checks the truncated signed 16-bit Y, but retains the
            // fractional coordinate when that check admits it.
            let checked_y = i32::from(position[1] as i32 as i16);
            if checked_y < minimum_y || checked_y >= maximum_y {
                position[1] = if dimension == 2 { END_SPAWN_Y } else { 0.0 };
            }
        }
        // Custom dimensions have no published height range in the current
        // ingestion model. Probe their actual destination Y without a guessed
        // height bound or vanilla spawn fallback.
        Some(position)
    }
}
