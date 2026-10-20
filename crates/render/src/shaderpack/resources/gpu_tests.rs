use super::*;

/// Requests a device for resource fixtures, skipping only an absent adapter.
fn gpu() -> Option<GpuContext> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("skipping shader pack resource fixtures: missing GPU adapter fixture");
            return None;
        }
        Err(error) => panic!("resource fixture adapter: {error}"),
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("shader pack resource fixture"),
        ..Default::default()
    }))
    .expect("resource fixture device");
    Some(GpuContext {
        device,
        queue,
        adapter,
    })
}

/// Encodes two different rows so their upload order is observable.
fn png() -> Vec<u8> {
    let mut bytes = vec![];
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 0, 0, 255, 0, 0, 255, 255])
            .unwrap();
    }
    bytes
}

#[test]
fn decoded_png_rows_reach_the_texture_in_their_original_order() {
    let Some(gpu) = gpu() else { return };
    let pack = ShaderPack::new(shaderpack::PackSource::from_files([("sample.png", png())]));
    let translation = pack.translate(&Default::default());
    let vanilla = VanillaTextures::default();
    let builder = ConstantBuilder::new(&gpu, &pack, &translation, &vanilla, gpu.support());
    let (_, view) = builder.decode_png("sample.png").unwrap();
    let witness = crate::gpu_snapshot::Gpu {
        device: gpu.device.clone(),
        queue: gpu.queue.clone(),
        backend: gpu.adapter.get_info().backend,
    };
    let rows = witness.compute_readback(
        "@group(0) @binding(0) var image: texture_2d<f32>;
         @group(0) @binding(1) var<storage, read_write> rows: array<vec4<u32>>;
         @compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) id: vec3<u32>) {
             rows[id.x] = vec4<u32>(round(textureLoad(image, vec2<i32>(0, i32(id.x)), 0) * 255.0));
         }",
        &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
        1,
        2,
        2,
    );
    assert_eq!(rows, [[255, 0, 0, 255], [0, 0, 255, 255]]);
}

#[test]
fn custom_textures_match_both_spellings_of_buffer_aliases() {
    let Some(gpu) = gpu() else { return };
    for (property, sampler) in [
        ("gcolor", "colortex0"),
        ("gdepth", "colortex1"),
        ("gnormal", "colortex2"),
        ("composite", "colortex3"),
        ("gaux1", "colortex4"),
        ("gaux2", "colortex5"),
        ("gaux3", "colortex6"),
        ("gaux4", "colortex7"),
        ("shadowcolor", "shadowcolor0"),
        ("colortex4", "gaux1"),
        ("shadowcolor0", "shadowcolor"),
    ] {
        let properties = format!("texture.composite.{property} = sample.png\n");
        let pack = ShaderPack::new(shaderpack::PackSource::from_files([
            ("sample.png", png()),
            ("shaders.properties", properties.into_bytes()),
        ]));
        let translation = pack.translate(&Default::default());
        let vanilla = VanillaTextures::default();
        let mut builder = ConstantBuilder::new(&gpu, &pack, &translation, &vanilla, gpu.support());
        assert!(
            builder
                .custom("composite", sampler, TextureDimension::D2)
                .is_some(),
            "{property} aliases {sampler}"
        );
        assert!(
            builder
                .custom("deferred", sampler, TextureDimension::D2)
                .is_none()
        );
        assert!(
            builder
                .custom("composite", sampler, TextureDimension::D3)
                .is_none()
        );
    }
}

#[test]
fn depth_placeholders_bind_with_the_requested_array_or_cube_shape() {
    let Some(gpu) = gpu() else { return };
    let pack = ShaderPack::new(shaderpack::PackSource::from_files([] as [(&str, &str); 0]));
    let translation = pack.translate(&Default::default());
    let vanilla = VanillaTextures::default();
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut builder = ConstantBuilder::new(&gpu, &pack, &translation, &vanilla, gpu.support());
    for (dimension, view_dimension) in [
        (
            TextureDimension::D2Array,
            wgpu::TextureViewDimension::D2Array,
        ),
        (TextureDimension::Cube, wgpu::TextureViewDimension::Cube),
        (
            TextureDimension::CubeArray,
            wgpu::TextureViewDimension::CubeArray,
        ),
    ] {
        let (index, _) = builder
            .placeholder(TextureFormat::Depth32Float, dimension, false)
            .unwrap();
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&builder.out.textures[index].view),
            }],
        });
    }
    builder.finish();
    assert!(bevy::tasks::block_on(scope.pop()).is_none());
}
