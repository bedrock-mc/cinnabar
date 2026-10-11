//! Queues first-use pipeline variants as soon as a view exists, so loading absorbs compilation.

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(test)]
use bevy::prelude::{Handle, Mut, Shader, World, default};
use bevy::{
    prelude::{
        App, BevyError, IntoScheduleConfigs, Local, Msaa, Query, Res, ResMut, Resource, Result,
        SubApp, SystemSet, error,
    },
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{
            CachedPipelineState, CachedRenderPipelineId, PipelineCache, TextureFormat,
        },
        view::ViewTarget,
    },
    shader::ShaderCacheError,
};

/// True once every registered variant for the current views has compiled.
#[derive(Resource, Clone, Debug, Default)]
pub struct PipelineWarmupReadiness(Arc<AtomicBool>);

impl PipelineWarmupReadiness {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// The view properties our built-in pipeline keys depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WarmView {
    pub msaa: Msaa,
    pub hdr: bool,
    pub enhanced: bool,
    /// The surface or image format the view finally writes; absent until its target exists.
    pub output: Option<TextureFormat>,
}

pub(crate) type WarmupIds = Vec<CachedRenderPipelineId>;

/// Implemented by each pipeline owner so warmup fills the same memoized variants drawing uses.
pub(crate) trait PrewarmPipelines:
    Resource<Mutability = bevy::ecs::component::Mutable>
{
    /// Pushes the ID of every variant a draw for `view` may later request.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError>;
}

/// Implemented by owners that compile outside the pipeline cache, as their inputs appear.
pub(crate) trait PendingPipelines: Resource {
    /// Whether a pipeline a draw may request this frame is still compiling.
    fn pending(&self) -> bool;
}

#[derive(Resource, Default)]
struct WarmupRegistry {
    views: Vec<WarmView>,
    ids: WarmupIds,
    failed: HashSet<CachedRenderPipelineId>,
    /// Owned compiles still running this frame.
    pending: bool,
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
enum WarmupSet {
    Views,
    Owners,
}

#[derive(Resource)]
struct WarmupOwner<T>(std::marker::PhantomData<T>);

/// Idempotent, so plugins that install from both `build` and `finish` may call it twice.
pub(crate) fn register<T: PrewarmPipelines>(app: &mut App) {
    let Some(render_app) = install(app) else {
        return;
    };
    if !render_app.world().contains_resource::<WarmupOwner<T>>() {
        render_app
            .insert_resource(WarmupOwner::<T>(std::marker::PhantomData))
            .add_systems(Render, prewarm_owner::<T>.in_set(WarmupSet::Owners));
    }
}

/// Holds readiness while `T` reports a compile in flight; idempotent like [`register`].
pub(crate) fn register_pending<T: PendingPipelines>(app: &mut App) {
    let Some(render_app) = install(app) else {
        return;
    };
    if !render_app.world().contains_resource::<WarmupOwner<T>>() {
        render_app
            .insert_resource(WarmupOwner::<T>(std::marker::PhantomData))
            .add_systems(
                Render,
                owner_pending::<T>
                    .in_set(RenderSystems::Cleanup)
                    .before(publish_readiness),
            );
    }
}

fn install(app: &mut App) -> Option<&mut SubApp> {
    app.init_resource::<PipelineWarmupReadiness>();
    let shared = app.world().resource::<PipelineWarmupReadiness>().clone();
    let render_app = app.get_sub_app_mut(RenderApp)?;
    if !render_app.world().contains_resource::<WarmupRegistry>() {
        render_app
            .insert_resource(shared)
            .init_resource::<WarmupRegistry>()
            .configure_sets(
                Render,
                (WarmupSet::Views, WarmupSet::Owners)
                    .chain()
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue),
            )
            .add_systems(Render, collect_views.in_set(WarmupSet::Views))
            .add_systems(Render, publish_readiness.in_set(RenderSystems::Cleanup));
    }
    Some(render_app)
}

fn collect_views(
    views: Query<(
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
        Option<&ViewTarget>,
        Option<&crate::EnhancedRendering>,
    )>,
    mut registry: ResMut<WarmupRegistry>,
) {
    registry.views.clear();
    registry.pending = false;
    for (camera, msaa, target, enhanced) in &views {
        let key = WarmView {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: enhanced.is_some(),
            output: target.and_then(ViewTarget::out_texture_view_format),
        };
        if !registry.views.contains(&key) {
            registry.views.push(key);
        }
    }
}

/// Specializes once per view configuration; later frames only compare keys.
fn prewarm_owner<T: PrewarmPipelines>(
    owner: Option<ResMut<T>>,
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
    mut warmed: Local<Vec<WarmView>>,
) {
    // An owner that never initialised has nothing it could draw.
    let Some(mut owner) = owner else {
        return;
    };
    let WarmupRegistry { views, ids, .. } = &mut *registry;
    for &view in views.iter() {
        if warmed.contains(&view) {
            continue;
        }
        // The draw path would fail the same way, so the error does not hold loading.
        if let Err(error) = owner.prewarm(&cache, view, ids) {
            error!("pipeline prewarm {}: {error}", std::any::type_name::<T>());
        }
        warmed.push(view);
    }
}

fn owner_pending<T: PendingPipelines>(owner: Option<Res<T>>, mut registry: ResMut<WarmupRegistry>) {
    registry.pending |= owner.is_some_and(|owner| owner.pending());
}

fn publish_readiness(
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
    shared: Res<PipelineWarmupReadiness>,
) {
    let ready = registered_pipelines_ready(&cache, &mut registry);
    shared.0.store(ready, Ordering::Release);
}

/// Shader loads keep loading held; terminal compile errors are logged once and stop blocking.
fn registered_pipelines_ready(cache: &PipelineCache, registry: &mut WarmupRegistry) -> bool {
    let WarmupRegistry {
        views,
        ids,
        failed,
        pending,
    } = registry;
    let mut ready = !views.is_empty() && !*pending;
    for &id in ids.iter() {
        ready &= match cache.get_render_pipeline_state(id) {
            CachedPipelineState::Ok(_) => true,
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => false,
            CachedPipelineState::Err(error) => {
                if failed.insert(id) {
                    error!("pipeline prewarm {id:?}: {error}");
                }
                true
            }
            _ => false,
        };
    }
    ready
}

#[cfg(test)]
#[path = "pipeline_warmup/tests.rs"]
mod tests;
