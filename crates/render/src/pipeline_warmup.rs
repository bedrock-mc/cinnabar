//! Queues first-use pipeline variants as soon as a view exists, so loading absorbs compilation.

use bevy::prelude::IntoScheduleConfigs;
use render_scene::PipelineWarmupReadiness;

use std::collections::HashSet;

#[cfg(test)]
use bevy::prelude::{Handle, Mut, Shader, World, default};
use bevy::{
    prelude::{
        App, BevyError, Has, Local, Msaa, Query, Res, ResMut, Resource, Result, SubApp, SystemSet,
        error,
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

/// The view properties our built-in pipeline keys depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WarmView {
    pub msaa: Msaa,
    pub hdr: bool,
    pub enhanced: bool,
    pub shader_pack: bool,
    /// The surface or image format the view finally writes; absent until its target exists.
    pub output: Option<TextureFormat>,
}

pub(crate) type WarmupIds = Vec<CachedRenderPipelineId>;

/// Enumerates active coverage and Performance's single-sample path without duplicates.
pub(crate) fn sample_modes(current: Msaa) -> impl Iterator<Item = Msaa> {
    [Some(current), (current != Msaa::Off).then_some(Msaa::Off)]
        .into_iter()
        .flatten()
}

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
            .add_systems(
                Render,
                prewarm_owner::<T>
                    .in_set(WarmupSet::Owners)
                    .run_if(render_scene::startup_work::world_work_allowed),
            );
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
                    .run_if(render_scene::startup_work::world_work_allowed)
                    .in_set(RenderSystems::Cleanup)
                    .before(publish_readiness),
            );
    }
}

/// Holds world-entry readiness while an asynchronous GPU owner is not yet installed.
pub(crate) fn register_pending_resource<T: Resource>(app: &mut SubApp) {
    app.init_resource::<WarmupRegistry>();
    app.add_systems(
        Render,
        pending_resource::<T>
            .in_set(RenderSystems::Cleanup)
            .before(publish_readiness),
    );
}

/// Folds the existence of an unfinished initializer into the same pipeline-readiness gate.
fn pending_resource<T: Resource>(pending: Option<Res<T>>, mut registry: ResMut<WarmupRegistry>) {
    registry.pending |= pending.is_some();
}

/// Installs shared scene formats and readiness tracking for custom pipeline owners.
fn install(app: &mut App) -> Option<&mut SubApp> {
    app.init_resource::<PipelineWarmupReadiness>();
    app.init_resource::<render_scene::startup_work::WorldWorkAdmission>();
    let admission = app
        .world()
        .resource::<render_scene::startup_work::WorldWorkAdmission>()
        .clone();
    let shared = app.world().resource::<PipelineWarmupReadiness>().clone();
    let render_app = app.get_sub_app_mut(RenderApp)?;
    render_app.insert_resource(admission);
    crate::scene_target::install_formats(render_app);
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

/// The camera state that selects each view's pipeline variants.
type ViewKeys<'w, 's> = Query<
    'w,
    's,
    (
        &'static bevy::render::camera::ExtractedCamera,
        &'static Msaa,
        Option<&'static ViewTarget>,
        Option<&'static render_scene::EnhancedRendering>,
        Has<render_scene::shaderpack::ShaderPackRendering>,
    ),
>;

fn collect_views(views: ViewKeys, mut registry: ResMut<WarmupRegistry>) {
    registry.views.clear();
    registry.pending = false;
    for (camera, msaa, target, enhanced, shader_pack) in &views {
        let key = WarmView {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: enhanced.is_some(),
            shader_pack,
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
        for msaa in sample_modes(view.msaa) {
            let view = WarmView { msaa, ..view };
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
    shared.set_ready(ready);
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
