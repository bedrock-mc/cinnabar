use std::mem::size_of;
mod artwork;
pub(crate) mod phase;
mod pipeline;
mod skins;
use artwork::{GpuArtwork, draw_spans};
use phase::{DrawActorCommands, DrawTransparentActorCommands, queue_actors};
use pipeline::*;
use skins::GpuSkinArrays;

use crate::actor::{
    ActorDrawFrame, ActorDrawWitness, ActorGpuInstance, ActorPrepareWitness, ActorPresentationGate,
    ActorQueueWitness, ActorRenderFrame, ActorRigGeometrySpan, ActorRuntimeWitness,
    ActorSubmitWitness, gpu::ActorDrawTracker,
};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{
        CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey, Transparent3d,
    },
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParam, SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
            PhaseItemExtraIndex, RenderCommand, RenderCommandResult, SetItemPipeline,
            TrackedRenderPass, ViewBinnedRenderPhases, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CommandEncoderDescriptor, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, PipelineCache, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            ShaderType, Specializer, SpecializerKey, TexelCopyBufferLayout, TexelCopyTextureInfo,
            Texture, TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use render_model::ActorRigVertex;

const ACTOR_SHADER_HANDLE: Handle<Shader> = uuid_handle!("09d34708-6fd4-4c65-b27e-ce22f172cc73");
#[cfg(test)]
const ACTOR_SHADER_SOURCE: &str = include_str!("actor.wgsl");

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
        .init_resource::<crate::ActorPipelineReadiness>()
        .init_resource::<ActorRuntimeWitness>();
    crate::lighting::install(app);
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
        .resource::<crate::ActorPipelineReadiness>()
        .clone();
    app.add_plugins(ExtractResourcePlugin::<ActorRenderFrame>::default());
    load_internal_asset!(
        app,
        ACTOR_SHADER_HANDLE,
        "actor.wgsl",
        crate::shader_safety::from_actor_wgsl,
        crate::actor::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS
    );
    crate::nametag_render::install_nametag_render(app);
    crate::install_opaque_phase_reset(app.sub_app_mut(RenderApp));
    crate::device_poll::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .insert_resource(ActorRenderInstalled)
        .insert_resource(presentation_gate)
        .insert_resource(runtime_witness)
        .insert_resource(pipeline_readiness)
        .init_resource::<ActorPipeline>()
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
                prepare_actor_pipelines
                    .in_set(RenderSystems::Queue)
                    .before(queue_actors),
                queue_actors
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
                submit_actor_presented_frame.in_set(crate::device_poll::FrameSubmissions),
            ),
        );
}

#[derive(Resource)]
pub(crate) struct ActorGpu {
    artwork: GpuArtwork,
    player_material: Buffer,
    neutral_material: Buffer,
    color_mask_material: Buffer,
    multitexture_material: Buffer,
    spans: Vec<crate::actor::gpu::ActorDrawSpan>,
    instances: std::sync::Arc<[ActorGpuInstance]>,
    executed_instances: std::sync::atomic::AtomicU32,
    artwork_identity: [u8; 32],
    artwork_current: bool,
    instance_buffer: Buffer,
    previous_bone_buffer: Buffer,
    current_bone_buffer: Buffer,
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
    skin_revision: u64,
    view_buffer_id: Option<BufferId>,
    manifest: std::sync::Arc<[crate::actor::ActorDrawManifestEntry]>,
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

fn init_actor_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("nearest shared actor artwork sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: FilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(ActorGpu {
        artwork: GpuArtwork::default(),
        player_material: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("unchanged player material class"),
            contents: bytemuck::cast_slice(&[0u32; 4]),
            usage: BufferUsages::UNIFORM,
        }),
        neutral_material: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("neutral binary-alpha material class"),
            contents: bytemuck::cast_slice(&[1u32, 0, 0, 0]),
            usage: BufferUsages::UNIFORM,
        }),
        color_mask_material: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("native actor color-mask material"),
            // The shader consumes the second word as a Boolean, not a duplicated class ID.
            contents: bytemuck::cast_slice(&[0u32, 1, 0, 0]),
            usage: BufferUsages::UNIFORM,
        }),
        multitexture_material: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("native actor three-sampler material"),
            contents: bytemuck::cast_slice(&[0u32, 0, 1, 0]),
            usage: BufferUsages::UNIFORM,
        }),
        spans: Vec::new(),
        instances: std::sync::Arc::from([]),
        executed_instances: std::sync::atomic::AtomicU32::new(0),
        artwork_identity: [0; 32],
        artwork_current: false,
        instance_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor instance arena"),
            size: (crate::actor::MAX_ACTOR_RENDER_INSTANCES * size_of::<ActorGpuInstance>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        previous_bone_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor previous-bone arena"),
            size: (crate::actor::MAX_ACTOR_BONE_ARENA_BYTES / 2) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        current_bone_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor current-bone arena"),
            size: (crate::actor::MAX_ACTOR_BONE_ARENA_BYTES / 2) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        geometry_vertices: default(),
        geometry_span_buffer: None,
        instance_count: 0,
        maximum_vertex_count: 0,
        skins: GpuSkinArrays::new(&render_device),
        glint_sampler: render_device.create_sampler(&SamplerDescriptor {
            label: Some("repeat actor glint sampler"),
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        }),
        sampler,
        bind_group: None,
        frame_generation: u64::MAX,
        geometry_revision: u64::MAX,
        skin_revision: u64::MAX,
        view_buffer_id: None,
        manifest: std::sync::Arc::from([]),
    });
}

