//! One retained uniform and two texture bindings implement target highlight drawing.

use super::{AimAssistHighlightScene, AimAssistTexture};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
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
            BufferBindingType, BufferDescriptor, BufferId, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CompareFunction, DepthBiasState, DepthStencilState,
            Extent3d, FilterMode, FragmentState, PipelineCache, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            ShaderType, Specializer, SpecializerKey, Texture, TextureDataOrder, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use std::sync::Arc;

const SHADER: Handle<Shader> = uuid_handle!("611dc958-9030-414c-9530-297ebf8f6846");
const RECORD_BYTES: u64 = 48;

#[derive(Clone, Copy, Debug, Default)]
pub struct AimAssistHighlightPlugin;

impl Plugin for AimAssistHighlightPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }
    fn finish(&self, app: &mut App) {
        install(app);
    }
}

#[derive(Resource)]
struct Installed;

/// Delayed render-app creation and headless app tests share the same registration path.
fn install(app: &mut App) {
    app.init_resource::<AimAssistHighlightScene>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<AimAssistHighlightScene>::default());
    load_internal_asset!(
        app,
        SHADER,
        "highlight.wgsl",
        crate::shader_safety::from_wgsl
    );
    app.sub_app_mut(RenderApp)
        .insert_resource(Installed)
        .init_resource::<HighlightPipeline>()
        .add_render_command::<Transparent3d, DrawCommands>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare.in_set(RenderSystems::PrepareBindGroups),
                warm_pipeline
                    .in_set(RenderSystems::Queue)
                    .before(queue_highlight),
                queue_highlight
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Default)]
struct TextureSlot {
    source: Option<Arc<AimAssistTexture>>,
    texture: Option<Texture>,
    view: Option<TextureView>,
    bind_group: Option<BindGroup>,
    view_buffer: Option<BufferId>,
}

#[derive(Resource)]
struct HighlightGpu {
    sampler: Sampler,
    record_buffer: Buffer,
    record: Option<[f32; 12]>,
    slots: [TextureSlot; 2],
    selected: Option<usize>,
    texture_uploads: u64,
    uniform_uploads: u64,
    bind_rebuilds: u64,
}

/// Creates the fixed draw resources before any frame can publish a target.
fn init_gpu(mut commands: Commands, device: Res<RenderDevice>) {
    commands.insert_resource(HighlightGpu {
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("aim highlight sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        record_buffer: device.create_buffer(&BufferDescriptor {
            label: Some("aim highlight pose"),
            size: RECORD_BYTES,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        record: None,
        slots: Default::default(),
        selected: None,
        texture_uploads: 0,
        uniform_uploads: 0,
        bind_rebuilds: 0,
    });
}

/// Only a changed texture identity, target pose, or view buffer incurs GPU preparation work.
fn prepare(
    scene: Res<AimAssistHighlightScene>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<HighlightPipeline>,
    view_uniforms: Res<ViewUniforms>,
    gpu: Option<ResMut<HighlightGpu>>,
) {
    let Some(mut gpu) = gpu else { return };
    let gpu = &mut *gpu;
    let view = view_uniforms
        .uniforms
        .binding()
        .zip(view_uniforms.uniforms.buffer().map(|buffer| buffer.id()));
    for (slot, source) in gpu.slots.iter_mut().zip(&scene.textures) {
        let same = match (&slot.source, source) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            *slot = TextureSlot {
                source: source.clone(),
                ..default()
            };
            if let Some(source) = source {
                let texture = device.create_texture_with_data(
                    &queue,
                    &TextureDescriptor {
                        label: Some("aim highlight texture"),
                        size: Extent3d {
                            width: source.size[0],
                            height: source.size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: TextureFormat::Rgba8UnormSrgb,
                        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                        view_formats: &[],
                    },
                    TextureDataOrder::LayerMajor,
                    &source.rgba,
                );
                slot.view = Some(texture.create_view(&TextureViewDescriptor::default()));
                slot.texture = Some(texture);
                gpu.texture_uploads += 1;
            }
        }
        if let Some((view_binding, view_id)) = view.clone()
            && slot.view_buffer != Some(view_id)
            && let Some(texture_view) = &slot.view
        {
            slot.bind_group = Some(device.create_bind_group(
                "aim highlight bindings",
                &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: view_binding,
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: gpu.record_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: BindingResource::TextureView(texture_view),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: BindingResource::Sampler(&gpu.sampler),
                    },
                ],
            ));
            slot.view_buffer = Some(view_id);
            gpu.bind_rebuilds += 1;
        }
    }
    gpu.selected = scene.target.map(|target| target.texture).filter(|index| {
        gpu.slots
            .get(*index)
            .is_some_and(|slot| slot.bind_group.is_some())
    });
    if let Some(target) = scene.target.filter(|_| gpu.selected.is_some()) {
        let record = target.record();
        if gpu.record != Some(record) {
            queue.write_buffer(&gpu.record_buffer, 0, bytemuck::cast_slice(&record));
            gpu.record = Some(record);
            gpu.uniform_uploads += 1;
        }
    }
}

