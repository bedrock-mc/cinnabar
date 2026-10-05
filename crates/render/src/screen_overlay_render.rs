//! Draws camera effects after first-person geometry and before the JSON-UI HUD.
use crate::screen_overlay::{
    MAX_SCREEN_OVERLAY_LAYERS, SCREEN_OVERLAY_TEXTURE_SIDE, ScreenOverlayScene,
};
use crate::screen_overlay_portal::PortalTexture;
use crate::{ChunkAnimationClock, ChunkTextureAssetIdentity, ChunkTextureAssets};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer,
            BufferBindingType, BufferInitDescriptor, BufferSize, BufferUsages,
            CachedRenderPipelineId, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, PipelineCache,
            RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, Specializer, SpecializerKey,
            Texture, TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget},
    },
};
use std::collections::HashMap;

const OVERLAY_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2f6d4c1a-8b73-4e0c-a5d9-61c7b3e8f204");
const UNIFORM_BYTES: usize = std::mem::size_of::<OverlayUniform>();

/// Mirrors `Overlays` in `screen_overlay.wgsl`: each layer is `[r, g, b, alpha, kind, 0, 0, 0]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct OverlayUniform {
    header: [f32; 4],
    fire: [f32; 4],
    projection: [f32; 4],
    portal_frames: [f32; 4],
    portal_from_clip: [[f32; 4]; 4],
    layers: [[f32; 8]; MAX_SCREEN_OVERLAY_LAYERS],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScreenOverlayRenderPlugin;

impl Plugin for ScreenOverlayRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }

    fn finish(&self, app: &mut App) {
        install(app);
    }
}

#[derive(Resource)]
struct Installed;

fn install(app: &mut App) {
    app.init_resource::<ScreenOverlayScene>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<ScreenOverlayScene>::default());
    load_internal_asset!(
        app,
        OVERLAY_SHADER_HANDLE,
        "screen_overlay.wgsl",
        crate::shader_safety::from_wgsl
    );
    app.sub_app_mut(RenderApp)
        .insert_resource(Installed)
        .init_resource::<OverlayPipeline>()
        .add_render_command::<Transparent3d, DrawOverlayCommands>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare_overlay.in_set(RenderSystems::PrepareResources),
                prepare_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_overlay
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
struct OverlayGpu {
    uniform: Buffer,
    sampler: Sampler,
    _texture: Texture,
    texture_view: TextureView,
    _fire_texture: Texture,
    fire_view: TextureView,
    fire_sampler: Sampler,
    fire_revision: Option<u64>,
    fire_present: bool,
    _portal_texture: Texture,
    portal_texture_view: TextureView,
    portal_sampler: Sampler,
    portal: Option<PortalTexture>,
    portal_identity: Option<ChunkTextureAssetIdentity>,
    textures_revision: Option<u64>,
    layer_count: u32,
    bind_group: Option<BindGroup>,
    view_pipelines: HashMap<Entity, CachedRenderPipelineId>,
}

fn texture_array(
    device: &RenderDevice,
    queue: &RenderQueue,
    side: u32,
    layers: u32,
    pixels: &[u8],
) -> (Texture, TextureView) {
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("screen overlay textures"),
            size: Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        pixels,
    );
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some("screen overlay texture layers"),
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    (texture, view)
}

fn init_gpu(mut commands: Commands, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let (texture, texture_view) = texture_array(&device, &queue, 1, 2, &[255; 8]);
    let (fire_texture, fire_view) = texture_array(&device, &queue, 1, 1, &[0; 4]);
    let (portal_texture, portal_texture_view) = texture_array(&device, &queue, 1, 1, &[0; 4]);
    commands.insert_resource(OverlayGpu {
        uniform: device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("screen overlay layers"),
            contents: bytemuck::bytes_of(&OverlayUniform::default()),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        }),
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("screen overlay sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Nearest,
            ..default()
        }),
        _texture: texture,
        texture_view,
        _fire_texture: fire_texture,
        fire_view,
        fire_sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("native fire point sampler"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            ..default()
        }),
        fire_revision: None,
        fire_present: false,
        _portal_texture: portal_texture,
        portal_texture_view,
        portal_sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("portal overlay atlas sampler"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            ..default()
        }),
        portal: None,
        portal_identity: None,
        textures_revision: None,
        layer_count: 0,
        bind_group: None,
        view_pipelines: HashMap::new(),
    });
}

