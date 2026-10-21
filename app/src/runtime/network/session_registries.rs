//! Prepares session block registries against captured immutable carrier inputs.

use std::sync::Arc;

use gameplay::movement::{PhysicsCollisionRegistries, session_blocks::SessionBlockRegistration};
use simulation_clock::identity::SessionGeneration;

use super::resource_packs::{PackApplication, session_runtime_assets};

/// Cheap snapshots of the exact carrier inputs a join will publish over.
#[derive(Clone)]
pub(crate) struct RegistryBase {
    assets: Arc<assets::RuntimeAssets>,
    collisions: PhysicsCollisionRegistries,
}

impl std::fmt::Debug for RegistryBase {
    /// Shows registry counts without formatting the opaque carrier asset tables.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegistryBase")
            .field("visual_count", &self.assets.visual_count())
            .field(
                "collision_records",
                &self.collisions.available_record_count(),
            )
            .finish_non_exhaustive()
    }
}

impl RegistryBase {
    /// Captures shared registry tables without copying them on the frame thread.
    pub(crate) fn new(
        assets: &Arc<assets::RuntimeAssets>,
        collisions: &PhysicsCollisionRegistries,
    ) -> Self {
        Self {
            assets: Arc::clone(assets),
            collisions: collisions.clone(),
        }
    }

    /// Builds a join's block facts on its existing preparation worker, publishing nothing on cancel.
    pub(super) fn prepare(
        self,
        generation: SessionGeneration,
        hashed: bool,
        custom: &protocol::CustomBlocks,
        packs: &PackApplication,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Option<PreparedSessionRegistries> {
        if cancelled() {
            return None;
        }
        let mut collisions = self.collisions;
        let registration = collisions.begin_session_blocks(custom, hashed);
        if cancelled() {
            return None;
        }
        let mode = if hashed {
            assets::NetworkIdMode::Hashed
        } else {
            assets::NetworkIdMode::Sequential
        };
        let identities = client_world::PreparedBlockIdentities::new(
            custom,
            mode,
            registration.custom_ids.clone().unwrap_or_default(),
        );
        if cancelled() {
            return None;
        }
        // Hashed visuals append after the base carrier, independent of their network hashes.
        let overlay_ids = if hashed {
            packs.block_overlay.as_ref().map(|compiled| {
                let first = self.assets.visual_count() as u32;
                first..first + compiled.overlay.visuals.len() as u32
            })
        } else {
            registration.custom_ids.clone()
        };
        let assets = session_runtime_assets(
            &self.assets,
            overlay_ids.as_ref(),
            packs.block_overlay.as_ref(),
        );
        let mining_filters = Arc::new(pack_runtime::CustomMiningFilters::new(custom));
        (!cancelled()).then_some(PreparedSessionRegistries {
            generation,
            hashed,
            source_assets: self.assets,
            custom: custom.clone(),
            collisions,
            registration,
            identities,
            assets,
            mining_filters,
        })
    }
}

/// A complete registry bundle awaiting the matching session's atomic bootstrap boundary.
#[derive(Clone)]
pub(super) struct PreparedSessionRegistries {
    generation: SessionGeneration,
    hashed: bool,
    source_assets: Arc<assets::RuntimeAssets>,
    custom: protocol::CustomBlocks,
    pub(super) collisions: PhysicsCollisionRegistries,
    pub(super) registration: SessionBlockRegistration,
    pub(super) identities: client_world::PreparedBlockIdentities,
    pub(super) assets: Arc<assets::RuntimeAssets>,
    pub(super) mining_filters: Arc<pack_runtime::CustomMiningFilters>,
}

impl std::fmt::Debug for PreparedSessionRegistries {
    /// Shows session identity and counts without formatting the opaque carrier asset tables.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedSessionRegistries")
            .field("generation", &self.generation)
            .field("hashed", &self.hashed)
            .field("custom_blocks", &self.custom.blocks.len())
            .finish_non_exhaustive()
    }
}

impl PreparedSessionRegistries {
    /// Rejects a late or mismatched result before any live session state is changed.
    pub(super) fn matches(
        &self,
        generation: SessionGeneration,
        hashed: bool,
        assets: &Arc<assets::RuntimeAssets>,
        collisions: &PhysicsCollisionRegistries,
        custom: &protocol::CustomBlocks,
    ) -> bool {
        self.generation == generation
            && self.hashed == hashed
            && Arc::ptr_eq(&self.source_assets, assets)
            && self.collisions.shares_carrier_with(collisions)
            && Arc::ptr_eq(&self.custom.blocks, &custom.blocks)
            && Arc::ptr_eq(&self.custom.vanilla_blocks, &custom.vanilla_blocks)
            && self.custom.skipped == custom.skipped
    }
}

#[cfg(test)]
pub(crate) mod tests;