fn prepare_actor_resources(
    frame: Res<ActorRenderFrame>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<ActorGpu>,
    witness: Res<ActorRuntimeWitness>,
    gate: Res<ActorPresentationGate>,
    tracker: Res<ActorDrawTracker>,
) {
    let rig = &frame.rig;
    let artwork_valid = gpu
        .artwork
        .prepare(&frame.artwork, &render_device, &render_queue);
    gpu.artwork_current = artwork_valid;
    if gpu.artwork_identity != frame.artwork.identity() {
        gate.clear();
        tracker.clear();
        gpu.artwork_identity = frame.artwork.identity();
        gpu.frame_generation = u64::MAX;
    }
    let skins_resident = player_skins_resident(&frame);
    let structurally_valid = !rig.instances.is_empty()
        && rig.instances.len() <= crate::actor::MAX_ACTOR_RENDER_INSTANCES
        && rig.previous_bones.len() == rig.current_bones.len()
        && rig.previous_bones.len() <= crate::actor::MAX_ACTOR_POSE_BONES
        && rig.manifest.len() == rig.instances.len()
        && rig.maximum_vertex_count != 0
        && skins_resident
        && frame.instance_pages.len() == rig.instances.len();
    if gpu.geometry_revision != rig.geometry_revision {
        gate.clear();
        tracker.clear();
        gpu.frame_generation = u64::MAX;
        // A new skin model or item mesh uploads only its own vertices.
        gpu.geometry_vertices.sync(
            &render_device,
            &render_queue,
            "shared actor rig vertices",
            &rig.geometry_vertices,
        );
        gpu.geometry_span_buffer = (!rig.geometry_spans.is_empty()).then(|| {
            render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("shared actor rig geometry spans"),
                contents: bytemuck::cast_slice::<ActorRigGeometrySpan, u8>(&rig.geometry_spans),
                usage: BufferUsages::STORAGE,
            })
        });
        gpu.geometry_revision = rig.geometry_revision;
        gpu.bind_group = None;
        gpu.artwork.invalidate_bindings();
    }
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
        if structurally_valid {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!(
                "actor.frame_upload",
                generation = rig.frame_generation,
                instances = rig.instances.len(),
                bones = rig.current_bones.len(),
                bytes = std::mem::size_of_val(&*rig.instances)
                    + std::mem::size_of_val(&*rig.previous_bones)
                    + std::mem::size_of_val(&*rig.current_bones),
            )
            .entered();
            render_queue.write_buffer(
                &gpu.instance_buffer,
                0,
                bytemuck::cast_slice::<ActorGpuInstance, u8>(&rig.instances),
            );
            render_queue.write_buffer(
                &gpu.previous_bone_buffer,
                0,
                bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&rig.previous_bones),
            );
            render_queue.write_buffer(
                &gpu.current_bone_buffer,
                0,
                bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&rig.current_bones),
            );
            #[cfg(feature = "tracy")]
            drop(_span);
            gpu.instance_count = rig.instances.len() as u32;
            gpu.maximum_vertex_count = rig.maximum_vertex_count;
            gpu.manifest = std::sync::Arc::clone(&rig.manifest);
            gpu.spans = draw_spans(&frame.instance_pages, &rig.instances, &rig.geometry_spans);
            gpu.instances = std::sync::Arc::clone(&rig.instances);
        } else {
            gpu.instance_count = 0;
            gpu.maximum_vertex_count = 0;
            gpu.manifest = std::sync::Arc::from([]);
            gpu.spans.clear();
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
        if gpu.skins.sync(&frame.skins, &render_device, &render_queue) {
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

fn prepare_actor_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<ActorPipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<ActorGpu>,
) {
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
    let Some((_, glint_view)) = gpu.artwork.glint.as_ref() else {
        gpu.bind_group = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    if gpu.bind_group.is_some()
        && gpu.view_buffer_id == Some(view_buffer.id())
        && gpu
            .artwork
            .pages
            .iter()
            .all(|page| page.bind_group.is_some())
    {
        return;
    }
    let generic_groups: Vec<_> = gpu
        .artwork
        .pages
        .iter()
        .map(|page| {
            render_device.create_bind_group(
                "neutral actor page bind group",
                &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
                &[
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
                        binding: 11,
                        resource: BindingResource::TextureView(&gpu.skins.placeholder),
                    },
                ],
            )
        })
        .collect();
    gpu.bind_group = Some(render_device.create_bind_group(
        "instanced standard actor bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
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
                binding: 11,
                resource: BindingResource::TextureView(gpu.skins.view(3)),
            },
        ],
    ));
    for (page, group) in gpu.artwork.pages.iter_mut().zip(generic_groups) {
        page.bind_group = Some(group);
    }
    gpu.view_buffer_id = Some(view_buffer.id());
}

fn submit_actor_presented_frame(
    render_device: Res<RenderDevice>,
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
    let present_returned_at = bevy::platform::time::Instant::now();
    let encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("actor presented-frame completion sentinel"),
    });
    let command_buffer = encoder.finish();
    let callback_gate = gate.clone();
    let callback_witness = witness.clone();
    command_buffer.on_submitted_work_done(move || {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("actor.completion_callback").entered();
        let acknowledged = callback_gate.publish_reserved(
            token,
            present_returned_at,
            bevy::platform::time::Instant::now(),
        );
        callback_witness.observe_submit(ActorSubmitWitness {
            drawn_frame: true,
            exact: true,
            reserved: true,
            acknowledged,
        });
    });
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("actor.completion_submit").entered();
    render_queue.submit([command_buffer]);
}

#[cfg(test)]
mod tests;
