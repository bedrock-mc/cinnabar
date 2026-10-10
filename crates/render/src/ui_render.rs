use std::{
    mem::size_of,
    sync::{Arc, Weak},
};

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
    ecs::system::SystemChangeTick,
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::{
            AddressMode, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
            BlendComponent, BlendFactor, BlendOperation, BlendState, Buffer, BufferBindingType,
            BufferDescriptor, BufferInitDescriptor, BufferSize, BufferUsages,
            CachedRenderPipelineId, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, FilterMode, FragmentState, PipelineCache, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            Specializer, SpecializerKey, TextureFormat, TextureSampleType, TextureViewDimension,
            Variants, VertexAttribute, VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget},
    },
};
#[path = "ui_render/font_atlas.rs"]
mod font_atlas;
#[cfg(test)]
#[path = "ui_render/font_atlas_tests.rs"]
mod font_atlas_tests;
#[path = "ui_render/textures.rs"]
mod textures;
pub(crate) use textures::DeviceObservation;
use textures::{UiGpuTextures, prepare_ui_bind_group};
#[path = "ui_render/batches.rs"]
mod batches;
#[path = "ui_render/composite.rs"]
pub(crate) mod composite;
#[path = "ui_render/damage.rs"]
mod damage;
#[path = "ui_render/glint.rs"]
mod glint;
#[cfg(test)]
#[path = "ui_render/harness.rs"]
pub(crate) mod harness;
#[path = "ui_render/layer.rs"]
mod layer;
#[path = "ui_render/model_depth.rs"]
mod model_depth;
#[path = "ui_render/overlay.rs"]
pub(crate) mod overlay;
#[path = "ui_render/pipeline.rs"]
pub(crate) mod pipeline;
#[path = "ui_render/profile.rs"]
pub(crate) mod profile;
#[path = "ui_render/resources.rs"]
pub(crate) mod resources;
#[path = "ui_render/shader.rs"]
pub(crate) mod shader;
#[path = "ui_render/uploads.rs"]
mod uploads;
#[path = "ui_render/viewport.rs"]
mod viewport;
use batches::resolved_batches;
pub use glint::UiGlintSettings;
use overlay::queue_ui_overlay;
pub(crate) use overlay::{UiHandCoverage, UiOverlayLabel, UiWorldLabel, install_overlay_graph};
use shader::UiViewportUniform;

use render_model::{
    FontAtlasVertex, MAX_UI_INDICES, MAX_UI_VERTICES, UI_BLEND_INVERT, UiRenderBatch,
    UiRenderInput, UiRenderRejectReason, UiRenderScene, UiRenderStats, UiRenderVertex,
};

/// Main-world holder of the published [`UiRenderScene`], cloned into the render world.
#[derive(Resource, ExtractResource, Clone, Debug, Default, Deref, DerefMut)]
pub struct UiRenderSceneResource(pub UiRenderScene);

/// The [`UiRenderStats`] handle both worlds share.
#[derive(Resource, Clone, Debug, Default, Deref, DerefMut)]
pub struct UiRenderStatsResource(pub UiRenderStats);

const UI_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7cfb904c-c8cf-4dd2-9214-7d208ce454e7");

#[derive(Debug, Clone, Copy, Default)]
pub struct UiRenderPlugin;

impl Plugin for UiRenderPlugin {
    fn build(&self, app: &mut App) {
        install_ui_render(app);
    }

    fn finish(&self, app: &mut App) {
        install_ui_render(app);
    }
}

#[derive(Resource)]
struct UiRenderInstalled;

fn install_ui_render(app: &mut App) {
    crate::upload_staging::install(app);
    app.init_resource::<UiRenderSceneResource>()
        .init_resource::<UiGlintSettings>()
        .init_resource::<UiRenderStatsResource>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<UiRenderInstalled>() {
        install_overlay_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    crate::pipeline_warmup::register::<pipeline::UiPipeline>(app);
    crate::pipeline_warmup::register::<composite::UiCompositePipeline>(app);
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    app.add_plugins((
        ExtractResourcePlugin::<UiRenderSceneResource>::default(),
        ExtractResourcePlugin::<UiGlintSettings>::default(),
    ));
    load_internal_asset!(app, UI_SHADER_HANDLE, "ui.wgsl", shader::from_wgsl);
    load_internal_asset!(
        app,
        composite::UI_COMPOSITE_SHADER_HANDLE,
        "ui_composite.wgsl",
        crate::shader_safety::from_wgsl
    );
    app.sub_app_mut(RenderApp)
        .insert_resource(UiRenderInstalled)
        .init_resource::<pipeline::UiPipeline>()
        .init_resource::<composite::UiCompositePipeline>()
        .insert_resource(stats)
        .init_resource::<UiHandCoverage>()
        .init_resource::<composite::UiLayerStore>()
        .init_resource::<model_depth::UiModelDepths>()
        .add_systems(RenderStartup, resources::init_ui_gpu)
        .add_systems(Render, ui_systems());
    profile::install(app.sub_app_mut(RenderApp));
    install_overlay_graph(app.sub_app_mut(RenderApp).world_mut());
}

/// Prepares accepted UI resources, attachments, bindings and view pipelines each frame.
fn ui_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (
        resources::prepare_ui_resources.in_set(RenderSystems::PrepareResources),
        composite::prepare_ui_layers
            .in_set(RenderSystems::PrepareResources)
            .after(bevy::render::view::prepare_view_targets),
        prepare_ui_bind_group.in_set(RenderSystems::PrepareBindGroups),
        queue_ui_overlay
            .in_set(RenderSystems::PrepareBindGroups)
            .after(prepare_ui_bind_group),
    )
        .into_configs()
}

#[derive(Resource)]
pub(crate) struct UiGpu {
    device: wgpu::Device,
    device_observation: DeviceObservation,
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertex_capacity: usize,
    index_capacity: usize,
    vertex_arena_id: u64,
    index_arena_id: u64,
    viewport_buffer: Buffer,
    viewport_uploads: viewport::ViewportUploads,
    #[cfg(test)]
    geometry_writes: [usize; 2],
    viewport_size: [u32; 2],
    started: std::time::Instant,
    textures: UiGpuTextures,
    sampler: Sampler,
    /// `bilinear` sprites sample through this instead.
    linear_sampler: Sampler,
    batches: Arc<[UiRenderBatch]>,
    accepted_revision: Option<u64>,
    /// The accepted revision draws glint, which animates without a new revision.
    animated: bool,
    // Admission watermark survives every draw rejection, even after payload drop.
    last_admitted_revision: Option<u64>,
    last_admitted_publication: Weak<UiRenderInput>,
    index_count: usize,
    uploads: uploads::BufferUploads,
    view_pipelines:
        std::collections::BTreeMap<Entity, (CachedRenderPipelineId, CachedRenderPipelineId)>,
    /// Each view's UI-layer composite pipelines.
    composite_pipelines: std::collections::BTreeMap<Entity, composite::CompositePipelines>,
    world_view_pipelines: std::collections::BTreeMap<
        (Entity, bool, bool),
        (CachedRenderPipelineId, CachedRenderPipelineId),
    >,
    model_view_pipelines: std::collections::BTreeMap<
        (Entity, bool, bool),
        (CachedRenderPipelineId, CachedRenderPipelineId),
    >,
}

#[cfg(test)]
#[path = "ui_render/ordered_command_tests.rs"]
mod ordered_command_tests;

#[cfg(test)]
#[path = "ui_render/retained_tests.rs"]
mod retained_tests;
