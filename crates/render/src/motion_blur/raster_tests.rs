//! Pixel tests against the production camera exposure shader on a native headless adapter.

use super::{
    CameraMotionBlur,
    history::{CameraHistory, ExposureUniform},
};
use crate::gpu_snapshot::Gpu;
use bevy::math::{Mat4, UVec4, Vec3, Vec4};
use wgpu::util::DeviceExt;

const SIDE: u32 = 128;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const FILTER_SAMPLE_COUNTS: [u32; 3] = [3, 17, 32];

struct Raster {
    gpu: Gpu,
    depth_samples: Vec<u32>,
}

impl Raster {
    fn new(name: &str) -> Option<Self> {
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
                let supported = [COLOR, DEPTH].into_iter().all(|format| {
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

    fn texture(
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

    fn draw(&self, input: &[u8], exposure: ExposureUniform, samples: u32, z: f32) -> Vec<u8> {
        let source = super::shader_source(include_str!("../motion_blur.wgsl"), samples > 1);
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
                    entry_point: Some("fragment"),
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
        self.gpu.queue.write_texture(
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
                ],
            });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
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
}

fn projection() -> Mat4 {
    glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_2,
        1.0,
        0.1,
    )
}

fn exposure(world_from_view: Mat4) -> ExposureUniform {
    let viewport = UVec4::new(0, 0, SIDE, SIDE);
    let mut previous = CameraHistory::new(projection(), Mat4::IDENTITY, viewport, 0);
    previous.advance(
        CameraHistory::new(projection(), world_from_view, viewport, 0),
        CameraMotionBlur {
            exposure_seconds: 1.0 / 120.0,
            delta_seconds: 1.0 / 120.0,
            samples: FILTER_SAMPLE_COUNTS[1],
            reset_epoch: 0,
        },
    )
}

fn image(pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            bytes.extend(pixel(x, y));
        }
    }
    bytes
}

fn impulse() -> Vec<u8> {
    image(|x, y| {
        let value = if x.abs_diff(SIDE / 2) <= 1 && y.abs_diff(SIDE / 2) <= 1 {
            255
        } else {
            0
        };
        [value, value, value, 255]
    })
}

fn extent(bytes: &[u8]) -> [u32; 2] {
    let mut lower = [SIDE; 2];
    let mut upper = [0; 2];
    for (index, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        if pixel[0] > 8 {
            let position = [index as u32 % SIDE, index as u32 / SIDE];
            for axis in 0..2 {
                lower[axis] = lower[axis].min(position[axis]);
                upper[axis] = upper[axis].max(position[axis]);
            }
        }
    }
    assert!(
        lower[0] <= upper[0],
        "blur must preserve the visible impulse"
    );
    [upper[0] - lower[0] + 1, upper[1] - lower[1] + 1]
}

fn save(name: &str, pixels: &[u8]) {
    if let Some(directory) = std::env::var_os("CINNABAR_REVIEW_SNAPSHOT_DIR") {
        image::save_buffer(
            std::path::PathBuf::from(directory).join(format!("motion-blur-{name}.png")),
            pixels,
            SIDE,
            SIDE,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

#[test]
fn motion_blur_static_camera_matches_disabled_raster_for_supported_msaa() {
    let Some(raster) = Raster::new("motion blur static camera raster") else {
        return;
    };
    let input = image(|x, y| [(x * 7) as u8, (y * 11) as u8, (x ^ y) as u8, 255]);
    for samples in &raster.depth_samples {
        let mut uniform = exposure(Mat4::IDENTITY);
        let disabled = raster.draw(&input, uniform, *samples, 0.5);
        assert_eq!(
            disabled, input,
            "disabled pass must preserve every source pixel"
        );
        for count in FILTER_SAMPLE_COUNTS {
            uniform.strength = Vec4::new(1.0, count as f32, 0.0, 0.0);
            let enabled = raster.draw(&input, uniform, *samples, 0.5);
            assert_eq!(
                enabled, disabled,
                "stationary reprojection changed pixels at {samples}x MSAA/{count} samples"
            );
        }
    }
}

#[test]
fn motion_blur_rotating_camera_smears_pixels_along_its_rotation_axis() {
    let Some(raster) = Raster::new("motion blur rotating camera raster") else {
        return;
    };
    let input = impulse();
    for samples in &raster.depth_samples {
        for (name, pose, axis) in [
            ("yaw", Mat4::from_rotation_y(0.2), 0),
            ("pitch", Mat4::from_rotation_x(0.2), 1),
        ] {
            for count in FILTER_SAMPLE_COUNTS {
                let mut uniform = exposure(pose);
                uniform.strength.y = count as f32;
                let blurred = raster.draw(&input, uniform, *samples, 0.05);
                assert_ne!(blurred, input, "{name} must change pixels");
                let span = extent(&blurred);
                assert!(
                    span[axis] > 6,
                    "{name} must spread the impulse along its axis: {span:?}"
                );
                assert!(
                    span[1 - axis] <= 4,
                    "{name} must not smear the perpendicular axis: {span:?}"
                );
                if *samples == 1 && count == FILTER_SAMPLE_COUNTS[1] {
                    save(name, &blurred);
                }
            }
        }
    }
    save("off", &input);
}

#[test]
fn motion_blur_translation_uses_depth_and_keeps_sky_stationary() {
    let Some(raster) = Raster::new("motion blur translation raster") else {
        return;
    };
    let input = impulse();
    let uniform = exposure(Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0)));
    for samples in &raster.depth_samples {
        let near = raster.draw(
            &input,
            uniform,
            *samples,
            projection().project_point3(Vec3::new(0.0, 0.0, -2.0)).z,
        );
        let far = raster.draw(
            &input,
            uniform,
            *samples,
            projection().project_point3(Vec3::new(0.0, 0.0, -8.0)).z,
        );
        let sky = raster.draw(&input, uniform, *samples, 0.0);
        assert_eq!(sky, input, "translation must not move the sky at infinity");
        let near_span = extent(&near);
        let far_span = extent(&far);
        assert!(
            near_span[0] > far_span[0],
            "nearby surfaces must smear farther: {near_span:?} vs {far_span:?}"
        );
        assert!(
            far_span[0] > extent(&input)[0],
            "distant geometry still receives camera translation"
        );
        assert_eq!(near_span[1], extent(&input)[1]);
        assert_eq!(far_span[1], extent(&input)[1]);
    }
}
