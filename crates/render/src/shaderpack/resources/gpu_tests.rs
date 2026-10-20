use super::*;

/// Requests a device for resource fixtures, skipping only an absent adapter.
#[allow(
    clippy::print_stderr,
    reason = "names the absent GPU fixture when tests skip"
)]
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

#[test]
fn pack_load_keeps_depth_copies_at_float32_without_filtering() {
    let Some(gpu) = gpu() else { return };
    assert!(
        !gpu.device
            .features()
            .contains(wgpu::Features::FLOAT32_FILTERABLE)
    );
    let pack = ShaderPack::new(shaderpack::PackSource::from_files([] as [(&str, &str); 0]));
    let loaded = crate::shaderpack::loader::build(
        &pack,
        "depth fixture",
        "minecraft:overworld",
        None,
        &gpu,
        &VanillaTextures::default(),
    )
    .unwrap_or_else(|error| panic!("depth fixture: {error}"));
    assert_eq!(loaded.depth_format, TextureFormat::R32Float);
}

#[test]
fn pack_load_returns_an_error_on_allocation_failure() {
    let Some(gpu) = gpu() else { return };
    let pack = ShaderPack::new(shaderpack::PackSource::from_files([] as [(&str, &str); 0]));
    let _failure = crate::gpu_allocation::inject(|label, _| label == "shader pack load");
    let result = crate::shaderpack::loader::build(
        &pack,
        "allocation fixture",
        "minecraft:overworld",
        None,
        &gpu,
        &VanillaTextures::default(),
    );
    let error = result
        .err()
        .expect("allocation failure must reject the pack");
    assert!(error.contains("GPU memory"), "{error}");
}

#[test]
fn custom_storage_images_reject_mismatched_dimensions() {
    let Some(gpu) = gpu() else { return };
    for (dimensions, declaration, coordinate, matches) in [
        ("4 4", "image3D", "ivec3(0)", false),
        ("4 4 4", "image2D", "ivec2(0)", false),
        ("4", "image2D", "ivec2(0)", false),
        ("4 4", "image2DArray", "ivec3(0)", false),
        ("4", "image1D", "0", true),
        ("4 4", "image2D", "ivec2(0)", true),
        ("4 4 4", "image3D", "ivec3(0)", true),
    ] {
        let properties = format!(
            "image.fixture = fixtureSampler rgba rgba8 unsigned_byte false false {dimensions}\n"
        );
        let compute = format!(
            "#version 430\nlayout(local_size_x=1) in;\n\
             layout(rgba8) uniform writeonly {declaration} fixture;\n\
             void main() {{ imageStore(fixture, {coordinate}, vec4(1.0)); }}"
        );
        let pack = ShaderPack::new(shaderpack::PackSource::from_files([
            ("shaders.properties", properties.as_str()),
            ("composite.csh", compute.as_str()),
        ]));
        let translation = pack.translate(&Default::default());
        let reflection = translation.programs[0]
            .reflection
            .as_ref()
            .expect("storage image fixture");
        let vanilla = VanillaTextures::default();
        let mut constants =
            ConstantBuilder::new(&gpu, &pack, &translation, &vanilla, gpu.support());
        let formats = image_formats(&translation.properties.images, &[reflection], gpu.support());
        let result = bindings::resolve_bindings(
            &mut constants,
            ProgramGroup::Composite,
            reflection,
            &[None; MAX_COLOR_BUFFERS as usize],
            0,
            TextureFormat::R32Float,
            &translation.properties.images,
            &formats,
        );
        if matches {
            assert_eq!(result.unwrap().images, [ImageSource::Image(0)]);
        } else {
            let error = result
                .err()
                .expect("mismatched storage dimensions must reject the pack");
            assert!(error.contains("fixture"), "{error}");
        }
    }
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

/// Keeps a binary PNG on disk while its pack lazily reads the texture.
fn png_pack(properties: &str) -> (tempfile::TempDir, ShaderPack) {
    let directory = tempfile::tempdir().unwrap();
    let shaders = directory.path().join("shaders");
    std::fs::create_dir(&shaders).unwrap();
    std::fs::write(shaders.join("sample.png"), png()).unwrap();
    std::fs::write(shaders.join("shaders.properties"), properties).unwrap();
    let source = shaderpack::PackSource::open(directory.path()).unwrap();
    (directory, ShaderPack::new(source))
}

#[test]
fn decoded_png_rows_reach_the_texture_in_their_original_order() {
    let Some(gpu) = gpu() else { return };
    let (_directory, pack) = png_pack("");
    let translation = pack.translate(&Default::default());
    let vanilla = VanillaTextures::default();
    let builder = ConstantBuilder::new(&gpu, &pack, &translation, &vanilla, gpu.support());
    let (_, view) = builder.decode_png("sample.png").unwrap();
    assert_eq!(read_rows(&gpu, &view), [[255, 0, 0, 255], [0, 0, 255, 255]]);
}

/// Reads both RGBA8 rows through the texture interface used by pack samplers.
fn read_rows(gpu: &GpuContext, view: &wgpu::TextureView) -> Vec<[u32; 4]> {
    let witness = crate::gpu_snapshot::Gpu {
        device: gpu.device.clone(),
        queue: gpu.queue.clone(),
        backend: gpu.adapter.get_info().backend,
    };
    witness.compute_readback(
        "@group(0) @binding(0) var image: texture_2d<f32>;
         @group(0) @binding(1) var<storage, read_write> rows: array<vec4<u32>>;
         @compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) id: vec3<u32>) {
             rows[id.x] = vec4<u32>(round(textureLoad(image, vec2<i32>(0, i32(id.x)), 0) * 255.0));
         }",
        &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }],
        1,
        2,
        2,
    )
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
        let (_directory, pack) = png_pack(&properties);
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
        let _ = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
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

#[test]
fn resource_texture_rows_keep_their_decoded_order() {
    let Some(gpu) = gpu() else { return };
    let (_directory, pack) = png_pack("");
    let translation = pack.translate(&Default::default());
    let vanilla = VanillaTextures::default().with(
        "textures/environment/fixture.png",
        vanilla::VanillaImage {
            width: 1,
            height: 2,
            rgba8: std::sync::Arc::from([255, 0, 0, 255, 0, 0, 255, 255]),
        },
    );
    let builder = ConstantBuilder::new(&gpu, &pack, &translation, &vanilla, gpu.support());
    let (_, view) = builder
        .decode(&PackTexture::Resource(
            "minecraft:textures/environment/fixture.png".into(),
        ))
        .unwrap();
    assert_eq!(read_rows(&gpu, &view), [[255, 0, 0, 255], [0, 0, 255, 255]]);
}
