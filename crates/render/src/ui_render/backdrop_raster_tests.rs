//! Pixel tests of the production backdrop shaders on a native headless adapter: the pass order
//! of [`super::apply_backdrops`] over an sRGB scene, read back after the shapes draw.

use super::{PYRAMID_FORMAT, pyramid_extent};
use crate::gpu_snapshot::{Gpu, SNAPSHOT_SIDE, save};
use render_api::UiBackdropPaint;
use std::borrow::Cow;
use wgpu::util::DeviceExt;

const SIDE: u32 = SNAPSHOT_SIDE;
const SCENE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// What a raster composites: backdrops over a copied sRGB scene, or a blurred element filter
/// group into a cleared gamma-space UI layer.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Backdrop,
    Filter,
}

impl Mode {
    fn format(self) -> wgpu::TextureFormat {
        match self {
            Mode::Backdrop => SCENE,
            Mode::Filter => super::super::composite::UI_LAYER_FORMAT,
        }
    }

    fn entry(self) -> &'static str {
        match self {
            Mode::Backdrop => "backdrop_fragment",
            Mode::Filter => "filter_fragment",
        }
    }
}

/// The vertex fields the backdrop shader reads, at the shader locations of the UI layout.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 4],
    uv: [f32; 2],
    color: [u8; 4],
    overlay: [f32; 4],
}

/// An axis-aligned backdrop rectangle in physical pixels.
struct Shape {
    rect: [f32; 4],
    coverage: f32,
    paint: UiBackdropPaint,
}

impl Shape {
    fn vertices(&self) -> [Vertex; 6] {
        let [left, top, right, bottom] = self.rect;
        [
            [left, top],
            [right, top],
            [right, bottom],
            [left, top],
            [right, bottom],
            [left, bottom],
        ]
        .map(|[x, y]| Vertex {
            position: [x, y, 0.0, 1.0],
            uv: [0.0, self.coverage],
            color: [255; 4],
            overlay: self.paint.overlay(),
        })
    }
}

fn module(gpu: &Gpu, source: &'static str) -> wgpu::ShaderModule {
    gpu.device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
        })
}

/// Draws backdrop `shapes` over `scene` (sRGB bytes, [`SIDE`] square) and returns the result.
fn draw(gpu: &Gpu, scene: &[u8], shapes: &[Shape]) -> Vec<u8> {
    raster(gpu, Mode::Backdrop, scene, shapes)
}

/// Composites `shapes` in `mode` from `source` (bytes in the mode's format) and reads back
/// the target.
fn raster(gpu: &Gpu, mode: Mode, scene: &[u8], shapes: &[Shape]) -> Vec<u8> {
    raster_levels(gpu, mode, scene, shapes, true)
}

/// Runs either the optimized chain or the previous full chain through the unchanged production shaders.
fn raster_levels(gpu: &Gpu, mode: Mode, scene: &[u8], shapes: &[Shape], pruned: bool) -> Vec<u8> {
    let device = &gpu.device;
    let filter = module(gpu, include_str!("../ui_backdrop_filter.wgsl"));
    let apply = module(gpu, include_str!("../ui_backdrop.wgsl"));
    let extent = |width, height| wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let source = device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: extent(SIDE, SIDE),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: mode.format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        scene,
    );
    let ([width, height], mips) = pyramid_extent([SIDE, SIDE]);
    let pyramid = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent(width, height),
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PYRAMID_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent(SIDE, SIDE),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: mode.format(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let filter_pipeline = |entry: &str, format: wgpu::TextureFormat| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &filter,
                entry_point: Some("fullscreen_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &filter,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let downsample = filter_pipeline("downsample", PYRAMID_FORMAT);
    let copy = filter_pipeline("copy", mode.format());
    let attributes =
        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x2, 2 => Unorm8x4, 6 => Float32x4];
    let blend = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    };
    let shapes_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &apply,
            entry_point: Some("backdrop_vertex"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &apply,
            entry_point: Some(mode.entry()),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: mode.format(),
                blend: Some(wgpu::BlendState {
                    color: blend,
                    alpha: blend,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let level = |mip| {
        pyramid.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: mip,
            mip_level_count: Some(1),
            ..Default::default()
        })
    };
    let source_view = source.create_view(&Default::default());
    // Automatic layouts belong to one pipeline each.
    let group_for = |pipeline: &wgpu::RenderPipeline, view: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        })
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    let target = |view| wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
    };
    let levels: Vec<_> = (0..mips).map(level).collect();
    let inputs: Vec<_> = std::iter::once(&source_view)
        .chain(&levels)
        .take(mips as usize)
        .map(|view| group_for(&downsample, view))
        .collect();
    let used = if pruned {
        super::super::backdrop_levels::count(
            shapes
                .iter()
                .map(|shape| shape.paint.blur)
                .fold(0., f32::max),
            mips as usize,
        )
    } else {
        mips as usize
    };
    for (destination, input) in levels.iter().zip(&inputs).take(used) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(target(destination))],
            ..Default::default()
        });
        pass.set_pipeline(&downsample);
        pass.set_bind_group(0, input, &[]);
        pass.draw(0..3, 0..1);
    }
    let viewport = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[SIDE as f32, SIDE as f32, 0.0, 0.0]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let vertices: Vec<Vertex> = shapes.iter().flat_map(Shape::vertices).collect();
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let viewport_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &shapes_pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: viewport.as_entire_binding(),
        }],
    });
    let all = pyramid.create_view(&Default::default());
    let scene_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &shapes_pipeline.get_bind_group_layout(1),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&all),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let output_view = output.create_view(&Default::default());
    {
        // The copy reads texels without the sampler, so its automatic layout omits it.
        let copy_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &copy.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(target(&output_view))],
            ..Default::default()
        });
        // A filter composites into a layer that already holds its earlier draws; here, none.
        if mode == Mode::Backdrop {
            pass.set_pipeline(&copy);
            pass.set_bind_group(0, &copy_group, &[]);
            pass.draw(0..3, 0..1);
        }
        pass.set_pipeline(&shapes_pipeline);
        pass.set_bind_group(0, &viewport_group, &[]);
        pass.set_bind_group(1, &scene_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(SIDE * SIDE * 4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIDE * 4),
                rows_per_image: Some(SIDE),
            },
        },
        extent(SIDE, SIDE),
    );
    gpu.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    readback
        .slice(..)
        .get_mapped_range()
        .expect("readback buffer is mapped")
        .to_vec()
}

