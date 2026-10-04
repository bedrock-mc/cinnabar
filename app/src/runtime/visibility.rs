use std::collections::HashSet;

use bevy::prelude::{Add, On, Query, Remove, Res, ResMut, Resource, Transform, Visibility, With};
use render::{ChunkRenderInstance, RuntimeStage, RuntimeStageProfiler};
use world::SubChunkKey;

use crate::{
    camera::FlyCamera,
    runtime::{telemetry::camera_sub_chunk_key, world::ClientWorld},
};
use diagnostics::metrics::{DiagnosticQuadTracker, MetricsCollector};

#[derive(Resource, Default)]
pub(crate) struct CaveVisibilityCache {
    pub(crate) camera: Option<SubChunkKey>,
    pub(crate) graph_generation: Option<u64>,
    pub(crate) visible: HashSet<SubChunkKey>,
    next_visible: HashSet<SubChunkKey>,
    scratch: chunk_pipeline::CaveVisibilityScratch,
    pub(crate) rendered: HashSet<SubChunkKey>,
    pub(crate) visible_rendered: usize,
    pub(crate) initialized: bool,
}

impl CaveVisibilityCache {
    pub(crate) fn is_visible(&self, key: SubChunkKey) -> bool {
        !self.initialized || self.visible.contains(&key)
    }

    /// Whether the culler hides the box from `low` to `high` in `dimension`: as vanilla's
    /// `isAABBVisible`, only when the cache matches graph `generation` and every sub-chunk the
    /// box overlaps is `known` to that graph without being visible.
    pub(crate) fn hides_box(
        &self,
        dimension: i32,
        generation: u64,
        known: impl Fn(SubChunkKey) -> bool,
        low: [f32; 3],
        high: [f32; 3],
    ) -> bool {
        if !self.initialized
            || self
                .camera
                .is_none_or(|camera| camera.dimension != dimension)
            || self.graph_generation != Some(generation)
            || low.iter().chain(&high).any(|value| !value.is_finite())
        {
            return false;
        }
        let section = |value: f32| (value.floor() as i32).div_euclid(16);
        for x in section(low[0])..=section(high[0]) {
            for y in section(low[1])..=section(high[1]) {
                for z in section(low[2])..=section(high[2]) {
                    let key = SubChunkKey::new(dimension, x, y, z);
                    if !known(key) || self.visible.contains(&key) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[derive(Resource)]
pub(crate) struct AppMetrics(pub(crate) MetricsCollector);

#[derive(Resource, Default)]
pub(crate) struct DiagnosticQuads(pub(crate) DiagnosticQuadTracker);

pub(crate) fn refresh_cave_visibility(
    client_world: Res<ClientWorld>,
    camera: Query<&Transform, With<FlyCamera>>,
    mut cache: ResMut<CaveVisibilityCache>,
    mut chunks: Query<(&ChunkRenderInstance, &mut Visibility)>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::CaveVisibility));
    let (Some(stream), Ok(camera)) = (client_world.stream.as_ref(), camera.single()) else {
        return;
    };
    let camera_key = camera_sub_chunk_key(stream.current_dimension(), camera.translation);
    let generation = stream.connectivity_generation();
    if cache.camera == Some(camera_key)
        && cache.graph_generation == Some(generation)
        && cache.initialized
    {
        return;
    }

    let cache = &mut *cache;
    stream.cave_visible_sub_chunks_into(camera_key, &mut cache.scratch, &mut cache.next_visible);
    cache.camera = Some(camera_key);
    cache.graph_generation = Some(generation);
    if cache.initialized && cache.visible == cache.next_visible {
        return;
    }
    std::mem::swap(&mut cache.visible, &mut cache.next_visible);
    cache.initialized = true;
    cache.visible_rendered = 0;
    for (instance, mut visibility) in &mut chunks {
        let key = instance.key();
        let is_visible = cache.visible.contains(&key);
        let desired = if is_visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
        }
        cache.visible_rendered += usize::from(is_visible);
    }
}

pub(crate) fn apply_added_chunk_visibility(
    add: On<Add, ChunkRenderInstance>,
    mut cache: ResMut<CaveVisibilityCache>,
    mut chunks: Query<(&ChunkRenderInstance, &mut Visibility)>,
) {
    let Ok((instance, mut visibility)) = chunks.get_mut(add.entity) else {
        return;
    };
    let key = instance.key();
    let is_visible = cache.is_visible(key);
    *visibility = if is_visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if cache.rendered.insert(key) && is_visible {
        cache.visible_rendered += 1;
    }
}

pub(crate) fn remove_chunk_visibility(
    remove: On<Remove, ChunkRenderInstance>,
    mut cache: ResMut<CaveVisibilityCache>,
    chunks: Query<&ChunkRenderInstance>,
) {
    let Ok(instance) = chunks.get(remove.entity) else {
        return;
    };
    let key = instance.key();
    if cache.rendered.remove(&key) && cache.is_visible(key) {
        cache.visible_rendered = cache.visible_rendered.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An actor is hidden only when every sub-chunk its box touches is known and not visible.
    #[test]
    fn a_box_is_hidden_only_when_all_its_known_sub_chunks_are_invisible() {
        let key = |x, y, z| SubChunkKey::new(0, x, y, z);
        let cache = CaveVisibilityCache {
            camera: Some(key(0, 4, 0)),
            graph_generation: Some(7),
            visible: [key(1, 4, 0)].into_iter().collect(),
            initialized: true,
            ..CaveVisibilityCache::default()
        };
        let known = |key: SubChunkKey| key.y < 8;
        let hides = |low, high| cache.hides_box(0, 7, known, low, high);
        assert!(hides([-8.0, 64.0, 4.0], [-7.0, 66.0, 5.0]));
        // Straddling into the visible neighbour, or reaching an unknown sub-chunk, draws it.
        assert!(!hides([15.5, 64.0, 4.0], [16.5, 66.0, 5.0]));
        assert!(!hides([-8.0, 127.0, 4.0], [-7.0, 129.0, 5.0]));
        // A stale graph or another dimension never hides anything.
        assert!(!cache.hides_box(0, 8, known, [-8.0, 64.0, 4.0], [-7.0, 66.0, 5.0]));
        assert!(!cache.hides_box(1, 7, known, [-8.0, 64.0, 4.0], [-7.0, 66.0, 5.0]));
    }
}
