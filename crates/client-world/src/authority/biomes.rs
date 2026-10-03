use super::*;

/// Normalization facts from one synchronous biome-definition commit.
pub struct BiomeCommitReport {
    pub resolution_failures: usize,
    pub revision_overflow: bool,
    pub changed: bool,
}

impl WorldAuthority {
    /// Resolves live biome definitions and advances their session revision exactly once.
    pub fn apply_biome_definitions(
        &mut self,
        definitions: Arc<[BiomeDefinitionEvent]>,
    ) -> BiomeCommitReport {
        let live = definitions
            .iter()
            .map(|definition| assets::LiveBiomeDefinition {
                name: &definition.name,
                biome_id: definition.biome_id,
                temperature: definition.temperature,
                downfall: definition.downfall,
                map_water_argb: definition.map_water_color,
            })
            .collect::<Vec<_>>();
        let Ok(resolved) = self.runtime_assets.biome_assets().resolve_live(&live) else {
            return BiomeCommitReport {
                resolution_failures: 1,
                revision_overflow: false,
                changed: false,
            };
        };
        let resolution_failures = resolved.skipped_definitions;
        let Some(next_revision) = self.biome_tint_revision.checked_add(1) else {
            return BiomeCommitReport {
                resolution_failures,
                revision_overflow: true,
                changed: false,
            };
        };
        self.biome_tint_revision = next_revision;
        self.biome_definitions = definitions;
        self.resolved_biome_tints = Arc::new(resolved);
        BiomeCommitReport {
            resolution_failures,
            revision_overflow: false,
            changed: true,
        }
    }
}
