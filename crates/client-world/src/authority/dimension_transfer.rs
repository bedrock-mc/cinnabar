use super::*;
use world::{SUB_CHUNK_SIDE, SubChunkKey};

impl WorldAuthority {
    /// Tests the surrounding area using the dimension's loading Y for out-of-range anchors.
    pub fn dimension_transfer_area_ready(&self, mut position: [f32; 3]) -> bool {
        if !position.into_iter().all(f32::is_finite) {
            return false;
        }
        let dimension = self.current_dimension();
        let Some(range) = self.dimension_range(dimension) else {
            return false;
        };
        let side = SUB_CHUNK_SIDE as i32;
        let minimum_y = range.base_sub_chunk_y * side;
        let maximum_y = minimum_y + (range.sub_chunk_count as i32) * side;
        let anchor_y = i32::from((position[1] as i32) as i16);
        if anchor_y < minimum_y || anchor_y >= maximum_y {
            position[1] = world::dimension_loading_fallback_y(dimension) as f32;
        }
        let lower = position.map(|value| ((value - side as f32).floor() as i32).div_euclid(side));
        let upper = position.map(|value| ((value + side as f32).floor() as i32).div_euclid(side));
        let lower_y = lower[1].max(range.base_sub_chunk_y);
        let upper_y = upper[1].min(range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1);
        for x in lower[0]..=upper[0] {
            for z in lower[2]..=upper[2] {
                let key = ChunkKey { dimension, x, z };
                if self.terrain.is_chunk_loaded(key) {
                    continue;
                }
                if lower_y > upper_y && !self.terrain.contains_column(key) {
                    return false;
                }
                for y in lower_y..=upper_y {
                    if !self
                        .terrain
                        .is_sub_chunk_loaded(SubChunkKey { dimension, x, y, z })
                    {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> WorldAuthority {
        WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 1,
                local_player_runtime_id: 1,
                player_position: [0.0, 4000.0, 0.0],
                world_spawn_position: [0; 3],
                air_network_id: SEQUENTIAL_AIR_NETWORK_ID,
                block_network_ids_are_hashes: false,
            },
            Arc::new(RuntimeAssets::diagnostic()),
            None,
            [0.0; 3],
            None,
        )
    }

    #[test]
    fn staging_anchor_requires_surrounding_columns_and_accepts_known_empty_terrain() {
        let mut authority = authority();
        let anchor = [0.0, 4000.0, 0.0];
        authority
            .mark_chunk_loaded(ChunkKey {
                dimension: 1,
                x: 0,
                z: 0,
            })
            .unwrap();
        assert!(!authority.dimension_transfer_area_ready(anchor));
        for x in -1..=1 {
            for z in -1..=1 {
                if [x, z] != [1, 1] {
                    authority
                        .mark_chunk_loaded(ChunkKey { dimension: 1, x, z })
                        .unwrap();
                }
            }
        }
        assert!(!authority.dimension_transfer_area_ready(anchor));
        authority
            .mark_chunk_loaded(ChunkKey {
                dimension: 1,
                x: 1,
                z: 1,
            })
            .unwrap();
        assert!(authority.dimension_transfer_area_ready(anchor));
        assert!(!authority.dimension_transfer_area_ready([f32::NAN, 0.0, 0.0]));
    }

    #[test]
    fn request_mode_air_uses_loading_height_and_preserves_fractional_boundary_checks() {
        let mut authority = authority();
        for x in -1..=1 {
            for z in -1..=1 {
                authority
                    .mark_sub_chunk_loaded(SubChunkKey {
                        dimension: 1,
                        x,
                        y: 0,
                        z,
                    })
                    .unwrap();
            }
        }
        assert!(authority.dimension_transfer_area_ready([0.0, -0.5, 0.0]));
        assert!(!authority.dimension_transfer_area_ready([0.0, 4000.0, 0.0]));
        for x in -1..=1 {
            for z in -1..=1 {
                authority
                    .mark_sub_chunk_loaded(SubChunkKey {
                        dimension: 1,
                        x,
                        y: 1,
                        z,
                    })
                    .unwrap();
            }
        }
        assert!(authority.dimension_transfer_area_ready([0.0, 4000.0, 0.0]));
        assert!(!authority.dimension_transfer_area_ready([-0.5, 4000.0, 0.0]));
    }

    #[test]
    fn custom_loading_area_outside_height_accepts_authoritative_air_columns() {
        let mut authority = authority();
        let dimension = 1000;
        authority.apply_dimension_heights(&[DimensionHeightDiagnostic {
            name: Arc::from("test:raised"),
            dimension,
            minimum_y: 256,
            height_range: 256,
            generator: 1,
        }]);
        authority.reset_dimension(1, dimension);
        let anchor = [0.0, 4000.0, 0.0];
        assert!(!authority.dimension_transfer_area_ready(anchor));
        for x in -1..=1 {
            for z in -1..=1 {
                if [x, z] != [1, 1] {
                    authority
                        .mark_sub_chunk_loaded(SubChunkKey {
                            dimension,
                            x,
                            y: 16,
                            z,
                        })
                        .unwrap();
                }
            }
        }
        assert!(!authority.dimension_transfer_area_ready(anchor));
        authority
            .mark_sub_chunk_loaded(SubChunkKey {
                dimension,
                x: 1,
                y: 16,
                z: 1,
            })
            .unwrap();
        assert!(authority.dimension_transfer_area_ready(anchor));
    }
}
