use std::collections::HashMap;

use bevy::prelude::{
    Add, Entity, On, Query, Remove, Res, ResMut, Resource, Transform, Visibility, With,
};
use chunk_pipeline::{CaveVisibilityWork, CaveVisibleSet, WorldStream};
use render::{ChunkRenderInstance, RuntimeStage, RuntimeStageProfiler};
use world::SubChunkKey;

use crate::{
    camera::FlyCamera,
    runtime::{telemetry::camera_sub_chunk_key, world::ClientWorld},
};
use diagnostics::metrics::{DiagnosticQuadTracker, MetricsCollector};

#[cfg(feature = "developer-control")]
mod telemetry;

#[derive(Resource, Default)]
pub(crate) struct CaveVisibilityCache {
    pub(crate) camera: Option<SubChunkKey>,
    pub(crate) graph_generation: Option<u64>,
    pub(crate) visible: CaveVisibleSet,
    next_visible: CaveVisibleSet,
    scratch: chunk_pipeline::CaveVisibilityScratch,
    pub(crate) rendered: HashMap<SubChunkKey, Entity>,
    pub(crate) visible_rendered: usize,
    pub(crate) initialized: bool,
    #[cfg(feature = "developer-control")]
    telemetry: telemetry::CaveFrameTrace,
}

impl CaveVisibilityCache {
    pub(crate) fn is_visible(&self, key: SubChunkKey) -> bool {
        !self.initialized || self.visible.contains(&key)
    }

    /// Adopts `next_visible`, calling `set` only for rendered entities whose visibility flips.
    fn publish_next(&mut self, mut set: impl FnMut(Entity, bool)) {
        std::mem::swap(&mut self.visible, &mut self.next_visible);
        if !std::mem::replace(&mut self.initialized, true) {
            // Everything counted as visible until the first result.
            self.visible_rendered = 0;
            for (key, &entity) in &self.rendered {
                let visible = self.visible.contains(key);
                if !visible {
                    set(entity, false);
                }
                self.visible_rendered += usize::from(visible);
            }
            return;
        }
        let (previous, current) = (&self.next_visible, &self.visible);
        for (key, visible) in previous
            .iter()
            .filter(|key| !current.contains(key))
            .map(|key| (key, false))
            .chain(
                current
                    .iter()
                    .filter(|key| !previous.contains(key))
                    .map(|key| (key, true)),
            )
        {
            if let Some(&entity) = self.rendered.get(&key) {
                set(entity, visible);
                if visible {
                    self.visible_rendered += 1;
                } else {
                    self.visible_rendered = self.visible_rendered.saturating_sub(1);
                }
            }
        }
    }

    /// Graph additions can only reveal entities, so publication visits just the added keys.
    fn publish_additions(&mut self, mut set: impl FnMut(Entity, bool)) {
        for key in self.scratch.added_visible() {
            if let Some(&entity) = self.rendered.get(key) {
                set(entity, true);
                self.visible_rendered += 1;
            }
        }
    }

    /// Hides a box only with a current camera and graph result, when every
    /// sub-chunk it touches is known and invisible.
    pub(crate) fn hides_box(
        &self,
        camera: SubChunkKey,
        generation: u64,
        known: impl Fn(SubChunkKey) -> bool,
        low: [f32; 3],
        high: [f32; 3],
    ) -> bool {
        if !self.initialized
            || self.camera != Some(camera)
            || self.graph_generation != Some(generation)
            || low.iter().chain(&high).any(|value| !value.is_finite())
        {
            return false;
        }
        let section = |value: f32| (value.floor() as i32).div_euclid(16);
        for x in section(low[0])..=section(high[0]) {
            for y in section(low[1])..=section(high[1]) {
                for z in section(low[2])..=section(high[2]) {
                    let key = SubChunkKey::new(camera.dimension, x, y, z);
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
    mut chunks: Query<&mut Visibility, With<ChunkRenderInstance>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::CaveVisibility));
    #[cfg(feature = "developer-control")]
    let started = std::time::Instant::now();
    let work = match (client_world.stream.as_ref(), camera.single()) {
        (Some(stream), Ok(camera)) => refresh_visible(
            stream,
            camera_sub_chunk_key(stream.current_dimension(), camera.translation),
            &mut cache,
            &mut chunks,
        ),
        _ => CaveVisibilityWork::default(),
    };
    #[cfg(feature = "developer-control")]
    {
        let camera = cache.camera;
        cache.telemetry.record(camera, work, started.elapsed());
    }
    #[cfg(not(feature = "developer-control"))]
    let _ = work;
}

/// Updates traversal and publishes only entities whose cave visibility changed.
fn refresh_visible(
    stream: &WorldStream,
    camera_key: SubChunkKey,
    cache: &mut CaveVisibilityCache,
    chunks: &mut Query<&mut Visibility, With<ChunkRenderInstance>>,
) -> CaveVisibilityWork {
    let generation = stream.connectivity_generation();
    if cache.camera == Some(camera_key)
        && cache.graph_generation == Some(generation)
        && cache.initialized
    {
        return CaveVisibilityWork::default();
    }

    let rebuilt = stream.update_cave_visible_sub_chunks(
        camera_key,
        &mut cache.scratch,
        &mut cache.visible,
        &mut cache.next_visible,
    );
    cache.camera = Some(camera_key);
    cache.graph_generation = Some(generation);
    if rebuilt && cache.initialized && cache.visible == cache.next_visible {
        return cache.scratch.work();
    }
    let set = |entity, visible| {
        let Ok(mut visibility) = chunks.get_mut(entity) else {
            return;
        };
        let desired = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
        }
    };
    if rebuilt {
        cache.publish_next(set);
    } else {
        cache.publish_additions(set);
    }
    cache.scratch.work()
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
    if cache.rendered.insert(key, add.entity).is_none() && is_visible {
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
    // A replacement entity at the same key may already own the slot.
    if cache.rendered.get(&key) == Some(&remove.entity)
        && cache.rendered.remove(&key).is_some()
        && cache.is_visible(key)
    {
        cache.visible_rendered = cache.visible_rendered.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests;