fn prepare_overlay(
    scene: Res<ScreenOverlayScene>,
    assets: Option<Res<ChunkTextureAssets>>,
    clock: Option<Res<ChunkAnimationClock>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<OverlayGpu>,
) {
    if gpu.textures_revision != Some(scene.textures_revision) {
        let (texture, view) = match &scene.textures {
            Some(textures) => texture_array(
                &device,
                &queue,
                SCREEN_OVERLAY_TEXTURE_SIDE,
                2,
                &textures.layer_major(),
            ),
            None => texture_array(&device, &queue, 1, 2, &[255; 8]),
        };
        gpu._texture = texture;
        gpu.texture_view = view;
        gpu.textures_revision = Some(scene.textures_revision);
        gpu.bind_group = None;
    }
    if gpu.fire_revision != Some(scene.fire_revision) {
        gpu.fire_present = scene
            .fire
            .as_ref()
            .is_some_and(|fire| fire.frames <= device.limits().max_texture_array_layers);
        let (texture, view) = match &scene.fire {
            Some(fire) if gpu.fire_present => {
                texture_array(&device, &queue, fire.side, fire.frames, &fire.pixels)
            }
            Some(_) => {
                warn!(
                    "camera fire animation exceeds this adapter's texture layer limit; skipping effect"
                );
                texture_array(&device, &queue, 1, 1, &[0; 4])
            }
            None => texture_array(&device, &queue, 1, 1, &[0; 4]),
        };
        gpu._fire_texture = texture;
        gpu.fire_view = view;
        gpu.fire_revision = Some(scene.fire_revision);
        gpu.bind_group = None;
    }
    let identity = assets.as_deref().map(ChunkTextureAssets::identity);
    if gpu.portal_identity != identity {
        let portal = assets
            .as_deref()
            .and_then(|assets| PortalTexture::from_assets(assets.assets()))
            .filter(|portal| portal.layer_count() <= device.limits().max_texture_array_layers);
        let (texture, view) = portal.as_ref().map_or_else(
            || texture_array(&device, &queue, 1, 1, &[0; 4]),
            |portal| {
                texture_array(
                    &device,
                    &queue,
                    portal.side,
                    portal.layer_count(),
                    &portal.pixels,
                )
            },
        );
        gpu._portal_texture = texture;
        gpu.portal_texture_view = view;
        gpu.portal = portal;
        gpu.portal_identity = identity;
        gpu.bind_group = None;
    }
    let count = scene.layers.len().min(MAX_SCREEN_OVERLAY_LAYERS);
    gpu.layer_count = count as u32;
    if count == 0 {
        return;
    }
    let mut uniform = OverlayUniform {
        fire: scene
            .fire
            .as_ref()
            .filter(|_| gpu.fire_present)
            .map_or([0.0; 4], |fire| {
                fire.sample(clock.as_deref().copied().unwrap_or_default())
            }),
        projection: [scene.fire_projection[0], scene.fire_projection[1], 0.0, 0.0],
        header: [
            count as f32,
            scene.clock_seconds,
            if scene.textures.is_some() { 1.0 } else { 0.0 },
            0.0,
        ],
        portal_frames: gpu.portal.as_ref().zip(assets.as_deref()).map_or(
            [0.0; 4],
            |(portal, assets)| {
                portal.frame_uniform(
                    assets.assets(),
                    clock.as_deref().copied().unwrap_or_default(),
                )
            },
        ),
        portal_from_clip: scene.portal_from_clip.to_cols_array_2d(),
        layers: [[0.0; 8]; MAX_SCREEN_OVERLAY_LAYERS],
    };
    for (slot, layer) in uniform.layers.iter_mut().zip(&scene.layers) {
        *slot = [
            layer.rgb[0],
            layer.rgb[1],
            layer.rgb[2],
            layer.alpha,
            layer.kind as u32 as f32,
            0.0,
            0.0,
            0.0,
        ];
    }
    queue.write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&uniform));
}

struct OverlayPipelineSpecializer;

#[derive(Resource)]
struct OverlayPipeline {
    variants: Variants<RenderPipeline, OverlayPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for OverlayPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "screen overlay bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(UNIFORM_BYTES as u64),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 5,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("screen overlay pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: OVERLAY_SHADER_HANDLE,
                entry_point: Some("overlay_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: OVERLAY_SHADER_HANDLE,
                entry_point: Some("overlay_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::Always,
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(OverlayPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct OverlayPipelineKey {
    msaa: Msaa,
    hdr: bool,
    after_hand: bool,
}

impl Specializer<RenderPipeline> for OverlayPipelineSpecializer {
    type Key = OverlayPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        if key.after_hand {
            descriptor.depth_stencil = None;
        }
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .format = if key.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        Ok(key)
    }
}

fn prepare_bind_group(
    device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<OverlayPipeline>,
    mut gpu: ResMut<OverlayGpu>,
) {
    if gpu.bind_group.is_some() {
        return;
    }
    gpu.bind_group = Some(device.create_bind_group(
        "screen overlay bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: gpu.uniform.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(&gpu.texture_view),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::TextureView(&gpu.fire_view),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::Sampler(&gpu.fire_sampler),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::TextureView(&gpu.portal_texture_view),
            },
            BindGroupEntry {
                binding: 6,
                resource: BindingResource::Sampler(&gpu.portal_sampler),
            },
        ],
    ));
}

fn queue_overlay(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<OverlayPipeline>,
    scene: Res<ScreenOverlayScene>,
    (ui, mut gpu): (Option<Res<crate::ui_render::UiGpu>>, ResMut<OverlayGpu>),
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
) {
    gpu.view_pipelines.clear();
    if scene.layers.is_empty() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawOverlayCommands>();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            OverlayPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
                after_hand: ui.is_some(),
            },
        ) else {
            continue;
        };
        if ui.is_some() {
            gpu.view_pipelines.insert(view_entity, pipeline_id);
            continue;
        }
        phase.add(Transparent3d {
            entity: (view_entity, *main_entity),
            pipeline: pipeline_id,
            draw_function,
            // Sorts after every real transparent item.
            distance: f32::MAX,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        });
    }
}

