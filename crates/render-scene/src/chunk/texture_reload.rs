//! Coordinates optional atlas preparation before the main world publishes new material IDs.
use super::{ChunkRenderInstance, ChunkTextureAssetIdentity, ChunkTextureAssets};
use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    requested: Option<ChunkTextureAssets>,
    geometry: Option<Arc<[ChunkRenderInstance]>>,
    holding_geometry: bool,
    geometry_epoch: u64,
    result: Option<Result<ChunkTextureAssetIdentity, String>>,
}

/// One immutable worker input, captured under the reload mailbox's lock.
pub struct ChunkTexturePreparation {
    pub assets: ChunkTextureAssets,
    pub geometry: Option<Arc<[ChunkRenderInstance]>>,
    pub epoch: u64,
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

    /// Drops the retired session snapshot and its readiness acknowledgement. Ordinary handoff
    /// resumes; the renderer completes or rejects the atlas against the surviving session.
    pub fn discard_geometry(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.geometry = None;
        state.holding_geometry = false;
        state.result = state
            .requested
            .as_ref()
            .map(|_| Err("World changed while preparing resource packs; apply again".into()));
        state.geometry_epoch += 1;
    }

    /// Releases only the published request, preserving a newer request or preparation hold.
    pub fn published(&self, identity: ChunkTextureAssetIdentity, epoch: u64) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.geometry_epoch != epoch
            || state
                .requested
                .as_ref()
                .is_none_or(|assets| assets.identity() != identity)
        {
            return;
        }
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

    /// Captures the candidate, geometry and reset epoch together for one worker invocation.
    pub fn preparation(&self) -> Option<ChunkTexturePreparation> {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        Some(ChunkTexturePreparation {
            assets: state.requested.clone()?,
            geometry: state.geometry.clone(),
            epoch: state.geometry_epoch,
        })
    }

    /// Identifies the current session snapshot independently of superseding pack requests.
    pub fn geometry_epoch(&self) -> u64 {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .geometry_epoch
    }

    /// Returns the current candidate without retaining its geometry snapshot.
    pub fn requested(&self) -> Option<ChunkTextureAssets> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .requested
            .clone()
    }

    /// Completes only the requested identity and session epoch; stale worker results retire on drop.
    pub fn finish(&self, identity: ChunkTextureAssetIdentity, epoch: u64, succeeded: bool) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.geometry_epoch == epoch
            && state
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
