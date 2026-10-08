//! Reprojected lamp and sunlight visibility share a receiver resolve pass.

use bevy::{
    core_pipeline::FullscreenShader,
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use std::sync::atomic::{AtomicBool, Ordering};

const RESPONSE_SECONDS: f32 = 0.045;

fn update_weight(delta: f32) -> f32 {
    if !delta.is_finite() || delta <= 0.0 {
        return 1.0;
    }
    1.0 - (-delta / RESPONSE_SECONDS).exp()
}

pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    let entries = [
        BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Depth,
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        BindingType::Sampler(SamplerBindingType::Filtering),
        BindingType::Texture {
            sample_type: TextureSampleType::Depth,
            view_dimension: TextureViewDimension::D2Array,
            multisampled: false,
        },
        BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        BindingType::Sampler(SamplerBindingType::Comparison),
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Depth,
            view_dimension: TextureViewDimension::D2Array,
            multisampled: false,
        },
        BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(binding, ty)| BindGroupLayoutEntry {
        binding: binding as u32,
        visibility: ShaderStages::FRAGMENT,
        ty,
        count: None,
    })
    .collect::<Vec<_>>();
    BindGroupLayoutDescriptor::new("reprojected local shadow visibility", &entries)
}

#[derive(Resource)]
pub(crate) struct LocalShadowPipeline(pub CachedRenderPipelineId);
impl FromWorld for LocalShadowPipeline {
    fn from_world(world: &mut World) -> Self {
        Self(
            world
                .resource::<PipelineCache>()
                .queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("local shadow visibility history".into()),
                    layout: vec![layout()],
                    vertex: world.resource::<FullscreenShader>().to_vertex_state(),
                    fragment: Some(FragmentState {
                        shader: super::LOCAL_SHADOW_HISTORY_SHADER,
                        entry_point: Some("resolve_local_shadows".into()),
                        targets: vec![
                            Some(ColorTargetState {
                                format: TextureFormat::Rgba16Float,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            }),
                            Some(ColorTargetState {
                                format: TextureFormat::Rgba16Float,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            }),
                            Some(ColorTargetState {
                                format: TextureFormat::Rgba16Float,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            }),
                        ],
                        ..default()
                    }),
                    ..default()
                }),
        )
    }
}
impl LocalShadowPipeline {
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.0).is_some()
    }
}

pub(crate) struct LocalShadowHistory {
    pub size: [u32; 2],
    pub sun: super::sun_shadow_history::SunShadowHistory,
    output: Texture,
    history: Texture,
    metadata_output: Texture,
    metadata_history: Texture,
    pub view: TextureView,
    pub metadata_view: TextureView,
    history_view: TextureView,
    metadata_history_view: TextureView,
    parameters: Buffer,
    uploaded_parameters: Option<[f32; 4]>,
    #[cfg(test)]
    parameter_uploads: u32,
    binding_key: Option<(
        TextureViewId,
        TextureViewId,
        TextureViewId,
        TextureViewId,
        TextureViewId,
        BufferId,
    )>,
    group: Option<BindGroup>,
    source: Option<[Option<u64>; super::local_lights::MAX_SHADOWED_LIGHTS]>,
    response_seconds: f32,
    submitted: AtomicBool,
}

