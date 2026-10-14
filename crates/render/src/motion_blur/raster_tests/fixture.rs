//! Native raster fixtures share the production tile reduction and exposure shader.

use super::*;
use crate::queue_writes::ProfiledQueue as _;

impl Raster {
    pub(super) fn new(name: &str) -> Option<Self> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
            Ok(adapter) if adapter.get_info().backend != wgpu::Backend::Noop => adapter,
            Ok(_) | Err(wgpu::RequestAdapterError::NotFound { .. }) => {
                eprintln!("skipping {name}: missing native GPU adapter fixture");
                return None;
            }
            Err(error) => panic!("{name}: adapter request failed: {error}"),
        };
        let depth_samples = [1, 2, 4, 8]
            .into_iter()
            .filter(|samples| {
                let supported = [COLOR, DEPTH, crate::actor_render::motion::MOTION_FORMAT]
                    .into_iter()
                    .all(|format| {
                        adapter
                            .get_texture_format_features(format)
                            .flags
                            .sample_count_supported(*samples)
                    });
                if !supported {
                    eprintln!("skipping {name} at {samples}x MSAA: missing depth format support");
                }
                supported
            })
            .collect();
        let (device, queue) =
            bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: adapter.features()
                    & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                ..Default::default()
            }))
            .expect("motion blur raster device");
        Some(Self {
            gpu: Gpu {
                device,
                queue,
                backend: adapter.get_info().backend,
            },
            depth_samples,
        })
    }

    pub(super) fn texture(
        &self,
        format: wgpu::TextureFormat,
        samples: u32,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("motion blur raster fixture"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    pub(super) fn draw(
        &self,
        input: &[u8],
        exposure: ExposureUniform,
        samples: u32,
        z: f32,
    ) -> Vec<u8> {
        self.draw_with_motion(input, exposure, samples, z, None)
    }

    /// Supplies bounded actor coverage to the production exposure fragment.
    pub(super) fn draw_with_motion(
        &self,
        input: &[u8],
        exposure: ExposureUniform,
        samples: u32,
        z: f32,
        motion: Option<ActorMotion>,
    ) -> Vec<u8> {
        self.draw_with_motion_mask(input, exposure, samples, z, motion, !0)
    }

    /// Writes only selected coverage samples so sample-zero omissions exercise the real fragment.
    pub(super) fn draw_with_motion_mask(
        &self,
        input: &[u8],
        exposure: ExposureUniform,
        samples: u32,
        z: f32,
        motion: Option<ActorMotion>,
        sample_mask: u64,
    ) -> Vec<u8> {
        self.draw_with_coverage(
            input,
            exposure,
            samples,
            z,
            motion,
            Coverage {
                sample_mask,
                occluder: None,
                occluder_mask: !0,
                occluder_depth: super::occlusion::NEAR_DEPTH,
                depth_slope: 0.0,
            },
        )
    }

    /// Runs the production exposure and tile reduction against optional nearby fixture geometry.
    pub(super) fn draw_with_coverage(
        &self,
        input: &[u8],
        exposure: ExposureUniform,
        samples: u32,
        z: f32,
        motion: Option<ActorMotion>,
        coverage: Coverage,
    ) -> Vec<u8> {
        self.draw_with_fragment(input, exposure, samples, z, motion, coverage, "fragment")
    }

    /// Reads coverage and depth from the same attachments used by the production fragment.
    pub(super) fn probe_coverage(
        &self,
        input: &[u8],
        exposure: ExposureUniform,
        samples: u32,
        z: f32,
        motion: Option<ActorMotion>,
        coverage: Coverage,
    ) -> Vec<u8> {
        self.draw_with_fragment(
            input,
            exposure,
            samples,
            z,
            motion,
            coverage,
            "coverage_probe",
        )
    }

    /// Selects exposure or attachment diagnostics without changing fixture geometry or bindings.
    #[expect(
        clippy::too_many_arguments,
        reason = "fixture selects geometry, coverage and fragment"
    )]
    fn draw_with_fragment(
        &self,
        input: &[u8],
        mut exposure: ExposureUniform,
        samples: u32,
        z: f32,
        motion: Option<ActorMotion>,
        coverage: Coverage,
        fragment: &str,
    ) -> Vec<u8> {
        exposure.strength.z = if motion.is_some() { 1.0 } else { 0.0 };
        let mut source =
            super::super::shader::source(include_str!("../../motion_blur.wgsl"), samples > 1);
        source.push_str(r#"
@fragment fn coverage_probe(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let bindings = textureLoad(scene, pixel, 0).a
        + textureSampleLevel(scene, scene_sampler, position.xy / vec2<f32>(textureDimensions(scene)), 0.0).a
        + textureLoad(motion_tiles, vec2<i32>(0), 0).x + exposure.strength.x;
    return vec4<f32>(scene_depth(pixel), actor_depth(pixel) * 10.0, object_motion(pixel).w, bindings);
}
"#);
        let shader = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("production camera motion blur"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self
            .gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("camera motion blur raster"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                depth_stencil: None,
                primitive: Default::default(),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            });
        let scene = self.texture(
            COLOR,
            1,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        self.gpu.queue.profiled_write_texture(
            scene.as_image_copy(),
            input,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIDE * 4),
                rows_per_image: Some(SIDE),
            },
            scene.size(),
        );
        let scene_view = scene.create_view(&Default::default());
        let depth = self.texture(
            DEPTH,
            samples,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let depth_view = depth.create_view(&Default::default());
        let actor_velocity = self.texture(
            crate::actor_render::motion::MOTION_FORMAT,
            samples,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let actor_view = actor_velocity.create_view(&Default::default());
        let target = self.texture(
            COLOR,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let target_view = target.create_view(&Default::default());
        let multisampled = (samples > 1)
            .then(|| self.texture(COLOR, samples, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let multisampled_view = multisampled
            .as_ref()
            .map(|texture| texture.create_view(&Default::default()));
        let sampler = self.gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = self
            .gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("motion blur raster exposure"),
                contents: bytemuck::bytes_of(&exposure),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let tiles = self.tile_target();
        let group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&scene_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&actor_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&tiles),
                    },
                ],
            });
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("motion blur fixture depth clear"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(z),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.draw_motion(
            &mut encoder,
            &actor_view,
            &depth_view,
            samples,
            motion,
            coverage.sample_mask,
            z,
            coverage.depth_slope,
        );
        if let Some(bounds) = coverage.occluder {
            self.draw_occluder(
                &mut encoder,
                &depth_view,
                samples,
                bounds,
                coverage.occluder_mask,
                coverage.occluder_depth,
            );
        }
        self.draw_tiles(&mut encoder, &shader, &uniform, &actor_view, &tiles);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("motion blur production fragment"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: multisampled_view.as_ref().unwrap_or(&target_view),
                    depth_slice: None,
                    resolve_target: multisampled_view.as_ref().map(|_| &target_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        let readback = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIDE * SIDE * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIDE * 4),
                    rows_per_image: Some(SIDE),
                },
            },
            target.size(),
        );
        self.gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        self.gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        readback
            .slice(..)
            .get_mapped_range()
            .expect("readback buffer is mapped")
            .to_vec()
    }

    /// Creates a target with the same tile dimensions and format as the production pass.
    pub(super) fn tile_target(&self) -> wgpu::TextureView {
        self.gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("actor tile fixture"),
                size: wgpu::Extent3d {
                    width: SIDE.div_ceil(super::super::shader::TILE_SIZE),
                    height: SIDE.div_ceil(super::super::shader::TILE_SIZE),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: super::super::footprint::FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    /// Runs the production tile reduction before the final exposure gather.
    pub(super) fn draw_tiles(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        shader: &wgpu::ShaderModule,
        uniform: &wgpu::Buffer,
        actors: &wgpu::TextureView,
        target: &wgpu::TextureView,
    ) {
        let pipeline = self
            .gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("production actor tile reduction"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("tile_fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: super::super::footprint::FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let binding = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(actors),
                    },
                ],
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("actor tile fixture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &binding, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Clears world coverage and rasterizes the actor rectangle at every supported sample count.
    #[expect(
        clippy::too_many_arguments,
        reason = "fixture controls the actor coverage and depth attachments"
    )]
    pub(super) fn draw_motion(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        samples: u32,
        motion: Option<ActorMotion>,
        sample_mask: u64,
        z: f32,
        depth_slope: f32,
    ) {
        let resources = motion.map(|motion| {
            let source = format!(
                r#"
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {{
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}}
@group(0) @binding(0) var<uniform> velocity: vec4<f32>;
struct Motion {{
    @location(0) velocity: vec4<f32>,
    @builtin(frag_depth) depth: f32,
    @builtin(sample_mask) coverage: u32,
}}
@fragment fn motion_fragment(@builtin(position) position: vec4<f32>) -> Motion {{
    return Motion(velocity, {depth} + (position.x - {centre}) * {depth_slope}, {mask}u);
}}
"#,
                depth = motion.depth.unwrap_or(z),
                centre = SIDE as f32 * 0.5,
                mask = sample_mask as u32,
            );
            let shader = self
                .gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("bounded actor velocity fixture"),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                });
            let pipeline =
                self.gpu
                    .device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("actor coverage fixture"),
                        layout: None,
                        vertex: wgpu::VertexState {
                            module: &shader,
                            entry_point: Some("vertex"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &shader,
                            entry_point: Some("motion_fragment"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: crate::actor_render::motion::MOTION_FORMAT,
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: Default::default(),
                        depth_stencil: Some(wgpu::DepthStencilState {
                            format: DEPTH,
                            depth_write_enabled: Some(true),
                            depth_compare: Some(wgpu::CompareFunction::Always),
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: wgpu::MultisampleState {
                            count: samples,
                            mask: sample_mask,
                            ..Default::default()
                        },
                        multiview_mask: None,
                        cache: None,
                    });
            let uniform = self
                .gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("actor fixture velocity"),
                    contents: bytemuck::bytes_of(&motion.velocity),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let binding = self
                .gpu
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
            (pipeline, binding, motion.bounds)
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("actor velocity raster fixture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some((pipeline, binding, bounds)) = &resources {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, binding, &[]);
            pass.set_scissor_rect(bounds.x, bounds.y, bounds.z, bounds.w);
            pass.draw(0..3, 0..1);
        }
    }
}
