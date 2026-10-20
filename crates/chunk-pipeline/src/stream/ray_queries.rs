use super::*;

impl WorldStream {
    /// Distance along the unit `direction` from `origin` to the first block surface,
    /// intersecting model geometry rather than its enclosing cell. Unloaded
    /// sub-chunks read as air. `None` when nothing is hit or a cell is unrepresentable.
    #[must_use]
    pub fn first_block_distance(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
    ) -> Option<f32> {
        if !origin
            .iter()
            .chain(&direction)
            .all(|value| value.is_finite())
        {
            return None;
        }
        let dimension = self.authority.current_dimension();
        let [Some(x), Some(y), Some(z)] = origin.map(|value| {
            let cell = f64::from(value).floor();
            (cell >= f64::from(i32::MIN) && cell <= f64::from(i32::MAX)).then_some(cell as i32)
        }) else {
            return None;
        };
        let mut cell = [x, y, z];
        let step = direction.map(|d| if d < 0.0 { -1 } else { 1 });
        // Distance to cross one cell, and to the first boundary, per axis.
        let across = direction.map(|d| {
            if d == 0.0 {
                f32::INFINITY
            } else {
                d.abs().recip()
            }
        });
        let mut next: [f32; 3] = std::array::from_fn(|axis| {
            let d = direction[axis];
            if d == 0.0 {
                return f32::INFINITY;
            }
            let boundary = if d > 0.0 {
                cell[axis] as f32 + 1.0
            } else {
                cell[axis] as f32
            };
            (boundary - origin[axis]) / d
        });
        let mut travelled = 0.0;
        let mut cached: Option<(SubChunkKey, Option<Arc<SubChunk>>)> = None;
        let mut model_snapshot: Option<MeshSnapshot> = None;
        let mut nearest: Option<f32> = None;
        while travelled <= max_distance {
            let key = SubChunkKey::new(
                dimension,
                cell[0].div_euclid(16),
                cell[1].div_euclid(16),
                cell[2].div_euclid(16),
            );
            if cached.as_ref().is_none_or(|(cached, _)| *cached != key) {
                cached = Some((key, self.authority.terrain().sub_chunk(key)));
                model_snapshot = None;
            }
            if let Some((_, Some(sub_chunk))) = &cached {
                let local = |axis: usize| cell[axis].rem_euclid(16) as u8;
                let (x, y, z) = (local(0), local(1), local(2));
                let assets = self.authority.runtime_assets();
                let mode = self.authority.network_id_mode();
                let mut has_model = false;
                for layer in 0..sub_chunk.storages().len() {
                    let Some(id) = sub_chunk.runtime_id(layer, x, y, z) else {
                        continue;
                    };
                    if self.classifier.is_air(id) {
                        continue;
                    }
                    match assets.resolve(mode, id).kind() {
                        assets::VisualKind::Cube
                        | assets::VisualKind::Diagnostic
                        | assets::VisualKind::Liquid => return Some(travelled),
                        assets::VisualKind::Model | assets::VisualKind::Cross => has_model = true,
                        assets::VisualKind::Invisible => {}
                    }
                }
                if has_model {
                    let snapshot = model_snapshot.get_or_insert_with(|| {
                        self.mesh_snapshot(
                            key,
                            Arc::clone(sub_chunk),
                            MeshLightHalo {
                                center: Some(key),
                                ..Default::default()
                            },
                        )
                    });
                    let local_origin = std::array::from_fn(|axis| origin[axis] - cell[axis] as f32);
                    if let Some(distance) = ::meshing::model_ray_distance(
                        self.classifier,
                        assets,
                        mode,
                        &snapshot.neighbourhood(),
                        [x, y, z].map(usize::from),
                        local_origin,
                        direction,
                    ) && distance <= max_distance
                        && nearest.is_none_or(|old| distance < old)
                    {
                        nearest = Some(distance);
                    }
                }
            }
            let axis = (0..3)
                .min_by(|a, b| next[*a].total_cmp(&next[*b]))
                .unwrap_or(0);
            travelled = next[axis];
            if nearest.is_some_and(|hit| hit <= travelled) {
                return nearest;
            }
            next[axis] += across[axis];
            cell[axis] = cell[axis].checked_add(step[axis])?;
        }
        nearest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds opaque half-slab and stair surfaces without external carriers.
    fn shaped_stream(stair: bool) -> WorldStream {
        use assets::{
            BlockFlags, BlockOverlay, BlockVisual, ContributorRole, ModelTemplate, VisualKind,
        };
        let unit = assets::MODEL_POSITION_SCALE;
        let mut quads = box_quads([0, 0, 0], [unit, unit / 2, unit]);
        if stair {
            quads.extend(box_quads([unit / 2, unit / 2, 0], [unit; 3]));
        }
        let overlay = BlockOverlay {
            visuals: vec![BlockVisual {
                kind: VisualKind::Model,
                support: assets::VisualSupport::Exact,
                contributor_role: ContributorRole::Primary,
                flags: BlockFlags::empty(),
                model_template: 0,
                ..BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary)
            }],
            light_properties: vec![assets::LightProperties::OPAQUE_DARK],
            materials: vec![assets::Material {
                texture: assets::TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: assets::NO_ANIMATION,
                ..assets::Material::unvaried()
            }],
            model_templates: vec![ModelTemplate {
                quad_start: 0,
                quad_count: quads.len() as u32,
                flags: 0,
            }],
            model_quads: quads,
            ..BlockOverlay::default()
        };
        let assets = RuntimeAssets::diagnostic()
            .with_block_overlay(1, &overlay)
            .unwrap();
        let mut stream = WorldStream::new_with_assets(
            protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            Arc::new(assets),
            [0.0; 3],
            None,
        );
        let prepared = ChunkStore::prepare_sub_chunk_blocks(
            SubChunkKey::new(0, 0, 0, 0),
            None,
            &[BlockUpdate {
                layer: 0,
                x: 0,
                y: 0,
                z: 0,
                runtime_id: 1,
            }],
            0,
        )
        .unwrap();
        stream
            .authority
            .commit_prepared_block_updates(vec![prepared])
            .unwrap();
        stream
    }

