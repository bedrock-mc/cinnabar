use crate::queue_writes::ProfiledQueue as _;
use bevy::prelude::IntoScheduleConfigs;
use bevy::render::GpuResourceAppExt;
use bevy::render::render_phase::AddRenderCommand;
use bevy::render::render_resource::ShaderType;
use render_scene::actor::{
    ActorDrawWitness, ActorPrepareWitness, ActorQueueWitness, ActorSubmitWitness,
};

use std::mem::size_of;
mod artwork;
mod bones;
mod dissolve;
pub(crate) mod masks;
pub(crate) mod motion;
pub(crate) mod phase;
pub(crate) mod pipeline;
#[cfg(feature = "publication-test-support")]
pub(crate) mod publication_test_support;
#[cfg(feature = "shaderpacks")]
pub(crate) mod shaderpack_source;
mod skins;
use crate::gpu_allocation;
use artwork::{GpuArtwork, draw_spans};
use phase::{DrawActorCommands, DrawTransparentActorCommands, queue_actors};
use pipeline::*;
use skins::GpuSkinArrays;

use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{
        CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey, Transparent3d,
    },
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParam, SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    prelude::{
        App, Assets, BevyError, Commands, Entity, FromWorld, Handle, Local, Msaa, Plugin, Query,
        Res, ResMut, Resource, Result, Shader, Vec3, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
            PhaseItemExtraIndex, RenderCommand, RenderCommandResult, SetItemPipeline,
            TrackedRenderPass, ViewBinnedRenderPhases, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferId, BufferSize, BufferUsages, CachedRenderPipelineId,
            Canonical, ColorTargetState, ColorWrites, CommandEncoderDescriptor, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, PipelineCache,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            Specializer, SpecializerKey, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
            TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
            TextureView, TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use render_model::ActorRigVertex;
use {
    crate::actor::gpu::{ActorDrawBatch, ActorDrawTracker},
    render_model::{ActorGpuInstance, ActorRigGeometrySpan},
    render_scene::{ActorPresentationGate, ActorRenderFrame, ActorRuntimeWitness},
};

const ACTOR_SHADER_HANDLE: Handle<Shader> = uuid_handle!("09d34708-6fd4-4c65-b27e-ce22f172cc73");
const ACTOR_RIG_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("5b1f0c6e-2f4e-4c55-9a43-7c1d2e8b9f30");

/// Registers `render::actor_rig`, the rig skinning the actor and hand passes share.
pub(crate) fn load_rig_shader(app: &mut App) {
    let (id, shader) = rig_shader_asset();
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(id, shader)
        .expect("the rig uses a stable shader ID");
}

/// Builds the shared rig module for a renderer-owned shader cache.
pub(crate) fn rig_shader_asset() -> (AssetId<Shader>, Shader) {
    crate::shader_asset::internal_shader_asset!(
        ACTOR_RIG_SHADER_HANDLE,
        "actor_rig.wesl",
        crate::shader_safety::from_wesl
    )
}

#[cfg(test)]
const ACTOR_SHADER_SOURCE: &str = include_str!("actor.wesl");

#[derive(Debug, Clone, Copy, Default)]
pub struct ActorRenderPlugin;

impl Plugin for ActorRenderPlugin {
    fn build(&self, app: &mut App) {
        install_actor_render(app);
    }

    fn finish(&self, app: &mut App) {
        install_actor_render(app);
    }
}

#[derive(Resource)]
struct ActorRenderInstalled;

fn install_actor_render(app: &mut App) {
    app.init_resource::<ActorRenderFrame>()
        .init_resource::<ActorPresentationGate>()
        .init_resource::<render_scene::ActorPipelineReadiness>()
        .init_resource::<ActorRuntimeWitness>();
    crate::lighting::install(app);
    crate::upload_staging::install(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<ActorRenderInstalled>()
    {
        return;
    }
    let presentation_gate = app.world().resource::<ActorPresentationGate>().clone();
    let runtime_witness = app.world().resource::<ActorRuntimeWitness>().clone();
    let pipeline_readiness = app
        .world()
        .resource::<render_scene::ActorPipelineReadiness>()
        .clone();
    app.add_plugins(ExtractResourcePlugin::<ActorRenderFrame>::default());
    load_rig_shader(app);
    load_internal_asset!(
        app,
        ACTOR_SHADER_HANDLE,
        "actor.wesl",
        crate::shader_safety::from_actor_wesl,
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS
    );
    crate::nametag_render::install_nametag_render(app);
    crate::install_opaque_phase_reset(app.sub_app_mut(RenderApp));
    crate::device_poll::install(app.sub_app_mut(RenderApp));
    crate::transparent_phase::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .insert_resource(ActorRenderInstalled)
        .insert_resource(presentation_gate)
        .insert_resource(runtime_witness)
        .insert_resource(pipeline_readiness)
        .init_gpu_resource::<ActorPipeline>()
        .init_resource::<ActorDrawTracker>()
        .add_render_command::<Opaque3d, DrawActorCommands>()
        .add_render_command::<Transparent3d, DrawTransparentActorCommands>()
        .add_systems(RenderStartup, init_actor_gpu)
        .add_systems(
            Render,
            (
                prepare_actor_resources
                    .in_set(RenderSystems::Queue)
                    .before(queue_actors),
                prepare_actor_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_actors
                    .run_if(render_scene::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
                submit_actor_presented_frame.in_set(crate::device_poll::FrameSubmissions),
            )
                .run_if(bevy::ecs::schedule::common_conditions::resource_exists::<ActorGpu>),
        )
        // Loading waits on pipeline readiness, which needs no actor allocations.
        .add_systems(
            Render,
            prepare_actor_pipelines
                .in_set(RenderSystems::Queue)
                .before(queue_actors),
        );
    masks::install(app.sub_app_mut(RenderApp));
}

#[derive(Resource)]
pub(crate) struct ActorGpu {
    artwork: GpuArtwork,
    player_material: Buffer,
    neutral_material: Buffer,
    color_mask_material: Buffer,
    multitexture_material: Buffer,
    spans: Vec<crate::actor::gpu::ActorDrawSpan>,
    mask_spans: Vec<crate::actor::gpu::ActorDrawSpan>,
    main_manifest: std::sync::Arc<[render_model::ActorDrawManifestEntry]>,
    sorted: dissolve::SortedDraws,
    instances: std::sync::Arc<[ActorGpuInstance]>,
    executed_instances: std::sync::atomic::AtomicU32,
    /// The immutable draw identity installed by the current queue pass.
    queued_draw: Option<ActorDrawBatch>,
    artwork_identity: [u8; 32],
    artwork_current: bool,
    instance_buffer: Buffer,
    previous_bone_buffer: Buffer,
    current_bone_buffer: Buffer,
    current_bone_offset: u64,
    bones: bones::BoneResidency,
    geometry_vertices: crate::actor::gpu::SegmentedVertexBuffer,
    geometry_span_buffer: Option<Buffer>,
    instance_count: u32,
    maximum_vertex_count: u32,
    skins: GpuSkinArrays,
    sampler: Sampler,
    glint_sampler: Sampler,
    bind_group: Option<BindGroup>,
    frame_generation: u64,
    geometry_revision: u64,
    geometry_ready: bool,
    skin_revision: u64,
    view_buffer_id: Option<BufferId>,
    manifest: std::sync::Arc<[render_model::ActorDrawManifestEntry]>,
}

/// Whether every player-page instance samples a resident skin slot.
fn player_skins_resident(frame: &ActorRenderFrame) -> bool {
    !frame.rig.instances.is_empty()
        && frame
            .rig
            .instances
            .iter()
            .enumerate()
            .all(|(index, instance)| {
                frame.instance_pages.get(index).copied().unwrap_or(0) != 0
                    || frame.skins.resident(instance.texture_layer).is_some()
            })
}

/// Leaves actor rendering disabled if its initial resources cannot be allocated.
fn init_actor_gpu(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    if let Some(gpu) = create_actor_gpu(&render_device, &render_queue) {
        commands.insert_resource(gpu);
    }
}

/// Builds checked actor resources without publishing partially initialized state.
fn create_actor_gpu(render_device: &RenderDevice, render_queue: &RenderQueue) -> Option<ActorGpu> {
    let (previous_bone_buffer, current_bone_buffer, current_bone_offset) =
        bones::gpu_buffers(render_device)?;
    let sampler = gpu_allocation::checked(render_device, "actor sampler", 0, || {
        render_device.create_sampler(&SamplerDescriptor {
            label: Some("nearest shared actor artwork sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..default()
        })
    })?;
    Some(ActorGpu {
        artwork: GpuArtwork::default(),
        player_material: gpu_allocation::buffer_with_data(
            render_device,
            render_queue,
            "unchanged player material class",
            bytemuck::cast_slice(&[0u32; 4]),
            BufferUsages::UNIFORM,
        )?,
        neutral_material: gpu_allocation::buffer_with_data(
            render_device,
            render_queue,
            "neutral binary-alpha material class",
            bytemuck::cast_slice(&[1u32, 0, 0, 0]),
            BufferUsages::UNIFORM,
        )?,
        color_mask_material: gpu_allocation::buffer_with_data(
            render_device,
            render_queue,
            "native actor color-mask material",
            bytemuck::cast_slice(&[0u32, 1, 0, 0]),
            BufferUsages::UNIFORM,
        )?,
        multitexture_material: gpu_allocation::buffer_with_data(
            render_device,
            render_queue,
            "native actor three-sampler material",
            bytemuck::cast_slice(&[0u32, 0, 1, 0]),
            BufferUsages::UNIFORM,
        )?,
        spans: Vec::new(),
        mask_spans: Vec::new(),
        main_manifest: std::sync::Arc::from([]),
        sorted: dissolve::SortedDraws::default(),
        instances: std::sync::Arc::from([]),
        executed_instances: std::sync::atomic::AtomicU32::new(0),
        queued_draw: None,
        artwork_identity: [0; 32],
        artwork_current: false,
        instance_buffer: gpu_allocation::buffer(
            render_device,
            &BufferDescriptor {
                label: Some("bounded shared actor instance arena"),
                size: (render_model::MAX_ACTOR_RENDER_INSTANCES * size_of::<ActorGpuInstance>())
                    as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        )?,
        previous_bone_buffer,
        current_bone_buffer,
        current_bone_offset,
        bones: bones::BoneResidency::default(),
        geometry_vertices: default(),
        geometry_span_buffer: None,
        instance_count: 0,
        maximum_vertex_count: 0,
        skins: GpuSkinArrays::new(render_device)?,
        glint_sampler: gpu_allocation::checked(render_device, "actor glint sampler", 0, || {
            render_device.create_sampler(&SamplerDescriptor {
                label: Some("repeat actor glint sampler"),
                address_mode_u: AddressMode::Repeat,
                address_mode_v: AddressMode::Repeat,
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..Default::default()
            })
        })?,
        sampler,
        bind_group: None,
        frame_generation: u64::MAX,
        geometry_revision: u64::MAX,
        geometry_ready: false,
        skin_revision: u64::MAX,
        view_buffer_id: None,
        manifest: std::sync::Arc::from([]),
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_actor_resources(
    frame: Res<ActorRenderFrame>,
    uploads: crate::upload_staging::FrameUploads,
    mut gpu: ResMut<ActorGpu>,
    witness: Res<ActorRuntimeWitness>,
    gate: Res<ActorPresentationGate>,
    tracker: Res<ActorDrawTracker>,
    raw_views: Option<Res<crate::actor::gpu::ArtworkRawViews>>,
) {
    // Preparation can replace any part of the queued identity, even before queue runs.
    gpu.queued_draw = None;
    let raw_views = crate::actor::gpu::ArtworkRawViews::current(raw_views);
    gpu.artwork.set_raw_views(raw_views);
    gpu.skins.set_raw_views(raw_views);
    if gpu.skins.failed(&frame.skins) {
        gpu.instance_count = 0;
        gate.clear();
        tracker.clear();
        return;
    }
    let rig = &frame.rig;
    let artwork_valid = gpu.artwork.prepare_demanded(
        &frame.artwork,
        &frame.instance_pages,
        &uploads.device,
        &uploads.queue,
    );
    gpu.artwork_current = artwork_valid;
    if gpu.artwork_identity != frame.artwork.identity() {
        gate.clear();
        tracker.clear();
        gpu.artwork_identity = frame.artwork.identity();
        gpu.frame_generation = u64::MAX;
        gpu.bones.clear();
        gpu.bind_group = None;
    }
    let skins_resident = player_skins_resident(&frame);
    let mut structurally_valid = !rig.instances.is_empty()
        && rig.instances.len() <= render_model::MAX_ACTOR_RENDER_INSTANCES
        && rig.previous_bones.len() == rig.current_bones.len()
        && rig.previous_bones.len() <= render_model::actor::MAX_ACTOR_POSE_BONES
        && rig.manifest.len() == rig.instances.len()
        && rig.maximum_vertex_count != 0
        && skins_resident
        && frame.instance_pages.len() == rig.instances.len();
    if gpu.geometry_revision != rig.geometry_revision {
        gate.clear();
        tracker.clear();
        gpu.frame_generation = u64::MAX;
        gpu.bones.clear();
        // A new skin model or item mesh uploads only its own vertices.
        gpu.geometry_ready = gpu.geometry_vertices.sync(
            &uploads.device,
            &uploads.queue,
            "shared actor rig vertices",
            &rig.geometry_vertices,
        );
        if !rig.geometry_spans.is_empty() && gpu.geometry_ready {
            if let Some(buffer) = gpu_allocation::buffer_with_data(
                &uploads.device,
                &uploads.queue,
                "shared actor rig geometry spans",
                bytemuck::cast_slice::<ActorRigGeometrySpan, u8>(&rig.geometry_spans),
                BufferUsages::STORAGE,
            ) {
                gpu.geometry_span_buffer = Some(buffer);
            } else {
                gpu.geometry_ready = false;
            }
        } else {
            gpu.geometry_ready = false;
        }
        gpu.geometry_revision = rig.geometry_revision;
        gpu.bind_group = None;
        gpu.artwork.invalidate_bindings();
    }
    structurally_valid &= gpu.geometry_ready;
    if gpu.frame_generation != rig.frame_generation {
        let lifetime_changed = gpu.manifest.len() != rig.manifest.len()
            || gpu
                .manifest
                .iter()
                .zip(rig.manifest.iter())
                .any(|(old, new)| {
                    let old = old.identity;
                    let new = new.identity;
                    (
                        old.session_id,
                        old.dimension,
                        old.runtime_id,
                        old.spawn_revision,
                    ) != (
                        new.session_id,
                        new.dimension,
                        new.runtime_id,
                        new.spawn_revision,
                    )
                });
        if lifetime_changed {
            gate.clear();
            tracker.clear();
        }
        let prepared = structurally_valid
            .then(|| {
                let instances = dissolve::instances(&rig.instances, &rig.manifest);
                gpu.bones.prepare(rig, instances)
            })
            .flatten();
        structurally_valid &= prepared.is_some();
        if let Some(prepared) = prepared {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!(
                "actor.frame_upload",
                generation = rig.frame_generation,
                instances = rig.instances.len(),
                bones = rig.current_bones.len(),
                bone_bytes = prepared.uploaded_bytes(),
                bone_writes = prepared.previous.len() + prepared.current.len(),
                compacted = prepared.compacted,
                bytes = std::mem::size_of_val(&*rig.instances) + prepared.uploaded_bytes(),
            )
            .entered();
            let uploaded = gpu_allocation::checked(
                &uploads.device,
                "actor instance and bone upload",
                (std::mem::size_of_val(&*prepared.instances) + prepared.uploaded_bytes()) as u64,
                || {
                    let mut writes =
                        Vec::with_capacity(1 + prepared.previous.len() + prepared.current.len());
                    writes.push((
                        &*gpu.instance_buffer,
                        0,
                        bytemuck::cast_slice::<ActorGpuInstance, u8>(&prepared.instances),
                    ));
                    gpu.bones.writes(&prepared, |current, offset, bytes| {
                        let buffer = if current {
                            &gpu.current_bone_buffer
                        } else {
                            &gpu.previous_bone_buffer
                        };
                        writes.push((
                            &**buffer,
                            offset + if current { gpu.current_bone_offset } else { 0 },
                            bytes,
                        ));
                    });
                    uploads.write_batch(&writes);
                },
            );
            if uploaded.is_none() {
                gpu.bones.clear();
                gpu.instance_count = 0;
                gpu.frame_generation = rig.frame_generation;
                gate.clear();
                tracker.clear();
                return;
            }
            #[cfg(feature = "tracy")]
            drop(_span);
            gpu.instance_count = rig.instances.len() as u32;
            gpu.maximum_vertex_count = rig.maximum_vertex_count;
            draw_spans(
                &mut gpu.spans,
                &frame.instance_pages,
                &prepared.instances,
                &rig.geometry_spans,
                prepared.manifest.iter().map(|entry| entry.identity.layer),
            );
            gpu.mask_spans.clear();
            if frame.mask_only_runtime_ids.is_empty() {
                gpu.main_manifest = std::sync::Arc::clone(&prepared.manifest);
            } else {
                gpu.mask_spans = gpu.spans.clone();
                gpu.spans = artwork::visible_spans(
                    &gpu.spans,
                    &prepared.manifest,
                    &frame.mask_only_runtime_ids,
                    frame.main_pass_instance_count,
                );
                let main_count = gpu
                    .spans
                    .last()
                    .map_or(0, |span| (span.first + span.count) as usize);
                // Coverage-only submissions follow the contiguous visible prefix at publication.
                gpu.main_manifest = std::sync::Arc::from(&prepared.manifest[..main_count]);
            }
            gpu.manifest = prepared.manifest;
            gpu.instances = prepared.instances;
            let ActorGpu {
                sorted,
                spans,
                manifest,
                ..
            } = &mut *gpu;
            sorted.prepare(spans, manifest);
        } else {
            gpu.bones.clear();
            gpu.instance_count = 0;
            gpu.maximum_vertex_count = 0;
            gpu.manifest = std::sync::Arc::from([]);
            gpu.spans.clear();
            gpu.mask_spans.clear();
            gpu.main_manifest = std::sync::Arc::from([]);
            gpu.sorted.prepare(&[], &[]);
            gpu.instances = std::sync::Arc::from([]);
            gate.clear();
            tracker.clear();
        }
        gpu.frame_generation = rig.frame_generation;
    }
    if gpu.skin_revision != frame.skin_revision || !gpu.skins.is_synced(&frame.skins) {
        gate.clear();
        tracker.clear();
        if !skins_resident {
            gpu.instance_count = 0;
            gpu.skin_revision = frame.skin_revision;
            gpu.bind_group = None;
            witness.observe_prepare(ActorPrepareWitness {
                input_instances: rig.instances.len(),
                input_manifest: rig.manifest.len(),
                skin_bytes: frame.skin_bytes(),
                skin_plan: false,
                valid: structurally_valid,
                prepared_instances: gpu.instance_count,
                maximum_vertices: gpu.maximum_vertex_count,
            });
            return;
        }
        let views_changed = gpu
            .skins
            .sync(&frame.skins, &uploads.device, &uploads.queue);
        if !gpu.skins.is_synced(&frame.skins) {
            gpu.bind_group = None;
            gpu.instance_count = 0;
            gpu.frame_generation = u64::MAX;
            return;
        }
        if views_changed {
            gpu.bind_group = None;
        }
        gpu.skin_revision = frame.skin_revision;
    }
    witness.observe_prepare(ActorPrepareWitness {
        input_instances: rig.instances.len(),
        input_manifest: rig.manifest.len(),
        skin_bytes: frame.skin_bytes(),
        skin_plan: skins_resident,
        valid: structurally_valid,
        prepared_instances: gpu.instance_count,
        maximum_vertices: gpu.maximum_vertex_count,
    });
}

pub(crate) fn prepare_actor_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<ActorPipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<ActorGpu>,
) {
    // Bindings matter only to draws: without instances they wait instead of being rebuilt every
    // frame, as a rig with no player skins resident (or no actors at all) would otherwise do.
    if gpu.instance_count == 0 {
        return;
    }
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.bind_group = None;
        return;
    };
    let Some(geometry_vertex_buffer) = gpu.geometry_vertices.buffer() else {
        gpu.bind_group = None;
        return;
    };
    let Some(geometry_span_buffer) = gpu.geometry_span_buffer.as_ref() else {
        gpu.bind_group = None;
        return;
    };
    let (Some((_, glint_view)), Some((_, item_glint_view))) = (
        gpu.artwork.glint.texture.as_ref(),
        gpu.artwork.item_glint.texture.as_ref(),
    ) else {
        gpu.bind_group = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    let view_changed = gpu.view_buffer_id != Some(view_buffer.id());
    if gpu.bind_group.is_some()
        && !view_changed
        && gpu
            .artwork
            .pages
            .iter()
            .flatten()
            .all(|page| page.bind_group.is_some())
    {
        return;
    }
    let Some((generic_groups, player_group)) =
        gpu_allocation::checked(&render_device, "actor bindings", 0, || {
            let generic_groups: Vec<_> = gpu
                .artwork
                .pages
                .iter()
                .flatten()
                .filter(|page| view_changed || page.bind_group.is_none())
                .map(|page| {
                    render_device.create_bind_group(
                        "neutral actor page bind group",
                        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
                        &pipeline.entries(&[
                            BindGroupEntry {
                                binding: 0,
                                resource: view_binding.clone(),
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource: gpu.instance_buffer.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 2,
                                resource: geometry_vertex_buffer.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 3,
                                resource: geometry_span_buffer.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 4,
                                resource: gpu.previous_bone_buffer.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 5,
                                resource: gpu.current_bone_buffer.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 6,
                                resource: BindingResource::TextureView(&page.view),
                            },
                            BindGroupEntry {
                                binding: 7,
                                resource: BindingResource::Sampler(&gpu.sampler),
                            },
                            BindGroupEntry {
                                binding: 8,
                                resource: if page.multitexture {
                                    gpu.multitexture_material.as_entire_binding()
                                } else if page.color_mask {
                                    gpu.color_mask_material.as_entire_binding()
                                } else {
                                    gpu.neutral_material.as_entire_binding()
                                },
                            },
                            BindGroupEntry {
                                binding: 9,
                                resource: BindingResource::TextureView(&gpu.skins.placeholder),
                            },
                            BindGroupEntry {
                                binding: 10,
                                resource: BindingResource::TextureView(&gpu.skins.placeholder),
                            },
                            BindGroupEntry {
                                binding: 12,
                                resource: BindingResource::TextureView(glint_view),
                            },
                            BindGroupEntry {
                                binding: 13,
                                resource: BindingResource::Sampler(&gpu.glint_sampler),
                            },
                            BindGroupEntry {
                                binding: 14,
                                resource: BindingResource::TextureView(item_glint_view),
                            },
                            BindGroupEntry {
                                binding: 11,
                                resource: BindingResource::TextureView(&gpu.skins.placeholder),
                            },
                        ]),
                    )
                })
                .collect();
            let player_group = render_device.create_bind_group(
                "instanced standard actor bind group",
                &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
                &pipeline.entries(&[
                    BindGroupEntry {
                        binding: 0,
                        resource: view_binding,
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: gpu.instance_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: geometry_vertex_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: geometry_span_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 4,
                        resource: gpu.previous_bone_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 5,
                        resource: gpu.current_bone_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 6,
                        resource: BindingResource::TextureView(gpu.skins.view(0)),
                    },
                    BindGroupEntry {
                        binding: 7,
                        resource: BindingResource::Sampler(&gpu.sampler),
                    },
                    BindGroupEntry {
                        binding: 8,
                        resource: gpu.player_material.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 9,
                        resource: BindingResource::TextureView(gpu.skins.view(1)),
                    },
                    BindGroupEntry {
                        binding: 10,
                        resource: BindingResource::TextureView(gpu.skins.view(2)),
                    },
                    BindGroupEntry {
                        binding: 12,
                        resource: BindingResource::TextureView(glint_view),
                    },
                    BindGroupEntry {
                        binding: 13,
                        resource: BindingResource::Sampler(&gpu.glint_sampler),
                    },
                    BindGroupEntry {
                        binding: 14,
                        resource: BindingResource::TextureView(item_glint_view),
                    },
                    BindGroupEntry {
                        binding: 11,
                        resource: BindingResource::TextureView(gpu.skins.view(3)),
                    },
                ]),
            );
            (generic_groups, player_group)
        })
    else {
        gpu.bind_group = None;
        gpu.artwork.invalidate_bindings();
        gpu.instance_count = 0;
        return;
    };
    gpu.bind_group = Some(player_group);
    for (page, group) in gpu
        .artwork
        .pages
        .iter_mut()
        .flatten()
        .filter(|page| view_changed || page.bind_group.is_none())
        .zip(generic_groups)
    {
        page.bind_group = Some(group);
    }
    gpu.view_buffer_id = Some(view_buffer.id());
}

fn submit_actor_presented_frame(
    render_queue: Res<RenderQueue>,
    tracker: Res<ActorDrawTracker>,
    gate: Res<ActorPresentationGate>,
    witness: Res<ActorRuntimeWitness>,
) {
    let Some(draw) = tracker.take_drawn() else {
        witness.observe_submit(ActorSubmitWitness {
            drawn_frame: false,
            exact: false,
            reserved: false,
            acknowledged: false,
        });
        return;
    };
    let exact = draw.is_exact();
    let Some(token) = gate.try_reserve_callback(draw) else {
        witness.observe_submit(ActorSubmitWitness {
            drawn_frame: true,
            exact,
            reserved: false,
            acknowledged: false,
        });
        return;
    };
    witness.observe_submit(ActorSubmitWitness {
        drawn_frame: true,
        exact,
        reserved: true,
        acknowledged: false,
    });
    let present_returned_at = web_time::Instant::now();
    let callback_gate = gate.clone();
    let callback_witness = witness.clone();
    crate::device_poll::on_frame_complete(&render_queue, move || {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("actor.completion_callback").entered();
        let acknowledged =
            callback_gate.publish_reserved(token, present_returned_at, web_time::Instant::now());
        callback_witness.observe_submit(ActorSubmitWitness {
            drawn_frame: true,
            exact: true,
            reserved: true,
            acknowledged,
        });
    });
}

#[cfg(test)]
mod tests;

#[cfg(feature = "publication-test-support")]
pub(crate) mod equipment_test_support;
