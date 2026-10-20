use super::*;

impl WorldAuthority {
    /// Reads the server's admitted range, falling back to the built-in dimension range.
    pub fn dimension_range(&self, dimension: i32) -> Option<DimensionRange> {
        self.dimension_ranges
            .get(&dimension)
            .map(|(_, range)| *range)
            .or_else(|| vanilla_dimension_range(dimension))
    }

    /// Freezes the effective range when terrain first uses the dimension.
    pub fn admit_dimension_range(&mut self, dimension: i32) -> Option<DimensionRange> {
        let range = self.dimension_range(dimension)?;
        self.frozen_dimension_ranges.insert(dimension);
        Some(range)
    }

    /// Retains the first valid named definition, preserving dimensions already in use.
    #[expect(
        clippy::print_stderr,
        reason = "DIMENSION_RANGE_SKIPPED is a stderr marker line read during live session runs"
    )]
    pub fn apply_dimension_heights(&mut self, heights: &[DimensionHeightDiagnostic]) {
        for height in heights {
            let name_already_defined = self
                .dimension_ranges
                .values()
                .any(|(name, _)| name.as_ref() == height.name.as_ref());
            let admitted = definition_dimension(height)
                .zip(definition_range(height))
                .filter(|(dimension, _)| {
                    name_already_defined
                        || self.dimension_ranges.contains_key(dimension)
                        || self.frozen_dimension_ranges.contains(dimension)
                        || self.dimension_ranges.len() < MAX_DIMENSION_DEFINITIONS
                });
            if let Some((dimension, range)) = admitted {
                // Vanilla keeps the first named definition and constructs each dimension once.
                if !name_already_defined && !self.frozen_dimension_ranges.contains(&dimension) {
                    self.dimension_ranges
                        .entry(dimension)
                        .or_insert_with(|| (Arc::clone(&height.name), range));
                }
            } else {
                self.dimension_range_skips = self.dimension_range_skips.saturating_add(1);
                if self.dimension_range_skips <= 8 {
                    eprintln!(
                        "DIMENSION_RANGE_SKIPPED name={} dimension={} minimum_y={} maximum_y={} skips={}",
                        height.name,
                        height.dimension,
                        height.minimum_y,
                        height.maximum_y,
                        self.dimension_range_skips,
                    );
                }
            }
        }
    }

    /// Counts well-formed dimension definitions outside supported bounds.
    pub const fn dimension_range_skip_count(&self) -> u64 {
        self.dimension_range_skips
    }
}

/// Resolves built-in names or an admitted custom dimension ID.
fn definition_dimension(height: &DimensionHeightDiagnostic) -> Option<i32> {
    assets::facts::dimension_id(&height.name).or_else(|| {
        (!height.name.is_empty() && (1000..=i32::from(u16::MAX)).contains(&height.dimension))
            .then_some(height.dimension)
    })
}

