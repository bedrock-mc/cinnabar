//! Emissive in-world quads that sample server media textures.

use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
use bevy::prelude::{IntoSystem, System};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    prelude::{
        App, BevyError, Commands, Entity, FromWorld, Handle, IntoScheduleConfigs, Msaa, Quat,
        Query, Res, ResMut, Resource, Result, Shader, Vec3, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferId, BufferSize, BufferUsages, Canonical, ColorTargetState,
            ColorWrites, CompareFunction, DepthStencilState, Extent3d, FilterMode, FragmentState,
            PipelineCache, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey,
            TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use crate::media::MediaTexture;

const MEDIA_SCREEN_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("5b8e2f4a-3c71-4d09-a6e2-91f0c7d3b852");
/// Matches `Screen` in media_screen.wgsl: centre+flag, right axis, up axis.
const RECORD_BYTES: u64 = 48;

pub const MAX_MEDIA_SCREENS: usize = 4;

/// Screens to draw this frame; at most `MAX_MEDIA_SCREENS` are used, in order.
#[derive(Resource, bevy::render::extract_resource::ExtractResource, Clone, Default)]
pub struct MediaScreenScene {
    pub screens: Vec<MediaScreen>,
    /// Texture bytes all screens together may allocate.
    pub gpu_budget_bytes: u64,
}

#[derive(Clone)]
pub struct MediaScreen {
    /// Keys the retained texture across frames.
    pub id: u64,
    pub center: [f32; 3],
    pub half_right: [f32; 3],
    pub half_up: [f32; 3],
    pub frame: Option<MediaFrame>,
}

/// sRGB RGBA8, row-major, top row first; uploaded once per `serial`.
#[derive(Clone)]
pub struct MediaFrame {
    pub serial: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

/// Centre and half-extent axes of a quad of `size` in its local XY plane facing local +Z, after
/// scale, xyzw rotation and translation.
pub fn media_screen_axes(
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
    size: [f32; 2],
) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let rotation = Quat::from_array(rotation);
    let right = rotation * Vec3::new(scale[0] * size[0] * 0.5, 0.0, 0.0);
    let up = rotation * Vec3::new(0.0, scale[1] * size[1] * 0.5, 0.0);
    (translation, right.to_array(), up.to_array())
}

pub(crate) fn install_media_screen_render(app: &mut App) {
    crate::pipeline_warmup::register::<MediaScreenPipeline>(app);
    load_internal_asset!(
        app,
        MEDIA_SCREEN_SHADER_HANDLE,
        "media_screen.wgsl",
        crate::shader_safety::from_wgsl
    );
    crate::transparent_phase::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .init_resource::<MediaScreenPipeline>()
        .add_render_command::<Transparent3d, DrawMediaScreenCommands>()
        .add_systems(RenderStartup, init_media_screen_gpu)
        .add_systems(
            Render,
            (
                prepare_media_screens.in_set(RenderSystems::PrepareBindGroups),
                queue_media_screens
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

struct ScreenState {
    texture: Option<MediaTexture>,
    texture_generation: u64,
    serial: Option<u64>,
    uploaded: bool,
    record: Option<[f32; 12]>,
    record_buffer: Buffer,
    bind_group: Option<BindGroup>,
    bound: Option<(BufferId, Option<u64>)>,
}

impl ScreenState {
    /// What the bind group was built from; a change means it samples a stale texture.
    fn bind_key(&self, view_buffer: BufferId) -> (BufferId, Option<u64>) {
        let textured = self.texture.is_some() && self.uploaded;
        (view_buffer, textured.then_some(self.texture_generation))
    }
}

#[derive(Resource)]
struct MediaScreenGpu {
    sampler: Sampler,
    placeholder: TextureView,
    screens: HashMap<u64, ScreenState>,
    /// Scene-order screen ids for this frame's phase items; `None` skips a duplicate id.
    draws: Vec<Option<u64>>,
}

fn init_media_screen_gpu(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("media screen sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    let placeholder = device
        .create_texture_with_data(
            &queue,
            &TextureDescriptor {
                label: Some("media screen placeholder"),
                size: Extent3d {
                    width: 1,
                    height: 1,
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
            &[0, 0, 0, 255],
        )
        .create_view(&TextureViewDescriptor::default());
    commands.insert_resource(MediaScreenGpu {
        sampler,
        placeholder,
        screens: HashMap::new(),
        draws: Vec::new(),
    });
}

/// Uploads only changed frames and records; rebinds only on texture or view-buffer change.
#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn prepare_media_screens(
    scene: Res<MediaScreenScene>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<MediaScreenPipeline>,
    view_uniforms: Res<ViewUniforms>,
    gpu: Option<ResMut<MediaScreenGpu>>,
) {
    let Some(mut gpu) = gpu else {
        return;
    };
    let gpu = &mut *gpu;
    gpu.draws.clear();
    // At most MAX_MEDIA_SCREENS entries, so linear scans beat allocating sets each frame.
    let screens = &scene.screens[..scene.screens.len().min(MAX_MEDIA_SCREENS)];
    gpu.screens
        .retain(|id, _| screens.iter().any(|screen| screen.id == *id));
    let view = view_uniforms
        .uniforms
        .binding()
        .zip(view_uniforms.uniforms.buffer().map(|b| b.id()));
    for (index, screen) in screens.iter().enumerate() {
        if screens[..index]
            .iter()
            .any(|earlier| earlier.id == screen.id)
        {
            gpu.draws.push(None);
            continue;
        }
        let others: u64 = gpu
            .screens
            .iter()
            .filter(|(id, _)| **id != screen.id)
            .filter_map(|(_, state)| state.texture.as_ref())
            .map(MediaTexture::allocated_bytes)
            .sum();
        let state = gpu.screens.entry(screen.id).or_insert_with(|| ScreenState {
            texture: None,
            texture_generation: 0,
            serial: None,
            uploaded: false,
            record: None,
            record_buffer: device.create_buffer(&BufferDescriptor {
                label: Some("media screen record"),
                size: RECORD_BYTES,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            bind_group: None,
            bound: None,
        });
        if let Some(frame) = &screen.frame
            && state.serial != Some(frame.serial)
        {
            state.serial = Some(frame.serial);
            let size = [frame.width, frame.height];
            if state
                .texture
                .as_ref()
                .is_none_or(|texture| texture.size() != size)
            {
                state.texture = None;
                state.uploaded = false;
                let generation = state.texture_generation + 1;
                state.texture = MediaTexture::new(
                    &device,
                    size,
                    generation,
                    scene.gpu_budget_bytes.saturating_sub(others),
                );
                if state.texture.is_some() {
                    state.texture_generation = generation;
                } else {
                    // Denied by the shared budget: retry this frame once others free memory.
                    state.serial = None;
                }
            }
            if let Some(texture) = &state.texture {
                state.uploaded |=
                    texture.upload(&queue, size, state.texture_generation, &frame.rgba);
            }
        }
        if screen.frame.is_none() {
            // A reused id with a new clip shows black until that clip's first frame.
            state.serial = None;
            state.uploaded = false;
        }
        let textured = state.texture.is_some() && state.uploaded;
        let record = screen_record(screen, textured);
        if state.record != Some(record) {
            queue.write_buffer(&state.record_buffer, 0, bytemuck::cast_slice(&record));
            state.record = Some(record);
        }
        if let Some((view_binding, view_buffer)) = view.clone()
            && state.bound != Some(state.bind_key(view_buffer))
        {
            let texture_view = state
                .texture
                .as_ref()
                .filter(|_| textured)
                .and_then(|texture| texture.view(state.texture_generation))
                .unwrap_or(&gpu.placeholder);
            state.bind_group = Some(device.create_bind_group(
                "media screen bind group",
                &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: view_binding,
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: state.record_buffer.as_entire_binding(),
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
            state.bound = Some(state.bind_key(view_buffer));
        }
        gpu.draws.push(Some(screen.id));
    }
}

fn screen_record(screen: &MediaScreen, textured: bool) -> [f32; 12] {
    let [cx, cy, cz] = screen.center;
    let [rx, ry, rz] = screen.half_right;
    let [ux, uy, uz] = screen.half_up;
    [
        cx,
        cy,
        cz,
        f32::from(u8::from(textured)),
        rx,
        ry,
        rz,
        0.0,
        ux,
        uy,
        uz,
        0.0,
    ]
}

struct MediaScreenPipelineSpecializer;

#[derive(Resource)]
struct MediaScreenPipeline {
    variants: Variants<RenderPipeline, MediaScreenPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for MediaScreenPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "media screen bind group layout",
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
            label: Some("media screen pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: MEDIA_SCREEN_SHADER_HANDLE,
                entry_point: Some("media_screen_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: MEDIA_SCREEN_SHADER_HANDLE,
                entry_point: Some("media_screen_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: crate::SCENE_COLOR_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(MediaScreenPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct MediaScreenPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for MediaScreenPipelineSpecializer {
    type Key = MediaScreenPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .format = if key.hdr {
            crate::SCENE_HDR_FORMAT
        } else {
            crate::SCENE_COLOR_FORMAT
        };
        Ok(key)
    }
}

fn queue_media_screens(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<MediaScreenPipeline>,
    scene: Res<MediaScreenScene>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    if scene.screens.is_empty() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawMediaScreenCommands>();
    for (view_entity, main_entity, view, extracted_camera, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            MediaScreenPipelineKey {
                msaa: *msaa,
                hdr: extracted_camera.hdr,
            },
        ) else {
            continue;
        };
        for (index, screen) in scene.screens.iter().take(MAX_MEDIA_SCREENS).enumerate() {
            crate::transparent_phase::add(
                phase,
                Transparent3d {
                    sorting_info:
                        bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                    entity: (view_entity, *main_entity),
                    pipeline: pipeline_id,
                    draw_function,
                    distance: view
                        .rangefinder3d()
                        .distance(&Vec3::from_array(screen.center)),
                    batch_range: index as u32..index as u32 + 1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: false,
                },
            );
        }
    }
}

type DrawMediaScreenCommands = (SetItemPipeline, DrawMediaScreen<0>);

struct DrawMediaScreen<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for DrawMediaScreen<I> {
    type Param = SRes<MediaScreenGpu>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        let Some(bind_group) = gpu
            .draws
            .get(item.batch_range().start as usize)
            .copied()
            .flatten()
            .and_then(|id| gpu.screens.get(&id))
            .and_then(|state| state.bind_group.as_ref())
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, bind_group, &[view_offset.offset]);
        pass.draw(0..6, 0..1);
        RenderCommandResult::Success
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for MediaScreenPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        ids.push(self.variants.specialize(
            cache,
            MediaScreenPipelineKey {
                msaa: view.msaa,
                hdr: view.hdr,
            },
        )?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_review_support as fixture;
    use bevy::ecs::system::RunSystemOnce;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    fn screen(id: u64, z: f32) -> MediaScreen {
        MediaScreen {
            id,
            center: [0.0, 0.0, z],
            half_right: [1.0, 0.0, 0.0],
            half_up: [0.0, 1.0, 0.0],
            frame: None,
        }
    }

    #[test]
    fn axes_follow_scale_rotation_and_translation() {
        let (center, right, up) =
            media_screen_axes([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3], [4.0, 2.0]);
        assert_eq!(center, [1.0, 2.0, 3.0]);
        assert!(close(right, [2.0, 0.0, 0.0]) && close(up, [0.0, 1.0, 0.0]));
        // A quarter turn about +Y maps the +Z face to +X and the right axis to -Z.
        let half = std::f32::consts::FRAC_PI_4;
        let (_, right, up) = media_screen_axes(
            [0.0; 3],
            [0.0, half.sin(), 0.0, half.cos()],
            [2.0, 3.0, 1.0],
            [4.0, 2.0],
        );
        assert!(close(right, [0.0, 0.0, -4.0]), "{right:?}");
        assert!(close(up, [0.0, 3.0, 0.0]), "{up:?}");
        let normal = Vec3::from_array(right).cross(Vec3::from_array(up));
        assert!(normal.x > 0.0);
    }

    #[test]
    fn queue_adds_one_item_per_bounded_screen() {
        let (mut app, view) = fixture::app();
        app.init_resource::<MediaScreenScene>()
            .init_resource::<MediaScreenPipeline>()
            .add_render_command::<Transparent3d, DrawMediaScreenCommands>();
        app.world_mut()
            .run_system_once(queue_media_screens)
            .unwrap();
        assert!(fixture::items(&app, view).is_empty());
        app.world_mut().resource_mut::<MediaScreenScene>().screens = (0..MAX_MEDIA_SCREENS as u64
            + 2)
            .map(|id| screen(id, -2.0 - id as f32))
            .collect();
        app.world_mut()
            .run_system_once(queue_media_screens)
            .unwrap();
        let items = fixture::items(&app, view);
        assert_eq!(items.len(), MAX_MEDIA_SCREENS);
        for (index, item) in items.iter().enumerate() {
            assert_eq!(item.batch_range, index as u32..index as u32 + 1);
        }
    }

    #[test]
    fn unchanged_frames_upload_once_and_textures_follow_the_budget() {
        let (mut app, _) = fixture::app();
        app.init_resource::<MediaScreenScene>()
            .init_resource::<MediaScreenPipeline>()
            .init_resource::<ViewUniforms>();
        app.world_mut()
            .run_system_once(init_media_screen_gpu)
            .unwrap();
        let frame = MediaFrame {
            serial: 1,
            width: 2,
            height: 2,
            rgba: Arc::from(vec![255; 16]),
        };
        {
            let mut scene = app.world_mut().resource_mut::<MediaScreenScene>();
            scene.gpu_budget_bytes = 16;
            scene.screens = vec![
                MediaScreen {
                    frame: Some(frame.clone()),
                    ..screen(7, 0.0)
                },
                MediaScreen {
                    frame: Some(frame.clone()),
                    ..screen(8, 0.0)
                },
            ];
        }
        app.world_mut()
            .run_system_once(prepare_media_screens)
            .unwrap();
        let gpu = app.world().resource::<MediaScreenGpu>();
        assert_eq!(gpu.draws, [Some(7), Some(8)]);
        let budgeted = gpu
            .screens
            .values()
            .filter(|state| state.texture.is_some())
            .count();
        assert_eq!(budgeted, 1, "the second texture exceeds the shared budget");
        let generation = gpu.screens[&7].texture_generation;
        app.world_mut()
            .run_system_once(prepare_media_screens)
            .unwrap();
        let gpu = app.world().resource::<MediaScreenGpu>();
        assert_eq!(gpu.screens[&7].texture_generation, generation);
        assert_eq!(gpu.screens[&7].serial, Some(1));
        app.world_mut()
            .resource_mut::<MediaScreenScene>()
            .screens
            .truncate(1);
        app.world_mut()
            .run_system_once(prepare_media_screens)
            .unwrap();
        let gpu = app.world().resource::<MediaScreenGpu>();
        assert!(!gpu.screens.contains_key(&8));
    }

    /// Prepares `screens` once in a fresh fixture with a 16-byte budget.
    fn prepared(app: &mut App, screens: Vec<MediaScreen>) {
        {
            let mut scene = app.world_mut().resource_mut::<MediaScreenScene>();
            scene.gpu_budget_bytes = 16;
            scene.screens = screens;
        }
        app.world_mut()
            .run_system_once(prepare_media_screens)
            .unwrap();
    }

    fn budget_app() -> App {
        let (mut app, _) = fixture::app();
        app.init_resource::<MediaScreenScene>()
            .init_resource::<MediaScreenPipeline>()
            .init_resource::<ViewUniforms>();
        app.world_mut()
            .run_system_once(init_media_screen_gpu)
            .unwrap();
        app
    }

    fn frame(serial: u64, width: u32) -> MediaFrame {
        MediaFrame {
            serial,
            width,
            height: 2,
            rgba: Arc::from(vec![255; width as usize * 8]),
        }
    }

    #[test]
    fn a_screen_denied_its_texture_gets_one_when_budget_frees() {
        let mut app = budget_app();
        let with = |id| MediaScreen {
            frame: Some(frame(1, 2)),
            ..screen(id, 0.0)
        };
        prepared(&mut app, vec![with(7), with(8)]);
        assert!(
            app.world().resource::<MediaScreenGpu>().screens[&8]
                .texture
                .is_none()
        );
        prepared(&mut app, vec![with(8)]);
        assert!(
            app.world().resource::<MediaScreenGpu>().screens[&8]
                .texture
                .is_some(),
            "a paused frame keeps its serial, so the retry cannot wait for a new one"
        );
    }

    #[test]
    fn a_replaced_texture_invalidates_the_bind_group() {
        let mut app = budget_app();
        let view = app
            .world()
            .resource::<RenderDevice>()
            .create_buffer(&BufferDescriptor {
                label: None,
                size: 16,
                usage: BufferUsages::UNIFORM,
                mapped_at_creation: false,
            })
            .id();
        let with = |frame| MediaScreen {
            frame: Some(frame),
            ..screen(7, 0.0)
        };
        prepared(&mut app, vec![with(frame(1, 2))]);
        let before = app.world().resource::<MediaScreenGpu>().screens[&7].bind_key(view);
        app.world_mut()
            .resource_mut::<MediaScreenScene>()
            .gpu_budget_bytes = 64;
        app.world_mut().resource_mut::<MediaScreenScene>().screens = vec![with(frame(2, 4))];
        app.world_mut()
            .run_system_once(prepare_media_screens)
            .unwrap();
        let state = &app.world().resource::<MediaScreenGpu>().screens[&7];
        assert_eq!(state.texture.as_ref().map(MediaTexture::size), Some([4, 2]));
        assert_ne!(state.bind_key(view), before);
    }

    #[test]
    fn a_reused_screen_without_a_frame_draws_black_not_the_old_clip() {
        let mut app = budget_app();
        let view = app
            .world()
            .resource::<RenderDevice>()
            .create_buffer(&BufferDescriptor {
                label: None,
                size: 16,
                usage: BufferUsages::UNIFORM,
                mapped_at_creation: false,
            })
            .id();
        prepared(
            &mut app,
            vec![MediaScreen {
                frame: Some(frame(1, 2)),
                ..screen(7, 0.0)
            }],
        );
        prepared(&mut app, vec![screen(7, 0.0)]);
        let state = &app.world().resource::<MediaScreenGpu>().screens[&7];
        assert_eq!(
            state.bind_key(view).1,
            None,
            "still sampling the previous clip"
        );
        assert_eq!(state.record.map(|record| record[3]), Some(0.0));
    }

    #[test]
    fn an_unchanged_scene_prepares_without_allocating() {
        let mut app = budget_app();
        let with = |id| MediaScreen {
            frame: Some(frame(1, 2)),
            ..screen(id, 0.0)
        };
        prepared(&mut app, vec![with(7), with(8), with(7)]);
        let mut system = IntoSystem::into_system(prepare_media_screens);
        system.initialize(app.world_mut());
        system.run((), app.world_mut()).unwrap();
        let before = crate::alloc_count::thread_allocations();
        system.run((), app.world_mut()).unwrap();
        assert_eq!(crate::alloc_count::thread_allocations() - before, 0);
        assert_eq!(
            app.world().resource::<MediaScreenGpu>().draws,
            [Some(7), Some(8), None]
        );
    }

    #[test]
    fn pipeline_is_opaque_double_sided_and_depth_writing() {
        let (mut app, _) = fixture::app();
        let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
        let mut pipeline = MediaScreenPipeline::from_world(&mut World::new());
        crate::shader_test_support::assert_binding_visibility(
            &crate::shader_source::standalone(include_str!("media_screen.wgsl"), &[]),
            0,
            &pipeline.bind_group_layout,
        );
        let id = pipeline
            .variants
            .specialize(
                &cache,
                MediaScreenPipelineKey {
                    msaa: Msaa::Sample4,
                    hdr: true,
                },
            )
            .unwrap();
        let descriptor = fixture::queued_descriptor(&mut cache, id);
        assert_eq!(descriptor.multisample.count, 4);
        assert_eq!(descriptor.primitive.cull_mode, None);
        let depth = descriptor.depth_stencil.as_ref().unwrap();
        assert_eq!(depth.depth_write_enabled, Some(true));
        assert_eq!(depth.depth_compare, Some(CompareFunction::GreaterEqual));
        let colour = descriptor.fragment.as_ref().unwrap().targets[0]
            .as_ref()
            .unwrap();
        assert_eq!(colour.blend, None);
        assert_eq!(colour.format, crate::SCENE_HDR_FORMAT);
    }
}