/// The HUD graph calls this after the hand pass, including Enhanced's post-grade path.
pub(crate) fn draw_before_hud(
    view: Entity,
    target: &ViewTarget,
    camera: &bevy::render::camera::ExtractedCamera,
    resolution: Option<&bevy::camera::MainPassResolutionOverride>,
    context: &mut RenderContext,
    world: &World,
) {
    if world
        .get_resource::<crate::PanoramaScene>()
        .is_some_and(|scene| !scene.game_visible())
    {
        return;
    }
    let (Some(gpu), Some(cache)) = (
        world.get_resource::<OverlayGpu>(),
        world.get_resource::<PipelineCache>(),
    ) else {
        return;
    };
    if gpu.layer_count == 0 {
        return;
    }
    let (Some(binding), Some(pipeline)) = (
        &gpu.bind_group,
        gpu.view_pipelines
            .get(&view)
            .and_then(|id| cache.get_render_pipeline(*id)),
    ) else {
        return;
    };
    let attachments = [Some(target.get_color_attachment())];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("camera effects before HUD"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    if let Some(viewport) =
        crate::ui_render::overlay::overlay_viewport(camera.viewport.as_ref(), resolution)
    {
        pass.set_camera_viewport(&viewport);
    }
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}

type DrawOverlayCommands = (SetItemPipeline, SetOverlayBindGroup, DrawOverlay);

struct SetOverlayBindGroup;

impl<P: PhaseItem> RenderCommand<P> for SetOverlayBindGroup {
    type Param = SRes<OverlayGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = &gpu.into_inner().bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[]);
        RenderCommandResult::Success
    }
}

struct DrawOverlay;

impl<P: PhaseItem> RenderCommand<P> for DrawOverlay {
    type Param = SRes<OverlayGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        if gpu.into_inner().layer_count == 0 {
            return RenderCommandResult::Skip;
        }
        pass.draw(0..3, 0..1);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_matches_the_wgsl_layout() {
        assert_eq!(UNIFORM_BYTES, 4 * 16 + 64 + MAX_SCREEN_OVERLAY_LAYERS * 32);
    }

    #[test]
    fn fire_shader_resources_match_the_pipeline_layout() {
        let mut world = World::new();
        let pipeline = OverlayPipeline::from_world(&mut world);
        crate::shader_test_support::assert_binding_visibility(
            include_str!("screen_overlay.wgsl"),
            0,
            &pipeline.bind_group_layout,
        );
    }

    #[test]
    fn fire_draws_after_the_hand_without_inheriting_world_depth() {
        let (mut app, _) = crate::queue_review_support::app();
        let cache = app.world().resource::<PipelineCache>();
        let mut pipeline = OverlayPipeline::from_world(&mut World::new());
        let id = pipeline
            .variants
            .specialize(
                cache,
                OverlayPipelineKey {
                    msaa: Msaa::Sample4,
                    hdr: false,
                    after_hand: true,
                },
            )
            .unwrap();
        let mut cache = app.world_mut().resource_mut::<PipelineCache>();
        let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
        assert!(descriptor.depth_stencil.is_none());
        assert_eq!(descriptor.multisample.count, 4);
        assert_eq!(
            descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend,
            Some(BlendState::ALPHA_BLENDING)
        );
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::queue_review_support as fixture;
    use bevy::ecs::system::RunSystemOnce;
    #[test]
    fn review_render_overlay_queue_uses_current_layers() {
        let (mut app, view) = fixture::app();
        app.init_resource::<ScreenOverlayScene>()
            .init_resource::<OverlayPipeline>()
            .add_render_command::<Transparent3d, DrawOverlayCommands>();
        app.world_mut().run_system_once(init_gpu).unwrap();
        app.world_mut()
            .resource_mut::<ScreenOverlayScene>()
            .set_layers(
                [crate::ScreenOverlayLayer {
                    kind: crate::ScreenOverlayKind::Flat,
                    rgb: [1.0; 3],
                    alpha: 1.0,
                }],
                0.0,
            );
        app.world_mut().run_system_once(queue_overlay).unwrap();
        assert_eq!(fixture::items(&app, view).len(), 1);
        fixture::clear(&mut app, view);
        app.world_mut().resource_mut::<OverlayGpu>().layer_count = 1;
        app.world_mut()
            .resource_mut::<ScreenOverlayScene>()
            .set_layers([], 0.0);
        app.world_mut().run_system_once(queue_overlay).unwrap();
        assert!(fixture::items(&app, view).is_empty());
    }
}
