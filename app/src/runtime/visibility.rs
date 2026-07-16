use crate::*;

#[derive(Resource, Default)]
pub(crate) struct CaveVisibilityCache {
    pub(crate) camera: Option<SubChunkKey>,
    pub(crate) graph_generation: Option<u64>,
    pub(crate) visible: BTreeSet<SubChunkKey>,
    pub(crate) rendered: HashSet<SubChunkKey>,
    pub(crate) visible_rendered: usize,
    pub(crate) initialized: bool,
}

impl CaveVisibilityCache {
    pub(crate) fn is_visible(&self, key: SubChunkKey) -> bool {
        !self.initialized || self.visible.contains(&key)
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
) {
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

    cache.visible = stream.cave_visible_sub_chunks(camera_key);
    cache.camera = Some(camera_key);
    cache.graph_generation = Some(generation);
    cache.initialized = true;
    cache.rendered.clear();
    cache.visible_rendered = 0;
    for (instance, mut visibility) in &mut chunks {
        let key = instance.key();
        cache.rendered.insert(key);
        let is_visible = cache.visible.contains(&key);
        *visibility = if is_visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
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