/// Converts exclusive block bounds into the complete sub-chunks kept in storage.
fn definition_range(height: &DimensionHeightDiagnostic) -> Option<DimensionRange> {
    let block_height = height.maximum_y.checked_sub(height.minimum_y)?;
    if height.minimum_y % 16 != 0 || block_height <= 0 {
        return None;
    }
    let sub_chunk_count = usize::try_from(block_height / 16).ok()?;
    // Inline columns address their slots through an unsigned byte.
    if sub_chunk_count > usize::from(u8::MAX) + 1 {
        return None;
    }
    Some(DimensionRange {
        base_sub_chunk_y: height.minimum_y / 16,
        sub_chunk_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> WorldAuthority {
        WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
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

    fn definition(
        name: &str,
        dimension: i32,
        minimum_y: i32,
        maximum_y: i32,
    ) -> DimensionHeightDiagnostic {
        DimensionHeightDiagnostic {
            name: Arc::from(name),
            dimension,
            minimum_y,
            maximum_y,
            generator: 1,
        }
    }

    #[test]
    fn custom_maximum_y_with_a_partial_subchunk_admits_whole_slots() {
        let mut authority = authority();
        authority.apply_dimension_heights(&[
            definition("example:dim_a", 1000, -64, 383),
            definition("example:dim_b", 1001, -64, 383),
        ]);
        authority.reset_dimension(0, 1001);
        let range = authority.admit_dimension_range(1001).unwrap();
        assert_eq!(range.base_sub_chunk_y, -4);
        assert_eq!(range.sub_chunk_count, 27);
        assert_eq!(
            (range.base_sub_chunk_y + range.sub_chunk_count as i32) * 16,
            368,
        );
        assert_eq!(authority.dimension_range(1000), Some(range));
        assert_eq!(authority.dimension_range_skip_count(), 0);
    }

    #[test]
    fn overworld_maximum_y_keeps_all_twenty_four_subchunks() {
        let mut authority = authority();
        authority.apply_dimension_heights(&[definition("minecraft:overworld", 3, -64, 320)]);
        assert_eq!(
            authority.admit_dimension_range(0),
            Some(DimensionRange {
                base_sub_chunk_y: -4,
                sub_chunk_count: 24,
            }),
        );
        assert_eq!(authority.dimension_range_skip_count(), 0);
    }

    #[test]
    fn named_overworld_definition_overrides_the_builtin_range() {
        let mut authority = authority();
        authority
            .apply_ordered_event(
                WorldEvent::DimensionHeights(vec![definition("minecraft:overworld", 3, 0, 256)]),
                None,
            )
            .unwrap();
        assert_eq!(
            authority.dimension_range(0),
            Some(DimensionRange {
                base_sub_chunk_y: 0,
                sub_chunk_count: 16
            }),
        );
        assert_eq!(authority.dimension_range(3), None);
        assert_eq!(authority.dimension_range(1), vanilla_dimension_range(1));
        assert_eq!(authority.dimension_range_skip_count(), 0);
    }

    #[test]
    fn odd_definitions_preserve_the_first_valid_range_and_keep_admitting() {
        let mut authority = authority();
        let valid = definition("minecraft:overworld", 3, -128, 384);
        authority.apply_dimension_heights(std::slice::from_ref(&valid));
        let previous = authority.dimension_range(0);
        for (minimum_y, maximum_y) in [
            (0, 0),
            (0, -16),
            (1, 256),
            (256, 256),
            (0, (i32::from(u8::MAX) + 2) * 16),
            (i32::MIN, i32::MAX),
        ] {
            authority.apply_dimension_heights(&[definition(
                "minecraft:overworld",
                3,
                minimum_y,
                maximum_y,
            )]);
            assert_eq!(authority.dimension_range(0), previous);
        }
        assert_eq!(authority.dimension_range_skip_count(), 6);
        authority.apply_dimension_heights(&[definition("minecraft:overworld", 3, 0, 256)]);
        assert_eq!(authority.dimension_range(0), previous);
        authority.apply_dimension_heights(&[definition("minecraft:nether", 3, 0, 256)]);
        assert_eq!(authority.dimension_range(1).unwrap().sub_chunk_count, 16);
    }

    #[test]
    fn custom_dimension_ranges_are_bounded_and_duplicates_preserve_the_first() {
        let mut authority = authority();
        let heights = (1000..)
            .take(MAX_DIMENSION_DEFINITIONS + 1)
            .map(|dimension| definition(&format!("example:{dimension}"), dimension, 0, 256))
            .collect::<Vec<_>>();
        authority.apply_dimension_heights(&heights);
        assert_eq!(authority.dimension_ranges.len(), MAX_DIMENSION_DEFINITIONS);
        assert_eq!(authority.dimension_range_skip_count(), 1);
        authority.apply_dimension_heights(&[definition("example:1000", 1000, -128, 384)]);
        assert_eq!(authority.dimension_range(1000).unwrap().base_sub_chunk_y, 0);
        assert_eq!(authority.dimension_range(1000).unwrap().sub_chunk_count, 16);
        assert_eq!(authority.dimension_range_skip_count(), 1);
    }

    #[test]
    fn duplicate_custom_names_preserve_the_first_dimension_id() {
        let mut authority = authority();
        authority.apply_dimension_heights(&[definition("example:realm", 1000, -128, 384)]);
        let original = authority.dimension_range(1000);
        authority.apply_dimension_heights(&[definition("example:realm", 1001, 0, 256)]);
        assert_eq!(authority.dimension_range(1000), original);
        assert_eq!(authority.dimension_range(1001), None);
        authority.apply_dimension_heights(&[definition("example:other", 1001, 0, 256)]);
        assert_eq!(authority.dimension_range(1001).unwrap().base_sub_chunk_y, 0);
        assert_eq!(authority.dimension_range_skip_count(), 0);
    }

    #[test]
    fn late_first_definition_preserves_used_dimensions_and_installs_unused_ones() {
        let mut authority = authority();
        let original = authority.admit_dimension_range(0);
        assert_eq!(original, vanilla_dimension_range(0));
        assert_eq!(authority.admit_dimension_range(1000), None);
        authority.apply_dimension_heights(&[
            definition("minecraft:overworld", 3, 0, 256),
            definition("example:unused", 1000, -128, 384),
        ]);
        assert_eq!(authority.dimension_range(0), original);
        let custom = authority.admit_dimension_range(1000).unwrap();
        assert_eq!(custom.base_sub_chunk_y, -8);
        assert_eq!(custom.sub_chunk_count, 32);
        authority.apply_dimension_heights(&[definition("example:unused", 1000, 0, 256)]);
        assert_eq!(authority.dimension_range(1000), Some(custom));
        assert_eq!(authority.dimension_range_skip_count(), 0);
    }

    #[test]
    fn custom_dimension_ids_follow_the_supported_registry_range() {
        let mut authority = authority();
        for id in [-1, 3, 999, i32::from(u16::MAX) + 1, i32::MAX] {
            authority.apply_dimension_heights(&[definition("example:outside", id, 0, 256)]);
            assert_eq!(authority.dimension_range(id), None);
        }
        for id in [1000, i32::from(u16::MAX)] {
            authority.apply_dimension_heights(&[definition(&format!("example:{id}"), id, 0, 256)]);
            assert!(authority.admit_dimension_range(id).is_some());
        }
        assert_eq!(authority.dimension_range_skip_count(), 5);
    }
}