struct PipelineSpecializer;

#[derive(Resource)]
struct HighlightPipeline {
    variants: Variants<RenderPipeline, PipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for HighlightPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "aim highlight layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(RECORD_BYTES),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("aim highlight pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: SHADER,
                entry_point: Some("highlight_vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: SHADER,
                entry_point: Some("highlight_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::Greater,
                stencil: default(),
                // Reverse depth preserves the native bias toward the camera.
                bias: DepthBiasState {
                    constant: 33,
                    slope_scale: 0.0,
                    clamp: 0.0,
                },
            }),
            ..default()
        };
        Self {
            variants: Variants::new(PipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct PipelineKey {
    msaa: Msaa,
    hdr: bool,
    occluded: bool,
}

impl Specializer<RenderPipeline> for PipelineSpecializer {
    type Key = PipelineKey;
    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        descriptor.depth_stencil.as_mut().unwrap().depth_compare = if key.occluded {
            CompareFunction::Less
        } else {
            CompareFunction::Greater
        };
        descriptor.fragment.as_mut().unwrap().entry_point = Some(
            if key.occluded {
                "highlight_occluded_fragment"
            } else {
                "highlight_fragment"
            }
            .into(),
        );
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

/// Each view requests its pipeline before a target can trigger first-use compilation.
fn warm_pipeline(
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<HighlightPipeline>,
    views: Query<(&ExtractedView, &Msaa)>,
) {
    for (view, msaa) in &views {
        for occluded in [true, false] {
            let _ = pipeline.variants.specialize(
                &cache,
                PipelineKey {
                    msaa: *msaa,
                    hdr: view.hdr,
                    occluded,
                },
            );
        }
    }
}

/// Each result draws occluded pixels at half opacity and visible pixels at full opacity.
fn queue_highlight(
    scene: Res<AimAssistHighlightScene>,
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<HighlightPipeline>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
) {
    let Some(target) = scene.target else { return };
    if scene
        .textures
        .get(target.texture)
        .is_none_or(Option::is_none)
    {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawCommands>();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        for occluded in [true, false] {
            let Ok(pipeline_id) = pipeline.variants.specialize(
                &pipeline_cache,
                PipelineKey {
                    msaa: *msaa,
                    hdr: view.hdr,
                    occluded,
                },
            ) else {
                continue;
            };
            phase.add(Transparent3d {
                entity: (view_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function,
                distance: view.rangefinder3d().distance(&target.center),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            });
        }
    }
}

type DrawCommands = (SetItemPipeline, DrawHighlight<0>);
struct DrawHighlight<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for DrawHighlight<I> {
    type Param = SRes<HighlightGpu>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();
    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        let Some(bind_group) = gpu
            .selected
            .and_then(|index| gpu.slots[index].bind_group.as_ref())
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, bind_group, &[view.offset]);
        pass.draw(0..6, 0..1);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AimAssistHighlight, queue_review_support as fixture};
    use bevy::ecs::system::RunSystemOnce;

    /// A small NOOP fixture retains a real view binding and exercises upload bookkeeping.
    fn app() -> App {
        let (mut app, _) = fixture::app();
        app.init_resource::<AimAssistHighlightScene>()
            .init_resource::<HighlightPipeline>()
            .init_resource::<ViewUniforms>()
            .add_render_command::<Transparent3d, DrawCommands>();
        let device = app.world().resource::<RenderDevice>().clone();
        let queue = app.world().resource::<RenderQueue>().clone();
        drop(
            app.world_mut()
                .resource_mut::<ViewUniforms>()
                .uniforms
                .get_writer(1, &device, &queue),
        );
        app.world_mut().run_system_once(init_gpu).unwrap();
        let mut scene = app.world_mut().resource_mut::<AimAssistHighlightScene>();
        scene.textures[0] = Some(Arc::new(
            AimAssistTexture::new([1, 1], Arc::from([255; 4])).unwrap(),
        ));
        scene.target = AimAssistHighlight::block(Vec3::ZERO, 2, Vec3::Z);
        app
    }

    #[test]
    fn unchanged_targets_never_upload_rebind_or_allocate() {
        let mut app = app();
        let mut system = IntoSystem::into_system(prepare);
        system.initialize(app.world_mut());
        system.run((), app.world_mut()).unwrap();
        let gpu = app.world().resource::<HighlightGpu>();
        assert_eq!(
            (gpu.texture_uploads, gpu.uniform_uploads, gpu.bind_rebuilds),
            (1, 1, 1)
        );
        let before = crate::alloc_count::thread_allocations();
        system.run((), app.world_mut()).unwrap();
        assert_eq!(crate::alloc_count::thread_allocations() - before, 0);
        let gpu = app.world().resource::<HighlightGpu>();
        assert_eq!(
            (gpu.texture_uploads, gpu.uniform_uploads, gpu.bind_rebuilds),
            (1, 1, 1)
        );
        app.world_mut()
            .resource_mut::<AimAssistHighlightScene>()
            .target = None;
        system.run((), app.world_mut()).unwrap();
        let gpu = app.world().resource::<HighlightGpu>();
        assert_eq!(gpu.selected, None);
        assert_eq!(
            (gpu.texture_uploads, gpu.uniform_uploads, gpu.bind_rebuilds),
            (1, 1, 1)
        );
    }

    #[test]
    fn one_target_queues_occluded_and_visible_draws_and_missing_art_queues_none() {
        let (mut app, view) = fixture::app();
        app.init_resource::<AimAssistHighlightScene>()
            .init_resource::<HighlightPipeline>()
            .add_render_command::<Transparent3d, DrawCommands>();
        app.world_mut()
            .resource_mut::<AimAssistHighlightScene>()
            .target = AimAssistHighlight::block(Vec3::ZERO, 2, Vec3::Z);
        app.world_mut().run_system_once(queue_highlight).unwrap();
        assert!(fixture::items(&app, view).is_empty());
        app.world_mut()
            .resource_mut::<AimAssistHighlightScene>()
            .textures[0] = Some(Arc::new(
            AimAssistTexture::new([1, 1], Arc::from([255; 4])).unwrap(),
        ));
        app.world_mut().run_system_once(queue_highlight).unwrap();
        assert_eq!(fixture::items(&app, view).len(), 2);
        let ids = [
            fixture::items(&app, view)[0].pipeline,
            fixture::items(&app, view)[1].pipeline,
        ];
        assert_ne!(ids[0], ids[1]);
        for (id, expected) in ids
            .into_iter()
            .zip([CompareFunction::Less, CompareFunction::Greater])
        {
            let mut cache = app.world_mut().resource_mut::<PipelineCache>();
            let descriptor = fixture::queued_descriptor(&mut cache, id);
            let depth = descriptor.depth_stencil.as_ref().unwrap();
            assert_eq!(depth.depth_compare, expected);
            assert_eq!(
                depth.bias,
                DepthBiasState {
                    constant: 33,
                    slope_scale: 0.0,
                    clamp: 0.0
                }
            );
            assert!(!depth.depth_write_enabled);
            let colour = descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap();
            assert_eq!(
                colour.write_mask,
                ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE
            );
            assert_eq!(colour.blend, Some(BlendState::ALPHA_BLENDING));
        }
    }

    #[test]
    fn empty_views_prepare_the_pipeline_before_the_first_target() {
        let (mut app, view) = fixture::app();
        app.init_resource::<HighlightPipeline>();
        app.world_mut().run_system_once(warm_pipeline).unwrap();
        assert!(fixture::items(&app, view).is_empty());
        let (&msaa, view) = app
            .world_mut()
            .query::<(&Msaa, &ExtractedView)>()
            .single(app.world())
            .unwrap();
        let hdr = view.hdr;
        app.world_mut()
            .resource_scope(|world, mut pipeline: Mut<HighlightPipeline>| {
                let cache = world.resource::<PipelineCache>();
                let before = crate::alloc_count::thread_allocations();
                for occluded in [true, false] {
                    let key = PipelineKey {
                        msaa,
                        hdr,
                        occluded,
                    };
                    let first = pipeline.variants.specialize(cache, key).unwrap();
                    let again = pipeline.variants.specialize(cache, key).unwrap();
                    assert_eq!(first, again);
                }
                assert_eq!(crate::alloc_count::thread_allocations() - before, 0);
            });
    }

    #[test]
    fn shader_resources_match_their_stage_visibility() {
        let pipeline = HighlightPipeline::from_world(&mut World::new());
        crate::shader_test_support::assert_binding_visibility(
            &crate::shader_source::standalone(include_str!("highlight.wgsl"), &[]),
            0,
            &pipeline.bind_group_layout,
        );
    }
}