    /// Emits the six surfaces of a fixed-point cuboid for geometry-ray fixtures.
    fn box_quads(min: [i16; 3], max: [i16; 3]) -> Vec<assets::ModelQuad> {
        (0..3)
            .flat_map(|axis| {
                [false, true].map(move |upper| {
                    let a = (axis + 1) % 3;
                    let b = (axis + 2) % 3;
                    assets::ModelQuad {
                        positions: [[false, false], [true, false], [true, true], [false, true]]
                            .map(|[u, v]| {
                                let mut p = min;
                                p[axis] = if upper { max[axis] } else { min[axis] };
                                p[a] = if u { max[a] } else { min[a] };
                                p[b] = if v { max[b] } else { min[b] };
                                p
                            }),
                        uvs: [[0; 2]; 4],
                        material: assets::DIAGNOSTIC_MATERIAL,
                        flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
                    }
                })
            })
            .collect()
    }

    #[test]
    fn first_block_distance_passes_through_slab_and_stair_gaps() {
        for stair in [false, true] {
            let stream = shaped_stream(stair);
            assert_eq!(
                stream.first_block_distance([0.25, 0.75, -1.0], [0.0, 0.0, 1.0], 4.0),
                None
            );
            assert_eq!(
                stream.first_block_distance([0.25, 0.25, -1.0], [0.0, 0.0, 1.0], 4.0),
                Some(1.0)
            );
            assert_eq!(
                stream.first_block_distance([0.25, 2.0, 0.5], [0.0, -1.0, 0.0], 4.0),
                Some(1.5)
            );
        }
        let stream = shaped_stream(true);
        assert_eq!(
            stream.first_block_distance([0.75, 2.0, 0.5], [0.0, -1.0, 0.0], 4.0),
            Some(1.0)
        );
        assert_eq!(
            stream.first_block_distance([-1.0, 0.75, 0.5], [1.0, 0.0, 0.0], 4.0),
            Some(1.5)
        );
    }

    #[test]
    fn first_block_distance_finds_geometry_beyond_a_gap_within_range() {
        let mut stream = shaped_stream(false);
        let key = SubChunkKey::new(0, 0, 0, 0);
        let previous = stream.authority.terrain().sub_chunk(key).unwrap();
        let prepared = ChunkStore::prepare_sub_chunk_blocks(
            key,
            Some(&previous),
            &[BlockUpdate {
                x: 0,
                y: 0,
                z: 2,
                layer: 0,
                runtime_id: 2,
            }],
            0,
        )
        .unwrap();
        stream
            .authority
            .commit_prepared_block_updates(vec![prepared])
            .unwrap();
        assert_eq!(
            stream.first_block_distance([0.25, 0.75, -1.0], [0.0, 0.0, 1.0], 4.0),
            Some(3.0)
        );
        assert_eq!(
            stream.first_block_distance([0.25, 0.75, -1.0], [0.0, 0.0, 1.0], 2.5),
            None
        );
    }

    #[test]
    fn first_block_distance_skips_unrepresentable_cells() {
        let stream = WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        });
        for axis in 0..3 {
            for coordinate in [i32::MIN as f32, i32::MAX as f32, f32::MIN, f32::MAX] {
                let mut origin = [0.0; 3];
                origin[axis] = coordinate;
                for sign in [-1.0, 1.0] {
                    let mut direction = [0.0; 3];
                    direction[axis] = sign;
                    assert_eq!(stream.first_block_distance(origin, direction, 16.0), None);
                }
            }
        }
    }
}
