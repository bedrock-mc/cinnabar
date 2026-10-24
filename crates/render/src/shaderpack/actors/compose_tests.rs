//! Layer composition against the native colour-mask and multitexture formulas, and the
//! draw planning that binds composed layers.

use super::*;
use crate::gpu_snapshot::Gpu;

/// Two texels per layer: masked and unmasked base colours, then two overlay layers.
const LAYERS: [[[u8; 4]; 2]; 3] = [
    [[200, 100, 50, 255], [200, 100, 50, 0]],
    [[10, 20, 30, 255], [0, 0, 0, 0]],
    [[0, 0, 0, 0], [90, 80, 70, 128]],
];

/// The native fragment's gamma composition of `texel` with every layer's texel.
fn native(compose: LayerCompose, texel: usize) -> [u8; 4] {
    let unit = |layer: usize| LAYERS[layer][texel].map(|byte| f32::from(byte) / 255.0);
    let base = unit(0);
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let rgb: [f32; 3] = match compose {
        LayerCompose::Plain => [base[0], base[1], base[2]],
        LayerCompose::ColorMask { tint } => {
            let dye = tint.to_le_bytes().map(|byte| f32::from(byte) / 255.0);
            if tint == 0 {
                [base[0], base[1], base[2]]
            } else {
                std::array::from_fn(|c| mix(base[c], base[c] * dye[c], base[3]))
            }
        }
        LayerCompose::Multitexture { layers } => {
            let [one, two] = layers.map(|layer| unit(layer as usize));
            std::array::from_fn(|c| mix(mix(base[c], one[c], one[3]), two[c], two[3]))
        }
    };
    let [r, g, b] = rgb.map(|value| (value * 255.0).round() as u8);
    [r, g, b, 255]
}

/// Composes layer 0 of an sRGB artwork array, as actor pages are stored, and reads it back.
fn compose_on_gpu(gpu: &Gpu, compose: LayerCompose) -> Vec<u8> {
    let device = &gpu.device;
    let size = wgpu::Extent3d {
        width: 2,
        height: 1,
        depth_or_array_layers: LAYERS.len() as u32,
    };
    let artwork = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[TextureFormat::Rgba8Unorm],
    });
    gpu.queue.write_texture(
        artwork.as_image_copy(),
        LAYERS.as_flattened().as_flattened(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: Some(1),
        },
        size,
    );
    let composed = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            depth_or_array_layers: 1,
            ..size
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COMPOSED_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let source = crate::shader_source::composed_prepared_with(
        &compose_shader_source(include_str!("compose.wesl")),
        &[],
        Vec::new(),
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &layout().entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("compose_layer"),
        compilation_options: Default::default(),
        cache: None,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    dispatch(
        device,
        &mut encoder,
        (&pipeline, &layout),
        &artwork,
        0,
        compose,
        &composed.create_view(&Default::default()),
    )
    .unwrap();
    encoder.copy_texture_to_buffer(
        composed.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                rows_per_image: Some(1),
            },
        },
        composed.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    readback
        .slice(..8)
        .get_mapped_range()
        .expect("readback buffer is mapped")
        .to_vec()
}

/// Alpha weights colour and never coverage: every composed texel is opaque and matches the
/// native material's gamma composition, including texels the mask leaves undyed.
#[test]
fn composed_layers_match_native_colour_mask_and_multitexture_materials() {
    let Some(gpu) = Gpu::for_fixture("shader-pack actor layer composition") else {
        return;
    };
    for compose in [
        LayerCompose::ColorMask {
            tint: u32::from_le_bytes([128, 255, 64, 255]),
        },
        LayerCompose::ColorMask { tint: 0 },
        LayerCompose::Multitexture { layers: [1, 2] },
    ] {
        let texels = compose_on_gpu(&gpu, compose);
        for (texel, actual) in texels.as_chunks::<4>().0.iter().enumerate() {
            let expected = native(compose, texel);
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(&a, e)| a.abs_diff(e) <= 1),
                "{compose:?} texel {texel}: {actual:?} != {expected:?}"
            );
        }
    }
}

/// Artwork that only presents its layers, sampled by the planner.
struct MaskedSource<'a> {
    instances: [render_model::ActorGpuInstance; 2],
    composes: [LayerCompose; 2],
    texture: &'a bevy::render::render_resource::Texture,
}

impl super::super::RigSource for MaskedSource<'_> {
    fn buffers(&self) -> super::super::RigBuffers<'_> {
        unreachable!("draw planning does not bind buffers")
    }

    fn for_each_instance(&self, visit: &mut dyn FnMut(super::super::RigInstance<'_>)) {
        for (index, (data, compose)) in self.instances.iter().zip(self.composes).enumerate() {
            visit(super::super::RigInstance {
                index: index as u32,
                data,
                vertex_count: 3,
                texture: super::super::LayerTexture {
                    texture: self.texture,
                    layer: 0,
                    compose,
                },
                runtime_id: None,
                camera: true,
                light: [15; 2],
                blend: None,
                held: None,
            });
        }
    }

    fn glint(&self, _item: bool) -> Option<&bevy::render::render_resource::Texture> {
        None
    }
}

/// A composed layer carries its dye, so its draw's vertex colour stays white instead of
/// multiplying the dye in a second time; plain layers keep the dye as vertex colour.
#[test]
fn composed_draws_bind_their_own_layer_without_a_second_dye() {
    use crate::shaderpack::draws::DrawKind;
    let (device, _) = wgpu::Device::noop(&Default::default());
    let texture = bevy::render::render_resource::Texture::from(device.create_texture(
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
    ));
    let tint = u32::from_le_bytes([128, 255, 64, 255]);
    let instance = render_model::ActorGpuInstance {
        tint,
        ..Default::default()
    };
    let masked = LayerCompose::for_page(true, false, &instance);
    assert_eq!(masked, LayerCompose::ColorMask { tint });
    let source = MaskedSource {
        instances: [instance; 2],
        composes: [masked, LayerCompose::Plain],
        texture: &texture,
    };
    let mut actors = super::super::ViewActors::default();
    actors.plan_rig(
        &source,
        false,
        &super::super::ActorSources::default(),
        &|_| true,
        &mut 0,
    );
    let draws: Vec<_> = actors
        .draws
        .iter()
        .filter(|draw| draw.kind == DrawKind::Entities)
        .collect();
    assert_eq!(draws.len(), 2);
    assert_eq!(draws[0].texture.2, masked);
    assert_eq!(draws[1].texture.2, LayerCompose::Plain);
    assert_ne!(draws[0].texture, draws[1].texture);
    assert!(actors.compositions.contains_key(&draws[0].texture));
    assert!(actors.views.contains_key(&draws[1].texture));
    assert_eq!(actors.jobs[0].colour, u32::MAX);
    assert_eq!(actors.jobs[1].colour, tint);
}

/// Multitexture composes only with both overlay layers, as the native material requires.
#[test]
fn multitexture_pages_compose_only_with_both_overlays() {
    let mut instance = render_model::ActorGpuInstance {
        multitexture_layers: [3, u32::MAX],
        ..Default::default()
    };
    assert_eq!(
        LayerCompose::for_page(false, true, &instance),
        LayerCompose::Plain
    );
    instance.multitexture_layers = [3, 4];
    assert_eq!(
        LayerCompose::for_page(false, true, &instance),
        LayerCompose::Multitexture { layers: [3, 4] }
    );
    assert_eq!(
        LayerCompose::for_page(false, false, &instance),
        LayerCompose::Plain
    );
}
