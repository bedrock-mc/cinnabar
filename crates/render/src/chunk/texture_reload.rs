//! Coordinates optional atlas preparation before the main world publishes new material IDs.
use super::{ChunkRenderInstance, ChunkTextureAssetIdentity, ChunkTextureAssets};
use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    requested: Option<ChunkTextureAssets>,
    geometry: Option<Arc<[ChunkRenderInstance]>>,
    holding_geometry: bool,
    result: Option<Result<ChunkTextureAssetIdentity, String>>,
}

/// Shared main/render-world mailbox; GPU replacements remain staged until CPU publication.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct ChunkTextureReload(Arc<Mutex<State>>);

impl ChunkTextureReload {
    /// Requests one immutable candidate, replacing any obsolete pending request.
    pub fn request(&self, assets: ChunkTextureAssets) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state
            .requested
            .as_ref()
            .is_none_or(|current| current.identity() != assets.identity())
        {
            state.requested = Some(assets);
            state.geometry = None;
            state.holding_geometry = false;
            state.result = None;
        }
    }

    /// Stages the complete resident mesh set alongside its new material tables.
    pub fn request_geometry(
        &self,
        assets: ChunkTextureAssets,
        geometry: Arc<[ChunkRenderInstance]>,
    ) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state
            .requested
            .as_ref()
            .is_some_and(|current| current.identity() == assets.identity())
            && state.geometry.is_some()
        {
            return;
        }
        state.requested = Some(assets);
        state.geometry = Some(geometry);
        state.holding_geometry = true;
        state.result = None;
    }

    /// Freezes the resident entity set while CPU neighbourhoods are rebuilt.
    pub fn hold_geometry(&self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .holding_geometry = true;
    }

    /// Keeps ordinary mesh handoff from changing the resident set during preparation.
    pub fn geometry_pending(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .holding_geometry
    }

    /// Returns the immutable replacement set for GPU preparation and CPU publication.
    pub fn geometry(&self) -> Option<Arc<[ChunkRenderInstance]>> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .geometry
            .clone()
    }

    /// Drops a replacement set captured for chunks a session reset retired, so ordinary mesh
    /// handoff resumes for the new session; the atlas request still completes and publishes.
    pub(in crate::chunk) fn discard_geometry(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.geometry = None;
        state.holding_geometry = false;
    }

    /// Releases the CPU replacement set after the render world publishes it.
    pub(in crate::chunk) fn published(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.requested = None;
        state.geometry = None;
        state.holding_geometry = false;
        state.result = None;
    }

    /// Reports whether this candidate is fully built, without blocking either world.
    pub fn status(&self, identity: ChunkTextureAssetIdentity) -> Option<Result<(), String>> {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state
            .requested
            .as_ref()
            .is_none_or(|assets| assets.identity() != identity)
        {
            return None;
        }
        state
            .result
            .as_ref()
            .map(|result| result.as_ref().map(|_| ()).map_err(Clone::clone))
    }

    /// Retires abandoned requests while preserving an atlas awaiting publication extraction.
    pub fn cancel_except(&self, published: ChunkTextureAssetIdentity) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state
            .requested
            .as_ref()
            .is_none_or(|assets| assets.identity() != published)
        {
            state.requested = None;
            state.geometry = None;
            state.holding_geometry = false;
            state.result = None;
        }
    }

    /// Clones the candidate for the render preparation worker.
    pub(in crate::chunk) fn requested(&self) -> Option<ChunkTextureAssets> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .requested
            .clone()
    }

    /// Completes only the currently requested generation; superseded results retire on drop.
    pub(in crate::chunk) fn finish(&self, identity: ChunkTextureAssetIdentity, succeeded: bool) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state
            .requested
            .as_ref()
            .is_some_and(|assets| assets.identity() == identity)
        {
            state.result = Some(if succeeded {
                Ok(identity)
            } else {
                Err("Resource pack atlas exceeds GPU limits or could not be prepared".into())
            });
        }
    }
}

#[cfg(test)]
#[path = "texture_reload_tests.rs"]
mod tests;