/// A scene of `pixel(x, y)` sRGB bytes.
fn scene(pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .flat_map(|(x, y)| pixel(x, y))
        .collect()
}

fn at(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * SIDE + x) * 4) as usize;
    pixels[index..index + 4].try_into().unwrap()
}

fn linear(byte: u8) -> f64 {
    let value = f64::from(byte) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Covers the whole scene with `paint`.
fn everywhere(paint: UiBackdropPaint) -> Shape {
    Shape {
        rect: [0.0, 0.0, SIDE as f32, SIDE as f32],
        coverage: 1.0,
        paint,
    }
}

#[test]
fn backdrops_keep_the_scene_outside_their_shape_and_filter_it_inside() {
    let Some(gpu) = Gpu::for_fixture("UI backdrop colour raster") else {
        return;
    };
    let input = scene(|x, _| {
        if x < SIDE / 2 {
            [200, 100, 50, 255]
        } else {
            [30, 160, 220, 255]
        }
    });
    let dimmed = UiBackdropPaint {
        blur: 0.0,
        saturation: 0.0,
        brightness: 0.5,
    };
    let output = draw(
        &gpu,
        &input,
        &[Shape {
            rect: [0.0, 0.0, 64.0, SIDE as f32],
            coverage: 1.0,
            paint: dimmed,
        }],
    );
    save("backdrop-dimmed", &output);
    for y in [0, SIDE / 2, SIDE - 1] {
        for x in 64..SIDE {
            assert_eq!(at(&output, x, y), at(&input, x, y), "outside at {x},{y}");
        }
        // CSS saturate(0) brightness(.5) on sRGB values: Rec. 709 luma, halved.
        let luma = 0.2126 * 200.0 + 0.7152 * 100.0 + 0.0722 * 50.0;
        let expected = (luma * 0.5_f64).round() as u8;
        let pixel = at(&output, 10, y);
        for channel in 0..3 {
            assert!(
                pixel[channel].abs_diff(expected) <= 1,
                "{pixel:?} vs {expected}"
            );
        }
        assert_eq!(pixel[3], 255);
    }
}

#[test]
fn partial_coverage_mixes_the_processed_scene_in_linear_light() {
    let Some(gpu) = Gpu::for_fixture("UI backdrop coverage raster") else {
        return;
    };
    let input = scene(|_, _| [180, 180, 180, 255]);
    let black = UiBackdropPaint {
        brightness: 0.0,
        ..UiBackdropPaint::IDENTITY
    };
    let output = draw(
        &gpu,
        &input,
        &[Shape {
            rect: [0.0, 0.0, SIDE as f32, SIDE as f32],
            coverage: 0.5,
            paint: black,
        }],
    );
    let expected = linear(180) * 0.5;
    let actual = linear(at(&output, 100, 100)[0]);
    assert!((actual - expected).abs() < 0.01, "{actual} vs {expected}");
}

#[test]
fn blur_spreads_a_line_by_the_requested_standard_deviation_and_keeps_its_energy() {
    let Some(gpu) = Gpu::for_fixture("UI backdrop blur raster") else {
        return;
    };
    let line = SIDE / 2;
    let input = scene(|x, _| {
        if x == line {
            [255, 255, 255, 255]
        } else {
            [0, 0, 0, 255]
        }
    });
    for sigma in [1.0f32, 3.0, 8.0, 20.0] {
        let output = draw(
            &gpu,
            &input,
            &[everywhere(UiBackdropPaint {
                blur: sigma,
                ..UiBackdropPaint::IDENTITY
            })],
        );
        if sigma == 8.0 {
            save("backdrop-blurred-line", &output);
        }
        let row: Vec<f64> = (0..SIDE)
            .map(|x| linear(at(&output, x, SIDE / 2)[0]))
            .collect();
        let energy: f64 = row.iter().sum();
        let mean = row
            .iter()
            .enumerate()
            .map(|(x, value)| x as f64 * value)
            .sum::<f64>()
            / energy;
        let deviation = (row
            .iter()
            .enumerate()
            .map(|(x, value)| (x as f64 + 0.5 - (mean + 0.5)).powi(2) * value)
            .sum::<f64>()
            / energy)
            .sqrt();
        assert!(
            (energy - 1.0).abs() < 0.15,
            "blur {sigma} changed the line's energy to {energy}"
        );
        assert!(
            (mean - f64::from(line)).abs() < 1.0 + f64::from(sigma) * 0.1,
            "blur {sigma} moved the line to {mean}"
        );
        assert!(
            // Below the first level the exact scene fades into it, widening the spread a little.
            (deviation - f64::from(sigma)).abs() < (f64::from(sigma) * 0.12).max(0.4),
            "blur {sigma} spread the line by {deviation}"
        );
    }
}

#[test]
fn element_filters_blur_their_premultiplied_group_without_losing_coverage() {
    let Some(gpu) = Gpu::for_fixture("UI element filter raster") else {
        return;
    };
    let (low, high) = (SIDE / 2 - 16, SIDE / 2 + 16);
    let square = |x: u32, y: u32| (low..high).contains(&x) && (low..high).contains(&y);
    let group = scene(|x, y| if square(x, y) { [255; 4] } else { [0; 4] });
    let sigma = 6.0f32;
    let output = raster(
        &gpu,
        Mode::Filter,
        &group,
        &[everywhere(UiBackdropPaint {
            blur: sigma,
            ..UiBackdropPaint::IDENTITY
        })],
    );
    save("element-filter-square", &output);
    let row: Vec<f64> = (0..SIDE)
        .map(|x| f64::from(at(&output, x, SIDE / 2)[3]) / 255.0)
        .collect();
    let energy: f64 = row.iter().sum();
    let mean = row
        .iter()
        .enumerate()
        .map(|(x, alpha)| x as f64 * alpha)
        .sum::<f64>()
        / energy;
    let spread = (row
        .iter()
        .enumerate()
        .map(|(x, alpha)| (x as f64 - mean).powi(2) * alpha)
        .sum::<f64>()
        / energy)
        .sqrt();
    // A 32-pixel box has variance 32^2 / 12; the blur adds sigma^2.
    let expected = (32.0f64.powi(2) / 12.0 + f64::from(sigma).powi(2)).sqrt();
    assert!((energy - 32.0).abs() < 1.5, "coverage changed to {energy}");
    assert!((spread - expected).abs() < 1.0, "{spread} vs {expected}");
    assert!(row[(low - 3 * sigma as u32) as usize] < 0.02);
    assert!(row[(low - 2) as usize] > 0.2 && row[(SIDE / 2) as usize] > 0.97);
    // Premultiplied white stays white wherever it spreads.
    for x in 0..SIDE {
        let pixel = at(&output, x, SIDE / 2);
        assert!(pixel[0].abs_diff(pixel[3]) <= 1, "{pixel:?} at {x}");
    }
}

/// Skipping unused levels preserves real backdrop and element-filter pixels at fractional LODs.
#[test]
fn backdrop_levels_preserve_full_chain_pixels_through_strength_transitions() {
    let Some(gpu) = Gpu::for_fixture("UI backdrop mip work") else {
        return;
    };
    let mut scene = vec![0; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let at = ((y * SIDE + x) * 4) as usize;
            scene[at..at + 4].copy_from_slice(&[
                if (x / 3 + y / 5) % 2 == 0 { 240 } else { 20 },
                ((x * 17) % 256) as u8,
                ((y * 29) % 256) as u8,
                255,
            ]);
        }
    }
    let mut strengths = vec![0., 0.25, 1., 3., 6., 12., 24., 120.];
    for level in 1..=5 {
        let boundary = (2. / 3. * 4f32.powi(level) - 0.25).sqrt();
        strengths.extend([boundary.next_down(), boundary, boundary.next_up()]);
    }
    for mode in [Mode::Backdrop, Mode::Filter] {
        for sigma in &strengths {
            let shapes = [everywhere(UiBackdropPaint {
                blur: *sigma,
                ..UiBackdropPaint::IDENTITY
            })];
            let pruned = raster_levels(&gpu, mode, &scene, &shapes, true);
            let full = raster_levels(&gpu, mode, &scene, &shapes, false);
            assert_eq!(pruned.len(), full.len());
            for (index, (a, b)) in pruned.iter().zip(&full).enumerate() {
                assert!(
                    (i16::from(*a) - i16::from(*b)).abs() <= 1,
                    "blur={sigma}, byte={index}: pruned={a}, full={b}"
                );
            }
        }
    }
}