impl LocalShadowHistory {
    pub fn new(device: &RenderDevice, size: [u32; 2]) -> Self {
        let create = |label, format, copy| {
            device.create_texture(&TextureDescriptor {
                label: Some(label),
                size: Extent3d {
                    width: size[0].max(1),
                    height: size[1].max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | copy,
                view_formats: &[],
            })
        };
        let output = create(
            "resolved local shadow visibility",
            TextureFormat::Rgba16Float,
            TextureUsages::COPY_SRC,
        );
        let history = create(
            "previous local shadow visibility",
            TextureFormat::Rgba16Float,
            TextureUsages::COPY_DST,
        );
        let metadata_output = create(
            "resolved local shadow metadata",
            TextureFormat::Rgba16Float,
            TextureUsages::COPY_SRC,
        );
        let metadata_history = create(
            "previous local shadow metadata",
            TextureFormat::Rgba16Float,
            TextureUsages::COPY_DST,
        );
        let view = output.create_view(&default());
        let metadata_view = metadata_output.create_view(&default());
        let history_view = history.create_view(&default());
        let metadata_history_view = metadata_history.create_view(&default());
        let parameters = device.create_buffer(&BufferDescriptor {
            label: Some("local shadow history policy"),
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            size,
            sun: super::sun_shadow_history::SunShadowHistory::new(device, size),
            output,
            history,
            metadata_output,
            metadata_history,
            view,
            metadata_view,
            history_view,
            metadata_history_view,
            parameters,
            uploaded_parameters: None,
            #[cfg(test)]
            parameter_uploads: 0,
            binding_key: None,
            group: None,
            source: None,
            response_seconds: RESPONSE_SECONDS,
            submitted: AtomicBool::new(false),
        }
    }

    /// Spans two sampled caster updates while retaining the short response for continuous captures.
    pub fn set_caster_interval(&mut self, seconds: f32) {
        self.response_seconds = if seconds.is_finite() {
            RESPONSE_SECONDS.max(seconds * 2.0)
        } else {
            RESPONSE_SECONDS
        };
    }

    pub fn prepare(
        &mut self,
        queue: &RenderQueue,
        camera_valid: bool,
        source: [Option<u64>; super::local_lights::MAX_SHADOWED_LIGHTS],
        delta: f32,
    ) {
        let valid = self.submitted.swap(false, Ordering::Relaxed) && camera_valid;
        let mut lanes = 0u32;
        let mut retained = 0u32;
        if valid && let Some(previous) = self.source {
            for (lane, identity) in source.into_iter().enumerate() {
                if identity.is_some()
                    && let Some(old_lane) = previous.iter().position(|old| *old == identity)
                {
                    lanes |= (old_lane as u32) << (lane * 2);
                    retained |= 1 << lane;
                }
            }
        }
        self.source = Some(source);
        let parameters = [
            f32::from(u8::from(valid)),
            update_weight(delta * RESPONSE_SECONDS / self.response_seconds),
            lanes as f32,
            retained as f32,
        ];
        if self.uploaded_parameters != Some(parameters) {
            queue.write_buffer(&self.parameters, 0, bytemuck::bytes_of(&parameters));
            self.uploaded_parameters = Some(parameters);
            #[cfg(test)]
            {
                self.parameter_uploads += 1;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bind(
        &mut self,
        device: &RenderDevice,
        cache: &PipelineCache,
        frame: &Buffer,
        depth: &TextureView,
        receiver_normal: &TextureView,
        motion: &TextureView,
        point_shadow: &TextureView,
        sun_shadow: &TextureView,
        sources: &Buffer,
        linear: &Sampler,
        comparison: &Sampler,
    ) {
        let key = (
            depth.id(),
            motion.id(),
            point_shadow.id(),
            sun_shadow.id(),
            receiver_normal.id(),
            sources.id(),
        );
        if self.binding_key == Some(key) {
            return;
        }
        self.group = Some(device.create_bind_group(
            "cached local shadow history",
            &cache.get_bind_group_layout(&layout()),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(depth),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&self.history_view),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(linear),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(point_shadow),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: sources.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: self.parameters.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 7,
                    resource: BindingResource::TextureView(motion),
                },
                BindGroupEntry {
                    binding: 8,
                    resource: BindingResource::Sampler(comparison),
                },
                BindGroupEntry {
                    binding: 9,
                    resource: BindingResource::TextureView(&self.metadata_history_view),
                },
                BindGroupEntry {
                    binding: 10,
                    resource: BindingResource::TextureView(&self.sun.history_view),
                },
                BindGroupEntry {
                    binding: 11,
                    resource: BindingResource::TextureView(sun_shadow),
                },
                BindGroupEntry {
                    binding: 12,
                    resource: self.sun.parameters.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 13,
                    resource: BindingResource::TextureView(receiver_normal),
                },
            ],
        ));
        self.binding_key = Some(key);
    }

    pub fn render(&self, context: &mut RenderContext, pipeline: &RenderPipeline) {
        let Some(group) = &self.group else {
            return;
        };
        {
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("resolve receiver shadow visibility"),
                color_attachments: &[
                    Some(RenderPassColorAttachment {
                        view: &self.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: StoreOp::Store,
                        },
                    }),
                    Some(RenderPassColorAttachment {
                        view: &self.metadata_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: StoreOp::Store,
                        },
                    }),
                    Some(RenderPassColorAttachment {
                        view: &self.sun.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: StoreOp::Store,
                        },
                    }),
                ],
                ..default()
            });
            pass.set_render_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        context.command_encoder().copy_texture_to_texture(
            self.output.as_image_copy(),
            self.history.as_image_copy(),
            self.output.size(),
        );
        context.command_encoder().copy_texture_to_texture(
            self.metadata_output.as_image_copy(),
            self.metadata_history.as_image_copy(),
            self.metadata_output.size(),
        );
        self.sun.store(context);
        self.submitted.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "local_shadow_history_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "local_shadow_temporal_tests.rs"]
mod temporal_tests;

#[cfg(test)]
#[path = "local_shadow_sampling_tests.rs"]
mod sampling_tests;
