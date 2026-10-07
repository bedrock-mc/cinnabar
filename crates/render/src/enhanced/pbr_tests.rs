//! Native material response and UV orientation regression fixtures.
use super::post_regressions::execute;

#[test]
fn authored_materials_decode_dielectric_metal_subsurface_and_bounded_relief() {
    let source = crate::material_shader::source(
        r#"
#import cinnabar::enhanced_pbr::{material_basis,material_normal,material_response,material_relief_weight,material_relief_ray}
// ENHANCED_PBR_CONSTANTS
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,8>;
@compute @workgroup_size(1) fn regression(){
    let dielectric=material_response(vec4(10.0/255.0,0.0,0.65,32.0/255.0),PBR_REF_LABPBR,vec3(0.5));
    let metal=material_response(vec4(255.0/255.0,0.0,0.2,0.0),PBR_REF_LABPBR,vec3(0.8,0.3,0.1));
    let tissue=material_response(vec4(0.0,0.0,0.8,160.0/255.0),PBR_REF_LABPBR|PBR_REF_SUBSURFACE,vec3(0.5));
    results[0]=vec4(dielectric.fzero,dielectric.metallic);
    results[1]=vec4(metal.fzero,metal.metallic);
    results[2]=vec4(dielectric.porosity,dielectric.subsurface,tissue.subsurface,tissue.porosity);
    let basis=material_basis(vec3(0.0,0.0,1.0),vec3(1.0,0.0,0.0),vec3(0.0,1.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0));
    let mirrored=material_basis(vec3(0.0,0.0,1.0),vec3(1.0,0.0,0.0),vec3(0.0,1.0,0.0),vec2(-1.0,0.0),vec2(0.0,1.0));
    let rotated=material_basis(vec3(0.0,0.0,1.0),vec3(1.0,0.0,0.0),vec3(0.0,1.0,0.0),vec2(0.0,1.0),vec2(-1.0,0.0));
    results[3]=vec4(material_normal(vec4(0.75,0.5,1.0,0.5),basis),1.0);
    results[4]=vec4(material_normal(vec4(0.75,0.5,1.0,0.5),mirrored),1.0);
    results[5]=vec4(material_normal(vec4(0.75,0.5,1.0,0.5),rotated),1.0);
    results[6]=vec4(material_relief_weight(PBR_REF_HEIGHT,true,2.0,0.001,0.8),
        material_relief_weight(0u,true,2.0,0.001,0.8),
        material_relief_weight(PBR_REF_HEIGHT,false,2.0,0.001,0.8),
        material_relief_weight(PBR_REF_HEIGHT,true,20.0,0.001,0.8));
    results[7]=vec4(material_relief_ray(normalize(vec3(1.0,0.0,1.0)),basis,1.0),
        material_relief_weight(PBR_REF_HEIGHT,true,2.0,0.03,0.8),
        material_relief_weight(PBR_REF_HEIGHT,true,2.0,0.001,0.05));
}
"#,
    );
    let Some(v) = execute(&source, 8) else {
        return;
    };
    for value in &v[0][..3] {
        assert!((*value - 10.0 / 255.0).abs() < 0.0001);
    }
    assert_eq!(v[0][3], 0.0);
    assert_eq!(v[1], [0.8, 0.3, 0.1, 1.0]);
    assert!((v[2][0] - 0.5).abs() < 0.0001);
    assert_eq!(v[2][1], 0.0);
    assert!((v[2][2] - 0.5).abs() < 0.0001);
    assert_eq!(v[2][3], 0.0);
    assert!((v[3][0] - 0.5).abs() < 0.0001 && v[3][2] > 0.86);
    assert!((v[4][0] + 0.5).abs() < 0.0001 && v[4][2] > 0.86);
    assert!((v[5][1] + 0.5).abs() < 0.0001 && v[5][2] > 0.86);
    assert_eq!(v[6], [1.0, 0.0, 0.0, 0.0]);
    assert!((v[7][0] + assets::PBR_HEIGHT_SCALE).abs() < 0.0001);
    assert_eq!(&v[7][1..], &[0.0, 0.0, 0.0]);
}

