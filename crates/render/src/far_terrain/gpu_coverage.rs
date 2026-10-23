use super::*;

impl FarTerrainGpu {
    /// Replaces the near-column mask, preserving its storage when required growth fails.
    pub(super) fn apply_mask(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        mask: &NearMask,
    ) -> Result<(), crate::gpu_allocation::TerrainWriteError> {
        let [x, z] = mask.origin();
        let mut words = vec![x as u32, z as u32, mask.side(), 0];
        words.extend_from_slice(mask.words());
        if words.len() as u64 > self.mask_words {
            let mask_words = (words.len() as u64).next_power_of_two();
            let Some(buffer) = checked_storage_buffer(
                device,
                "far terrain near columns",
                mask_words * 4,
                Requirement::Required,
            ) else {
                return Err(crate::gpu_allocation::TerrainWriteError);
            };
            self.mask_words = mask_words;
            self.mask = buffer;
        }
        crate::gpu_allocation::terrain_write(
            device,
            "far terrain mask upload",
            words.len() as u64 * 4,
            || {
                queue.profiled_write_buffer(&self.mask, 0, bytemuck::cast_slice(&words));
            },
        )
    }

    /// Invalidates retained coverage when CPU-ready empty selections change.
    pub(super) fn set_empty_coverage(&mut self, empty: Option<&HashSet<ColumnPos>>) {
        let unchanged = empty.map_or(self.empty_far.is_empty(), |empty| self.empty_far == *empty);
        if !unchanged {
            self.empty_far = empty.cloned().unwrap_or_default();
            self.applied_tiles = None;
        }
    }

    /// Pins far coverage while near resources are staged or retiring behind the far drawer.
    pub(crate) fn protect_coverage(&mut self, columns: impl Iterator<Item = ColumnPos>) {
        let columns = columns.collect();
        if self.protected != columns {
            self.applied_tiles = None;
        }
        self.protected = columns;
    }

    /// Whether the tile covers a column whose near drawer is suppressed.
    pub(super) fn protects(&self, key: TileKey) -> bool {
        self.protected
            .iter()
            .any(|column| contains_column(key, *column))
    }

    /// Requires current replacements over both the selected area and pinned outgoing columns.
    pub(super) fn replaced(
        &self,
        key: TileKey,
        tiles: &[Arc<TileMesh>],
        next: Option<TileKey>,
    ) -> bool {
        let ready = |mesh: &Arc<TileMesh>| Some(mesh.key) == next || self.current(mesh);
        let covered = |column| {
            self.empty_far.contains(&column)
                || tiles.iter().any(|mesh| mesh.covers(column) && ready(mesh))
        };
        tiles
            .iter()
            .filter(|mesh| key.overlaps(mesh.key))
            .all(ready)
            && self
                .protected
                .iter()
                .filter(|column| contains_column(key, **column))
                .all(|column| covered(*column))
            && (!tiles.iter().any(|mesh| {
                key.overlaps(mesh.key)
                    && !matches!(&mesh.coverage, far_terrain::TileCoverage::Complete)
            }) || self.resident.get(&key).is_none_or(|old| {
                old.mesh
                    .covered_columns()
                    .filter(|column| {
                        self.empty_far.contains(column)
                            || tiles.iter().any(|mesh| contains_column(mesh.key, *column))
                    })
                    .all(covered)
            }))
    }

    /// Whether decoded evidence has a visible far drawer or an acknowledged empty selection.
    pub(crate) fn covers_column(&self, column: ColumnPos) -> bool {
        self.empty_far.contains(&column) || self.draws_far(column)
    }

    /// Whether a visible far tile draws `column`.
    fn draws_far(&self, column: ColumnPos) -> bool {
        (0..LEVEL_COUNT).any(|level| {
            let key = TileKey::containing(
                level,
                column.x * far_terrain::COLUMN_SIDE as i32,
                column.z * far_terrain::COLUMN_SIDE as i32,
            );
            self.resident
                .get(&key)
                .is_some_and(|resident| resident.visible && resident.mesh.covers(column))
        })
    }

    /// Publishes the mask chosen with near draw admission in the current render frame. False,
    /// keeping the previous mask, when the device is out of memory for its storage.
    #[must_use]
    pub(crate) fn publish_mask(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        mask: Arc<NearMask>,
    ) -> bool {
        let published = self.apply_mask(device, queue, &mask).is_ok();
        if published {
            self.applied_mask = Some(mask);
        }
        published
    }

    /// Changes whenever bounded far preparation can change drawable coverage.
    pub(crate) fn publication_revision(&self) -> u64 {
        self.publication_revision
    }
}

/// Tests column containment using the same tile key mapping as selection and receipts.
fn contains_column(key: TileKey, column: ColumnPos) -> bool {
    key == TileKey::containing(
        key.level,
        column.x * far_terrain::COLUMN_SIDE as i32,
        column.z * far_terrain::COLUMN_SIDE as i32,
    )
}
