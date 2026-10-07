use super::{super::*, array};
use bevy::render::renderer::WgpuWrapper;

#[test]
fn native_authored_stripes_publish_complete_mips_and_preserve_carrier_resources() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU adapter for bounded authored terrain upload");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("authored upload fixture device");
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let base = ChunkTextureAssets::default();
    let (mut prepared, mut stats) =
        crate::chunk::gpu::texture_upload::build_chunk_texture_assets(&base, &device, &queue)
            .unwrap();
    let carrier_texture = prepared._textures[0].id();
    let carrier_materials = prepared.material_buffer.id();
    let old_authored_view = prepared.enhanced_views[0].id();
    let original_identity = prepared.identity;
    let arrays: [TextureArray; 6] = std::array::from_fn(|page| {
        let mut array = array(4, 2);
        for (level, mip) in array.mips.iter_mut().enumerate() {
            let layer_bytes = mip.size as usize * mip.size as usize * 4;
            for layer in 0..2 {
                let pixel = [
                    20 + page as u8 * 20 + level as u8 * 7 + layer as u8 * 3,
                    0,
                    0,
                    255,
                ];
                for target in
                    mip.rgba8[layer * layer_bytes..(layer + 1) * layer_bytes].chunks_exact_mut(4)
                {
                    target.copy_from_slice(&pixel);
                }
            }
        }
        array
    });
    let [color0, color1, normal0, normal1, material0, material1] = arrays;
    let mut references = vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS];
    references[0] = assets::PBR_REF_COLOR;
    let source = Arc::new(
        EnhancedTextureAssets::new(
            [color0, color1],
            [normal0, normal1],
            [material0, material1],
            references.into_boxed_slice(),
        )
        .unwrap(),
    );
    let candidate = base.with_updated_enhanced(Some(source.clone()));
    let mut pending = AuthoredUpload::new(candidate.identity(), source, &device).unwrap();
    let reference_bytes = assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS * 4;
    assert_eq!(
        pending.step(&device, &queue, reference_bytes + 32),
        reference_bytes + 32
    );
    assert!(!pending.complete());
    let mut frames = 1;
    while !pending.complete() {
        assert_eq!(prepared.enhanced_views[0].id(), old_authored_view);
        assert_eq!(prepared.identity, original_identity);
        let written = pending.step(&device, &queue, 32);
        assert!(written > 0 && written <= 32);
        frames += 1;
    }
    assert!(frames > 2);
    assert_eq!(pending.step(&device, &queue, FRAME_UPLOAD_BYTES), 0);
    pending.publish(&mut prepared, &mut stats);
    assert_eq!(prepared.identity, candidate.identity());
    assert_eq!(prepared._textures[0].id(), carrier_texture);
    assert_eq!(prepared.material_buffer.id(), carrier_materials);

    let shader = device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("completed authored mip sampling fixture"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var color0:texture_2d_array<f32>;
@group(0) @binding(1) var color1:texture_2d_array<f32>;
@group(0) @binding(2) var normal0:texture_2d_array<f32>;
@group(0) @binding(3) var normal1:texture_2d_array<f32>;
@group(0) @binding(4) var material0:texture_2d_array<f32>;
@group(0) @binding(5) var material1:texture_2d_array<f32>;
@group(0) @binding(6) var<storage,read> references:array<u32>;
@group(0) @binding(7) var<storage,read_write> results:array<vec4<f32>,37>;
fn sample_page(page:u32,layer:i32,mip:i32)->vec4<f32>{
    let uv=vec2<i32>(0);
    if page==0u{return textureLoad(color0,uv,layer,mip);}
    if page==1u{return textureLoad(color1,uv,layer,mip);}
    if page==2u{return textureLoad(normal0,uv,layer,mip);}
    if page==3u{return textureLoad(normal1,uv,layer,mip);}
    if page==4u{return textureLoad(material0,uv,layer,mip);}
    return textureLoad(material1,uv,layer,mip);
}
@compute @workgroup_size(1) fn read_mips(){
    for(var page=0u;page<6u;page++){
        for(var mip=0u;mip<3u;mip++){
            for(var layer=0u;layer<2u;layer++){
                results[page*6u+mip*2u+layer]=sample_page(page,i32(layer),i32(mip));
            }
        }
    }
    results[36]=vec4(f32(references[0]),0.0,0.0,1.0);
}
"#
                .into(),
            ),
        });
    let pipeline = device
        .wgpu_device()
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("completed authored mip fixture"),
            layout: None,
            module: &shader,
            entry_point: Some("read_mips"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bytes = 37 * 16;
    let output = device.create_buffer(&BufferDescriptor {
        label: Some("authored mip fixture samples"),
        size: bytes,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("authored mip fixture readback"),
        size: bytes,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut entries = prepared
        .enhanced_views
        .iter()
        .enumerate()
        .map(|(index, view)| wgpu::BindGroupEntry {
            binding: index as u32,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect::<Vec<_>>();
    entries.push(wgpu::BindGroupEntry {
        binding: 6,
        resource: prepared.enhanced_texture_refs.as_entire_binding(),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: 7,
        resource: output.as_entire_binding(),
    });
    let group = device
        .wgpu_device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("complete authored views fixture"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    receiver.recv().unwrap().unwrap();
    let mapped = readback.slice(..).get_mapped_range();
    let values: &[[f32; 4]] = bytemuck::cast_slice(&mapped);
    for page in 0..6 {
        for mip in 0..3 {
            for layer in 0..2 {
                let encoded = (20 + page * 20 + mip * 7 + layer * 3) as f32 / 255.0;
                let expected = if page < 2 {
                    if encoded <= 0.04045 {
                        encoded / 12.92
                    } else {
                        ((encoded + 0.055) / 1.055).powf(2.4)
                    }
                } else {
                    encoded
                };
                let value = values[page * 6 + mip * 2 + layer];
                assert!(
                    (value[0] - expected).abs() < 0.0005,
                    "page {page}, mip {mip}, layer {layer}: {value:?}"
                );
                assert_eq!(value[3], 1.0);
            }
        }
    }
    assert_eq!(values[36][0], assets::PBR_REF_COLOR as f32);
    super::super::detach(&mut prepared, original_identity, &device, &mut stats);
    assert_eq!(prepared.identity, original_identity);
    assert_eq!(prepared._textures[0].id(), carrier_texture);
    assert_eq!(prepared.material_buffer.id(), carrier_materials);
    assert_eq!(prepared._enhanced_textures[0].id(), carrier_texture);
}