#[test]
fn production_parallax_intersects_height_and_skips_unmapped_or_subpixel_surfaces() {
    use wgpu::util::DeviceExt;
    let instance = wgpu::Instance::new(&Default::default());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU adapter for authored parallax");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let declarations = format!(
        "@group(0) @binding({}) var enhanced_normal_page_0:texture_2d_array<f32>;\n@group(0) @binding({}) var enhanced_normal_page_1:texture_2d_array<f32>;\n@group(0) @binding({}) var enhanced_mer_page_0:texture_2d_array<f32>;\n@group(0) @binding({}) var enhanced_mer_page_1:texture_2d_array<f32>;\n@group(0) @binding({}) var enhanced_sampler:sampler;\n@group(0) @binding({}) var<storage,read> enhanced_texture_refs:array<u32>;\n",
        crate::material_shader::ENHANCED_NORMAL_TEXTURE_BINDINGS[0],
        crate::material_shader::ENHANCED_NORMAL_TEXTURE_BINDINGS[1],
        crate::material_shader::ENHANCED_MER_TEXTURE_BINDINGS[0],
        crate::material_shader::ENHANCED_MER_TEXTURE_BINDINGS[1],
        crate::material_shader::ENHANCED_SAMPLER_BINDING,
        crate::material_shader::ENHANCED_TEXTURE_REF_BINDING
    );
    let source = declarations
        + r#"
// ENHANCED_PBR_SAMPLING
@group(1) @binding(0) var<storage,read_write> results:array<vec4<f32>,5>;
@compute @workgroup_size(1) fn regression(){
    let basis=material_basis(vec3(0.0,0.0,1.0),vec3(1.0,0.0,0.0),vec3(0.0,1.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0));
    let view=normalize(vec3(1.0,0.0,1.0));let dx=vec2(0.001,0.0);let dy=vec2(0.0,0.001);let uv=vec2(0.5);
    results[0]=vec4(parallax_material_uv(0u,uv,dx,dy,view,basis,2.0,true),parallax_material_uv(1u,uv,dx,dy,view,basis,2.0,true));
    results[1]=vec4(parallax_material_uv(0u,uv,dx,dy,view,basis,2.0,false),parallax_material_uv(0u,uv,dx,dy,view,basis,30.0,true));
    results[2]=vec4(parallax_direct_visibility(0u,uv,dx,dy,view,basis,2.0,true),
        parallax_direct_visibility(0u,uv,dx,dy,-view,basis,2.0,true),1.0,1.0);
    results[3]=sample_pbr_texture(false,2u,vec2(0.49,0.5),vec2(0.001,0.0),vec2(0.0,0.001));
    results[4]=sample_pbr_texture(false,2u,vec2(0.51,0.5),vec2(0.001,0.0),vec2(0.0,0.001));
}
"#;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production POM sampling"),
        source: wgpu::ShaderSource::Wgsl(
            crate::shader_source::composed(&source, &["ENHANCED"]).into(),
        ),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let texture = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[128u8, 128, 255, 128].repeat(16),
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let material_data: Vec<u8> = (0..16)
        .flat_map(|pixel| {
            if pixel % 4 < 2 {
                [237u8, 0, 128, 32]
            } else {
                [10u8, 0, 128, 160]
            }
        })
        .collect();
    let material = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("categorical LabPBR fixture"),
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &material_data,
    );
    let material_view = material.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&crate::material_shader::pbr_sampler_descriptor());
    let refs = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[assets::PBR_REF_HEIGHT, 0u32, assets::PBR_REF_LABPBR]),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let mut entries = Vec::new();
    for binding in crate::material_shader::ENHANCED_NORMAL_TEXTURE_BINDINGS
        .into_iter()
        .chain(crate::material_shader::ENHANCED_MER_TEXTURE_BINDINGS)
    {
        entries.push(wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(
                if crate::material_shader::ENHANCED_NORMAL_TEXTURE_BINDINGS.contains(&binding) {
                    &view
                } else {
                    &material_view
                },
            ),
        });
    }
    entries.push(wgpu::BindGroupEntry {
        binding: crate::material_shader::ENHANCED_SAMPLER_BINDING,
        resource: wgpu::BindingResource::Sampler(&sampler),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: crate::material_shader::ENHANCED_TEXTURE_REF_BINDING,
        resource: refs.as_entire_binding(),
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 80,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 80,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let outputs = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: output.as_entire_binding(),
        }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.set_bind_group(1, &outputs, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 80);
    queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = readback.slice(..).get_mapped_range();
    let values: &[[f32; 4]] = bytemuck::cast_slice(&bytes);
    assert!(
        (values[0][0] - (0.5 - assets::PBR_HEIGHT_SCALE * (1.0 - 128.0 / 255.0))).abs() < 0.0001
    );
    assert_eq!(&values[0][1..], &[0.5, 0.5, 0.5]);
    assert_eq!(values[1], [0.5; 4]);
    assert_eq!(values[2], [1.0, 0.0, 1.0, 1.0]);
    assert!((values[3][0] * 255.0 - 237.0).abs() < 0.001);
    assert!((values[4][0] * 255.0 - 10.0).abs() < 0.001);
    assert!((values[3][3] * 255.0 - 32.0).abs() < 0.001);
    assert!((values[4][3] * 255.0 - 160.0).abs() < 0.001);
}
